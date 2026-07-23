<script setup>
import { ref, computed, watch, nextTick, inject, provide } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import BlockView from './BlockView.vue';
import SlashMenu from './SlashMenu.vue';
import {
  blockToHtml,
  domToBlockDtos,
  emptyParagraphHtml,
  applyMarkdownShortcuts,
  convertFenceToCodeBlock,
  convertSectionToHtmlBlock,
  SLASH_ITEMS,
  SLASH_TEXT_BADGES,
  LANG_BADGES,
  applySlashCommand,
  placeCursorAtEnd,
  placeCursorAtStart,
  placeCaretAtTextOffset,
  insertTextAtCursor,
  insertLineBreakAtCursor,
  numberedOrdinals,
  isCaretAtStart,
  deleteTaskCheckboxBeforeCaret,
  handleListEnter,
  handleListTab,
  paragraphDtoFromTree,
  demoteEditableToParagraph,
  plainText,
} from '../utils/wysiwyg.js';
import { getPref, prefsVersion } from '../utils/prefs.js';

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
// 已保存快照：脏标记 = 当前内容与快照不一致
const savedContent = ref('');
const isDirty = computed(() => content.value !== savedContent.value);
const blocks = ref([]);
// 根块的有序列表序号表（id -> 1 基序号）
const renderedBlocks = computed(() => blocks.value.filter((b) => b.type !== 'footnoteDefinition'));
const footnotes = computed(() => blocks.value.filter((b) => b.type === 'footnoteDefinition'));
// 根块的有序列表序号表（id -> 1 基序号；footnoteDefinition 已过滤到末尾，不中断正文列表编号）
const rootOrdinals = computed(() => numberedOrdinals(renderedBlocks.value));
// 脚注引用全局序号表：按文档顺序（块顺序 + 递归子块）为每个 [^id] 分配 1 基 refIndex，
// 用于定义区返回链接与引用区锚点一一对应。项中保留 blockId 与 occurrenceIndex 以精确匹配。
const footnoteRefList = computed(() => {
  const list = [];
  function scanTree(tree, blockId) {
    for (const f of tree?.fragments || []) {
      if (f.footnote) {
        list.push({
          id: f.footnote.id,
          occurrenceIndex: f.footnote.occurrenceIndex,
          blockId,
          refIndex: list.length + 1,
        });
      }
    }
  }
  function scanBlock(block) {
    scanTree(block.title, block.id);
    for (const child of block.children || []) scanBlock(child);
  }
  for (const b of renderedBlocks.value) scanBlock(b);
  return list;
});
provide('footnoteRefList', footnoteRefList);
provide('getFootnoteRefIndex', (id, occurrenceIndex, blockId) => {
  const idx = footnoteRefList.value.findIndex(
    (r) => r.id === id && r.blockId === blockId && r.occurrenceIndex === occurrenceIndex,
  );
  return idx >= 0 ? idx + 1 : 0;
});

// 展示公式编号表（块 id -> 1 基序号）：AMS 编号环境判定在 Rust（mathNumbered 字段），
// 编号序列按偏好 math_numbering（off/ams/all）生成（视图层），经 provide 供块渲染层取用
const mathNumbers = computed(() => {
  prefsVersion.value;
  const map = new Map();
  const mode = getPref('math_numbering');
  if (mode === 'off') return map;
  let n = 0;
  for (const b of blocks.value) {
    if (b.type !== 'mathBlock') continue;
    if (mode === 'ams' && !b.mathNumbered) continue;
    n += 1;
    map.set(b.id, n);
  }
  return map;
});
provide('mathNumbers', mathNumbers);

// ---------- 斜杠命令菜单（/ 召唤 Markdown 语法菜单） ----------
const slashOpen = ref(false);
const slashQuery = ref('');
const slashIndex = ref(0);
// 文本行当前徽章下标（0-9：H1-H6 + 无序/有序/任务列表 + 行内代码，←/→ 或悬停徽章调整）
const slashTextIndex = ref(0);
// 表格行行列数量与当前调节字段（'item' 表格项本身 / 'rows' 行数 / 'cols' 列数；
// 'item' 时 ↑/↓ 为菜单导航，←/→ 在 项→行→列 间循环，进入字段后 ↑/↓ 增减）
const slashTableRows = ref(2);
const slashTableCols = ref(2);
const slashTableField = ref('item');
const slashPos = ref({ left: 0, top: 0 });

// 按查询串过滤菜单项（匹配中文名 / 拼音与英文别名 / id）
const slashItems = computed(() => {
  const q = slashQuery.value.trim().toLowerCase();
  if (!q) return SLASH_ITEMS;
  return SLASH_ITEMS.filter(
    (item) =>
      item.label.toLowerCase().includes(q) || item.keywords.includes(q) || item.id.toLowerCase().includes(q),
  );
});

// 光标处的斜杠上下文：块内文本（光标前）以 / 开头时返回查询串与菜单位置；
// 代码/原文 pre 编辑内不触发
function slashContext() {
  const el = currentEditable();
  if (!el || editingId.value === null) return null;
  const sel = window.getSelection();
  if (!sel.rangeCount || !sel.isCollapsed) return null;
  const caret = sel.getRangeAt(0);
  if (!el.contains(caret.startContainer)) return null;
  const anchorEl = caret.startContainer.nodeType === Node.TEXT_NODE ? caret.startContainer.parentElement : caret.startContainer;
  if (anchorEl?.closest('pre')) return null;
  const before = document.createRange();
  before.selectNodeContents(el);
  before.setEnd(caret.startContainer, caret.startOffset);
  const text = before.toString();
  if (!text.startsWith('/')) return null;
  const rect = caret.getBoundingClientRect();
  // 视口底部空间不足时向上翻（菜单最大高度 264 + 间隙）；文本行徽章较宽，左侧防溢出
  const estimatedHeight = 270;
  const top = rect.bottom + estimatedHeight > window.innerHeight
    ? Math.max(8, rect.top - estimatedHeight)
    : rect.bottom + 6;
  const left = Math.max(8, Math.min(rect.left, window.innerWidth - 380));
  return { query: text.slice(1), pos: { left, top } };
}

// 输入后刷新菜单：触发文本被删改（不再是 / 开头、含空格/换行）则关闭
function updateSlashMenu() {
  const ctx = slashContext();
  if (!ctx || ctx.query.includes('\n') || ctx.query.includes(' ')) {
    slashOpen.value = false;
    return;
  }
  slashQuery.value = ctx.query;
  if (slashIndex.value >= slashItems.value.length) slashIndex.value = 0;
  slashTextIndex.value = 0;
  slashTableField.value = 'item';
  slashPos.value = ctx.pos;
  slashOpen.value = true;
}

async function applySlashItem(item, option) {
  const el = currentEditable();
  if (!el || !item) return;
  slashOpen.value = false;
  // 文本行按当前/点选徽章应用；表格带行列数量；其余项直接应用
  let id = item.id;
  let opts;
  if (id === 'text') id = SLASH_TEXT_BADGES[option ?? slashTextIndex.value].id;
  else if (id === 'table') opts = { rows: slashTableRows.value, cols: slashTableCols.value };
  // 替换 DOM 期间抑制 blur 误提交：清空聚焦中的容器可能触发同步 blur，
  // 不抑制则 onEditableBlur 抢先提交空内容（块被删，新内容插到已卸载节点上）
  suppressBlurCommit = true;
  try {
    await applySlashCommand(el, id, opts);
  } finally {
    suppressBlurCommit = false;
  }
  // 替换导致失焦时恢复焦点（光标已由构建函数放置）
  if (document.activeElement !== el) el.focus({ preventScroll: true });
}

// ---------- 围栏语言自动补全（``` 后弹出语言菜单，含 mermaid/math） ----------
const langOpen = ref(false);
const langQuery = ref('');
const langIndex = ref(0);
const langPos = ref({ left: 0, top: 0 });
// 语言清单（Rust code_languages：mermaid/math + tree-sitter 高亮语言），首次打开时拉取
const langList = ref([]);

// 前缀匹配过滤（清单由 Rust code_languages 按字典序返回）
const langItems = computed(() => {
  const q = langQuery.value.trim().toLowerCase();
  if (!q) return langList.value;
  return langList.value.filter((lang) => lang.toLowerCase().startsWith(q));
});

// 菜单展示项（每语言专属缩写徽章 + 品牌色）
const langMenuItems = computed(() =>
  langItems.value.map((lang) => {
    const badge = LANG_BADGES[lang] || { text: lang.slice(0, 2), color: 'currentColor' };
    return { id: lang, label: lang, icon: `<span class="md-lang-badge" style="color:${badge.color}">${badge.text}</span>` };
  }),
);

// 光标前文本为 ```/~~~ 加部分语言名（无空格无换行）时返回补全上下文；否则 null
function fenceContext() {
  const el = currentEditable();
  if (!el || editingId.value === null) return null;
  const sel = window.getSelection();
  if (!sel.rangeCount || !sel.isCollapsed) return null;
  const caret = sel.getRangeAt(0);
  if (!el.contains(caret.startContainer)) return null;
  const anchorEl = caret.startContainer.nodeType === Node.TEXT_NODE ? caret.startContainer.parentElement : caret.startContainer;
  if (anchorEl?.closest('pre')) return null;
  const before = document.createRange();
  before.selectNodeContents(el);
  before.setEnd(caret.startContainer, caret.startOffset);
  const text = before.toString();
  const match = /^(`{3,}|~{3,})([a-zA-Z0-9#+_-]*)$/.exec(text);
  if (!match) return null;
  const rect = caret.getBoundingClientRect();
  const estimatedHeight = 270;
  const top = rect.bottom + estimatedHeight > window.innerHeight
    ? Math.max(8, rect.top - estimatedHeight)
    : rect.bottom + 6;
  return { fence: match[1], query: match[2], pos: { left: rect.left, top } };
}

async function updateLangMenu() {
  const ctx = fenceContext();
  if (!ctx) {
    langOpen.value = false;
    return;
  }
  if (!langList.value.length) {
    langList.value = await invoke('code_languages').catch(() => []);
  }
  langQuery.value = ctx.query;
  if (langIndex.value >= langItems.value.length) langIndex.value = 0;
  langPos.value = ctx.pos;
  langOpen.value = true;
}

// 应用语言补全：围栏后的查询文本替换为所选语言，光标落在语言之后
//（此时仍是普通文本行，继续输入或 Enter 经既有流程转为代码块）
function applyLangItem(lang) {
  const el = currentEditable();
  const ctx = fenceContext();
  if (!el || !ctx || !lang) return;
  langOpen.value = false;
  const after = el.textContent.slice((ctx.fence + ctx.query).length);
  const p = el.querySelector('p') || el;
  p.textContent = ctx.fence + lang + after;
  placeCaretAtTextOffset(p, (ctx.fence + lang).length);
}
// 滚动定位闪烁的块 id / 当前滚动位置对应的标题 id（供大纲高亮）
const flashId = ref(null);
const activeHeadingId = ref(null);
// WYSIWYG 滚动容器（大纲定位用）
const scrollRoot = ref(null);
// 源码模式滚动容器
const sourceRoot = ref(null);
// 切换源码模式时保存的滚动锚点：content 字符偏移 + 元素/行距视口顶部的距离
let savedAnchorOffset = 0;
let savedTopMargin = 0;
// 切回 WYSIWYG 后需要在光标放置完成时再微调滚动，使焦点与切换前相对窗口位置一致
let restoreCaretAdjustment = null;
// 文档所在目录（粘贴图片的保存基准，App provide）
const documentDir = inject('documentDir', { value: null });
// 正在编辑的块 id；'__append__' 表示在文末追加新块
const editingId = ref(null);
// 换块/提交时关闭斜杠命令菜单与语言补全菜单
watch(editingId, () => {
  slashOpen.value = false;
  langOpen.value = false;
});
// 提交/重解析进行中时忽略点击，避免用过期的块区间切片
const syncing = ref(false);
const wordCount = ref(0);
const charCount = ref(0);
const lineCount = ref(1);

const cursorLine = ref(1);
const cursorColumn = ref(1);
let parseSeq = 0;
// 当前编辑容器元素
let editableEl = null;

// 当前编辑容器（引用丢失时从 DOM 恢复，保证提交/拆分链路不被静默早退）
function currentEditable() {
  if (editableEl && editableEl.isConnected) return editableEl;
  const el = scrollRoot.value?.querySelector('[contenteditable="true"]');
  if (el) editableEl = el;
  return editableEl;
}

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

// 源码模式不解析；切回所见即所得时立即解析一次并在解析完成后恢复滚动位置
watch(() => props.sourceMode, async (source) => {
  if (!source) await reparse();
  restoreScrollPosition();
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
// 意外重挂载防护：非提交路径的卸载（如父级重渲染重建容器）会暂存未提交内容，
// 同一编辑目标重挂载时恢复——否则 setEditableEl 会用初始 HTML 覆盖用户/命令写入的内容。
let stashedEdit = null; // { id, html }：意外卸载时暂存的编辑内容与目标
function setEditableEl(el) {
  if (!el) {
    // 卸载回调（可能后于新容器挂载执行）：旧容器已脱离文档才暂存并清空
    if (editableEl && !editableEl.isConnected) {
      stashedEdit = stashedEdit ?? { id: editingId.value, html: editableEl.innerHTML };
      editableEl = null;
    }
    return;
  }
  // 同一容器的 ref 重复触发（Vue 对 v-if 分支补丁时会重设函数 ref）：
  // 直接跳过——否则无条件重填初始 HTML 会把未提交内容原地抹掉
  if (el === editableEl) return;
  // 挂载回调：「先挂新、后卸旧」的替换中，旧容器仍在文档且持未提交内容，先暂存
  if (editableEl && editableEl !== el && editableEl.isConnected) {
    stashedEdit = { id: editingId.value, html: editableEl.innerHTML };
  }
  editableEl = el;
  const block = blocks.value.find((b) => b.id === editingId.value);
  // 原子/保留类块按原始 Markdown 切片编辑，其余按渲染 HTML 就地编辑
  const rawSource = block && block.start != null ? content.value.slice(block.start, block.end) : '';
  if (stashedEdit && stashedEdit.id === editingId.value) {
    el.innerHTML = stashedEdit.html;
  } else {
    el.innerHTML = editingId.value === '__append__' || !block ? emptyParagraphHtml() : blockToHtml(block, rawSource);
  }
  stashedEdit = null;
  nextTick(() => {
    // 换块 blur 抑制到此为止（无论是否成功聚焦）
    suppressBlurCommit = false;
    // 期间用户点击切换了编辑目标，或元素已被卸载，则放弃聚焦
    if (editableEl !== el || !el.isConnected) {
      pendingCaretOffset = null;
      pendingPreciseRawOffset = null;
      return;
    }
    el.focus({ preventScroll: true });
    if (pendingPreciseRawOffset != null) {
      // 源码 → WYSIWYG 切换：块内源码偏移经「序列化对齐」精确换算为 DOM 位置
      const target = pendingPreciseRawOffset;
      pendingPreciseRawOffset = null;
      markdownOffsetToPlainOffset(el, target).then((plain) => {
        if (editableEl === el && el.isConnected) placeCaretAtTextOffset(el, plain);
      });
    } else if (pendingCaretOffset != null) {
      // 合并两块后：光标落在上一块原文末尾（接缝处）
      const offset = pendingCaretOffset;
      pendingCaretOffset = null;
      placeCaretAtTextOffset(el, offset);
    } else if (cursorAtStart) {
      cursorAtStart = false;
      placeCursorAtStart(el);
    } else {
      placeCursorAtEnd(el);
    }
    // 源码/WYSIWYG 切换后：光标已放置，把光标滚动到窗口中间。
    // 含图片时等待图片加载完成后再重算一次，避免布局变化导致光标错位。
    if (restoreCaretAdjustment) {
      restoreCaretAdjustment = null;
      nextTick(() => {
        requestAnimationFrame(() => adjustCaretToViewportCenter());
        waitForImages(scrollRoot.value).then(() => {
          requestAnimationFrame(() => adjustCaretToViewportCenter());
        });
      });
    }
  });
}

// 等待 root 内所有图片加载完成（包含已完成/加载失败）
function waitForImages(root) {
  if (!root) return Promise.resolve();
  const imgs = root.querySelectorAll('img');
  if (!imgs.length) return Promise.resolve();
  return Promise.all(
    [...imgs].map((img) =>
      img.complete
        ? Promise.resolve()
        : new Promise((resolve) => {
            img.addEventListener('load', resolve, { once: true });
            img.addEventListener('error', resolve, { once: true });
          }),
    ),
  );
}

// 把光标滚动到窗口中间
function adjustCaretToViewportCenter() {
  const sel = window.getSelection();
  const root = scrollRoot.value;
  if (!sel?.rangeCount || !root) return;
  const rect = sel.getRangeAt(0).getBoundingClientRect();
  const rootRect = root.getBoundingClientRect();
  const viewportHeight = root.clientHeight;
  root.scrollTop += rect.top - rootRect.top - viewportHeight / 2;
}

// setEditableEl 的下一次挂载把光标放在内容开头（Enter 拆分进入新块）
let cursorAtStart = false;
// 下一次挂载把光标放在指定纯文本偏移（块首退格合并两块）；优先于 cursorAtStart
let pendingCaretOffset = null;
// 下一次挂载把源码偏移经序列化对齐精确换算为 DOM 位置（源码 → WYSIWYG 切换）
let pendingPreciseRawOffset = null;

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
  if (editingId.value === null || !currentEditable()) return;
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
  const el = currentEditable();
  if (editingId.value === null || !el) return;
  syncing.value = true;
  try {
    const sel = window.getSelection();
    let beforeDiv;
    let afterDiv;
    if (sel.rangeCount && el.contains(sel.getRangeAt(0).startContainer)) {
      // 以光标为界克隆前后两段 DOM
      const range = sel.getRangeAt(0);
      const beforeRange = range.cloneRange();
      beforeRange.selectNodeContents(el);
      beforeRange.setEnd(range.startContainer, range.startOffset);
      const afterRange = range.cloneRange();
      afterRange.selectNodeContents(el);
      afterRange.setStart(range.endContainer, range.endOffset);
      beforeDiv = document.createElement('div');
      beforeDiv.append(beforeRange.cloneContents());
      afterDiv = document.createElement('div');
      afterDiv.append(afterRange.cloneContents());
    } else {
      // 无有效光标（内容删空后浏览器丢失选区）：按块尾拆分，前段为整块内容
      beforeDiv = document.createElement('div');
      beforeDiv.innerHTML = el.innerHTML;
      afterDiv = document.createElement('div');
    }

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

// 块的纯文本（标题行 + 子块递归），用于块首退格合并后的光标接缝定位
function blockPlainText(b) {
  return plainText(b.title) + (b.children || []).map(blockPlainText).join('');
}

// 进入指定块编辑并把光标放在末尾（程序性换块走 setEditableEl 挂载链路）
function focusBlockAtEnd(id) {
  suppressBlurCommit = true;
  cursorAtStart = false;
  editingId.value = id;
}

// 非空块开头 + Backspace（Typora 式两步退格）：
// 标题/引用/列表项等样式块先降级为段落；段落并入上一文本块（块类型沿用上块）；
// 上一块是分割线则删除之；原子块/复杂结构只提交当前编辑并把光标移到上一块末尾。
async function backspaceAtBlockStart() {
  const el = currentEditable();
  if (editingId.value === null || !el || syncing.value) return;
  const TEXTLIKE = ['paragraph', 'heading', 'quote', 'bulletedListItem', 'numberedListItem', 'taskListItem'];
  const isAppend = editingId.value === '__append__';
  const index = isAppend ? blocks.value.length : blocks.value.findIndex((b) => b.id === editingId.value);
  if (index < 0) return;
  const dtos = domToBlockDtos(el);
  const single = dtos.length === 1 ? dtos[0] : null;

  // 样式块（标题/引用/列表项）：第一次退格降级为段落，嵌套子块提升为顶层
  //（仅改编辑态 DOM，提交时落源）
  if (single && single.type !== 'paragraph' && TEXTLIKE.includes(single.type)) {
    demoteEditableToParagraph(el, single);
    return;
  }

  const prev = blocks.value[index - 1];
  if (!prev) return; // 首块段落：前面没有可并入的块

  // 上一块是分割线：删除分割线，当前块保持编辑（光标仍在开头）
  if (prev.type === 'separator') {
    removePrevSeparator(index - 1);
    return;
  }

  // 当前块为单段落且上一块为文本类：并入上一块，光标落在接缝处
  if (single && single.type === 'paragraph' && TEXTLIKE.includes(prev.type)) {
    await mergeIntoPrevBlock(single, index, isAppend);
    return;
  }

  // 其余（原子块/嵌套结构）：提交当前编辑，光标移到上一块末尾
  await commitEdit();
  focusBlockAtEnd(prev.id);
}

// 删除上一块分割线（含相邻换行）并平移后续偏移；当前编辑块不受影响
function removePrevSeparator(prevIndex) {
  const prev = blocks.value[prevIndex];
  if (!prev) return;
  let removeStart = prev.start;
  let removeEnd = prev.end;
  if (content.value[removeEnd] === '\n') removeEnd += 1;
  else if (removeStart > 0 && content.value[removeStart - 1] === '\n') removeStart -= 1;
  const removedLen = removeEnd - removeStart;
  content.value = content.value.slice(0, removeStart) + content.value.slice(removeEnd);
  blocks.value.splice(prevIndex, 1);
  for (let i = prevIndex; i < blocks.value.length; i++) {
    const b = blocks.value[i];
    if (b.start != null) {
      b.start -= removedLen;
      b.end -= removedLen;
    }
  }
  publishBlocks();
}

// 当前段落块并入上一文本块：合并源 = 上块源 + 当前行内 Markdown（上块类型保留），
// 重解析合并区间并平移后续偏移，光标落在上块原文末尾（接缝处）
async function mergeIntoPrevBlock(dto, index, isAppend) {
  syncing.value = true;
  try {
    const prev = blocks.value[index - 1];
    const curEnd = isAppend ? content.value.length : blocks.value[index].end;
    const curMd = await invoke('serialize_markdown', { blocks: [paragraphDtoFromTree(dto.title)] });
    const prevSource = content.value.slice(prev.start, prev.end);
    // 合并规则（接缝处理）由 Rust 统一维护，前端只做机械区间替换
    const combined = await invoke('merge_block_markdown', { prevSource, appendedMarkdown: curMd });
    const junction = blockPlainText(prev).length;
    editableEl = null;
    content.value = content.value.slice(0, prev.start) + combined + content.value.slice(curEnd);
    const replacements = await parseAnchoredBlocks(combined, prev.start);
    const delta = combined.length - (curEnd - prev.start);
    blocks.value.splice(index - 1, isAppend ? 1 : 2, ...replacements);
    for (let i = index - 1 + replacements.length; i < blocks.value.length; i++) {
      const b = blocks.value[i];
      if (b.start != null) {
        b.start += delta;
        b.end += delta;
      }
    }
    publishBlocks();
    const merged = replacements[0];
    suppressBlurCommit = true;
    pendingCaretOffset = junction;
    editingId.value = merged ? merged.id : null;
    if (!merged) {
      suppressBlurCommit = false;
      pendingCaretOffset = null;
    }
  } finally {
    syncing.value = false;
  }
}

// 空块 + Backspace：删除当前块（含源码区间与相邻换行），光标移到上一块末尾；
// 首块为空则移到下一块开头；文档无块则落到追加区（Typora 式连续退格删除）。
async function deleteEmptyBlockAndFocusPrev() {
  if (editingId.value === null) return;
  syncing.value = true;
  try {
    let focusId = null;
    let focusAtStart = false;
    if (editingId.value === '__append__') {
      // 追加区为空：退格跳到末尾块
      const last = blocks.value[blocks.value.length - 1];
      if (last) {
        focusId = last.id;
      } else {
        // 文档无可跳块：追加区重置为空段落。editingId 不变时 Vue 不会重挂载容器，
        // 不重置则快捷转换出的空标题等元素会一直残留（表现为删空后仍是标题渲染）
        const el = currentEditable();
        if (el) {
          el.innerHTML = emptyParagraphHtml();
          placeCursorAtEnd(el);
        }
        return;
      }
    } else {
      const index = blocks.value.findIndex((b) => b.id === editingId.value);
      const oldBlock = blocks.value[index];
      editableEl = null;
      if (oldBlock) {
        // 删除块源码及其行尾换行（空段落块为零长源码，删除对应空行）
        let removeStart = oldBlock.start;
        let removeEnd = oldBlock.end;
        if (content.value[removeEnd] === '\n') removeEnd += 1;
        else if (removeStart > 0 && content.value[removeStart - 1] === '\n') removeStart -= 1;
        const removedLen = removeEnd - removeStart;
        content.value = content.value.slice(0, removeStart) + content.value.slice(removeEnd);
        blocks.value.splice(index, 1);
        for (let i = index; i < blocks.value.length; i++) {
          const b = blocks.value[i];
          if (b.start != null) {
            b.start -= removedLen;
            b.end -= removedLen;
          }
        }
        publishBlocks();
        const prev = blocks.value[index - 1];
        if (prev) focusId = prev.id;
        else if (blocks.value.length) {
          focusId = blocks.value[0].id;
          focusAtStart = true;
        }
      }
    }
    editableEl = null;
    suppressBlurCommit = true;
    editingId.value = focusId ?? '__append__';
    cursorAtStart = focusAtStart;
    if (!focusId) suppressBlurCommit = false;
  } finally {
    syncing.value = false;
  }
}

// 代码块/Mermaid 等 pre 编辑的 Ctrl+Enter：整块提交渲染（不拆分），
// 并在其后新建空段落进入编辑。普通 Enter 在 pre 内插入换行（见 keydown 路由）。
async function commitCodeAndNewBlock() {
  if (editingId.value === null || !currentEditable()) return;
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
  updateSlashMenu();
  updateLangMenu();
}

// 粘贴图片：按偏好保存到文档目录/assets 子目录并插入相对路径引用（未保存文档跳过）
async function onEditablePaste(e) {
  const items = Array.from(e.clipboardData?.items || []);
  const imageItem = items.find((item) => item.type.startsWith('image/'));
  if (!imageItem) return;
  e.preventDefault();
  const behavior = getPref('image_paste_behavior');
  if (behavior === 'off') return;
  const baseDir = documentDir.value;
  if (!baseDir) return;
  const file = imageItem.getAsFile();
  if (!file) return;
  const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
  const ext = imageItem.type.split('/')[1] || 'png';
  const subDir = behavior === 'assets' ? 'assets' : null;
  const relPath = await invoke('save_pasted_image', { bytes, baseDir, subDir, extension: ext }).catch(() => null);
  if (relPath) insertTextAtCursor(`![](${relPath})`);
}

function onEditableKeydown(e) {
  if (e.isComposing) return;
  // 斜杠菜单打开时优先路由：上下选择、Enter 应用、Esc 关闭
  if (slashOpen.value) {
    const items = slashItems.value;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const item = items[slashIndex.value];
      // 表格行且已进入行/列字段：↑/↓ 增减数量；字段为表格项本身时照常菜单导航
      if (item?.id === 'table' && slashTableField.value !== 'item') {
        const delta = e.key === 'ArrowUp' ? 1 : -1;
        if (slashTableField.value === 'rows') {
          slashTableRows.value = Math.min(50, Math.max(1, slashTableRows.value + delta));
        } else {
          slashTableCols.value = Math.min(20, Math.max(1, slashTableCols.value + delta));
        }
        return;
      }
      if (items.length) {
        // 循环导航（到底/到顶回绕），滚动跟随由菜单面板负责
        slashIndex.value =
          e.key === 'ArrowDown'
            ? (slashIndex.value + 1) % items.length
            : (slashIndex.value - 1 + items.length) % items.length;
      }
      return;
    }
    // 行内 ←/→：文本行移动徽章 / 表格在 项→行→列 字段间循环
    if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
      const item = items[slashIndex.value];
      if (item?.id === 'text') {
        e.preventDefault();
        slashTextIndex.value =
          e.key === 'ArrowRight'
            ? Math.min(SLASH_TEXT_BADGES.length - 1, slashTextIndex.value + 1)
            : Math.max(0, slashTextIndex.value - 1);
        return;
      }
      if (item?.id === 'table') {
        e.preventDefault();
        const fields = ['item', 'rows', 'cols'];
        const step = e.key === 'ArrowRight' ? 1 : -1;
        const next = (fields.indexOf(slashTableField.value) + step + fields.length) % fields.length;
        slashTableField.value = fields[next];
        return;
      }
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      applySlashItem(items[slashIndex.value]);
      return;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      slashOpen.value = false;
      return;
    }
  }
  // 围栏语言补全打开时：↑/↓ 选择（循环回绕）、Enter/Tab 补全、Esc 关闭
  if (langOpen.value) {
    const list = langItems.value;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (list.length) {
        langIndex.value =
          e.key === 'ArrowDown'
            ? (langIndex.value + 1) % list.length
            : (langIndex.value - 1 + list.length) % list.length;
      }
      return;
    }
    if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault();
      if (list.length) applyLangItem(list[langIndex.value]);
      return;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      langOpen.value = false;
      return;
    }
  }
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
    const editable = currentEditable();
    // 列表项内 Enter：非空项拆出同类型新列表项续写；空项退出列表转为段落（Typora 式）
    if (editable && handleListEnter(editable)) return;
    // 围栏行（```lang）按 Enter：转换为代码块并进入块内编辑，不做拆分
    if (editable) {
      convertFenceToCodeBlock(editable).then((converted) => {
        if (converted) return;
        // `<section>` 行按 Enter：补全闭合标签，进入图文排版块原文编辑
        convertSectionToHtmlBlock(editable).then((section) => {
          if (!section) splitAndCommit();
        });
      });
    } else {
      splitAndCommit();
    }
  } else if (e.key === 'Backspace') {
    if (e.ctrlKey || e.metaKey || e.shiftKey || !currentEditable()) return;
    // 块内容已删空：Backspace 删除该块并跳到上一块
    if (editableEl.textContent.replace(/\n/g, '') === '') {
      e.preventDefault();
      deleteEmptyBlockAndFocusPrev();
      return;
    }
    // 光标紧贴任务勾选框之后：删除勾选框，任务项降为普通列表项
    //（浏览器原生退格无法越过 contenteditable=false 的勾选框，不拦截则无反应）
    if (deleteTaskCheckboxBeforeCaret(editableEl)) {
      e.preventDefault();
      return;
    }
    // 光标在块开头：样式块降级为段落，段落并入上一块（Typora 式退格）；
    // 其余位置交给浏览器原生删除
    if (isCaretAtStart(editableEl)) {
      e.preventDefault();
      backspaceAtBlockStart();
    }
  } else if (e.key === 'Tab') {
    // Tab 默认行为是移出焦点，必须拦截：代码块内插入制表符；
    // 列表项内 Tab 缩进、Shift+Tab 取消缩进（Typora 式）
    e.preventDefault();
    const editable = currentEditable();
    if (!editable) return;
    const sel = window.getSelection();
    const anchor = sel.rangeCount ? sel.anchorNode : null;
    const el = anchor ? (anchor.nodeType === Node.TEXT_NODE ? anchor.parentElement : anchor) : null;
    if (el?.closest('pre')) {
      insertTextAtCursor('\t');
      return;
    }
    handleListTab(editable, e.shiftKey);
  } else if (e.key === 'Escape') {
    e.preventDefault();
    cancelEdit();
  }
}


// 任务列表勾选：标记替换由 Rust 完成（替换根块源码中第 occurrence 个 [ ]/[x]），
// 再增量重解析该根块。嵌套任务项没有自身源码区间：按块树 DFS 前序定位其根块与序号。
async function toggleTask(block) {
  if (block.type !== 'taskListItem') return;
  let rootBlock = block;
  let occurrence = 0;
  if (block.start == null) {
    const hit = locateTaskInRoot(block.id);
    if (!hit) return;
    rootBlock = hit.root;
    occurrence = hit.occurrence;
  }
  const index = blocks.value.findIndex((b) => b.id === rootBlock.id);
  if (index < 0) return;
  const source = content.value.slice(rootBlock.start, rootBlock.end);
  const next = await invoke('toggle_task_markdown', { source, checked: !block.checked, occurrence });
  if (next === source) return;
  content.value = content.value.slice(0, rootBlock.start) + next + content.value.slice(rootBlock.end);
  await reparseRegion(index, rootBlock, next);
}

// 在根块树中定位任务项：返回其根块与 DFS 前序序号（目标为第 occurrence 个任务标记）
function locateTaskInRoot(id) {
  for (const root of blocks.value) {
    let occurrence = 0;
    let found = false;
    const dfs = (node) => {
      if (found) return;
      if (node.type === 'taskListItem') {
        if (node.id === id) {
          found = true;
          return;
        }
        occurrence += 1;
      }
      for (const child of node.children || []) dfs(child);
    };
    dfs(root);
    if (found) return { root, occurrence };
  }
  return null;
}

// ---------- 大纲联动 ----------

// 滚动到指定脚注定义（底部集中区域）并短暂闪烁高亮
function scrollToFootnote(id) {
  const root = scrollRoot.value;
  if (!root) return;
  const el = root.querySelector(`[data-footnote-def="${id}"]`);
  if (!el) return;
  el.scrollIntoView({ behavior: 'smooth', block: 'center' });
  flashId.value = `fn-def-${id}`;
  setTimeout(() => {
    if (flashId.value === `fn-def-${id}`) flashId.value = null;
  }, 1200);
}

// 从脚注定义返回对应引用位置
function scrollToFootnoteRef(refIndex) {
  const root = scrollRoot.value;
  if (!root) return;
  const el = root.querySelector(`[data-footnote-ref="${refIndex}"]`);
  if (!el) return;
  el.scrollIntoView({ behavior: 'smooth', block: 'center' });
}

// 捕获当前编辑器的滚动锚点（content 字符偏移 + 视口顶部偏移），
// 用于源码/WYSIWYG 切换后尽可能保持同一文档位置。
function getLineStartBefore(text, offset) {
  let i = Math.max(0, Math.min(offset, text.length));
  while (i > 0 && text[i - 1] !== '\n') i--;
  return i;
}

// 两个字符串的最长公共前缀长度（UTF-16 码元数，与 Rust 端偏移约定一致）
function lcpLength(a, b) {
  const n = Math.min(a.length, b.length);
  let i = 0;
  while (i < n && a[i] === b[i]) i++;
  return i;
}

// 编辑态光标 → 源码精确偏移的原理（序列化对齐）：光标前 DOM 片段经
// serialize_markdown 序列化后，与整块序列化求最长公共前缀——前缀终点即
// 光标在块源码中的精确位置（前后两部分序列化在光标前必然一致，光标后开始分叉）。
// 块内 Markdown 源码偏移 → 编辑态 DOM 纯文本偏移（精确）：本地收集每个文本节点
// 结束位置的「光标前片段」DTO，经 Rust lcp_offsets 一次批量求出各位置源码偏移
//（代替逐节点往返调用），命中目标后按字符细化（节点内文本与源码基本 1:1）。
async function markdownOffsetToPlainOffset(el, target) {
  const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
  const fullDtos = domToBlockDtos(el);
  const beforeParts = [];
  const nodes = [];
  let node;
  while ((node = walker.nextNode())) {
    const range = document.createRange();
    range.selectNodeContents(el);
    range.setEnd(node, node.textContent.length);
    const div = document.createElement('div');
    div.append(range.cloneContents());
    beforeParts.push(domToBlockDtos(div));
    nodes.push(node);
  }
  const mdOffs = await invoke('lcp_offsets', { fullBlocks: fullDtos, beforeParts }).catch(() => []);
  let plain = 0;
  let prevMdOff = 0;
  for (let i = 0; i < nodes.length; i++) {
    const mdOff = mdOffs[i] ?? 0;
    if (mdOff >= target) {
      const delta = target - prevMdOff;
      return plain + Math.min(Math.max(0, delta), nodes[i].textContent.length);
    }
    plain += nodes[i].textContent.length;
    prevMdOff = mdOff;
  }
  return plain;
}

// raw Markdown 源码偏移 → 编辑态 pre 中 escapeHtml 后的 DOM 文本偏移
function rawOffsetToDomOffset(rawSource, rawOffset) {
  let domOffset = 0;
  const limit = Math.max(0, Math.min(rawOffset, rawSource.length));
  for (let i = 0; i < limit; i++) {
    const c = rawSource[i];
    if (c === '&') domOffset += 5;
    else if (c === '<') domOffset += 4;
    else if (c === '>') domOffset += 4;
    else if (c === '"') domOffset += 6;
    else domOffset += 1;
  }
  return domOffset;
}

const RAW_BLOCK_TYPES = new Set([
  'codeBlock',
  'table',
  'mathBlock',
  'mermaidBlock',
  'htmlBlock',
  'sectionBlock',
  'comment',
  'rawMarkdown',
  'footnoteDefinition',
]);

// 计算 textarea 内指定偏移光标的精确纵向位置（内容坐标）：
// 用同宽同字体的隐藏镜像 div 测量——长段落折行后「硬换行数 × 行高」会严重偏小，
// 镜像测量把折行也算进去（窗口越窄折行越多，估算误差越大）
function caretContentTopInTextarea(source, offset) {
  const style = window.getComputedStyle(source);
  const mirror = document.createElement('div');
  mirror.style.cssText =
    'position:absolute;visibility:hidden;white-space:pre-wrap;overflow-wrap:break-word;box-sizing:border-box;' +
    `width:${source.clientWidth}px;` +
    `font-family:${style.fontFamily};font-size:${style.fontSize};font-weight:${style.fontWeight};` +
    `line-height:${style.lineHeight};letter-spacing:${style.letterSpacing};` +
    `padding:${style.padding};border-width:${style.borderWidth};tab-size:${style.tabSize};`;
  mirror.textContent = source.value.slice(0, offset);
  const marker = document.createElement('span');
  marker.textContent = '​';
  mirror.append(marker);
  document.body.append(mirror);
  const top = marker.offsetTop;
  mirror.remove();
  return top;
}

async function captureScrollPosition() {
  const source = sourceRoot.value;
  const wysiwyg = scrollRoot.value;
  if (source) {
    // 源码模式：textarea 的 selectionStart 即精确 UTF-16 偏移
    savedAnchorOffset = source.selectionStart;
    const style = window.getComputedStyle(source);
    const lineHeight = parseFloat(style.lineHeight);
    const textBefore = source.value.slice(0, savedAnchorOffset);
    const topLine = lineHeight ? (textBefore.match(/\n/g) || []).length : 0;
    savedTopMargin = source.scrollTop - topLine * lineHeight;
  } else if (wysiwyg) {
    const root = wysiwyg;
    const el = currentEditable();
    // 编辑态：先按「序列化对齐」求光标的精确源码偏移，再提交当前编辑
    //（保证源码视图内容与编辑器一致；偏移在提交后的 content 中依然成立）
    if (el && editingId.value !== null) {
      const sel = window.getSelection();
      if (sel.rangeCount && el.contains(sel.anchorNode)) {
        const range = document.createRange();
        range.selectNodeContents(el);
        range.setEnd(sel.anchorNode, sel.anchorOffset);
        const beforeDiv = document.createElement('div');
        beforeDiv.append(range.cloneContents());
        const beforeMd = await invoke('serialize_markdown', { blocks: domToBlockDtos(beforeDiv) }).catch(() => '');
        if (editingId.value === '__append__') {
          const fullMd = (await invoke('serialize_markdown', { blocks: domToBlockDtos(el) }).catch(() => '')).trim();
          const base = content.value ? content.value.replace(/\s*$/, '') : '';
          const anchor = base ? base.length + 2 : 0;
          savedAnchorOffset = fullMd ? anchor + lcpLength(beforeMd, fullMd) : base.length;
        } else {
          const block = blocks.value.find((b) => b.id === editingId.value);
          const fullMd = await invoke('serialize_markdown', { blocks: domToBlockDtos(el) }).catch(() => '');
          savedAnchorOffset = block && block.start != null ? block.start + lcpLength(beforeMd, fullMd) : 0;
        }
        const rect = sel.getRangeAt(0).getBoundingClientRect();
        savedTopMargin = rect.top - root.getBoundingClientRect().top;
        await commitEdit();
        return;
      }
    }
    // 无编辑态时回退到视口顶部块
    const els = root.querySelectorAll('[data-block-id]');
    let anchorEl = null;
    for (const el2 of els) {
      if (el2.offsetTop >= root.scrollTop) {
        anchorEl = el2;
        break;
      }
    }
    if (!anchorEl && els.length) anchorEl = els[els.length - 1];
    const blockId = anchorEl?.getAttribute('data-block-id');
    const block = blocks.value.find((b) => b.id === blockId);
    savedAnchorOffset = block?.start != null ? getLineStartBefore(content.value, block.start) : 0;
    savedTopMargin = anchorEl ? anchorEl.offsetTop - root.scrollTop : 0;
  } else {
    savedAnchorOffset = 0;
    savedTopMargin = 0;
  }
}

function restoreScrollPosition() {
  nextTick(() => {
    requestAnimationFrame(() => {
      const source = sourceRoot.value;
      const wysiwyg = scrollRoot.value;
      if (source) {
        // 镜像测量光标真实纵坐标（含折行），再居中；先滚动后放选区避免原生滚动抢占
        const caretY = caretContentTopInTextarea(source, savedAnchorOffset);
        const targetTop = Math.max(0, caretY - source.clientHeight / 2);
        source.scrollTop = targetTop;
        source.focus({ preventScroll: true });
        source.setSelectionRange(savedAnchorOffset, savedAnchorOffset);
        setTimeout(() => {
          if (source.isConnected) source.scrollTop = targetTop;
        }, 0);
      } else if (wysiwyg) {
        const root = wysiwyg;
        const block =
          blocks.value.find((b) => b.start != null && b.start <= savedAnchorOffset && (b.end ?? Infinity) >= savedAnchorOffset) ??
          blocks.value.find((b) => b.start != null && b.start >= savedAnchorOffset);
        if (!block) return;
        const el = root.querySelector(`[data-block-id="${block.id}"]`);
        if (!el) return;
        // 初始把目标块大致放到视口中央，随后 setEditableEl 会把光标精确居中
        root.scrollTop = el.offsetTop - root.clientHeight / 2;
        // 切回 WYSIWYG 后自动进入锚点块编辑态并放置光标，
        // 后续在 setEditableEl 中再把光标精确滚动到窗口中间。
        suppressBlurCommit = true;
        editingId.value = block.id;
        cursorAtStart = false;
        restoreCaretAdjustment = true;
        const rawOffset = Math.max(0, savedAnchorOffset - block.start);
        if (RAW_BLOCK_TYPES.has(block.type)) {
          // 原子/原文块：raw 源码与 DOM 文本 escape 换算后 1:1（精确）
          const rawSource = content.value.slice(block.start, block.end);
          pendingCaretOffset = rawOffsetToDomOffset(rawSource, rawOffset);
        } else {
          // 富文本块：交给 setEditableEl 经「序列化对齐」精确换算（见 pendingPreciseRawOffset）
          pendingPreciseRawOffset = rawOffset;
        }
      }
    });
  });
}

provide('scrollToFootnote', (id) => scrollToFootnote(id));
provide('scrollToFootnoteRef', (refIndex) => scrollToFootnoteRef(refIndex));

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

// 加载新文档（打开/新建文件）：替换全文并整树重解析，退出当前编辑态
async function loadDocument(text) {
  editingId.value = null;
  editableEl = null;
  content.value = text;
  savedContent.value = text;
  await reparse();
}

// 保存/另存：返回当前全文；保存成功后同步快照（清除脏标记）
function getContent() {
  return content.value;
}
function markSaved() {
  savedContent.value = content.value;
}

defineExpose({ scrollToBlock, loadDocument, getContent, markSaved, isDirty: () => isDirty.value, captureScrollPosition, restoreScrollPosition });
</script>

<template>
  <div class="flex h-full flex-col">
    <textarea
      v-if="sourceMode"
      ref="sourceRoot"
      class="t-root flex-1 resize-none border-none font-mono outline-none"
      placeholder="开始写作..."
      v-model="content"
      @input="onInput"
      @keyup="onInput"
      @click="onInput"
    ></textarea>

    <div v-else ref="scrollRoot" class="t-root flex-1 overflow-y-auto" @scroll.passive="onEditorScroll">
      <div class="t-measure">
      <template v-for="block in renderedBlocks" :key="block.id">
        <!-- Typora 式就地编辑：渲染后的内容直接在 contenteditable 中编辑 -->
        <div
          v-if="editingId === block.id"
          :ref="setEditableEl"
          class="md-editing px-1 outline-none"
          contenteditable="true"
          spellcheck="false"
          @input="onEditableInput"
          @paste="onEditablePaste"
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
          @paste="onEditablePaste"
        @keydown="onEditableKeydown"
        @blur="onEditableBlur"
      ></div>
      <div v-else class="min-h-24 cursor-text p-4" @click="startAppend">
        <span v-if="renderedBlocks.length === 0" class="t-dim px-1">开始写作...</span>
      </div>

      <!-- 脚注定义集中显示在文档末尾，支持点击引用跳转与返回 -->
      <div v-if="footnotes.length" class="md-footnotes">
        <div class="md-footnotes-title">脚注</div>
        <div
          v-for="block in footnotes"
          :key="block.id"
          class="md-block cursor-text px-1"
          :class="{ 'md-flash': flashId === 'fn-def-' + plainText(block.title) }"
          :data-block-id="block.id"
          @click="startEdit(block)"
        >
          <BlockView :block="block" :ordinal="1" @toggle-task="toggleTask" />
        </div>
      </div>
      </div>
    </div>
  </div>
  <!-- 斜杠命令菜单 / 围栏语言补全菜单（同一面板，互斥触发）：Teleport 移出根布局（多根片段），其挂载/卸载不再引起
       布局子节点重建；.t-app 包装使主题 blocks 规则（.t-app 作用域）同样生效 -->
  <Teleport to="body">
    <div v-if="slashOpen || langOpen" class="t-app">
      <SlashMenu
        :items="langOpen ? langMenuItems : slashItems"
        :index="langOpen ? langIndex : slashIndex"
        :text-index="slashTextIndex"
        :table-rows="slashTableRows"
        :table-cols="slashTableCols"
        :table-field="slashTableField"
        :left="langOpen ? langPos.left : slashPos.left"
        :top="langOpen ? langPos.top : slashPos.top"
        @pick="(item, option) => (langOpen ? applyLangItem(item.id) : applySlashItem(item, option))"
        @hover="(i) => (langOpen ? (langIndex = i) : (slashIndex = i))"
        @text-index="(t) => (slashTextIndex = t)"
        @table-field="(f) => (slashTableField = f)"
      />
    </div>
  </Teleport>
</template>
