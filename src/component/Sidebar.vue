<script setup>
import { computed, ref } from 'vue';
import { plainText } from '../utils/wysiwyg.js';

// 侧边栏：「目录」（标题平铺缩进列表）与「大纲」（标题嵌套树）两个标签页。
// 数据源为 Editor 上报的块树（仅根块中的标题块参与）；点击条目滚动定位到对应块，
// 滚动位置对应的当前标题由 activeHeadingId 高亮。
const props = defineProps({
  visible: { type: Boolean, default: true },
  blocks: { type: Array, default: () => [] },
  activeHeadingId: { type: String, default: null },
});

const emit = defineEmits(['update:visible', 'select-block']);

const activeTab = ref('toc');

// 根块中的标题序列（保持文档顺序）
const headings = computed(() =>
  (props.blocks || [])
    .filter((b) => b.type === 'heading')
    .map((b) => ({ id: b.id, level: b.level, text: plainText(b.title) || '（无标题）' })),
);

// 目录：平铺 + 按层级缩进（相对最小层级归一）
const minLevel = computed(() => Math.min(...headings.value.map((h) => h.level), 6));
const tocItems = computed(() =>
  headings.value.map((h) => ({ ...h, indent: h.level - minLevel.value })),
);

// 大纲：按层级构建嵌套树，再展平为带深度的列表渲染
function buildTree(items) {
  const roots = [];
  const stack = [];
  for (const item of items) {
    const node = { ...item, children: [] };
    while (stack.length && stack[stack.length - 1].level >= item.level) stack.pop();
    if (stack.length) stack[stack.length - 1].children.push(node);
    else roots.push(node);
    stack.push(node);
  }
  return roots;
}
const outlineItems = computed(() => {
  const flat = [];
  const walk = (nodes, depth) => {
    for (const node of nodes) {
      flat.push({ ...node, indent: depth });
      walk(node.children, depth + 1);
    }
  };
  walk(buildTree(headings.value), 0);
  return flat;
});
</script>

<template>
  <Transition name="sidebar">
    <div
      v-show="visible"
      class="t-app flex h-full w-60 flex-col border-r border-(--t-table-border) text-[13px]"
    >
      <div class="flex border-b border-(--t-table-border)">
        <div
          class="flex-1 cursor-pointer px-4 py-2 text-center text-[12px] font-medium transition-[background] duration-[0.08s]"
          :class="activeTab === 'toc' ? 'bg-(--t-status-bar-button-hover)' : 'hover:bg-(--t-status-bar-button-hover)'"
          @click="activeTab = 'toc'"
        >
          目录
        </div>
        <div
          class="flex-1 cursor-pointer px-4 py-2 text-center text-[12px] font-medium transition-[background] duration-[0.08s]"
          :class="activeTab === 'outline' ? 'bg-(--t-status-bar-button-hover)' : 'hover:bg-(--t-status-bar-button-hover)'"
          @click="activeTab = 'outline'"
        >
          大纲
        </div>
      </div>

      <div class="flex-1 overflow-y-auto p-2">
        <template v-if="headings.length">
          <div
            v-for="item in activeTab === 'toc' ? tocItems : outlineItems"
            :key="item.id"
            class="cursor-pointer truncate rounded px-2 py-1 text-[12px] transition-[background] duration-[0.08s]"
            :class="item.id === activeHeadingId
              ? 'bg-(--t-selection) font-medium'
              : 'hover:bg-(--t-status-bar-button-hover)'"
            :style="{ paddingLeft: `${8 + item.indent * 14}px` }"
            :title="item.text"
            @click="emit('select-block', item.id)"
          >
            {{ item.text }}
          </div>
        </template>
        <div v-else class="t-dim rounded px-2 py-1 text-[12px]">
          {{ activeTab === 'toc' ? '暂无目录' : '暂无大纲' }}
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.sidebar-enter-active,
.sidebar-leave-active {
  transition: transform 0.2s ease, opacity 0.2s ease;
}
.sidebar-enter-from,
.sidebar-leave-to {
  transform: translateX(-100%);
  opacity: 0;
}
</style>
