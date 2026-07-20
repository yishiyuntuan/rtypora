<script setup>
import { ref, computed, watch, nextTick } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import BlockView from './BlockView.vue';
import {
  blockToHtml,
  domToBlockDtos,
  emptyParagraphHtml,
  applyMarkdownShortcuts,
  convertFenceToCodeBlock,
  placeCursorAtEnd,
  placeCursorAtStart,
  insertTextAtCursor,
  insertLineBreakAtCursor,
  numberedOrdinals,
} from '../utils/wysiwyg.js';

// 双模式编辑器：sourceMode 为 Markdown 原文编辑，否则为所见即所得（Typora 式就地编辑）。
// 全文 Markdown 字符串是唯一数据源；块树由 Rust 命令 parse_markdown 解析得到。
// 前端只负责渲染与 DOM 结构提取：Markdown 解析、序列化、勾选切换、统计、快捷判定
// 全部由 Rust 命令完成（parse_markdown / parse_blocks / serialize_markdown /
// toggle_task_markdown / text_stats / detect_block_shortcut）。
const props = defineProps({
  sourceMode: { type: Boolean, default: false },
});

const emit = defineEmits(['update:stats', 'update:blocks', 'update:active-heading']);

const content = ref('');
const blocks = ref([]);
// 根块的有序列表序号表（id -> 1 基序号）
const rootOrdinals = computed(() => numberedOrdinals(blocks.value));
// 滚动定位闪烁的块 id / 当前滚动位置对应的标题 id（供大纲高亮）
const flashId = ref(null);
const activeHeadingId = ref(null);
// WYSIWYG 滚动容器（大纲定位用）
const scrollRoot = ref(null);
// 正在编辑的块 id；'__append__' 表示在文末追加新块
const editingId = ref(null);
// 提交/重解析进行中时忽略点击，避免用过期的块区间切片
const syncing = ref(false);
const wordCount = ref(0);
const charCount = ref(0);
const lineCount = ref(1);

const cursorLine = ref(1);
const cursorColumn = ref(1);
let parseSeq = 0;
// 当前 contenteditable 编辑容器元素
let editableEl = null;

// 行/词/字符统计由 Rust text_stats 命令完成（CJK 感知词数、UTF-16 字符数），
// 内容变化后防抖更新，避免每次按键都跨进程调用
let statsTimer = null;
watch(content, () => {
  clearTimeout(statsTimer);
  statsTimer = setTimeout(async () => {
    try {
      const stats = await invoke('text_stats', { markdown: content.value });
      wordCount.value = stats.words;
      charCount.value = stats.chars;
      lineCount.value = stats.lines;
    } catch (e) {
      console.error('text_stats 调用失败:', e);
    }
  }, 150);
}, { immediate: true });

watch([wordCount, charCount, lineCount, cursorLine, cursorColumn], () => {
  emit('update:stats', {
    wordCount: wordCount.value,
    charCount: charCount.value,
    lineCount: lineCount.value,
    cursorLine: cursorLine.value,
    cursorColumn: cursorColumn.value,
  });
}, { immediate: true });

// 块树变化时上报给 App（侧边栏目录/大纲数据源）。
// 不用 deep watch（每次提交都会触发全树遍历）：所有变更点已显式调用本函数。
function publishBlocks() {
  emit('update:blocks', blocks.value);
}

// 源码模式不解析；切回所见即所得时立即解析一次
watch(() => props.sourceMode, (source) => {
  if (!source) reparse();
});

async function reparse() {
  const seq = ++parseSeq;
  try {
    const result = await invoke('parse_markdown', { markdown: content.value });
    if (seq === parseSeq) {
      blocks.value = result;
      publishBlocks();
    }
  } catch (e) {
    console.error('parse_markdown 调用失败:', e);
  }
}

// 解析片段并把相对偏移换算为锚点后的绝对偏移
async function parseAnchoredBlocks(md, anchor) {
  const parsed = await invoke('parse_blocks', { markdown: md });
  return parsed.map((b) => ({
    ...b,
    start: b.start != null ? anchor + b.start : null,
    end: b.end != null ? anchor + b.end : null,
  }));
}

// 增量更新：编辑提交后只重解析受影响的片段（parse_blocks 返回相对偏移），
// 原位替换旧块并平移后续块偏移；未变化的块 id 稳定，前端仅局部重渲染。
// 与 velotype 一致：输入完成只更新当前块，不整树重解析。失败时回退全文重解析。
async function reparseRegion(index, oldBlock, md) {
  try {
    const replacements = await parseAnchoredBlocks(md, oldBlock.start);
    // JS 字符串 length 即 UTF-16 码元数，与 Rust 端偏移约定一致
    const delta = md.length - (oldBlock.end - oldBlock.start);
    blocks.value.splice(index, 1, ...replacements);
    for (let i = index + replacements.length; i < blocks.value.length; i++) {
      const b = blocks.value[i];
      if (b.start != null) {
        b.start += delta;
        b.end += delta;
      }
    }
    publishBlocks();
  } catch (e) {
    console.error('parse_blocks 调用失败，回退全文重解析:', e);
    await reparse();
  }
}

function onInput(e) {
  const textarea = e.target;
  const text = textarea.value.substring(0, textarea.selectionStart);
  cursorLine.value = (text.match(/\n/g) || []).length + 1;
  cursorColumn.value = text.length - text.lastIndexOf('\n');
}

// 编辑容器挂载时：填充渲染好的 HTML；聚焦与光标放置推迟到 nextTick——
// ref 回调时元素可能尚未插入文档（Vue 键控列表换键场景），立即 focus 会静默失败。
// 注意：两个编辑容器（块容器与文末 __append__ 容器）共用本 ref，Vue 打补丁时
// 「挂载新容器 → 卸载旧容器」的顺序不定，卸载回调可能后于挂载执行——
// 因此卸载（el 为 null）时仅当当前容器已脱离文档才清空，避免误清新容器。
function setEditableEl(el) {
  if (!el) {
    if (editableEl && !editableEl.isConnected) editableEl = null;
    return;
  }
  editableEl = el;
  const block = blocks.value.find((b) => b.id === editingId.value);
  // 原子/保留类块按原始 Markdown 切片编辑，其余按渲染 HTML 就地编辑
  const rawSource = block && block.start != null ? content.value.slice(block.start, block.end) : '';
  el.innerHTML = editingId.value === '__append__' || !block ? emptyParagraphHtml() : blockToHtml(block, rawSource);
  nextTick(() => {
    // 换块 blur 抑制到此为止（无论是否成功聚焦）
    suppressBlurCommit = false;
    // 期间用户点击切换了编辑目标，或元素已被卸载，则放弃聚焦
    if (editableEl !== el || !el.isConnected) return;
    el.focus();
    if (cursorAtStart) {
      cursorAtStart = false;
      placeCursorAtStart(el);
    } else {
      placeCursorAtEnd(el);
    }
  });
}
// setEditableEl 的下一次挂载把光标放在内容开头（Enter 拆分进入新块）
let cursorAtStart = false;

function startEdit(block) {
  if (syncing.value) return;
  if (editingId.value !== null) {
    // 编辑态残留（无活动容器）时自愈，避免点不出光标
    if (!editableEl) editingId.value = null;
    else return;
  }
  editingId.value = block.id;
}

function startAppend() {
  if (syncing.value) return;
  if (editingId.value !== null) {
    if (!editableEl) editingId.value = null;
    else return;
  }
  editingId.value = '__append__';
}

async function commitEdit() {
  if (editingId.value === null || !editableEl) return;
  syncing.value = true;
  try {
    // 前端只提取 DOM 结构，Markdown 序列化由 Rust 完成
    const md = await invoke('serialize_markdown', { blocks: domToBlockDtos(editableEl) });
    editableEl = null;
    suppressBlurCommit = false;    if (editingId.value === '__append__') {
      const text = md.trim();
      if (text) {
        const base = content.value ? content.value.replace(/\s*$/, '') : '';
        const anchor = base ? base.length + 2 : 0;
        content.value = base ? base + '\n\n' + text : text;
        // 增量：只解析追加的片段并挂到块树末尾
        const parsed = await invoke('parse_blocks', { markdown: text });
        blocks.value.push(
          ...parsed.map((b) => ({
            ...b,
            start: b.start != null ? anchor + b.start : null,
            end: b.end != null ? anchor + b.end : null,
          })),
        );
        publishBlocks();
      }
    } else {
      const index = blocks.value.findIndex((b) => b.id === editingId.value);
      const block = blocks.value[index];
      if (block) {
        content.value = content.value.slice(0, block.start) + md + content.value.slice(block.end);
        // 增量：原位替换被编辑的块
        await reparseRegion(index, block, md);
      }
    }
    editingId.value = null;
  } finally {
    syncing.value = false;
  }
}

// Enter 提交：在光标处拆分当前块——光标前内容留在本块、光标后内容移入新块，
// 两段分别由 Rust 序列化与解析，光标落到新块开头。代码块/Mermaid 等 pre 编辑不经过此路径。
async function splitAndCommit() {
  if (editingId.value === null || !editableEl) return;
  syncing.value = true;
  try {
    const sel = window.getSelection();
    if (!sel.rangeCount) return;
    const range = sel.getRangeAt(0);
    // 以光标为界克隆前后两段 DOM
    const beforeRange = range.cloneRange();
    beforeRange.selectNodeContents(editableEl);
    beforeRange.setEnd(range.startContainer, range.startOffset);
    const afterRange = range.cloneRange();
    afterRange.selectNodeContents(editableEl);
    afterRange.setStart(range.endContainer, range.endOffset);
    const beforeDiv = document.createElement('div');
    beforeDiv.append(beforeRange.cloneContents());
    const afterDiv = document.createElement('div');
    afterDiv.append(afterRange.cloneContents());

    // 空段检测：拆分会在边界克隆出空的块级元素（如标题末尾的空 <h2>），
    // 若参与序列化会生成孤立的标记文本（## 等），此处按空段处理
    const isEmptyDiv = (div) =>
      div.textContent.trim() === '' && !div.querySelector('img, input, hr, table, pre');
    const beforeMd = isEmptyDiv(beforeDiv)
      ? ''
      : (await invoke('serialize_markdown', { blocks: domToBlockDtos(beforeDiv) })).trim();
    const afterMd = isEmptyDiv(afterDiv)
      ? ''
      : (await invoke('serialize_markdown', { blocks: domToBlockDtos(afterDiv) })).trim();

    // 追加流（文末 __append__）没有可替换的旧块：锚点为文末，拼接整段
    const isAppend = editingId.value === '__append__';
    let index;
    let oldBlock;
    let base = '';
    if (isAppend) {
      base = content.value ? content.value.replace(/\s*$/, '') : '';
      index = blocks.value.length;
      const anchor = base ? base.length + 2 : 0;
      oldBlock = { start: anchor, end: anchor };
    } else {
      index = blocks.value.findIndex((b) => b.id === editingId.value);
      oldBlock = blocks.value[index];
    }
    editableEl = null;
    if (!oldBlock) {
      editingId.value = null;
      return;
    }

    const combined = beforeMd + '\n\n' + afterMd;
    if (isAppend) {
      content.value = base ? base + '\n\n' + combined : combined;
    } else {
      content.value = content.value.slice(0, oldBlock.start) + combined + content.value.slice(oldBlock.end);
    }

    // 增量重解析两段（锚点各自计算），原位替换并平移后续偏移
    const beforeBlocks = await parseAnchoredBlocks(beforeMd, oldBlock.start);
    const afterBlocks = await parseAnchoredBlocks(afterMd, oldBlock.start + beforeMd.length + 2);
    const replacements = [...beforeBlocks, ...afterBlocks];
    const delta = combined.length - (oldBlock.end - oldBlock.start);
    blocks.value.splice(index, isAppend ? 0 : 1, ...replacements);
    for (let i = index + replacements.length; i < blocks.value.length; i++) {
      const b = blocks.value[i];
      if (b.start != null) {
        b.start += delta;
        b.end += delta;
      }
    }
    publishBlocks();

    // 进入拆分出的新块编辑，光标在内容开头；抑制换块卸载触发的 blur 误提交
    const next = afterBlocks[0] ?? replacements[replacements.length - 1];
    suppressBlurCommit = true;
    editingId.value = next ? next.id : null;
    cursorAtStart = true;
    if (!next) suppressBlurCommit = false;
  } finally {
    syncing.value = false;
  }
}

function cancelEdit() {
  editingId.value = null;
  editableEl = null;
  suppressBlurCommit = false;
}

// 代码块/Mermaid 等 pre 编辑的 Ctrl+Enter：整块提交渲染（不拆分），
// 并在其后新建空段落进入编辑。普通 Enter 在 pre 内插入换行（见 keydown 路由）。
async function commitCodeAndNewBlock() {
  if (editingId.value === null || !editableEl) return;
  syncing.value = true;
  try {
    const md = (await invoke('serialize_markdown', { blocks: domToBlockDtos(editableEl) })).trimEnd();

    const isAppend = editingId.value === '__append__';
    let index;
    let oldBlock;
    let base = '';
    if (isAppend) {
      base = content.value ? content.value.replace(/\s*$/, '') : '';
      index = blocks.value.length;
      const anchor = base ? base.length + 2 : 0;
      oldBlock = { start: anchor, end: anchor };
    } else {
      index = blocks.value.findIndex((b) => b.id === editingId.value);
      oldBlock = blocks.value[index];
    }
    editableEl = null;
    if (!oldBlock) {
      editingId.value = null;
      return;
    }

    // 整块 + 尾部空行（解析出一个空段落作为后续编辑块）
    const combined = md + '\n\n';
    if (isAppend) {
      content.value = base ? base + '\n\n' + combined : combined;
    } else {
      content.value = content.value.slice(0, oldBlock.start) + combined + content.value.slice(oldBlock.end);
    }
    const replacements = await parseAnchoredBlocks(combined, oldBlock.start);
    const delta = combined.length - (oldBlock.end - oldBlock.start);
    blocks.value.splice(index, isAppend ? 0 : 1, ...replacements);
    for (let i = index + replacements.length; i < blocks.value.length; i++) {
      const b = blocks.value[i];
      if (b.start != null) {
        b.start += delta;
        b.end += delta;
      }
    }
    publishBlocks();

    // 最后一个替换块即尾部空段落，进入编辑；抑制换块 blur 误提交
    const next = replacements[replacements.length - 1];
    suppressBlurCommit = true;
    editingId.value = next ? next.id : null;
    cursorAtStart = true;
    if (!next) suppressBlurCommit = false;
  } finally {
    syncing.value = false;
  }
}

// 失焦提交：仅当失焦元素就是当前编辑容器时生效。
// Enter 拆分等程序性换块会先卸载聚焦的旧容器再挂载新容器，卸载触发的 blur
// 不是用户主动失焦（此刻 editableEl 还指向旧容器，单靠元素比对拦不住），
// 用 suppressBlurCommit 标记抑制。
function onEditableBlur(e) {
  if (suppressBlurCommit) return;
  if (e.target !== editableEl) return;
  commitEdit();
}
// 程序性换块进行中：抑制卸载 blur 触发的误提交
let suppressBlurCommit = false;

// 输入时应用 Markdown 快捷转换（输入法组合期间跳过）
function onEditableInput(e) {
  if (e.isComposing || e.inputType === 'insertCompositionText') return;
  applyMarkdownShortcuts();
}

function onEditableKeydown(e) {
  if (e.isComposing) return;
  if (e.key === 'Enter') {
    const sel = window.getSelection();
    const anchor = sel.rangeCount ? sel.anchorNode : null;
    const el = anchor ? (anchor.nodeType === Node.TEXT_NODE ? anchor.parentElement : anchor) : null;
    const inPre = !!(el && el.closest('pre'));
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      // 代码块/Mermaid 等 pre：整块提交并切到新块；其余块直接整块提交
      if (inPre) commitCodeAndNewBlock();
      else commitEdit();
      return;
    }
    // 代码块/Mermaid 等 pre 内 Enter 插入换行符（行为不变）；Shift+Enter 软换行；
    // 普通 Enter 在光标处拆分：当前块提交渲染，光标进入新块开头
    if (inPre) {
      e.preventDefault();
      insertTextAtCursor('\n');
      return;
    }
    if (e.shiftKey) {
      e.preventDefault();
      insertLineBreakAtCursor();
      return;
    }
    e.preventDefault();
    // 围栏行（```lang）按 Enter：转换为代码块并进入块内编辑，不做拆分
    if (editableEl) {
      convertFenceToCodeBlock(editableEl).then((converted) => {
        if (!converted) splitAndCommit();
      });
    } else {
      splitAndCommit();
    }
  } else if (e.key === 'Escape') {
    e.preventDefault();
    cancelEdit();
  }
}


// 任务列表勾选：标记替换由 Rust 完成（替换源码切片内首个 [ ]/[x]），再增量重解析该块
async function toggleTask(block) {
  if (block.type !== 'taskListItem' || block.start == null) return;
  const index = blocks.value.findIndex((b) => b.id === block.id);
  if (index < 0) return;
  const source = content.value.slice(block.start, block.end);
  const next = await invoke('toggle_task_markdown', { source, checked: !block.checked });
  if (next === source) return;
  content.value = content.value.slice(0, block.start) + next + content.value.slice(block.end);
  await reparseRegion(index, block, next);
}

// ---------- 大纲联动 ----------

// 滚动到指定块并短暂闪烁高亮（侧边栏目录/大纲点击定位）
function scrollToBlock(id) {
  const root = scrollRoot.value;
  if (!root) return;
  const el = root.querySelector(`[data-block-id="${id}"]`);
  if (!el) return;
  el.scrollIntoView({ behavior: 'smooth', block: 'start' });
  flashId.value = id;
  setTimeout(() => {
    if (flashId.value === id) flashId.value = null;
  }, 1200);
}

// 滚动时同步当前标题（取滚动位置上方最近的标题块），供大纲高亮
let scrollTicking = false;
function onEditorScroll(e) {
  if (scrollTicking) return;
  scrollTicking = true;
  requestAnimationFrame(() => {
    scrollTicking = false;
    const root = e.target;
    const top = root.scrollTop + 8;
    let current = null;
    for (const b of blocks.value) {
      if (b.type !== 'heading') continue;
      const el = root.querySelector(`[data-block-id="${b.id}"]`);
      if (!el) continue;
      if (el.offsetTop <= top) current = b.id;
      else break;
    }
    if (current !== activeHeadingId.value) {
      activeHeadingId.value = current;
      emit('update:active-heading', current);
    }
  });
}

defineExpose({ scrollToBlock });
</script>

<template>
  <div class="flex h-full flex-col">
    <textarea
      v-if="sourceMode"
      class="t-root flex-1 resize-none border-none font-mono outline-none"
      placeholder="开始写作..."
      v-model="content"
      @input="onInput"
      @keyup="onInput"
      @click="onInput"
    ></textarea>

    <div v-else ref="scrollRoot" class="t-root flex-1 overflow-y-auto" @scroll.passive="onEditorScroll">
      <div class="t-measure">
      <template v-for="block in blocks" :key="block.id">
        <!-- Typora 式就地编辑：渲染后的内容直接在 contenteditable 中编辑 -->
        <div
          v-if="editingId === block.id"
          :ref="setEditableEl"
          class="md-editing px-1 outline-none"
          contenteditable="true"
          spellcheck="false"
          @input="onEditableInput"
          @keydown="onEditableKeydown"
          @blur="onEditableBlur"
        ></div>
        <div
          v-else
          class="md-block cursor-text px-1"
          :class="{ 'md-flash': flashId === block.id }"
          :data-block-id="block.id"
          @click="startEdit(block)"
        >
          <BlockView :block="block" :ordinal="rootOrdinals.get(block.id) || 1" @toggle-task="toggleTask" />
        </div>
      </template>

      <div
        v-if="editingId === '__append__'"
        :ref="setEditableEl"
        class="md-editing px-1 outline-none"
        contenteditable="true"
        spellcheck="false"
        @input="onEditableInput"
        @keydown="onEditableKeydown"
        @blur="onEditableBlur"
      ></div>
      <div v-else class="min-h-24 cursor-text" @click="startAppend">
        <span v-if="blocks.length === 0" class="t-dim px-1">开始写作...</span>
      </div>
      </div>
    </div>
  </div>
</template>
