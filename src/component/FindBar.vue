<script setup vapor>
import { ref, watch, nextTick } from 'vue';

// 文档内查找/替换浮动条（Ctrl+F 查找 / Ctrl+H 替换）：纯展示与事件转发，
// 匹配定位/替换由 Editor 完成（searchInDocument/replaceInDocument）。
const props = defineProps({
  visible: { type: Boolean, default: false },
  // 匹配计数（{ index, total }，Editor 回传）
  match: { type: Object, default: () => ({ index: 0, total: 0 }) },
  // 初始是否展开替换行（Ctrl+H 打开时为 true）
  withReplace: { type: Boolean, default: false },
});
const emit = defineEmits(['close', 'search', 'nav', 'replace', 'replace-all']);

const keyword = ref('');
const replaceText = ref('');
const showReplace = ref(props.withReplace);
const inputEl = ref(null);

watch(
  () => props.visible,
  async (v) => {
    if (v) {
      showReplace.value = props.withReplace;
      await nextTick();
      inputEl.value?.focus();
      inputEl.value?.select();
    }
  },
  { immediate: true },
);

function onInput() {
  emit('search', keyword.value);
}
function onKeydown(e) {
  if (e.key === 'Enter') {
    e.preventDefault();
    emit('nav', e.shiftKey ? -1 : 1);
  } else if (e.key === 'Escape') {
    e.preventDefault();
    emit('close');
  }
}
</script>

<template>
  <Teleport to="body">
    <div v-if="visible" class="find-bar t-app" role="dialog" aria-label="文档内查找替换">
      <div class="find-row">
        <input
          ref="inputEl"
          v-model="keyword"
          class="find-input"
          placeholder="查找…"
          aria-label="查找"
          @input="onInput"
          @keydown="onKeydown"
        />
        <span class="find-count">{{ match.total ? `${match.index}/${match.total}` : keyword ? '无结果' : '' }}</span>
        <button type="button" class="find-btn" title="上一个（Shift+Enter）" @click="emit('nav', -1)">↑</button>
        <button type="button" class="find-btn" title="下一个（Enter）" @click="emit('nav', 1)">↓</button>
        <button
          type="button"
          class="find-btn find-toggle"
          :class="{ active: showReplace }"
          title="替换"
          @click="showReplace = !showReplace"
        >⇄</button>
        <button type="button" class="find-btn" title="关闭（Esc）" @click="emit('close')">×</button>
      </div>
      <div v-if="showReplace" class="find-row">
        <input
          v-model="replaceText"
          class="find-input"
          placeholder="替换为…"
          aria-label="替换为"
          @keydown="onKeydown"
        />
        <button type="button" class="find-btn find-replace" @click="emit('replace', replaceText)">替换</button>
        <button type="button" class="find-btn find-replace" @click="emit('replace-all', replaceText)">全部替换</button>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.find-bar {
  position: fixed;
  top: 40px;
  right: 16px;
  z-index: 75;
  min-width: 340px;
  padding: 6px;
  border: 1px solid var(--t-table-border);
  border-radius: 8px;
  background: var(--t-editor-background);
  box-shadow: 0 8px 24px rgb(0 0 0 / 0.22);
}
.find-row {
  display: flex;
  align-items: center;
  gap: 4px;
}
.find-row + .find-row {
  margin-top: 6px;
}
.find-input {
  flex: 1;
  min-width: 0;
  padding: 3px 8px;
  border: 1px solid var(--t-table-border);
  border-radius: 5px;
  background: transparent;
  color: inherit;
  font-size: 12.5px;
  outline: none;
}
.find-input:focus {
  border-color: var(--t-tab-indicator);
}
.find-count {
  flex-shrink: 0;
  min-width: 42px;
  text-align: center;
  font-size: 11px;
  color: var(--t-text-placeholder);
  font-variant-numeric: tabular-nums;
}
.find-btn {
  flex-shrink: 0;
  padding: 2px 7px;
  border: none;
  border-radius: 5px;
  background: transparent;
  color: var(--t-text-placeholder);
  font-size: 12px;
  cursor: pointer;
  transition: background 0.12s ease, color 0.12s ease, transform 0.16s ease;
}
.find-btn:hover {
  background: var(--t-status-bar-button-hover);
  color: var(--t-text-default);
  transform: scale(1.12);
}
.find-btn.active {
  background: color-mix(in srgb, var(--t-tab-indicator) 18%, transparent);
  color: var(--t-tab-indicator);
}
.find-replace {
  font-size: 12px;
}
</style>
