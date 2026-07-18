<script setup>
import { ref, computed, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import BlockView from './BlockView.vue';

// 双模式编辑器：sourceMode 为 Markdown 原文编辑，否则为所见即所得（逐块编辑）。
// 全文 Markdown 字符串是唯一数据源；块树由 Rust 命令 parse_markdown 解析得到，
// 每个块带 start/end（UTF-16 码元偏移），块编辑 = 对全文做区间替换后重新解析。
const props = defineProps({
  sourceMode: { type: Boolean, default: false },
});

const emit = defineEmits(['update:stats']);

const content = ref('');
const blocks = ref([]);
// 正在编辑的块 id；'__append__' 表示在文末追加新块
const editingId = ref(null);
const draft = ref('');
// 提交/重解析进行中时忽略点击，避免用过期的块区间切片
const syncing = ref(false);
const cursorLine = ref(1);
const cursorColumn = ref(1);
let parseSeq = 0;

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

function startEdit(block) {
  if (syncing.value || editingId.value !== null) return;
  editingId.value = block.id;
  draft.value = content.value.slice(block.start, block.end);
}

function startAppend() {
  if (syncing.value || editingId.value !== null) return;
  editingId.value = '__append__';
  draft.value = '';
}

async function commitEdit() {
  if (editingId.value === null) return;
  syncing.value = true;
  try {
    if (editingId.value === '__append__') {
      const text = draft.value.trim();
      if (text) {
        content.value = content.value ? content.value.replace(/\s*$/, '') + '\n\n' + text : text;
      }
    } else {
      const block = blocks.value.find((b) => b.id === editingId.value);
      if (block) {
        content.value = content.value.slice(0, block.start) + draft.value + content.value.slice(block.end);
      }
    }
    editingId.value = null;
    draft.value = '';
    await reparse();
  } finally {
    syncing.value = false;
  }
}

function cancelEdit() {
  editingId.value = null;
  draft.value = '';
}

// 任务列表勾选：把 markerOffset 处的 [ ]/[x] 替换后重新解析
async function toggleTask(item) {
  if (item.markerOffset == null) return;
  const offset = item.markerOffset;
  content.value = content.value.slice(0, offset) + (item.checked ? '[ ]' : '[x]') + content.value.slice(offset + 3);
  await reparse();
}

// 编辑框自动聚焦
const vFocus = {
  mounted: (el) => el.focus(),
};
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
        <textarea
          v-if="editingId === block.id"
          v-focus
          v-model="draft"
          class="w-full resize-none rounded border border-black/15 bg-black/[0.03] p-2 font-mono text-[13px] leading-relaxed outline-none dark:border-white/20 dark:bg-white/5"
          :rows="Math.max(2, draft.split('\n').length)"
          placeholder="输入 Markdown..."
          @blur="commitEdit"
          @keydown.ctrl.enter="commitEdit"
          @keydown.esc="cancelEdit"
        ></textarea>
        <div
          v-else
          class="cursor-text rounded px-1 transition-[background] duration-[0.08s] hover:bg-black/[0.03] dark:hover:bg-white/[0.04]"
          @click="startEdit(block)"
        >
          <BlockView :block="block" @toggle-task="toggleTask" />
        </div>
      </template>

      <textarea
        v-if="editingId === '__append__'"
        v-focus
        v-model="draft"
        class="w-full resize-none rounded border border-black/15 bg-black/[0.03] p-2 font-mono text-[13px] leading-relaxed outline-none dark:border-white/20 dark:bg-white/5"
        :rows="Math.max(2, draft.split('\n').length)"
        placeholder="输入 Markdown..."
        @blur="commitEdit"
        @keydown.ctrl.enter="commitEdit"
        @keydown.esc="cancelEdit"
      ></textarea>
      <div v-else class="min-h-24 cursor-text" @click="startAppend">
        <span v-if="blocks.length === 0" class="px-1 text-[#999] dark:text-[#777]">开始写作...</span>
      </div>
    </div>
  </div>
</template>
