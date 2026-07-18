<script setup>
import { ref, computed, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import BlockView from './BlockView.vue';
import {
  blockToHtml,
  editableToMarkdown,
  emptyParagraphHtml,
  applyMarkdownShortcuts,
  placeCursorAtEnd,
  insertTextAtCursor,
  insertLineBreakAtCursor,
} from '../utils/wysiwyg.js';

// 双模式编辑器：sourceMode 为 Markdown 原文编辑，否则为所见即所得（Typora 式就地编辑）。
// 全文 Markdown 字符串是唯一数据源；块树由 Rust 命令 parse_markdown 解析得到。
// 点击块进入编辑：块以渲染后的 HTML 放入 contenteditable 就地编辑，
// 输入 Markdown 标记（#、-、>、** 等）即时转换，提交时把 DOM 序列化回 Markdown。
const props = defineProps({
  sourceMode: { type: Boolean, default: false },
});

const emit = defineEmits(['update:stats']);

const content = ref('');
const blocks = ref([]);
// 正在编辑的块 id；'__append__' 表示在文末追加新块
const editingId = ref(null);
// 提交/重解析进行中时忽略点击，避免用过期的块区间切片
const syncing = ref(false);
const cursorLine = ref(1);
const cursorColumn = ref(1);
let parseSeq = 0;
// 当前 contenteditable 编辑容器元素
let editableEl = null;

const wordCount = computed(() => {
  const text = content.value.trim();
  return text ? text.split(/\s+/).length : 0;
});

const charCount = computed(() => content.value.length);

const lineCount = computed(() => {
  return content.value ? content.value.split('\n').length : 1;
});

watch([wordCount, charCount, lineCount, cursorLine, cursorColumn], () => {
  emit('update:stats', {
    wordCount: wordCount.value,
    charCount: charCount.value,
    lineCount: lineCount.value,
    cursorLine: cursorLine.value,
    cursorColumn: cursorColumn.value,
  });
}, { immediate: true });

// 源码模式不解析；切回所见即所得时立即解析一次
watch(() => props.sourceMode, (source) => {
  if (!source) reparse();
});

async function reparse() {
  const seq = ++parseSeq;
  try {
    const result = await invoke('parse_markdown', { markdown: content.value });
    if (seq === parseSeq) blocks.value = result;
  } catch (e) {
    console.error('parse_markdown 调用失败:', e);
  }
}

function onInput(e) {
  const textarea = e.target;
  const text = textarea.value.substring(0, textarea.selectionStart);
  cursorLine.value = (text.match(/\n/g) || []).length + 1;
  cursorColumn.value = text.length - text.lastIndexOf('\n');
}

// 编辑容器挂载时：填充渲染好的 HTML 并聚焦到内容末尾
function setEditableEl(el) {
  editableEl = el;
  if (!el) return;
  const block = blocks.value.find((b) => b.id === editingId.value);
  el.innerHTML = editingId.value === '__append__' || !block ? emptyParagraphHtml() : blockToHtml(block);
  el.focus();
  placeCursorAtEnd(el);
}

function startEdit(block) {
  if (syncing.value || editingId.value !== null) return;
  editingId.value = block.id;
}

function startAppend() {
  if (syncing.value || editingId.value !== null) return;
  editingId.value = '__append__';
}

async function commitEdit() {
  if (editingId.value === null || !editableEl) return;
  syncing.value = true;
  try {
    const md = editableToMarkdown(editableEl);
    editableEl = null;
    if (editingId.value === '__append__') {
      const text = md.trim();
      if (text) {
        content.value = content.value ? content.value.replace(/\s*$/, '') + '\n\n' + text : text;
      }
    } else {
      const block = blocks.value.find((b) => b.id === editingId.value);
      if (block) {
        content.value = content.value.slice(0, block.start) + md + content.value.slice(block.end);
      }
    }
    editingId.value = null;
    await reparse();
  } finally {
    syncing.value = false;
  }
}

function cancelEdit() {
  editingId.value = null;
  editableEl = null;
}

// 输入时应用 Markdown 快捷转换（输入法组合期间跳过）
function onEditableInput(e) {
  if (e.isComposing || e.inputType === 'insertCompositionText') return;
  applyMarkdownShortcuts();
}

function onEditableKeydown(e) {
  if (e.isComposing) return;
  if (e.key === 'Enter') {
    e.preventDefault();
    if (e.ctrlKey || e.metaKey) {
      commitEdit();
      return;
    }
    const sel = window.getSelection();
    const anchor = sel.rangeCount ? sel.anchorNode : null;
    const el = anchor ? (anchor.nodeType === Node.TEXT_NODE ? anchor.parentElement : anchor) : null;
    // 代码块内换行插入换行符；其余块 Shift+Enter 软换行，Enter 提交
    if (el && el.closest('pre')) {
      insertTextAtCursor('\n');
      return;
    }
    if (e.shiftKey) {
      insertLineBreakAtCursor();
      return;
    }
    commitEdit();
  } else if (e.key === 'Escape') {
    e.preventDefault();
    cancelEdit();
  }
}

// 任务列表勾选：把 markerOffset 处的 [ ]/[x] 替换后重新解析
async function toggleTask(item) {
  if (item.markerOffset == null) return;
  const offset = item.markerOffset;
  content.value = content.value.slice(0, offset) + (item.checked ? '[ ]' : '[x]') + content.value.slice(offset + 3);
  await reparse();
}
</script>

<template>
  <div class="flex h-full flex-col">
    <textarea
      v-if="sourceMode"
      class="flex-1 resize-none border-none bg-transparent p-4 font-mono text-[14px] leading-relaxed text-inherit outline-none"
      placeholder="开始写作..."
      v-model="content"
      @input="onInput"
      @keyup="onInput"
      @click="onInput"
    ></textarea>

    <div v-else class="flex-1 overflow-y-auto p-4 text-[14px] leading-relaxed">
      <template v-for="block in blocks" :key="block.id">
        <!-- Typora 式就地编辑：渲染后的内容直接在 contenteditable 中编辑 -->
        <div
          v-if="editingId === block.id"
          :ref="setEditableEl"
          class="rounded bg-black/[0.02] px-1 outline-none dark:bg-white/[0.03]"
          contenteditable="true"
          spellcheck="false"
          @input="onEditableInput"
          @keydown="onEditableKeydown"
          @blur="commitEdit"
        ></div>
        <div
          v-else
          class="cursor-text rounded px-1 transition-[background] duration-[0.08s] hover:bg-black/[0.03] dark:hover:bg-white/[0.04]"
          @click="startEdit(block)"
        >
          <BlockView :block="block" @toggle-task="toggleTask" />
        </div>
      </template>

      <div
        v-if="editingId === '__append__'"
        :ref="setEditableEl"
        class="rounded bg-black/[0.02] px-1 outline-none dark:bg-white/[0.03]"
        contenteditable="true"
        spellcheck="false"
        @input="onEditableInput"
        @keydown="onEditableKeydown"
        @blur="commitEdit"
      ></div>
      <div v-else class="min-h-24 cursor-text" @click="startAppend">
        <span v-if="blocks.length === 0" class="px-1 text-[#999] dark:text-[#777]">开始写作...</span>
      </div>
    </div>
  </div>
</template>
