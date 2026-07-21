<script setup>
import { computed, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { plainText } from '../utils/wysiwyg.js';

// 侧边栏：「目录」（当前文件所在文件夹的文件树）与「大纲」（标题嵌套树）两个标签页。
// 文件树经 Rust list_dir 懒加载；大纲数据为 Editor 上报的块树。
const props = defineProps({
  visible: { type: Boolean, default: true },
  blocks: { type: Array, default: () => [] },
  activeHeadingId: { type: String, default: null },
  currentFilePath: { type: String, default: null },
});

const emit = defineEmits(['update:visible', 'select-block', 'open-file']);

const activeTab = ref('toc');

// ---------- 目录：当前文件所在文件夹 ----------

// 所在文件夹（未打开文件时为 null）
const rootDir = computed(() => {
  if (!props.currentFilePath) return null;
  const parts = props.currentFilePath.split(/[\\/]/);
  parts.pop();
  return parts.join('\\') || props.currentFilePath;
});

// 懒加载树：path -> { expanded, children }
const dirTree = ref(new Map());
// 平铺渲染列表：{ entry, depth }
const folderItems = ref([]);

async function loadChildren(path) {
  const children = await invoke('list_dir', { path }).catch(() => []);
  dirTree.value.set(path, { ...(dirTree.value.get(path) || {}), children });
  return children;
}

// 重新构建平铺列表（按展开状态）
function rebuildFolderItems() {
  const items = [];
  const walk = (path, depth) => {
    const node = dirTree.value.get(path);
    for (const child of node?.children || []) {
      items.push({ entry: child, depth });
      if (child.isDir && node && dirTree.value.get(child.path)?.expanded) {
        walk(child.path, depth + 1);
      }
    }
  };
  if (rootDir.value) walk(rootDir.value, 0);
  folderItems.value = items;
}

// 文件路径变化（打开/新建）时重置并加载根目录
watch(rootDir, async (dir) => {
  dirTree.value = new Map();
  if (!dir) {
    folderItems.value = [];
    return;
  }
  await loadChildren(dir);
  rebuildFolderItems();
}, { immediate: true });

async function toggleDir(path) {
  const node = dirTree.value.get(path) || {};
  const expanded = !node.expanded;
  dirTree.value.set(path, { ...node, expanded });
  if (expanded && !node.children) await loadChildren(path);
  rebuildFolderItems();
}

function onEntryClick(entry) {
  if (entry.isDir) {
    toggleDir(entry.path);
  } else if (entry.isMarkdown) {
    emit('open-file', entry.path);
  }
}

// ---------- 大纲：标题嵌套树 ----------

const headings = computed(() =>
  (props.blocks || [])
    .filter((b) => b.type === 'heading')
    .map((b) => ({ id: b.id, level: b.level, text: plainText(b.title) || '（无标题）' })),
);

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
          class="flex flex-1 cursor-pointer items-center justify-center gap-1.5 px-4 py-2 text-center text-[12px] font-medium transition-[background] duration-[0.08s]"
          :class="activeTab === 'toc' ? 'bg-(--t-status-bar-button-hover)' : 'hover:bg-(--t-status-bar-button-hover)'"
          @click="activeTab = 'toc'"
        >
          <svg viewBox="0 0 16 16" class="size-[13px]" aria-hidden="true">
            <path d="M2 3.5A1.5 1.5 0 0 1 3.5 2h2.6l1.4 1.6h5A1.5 1.5 0 0 1 14 5.1v6.4a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5v-8z" fill="none" stroke="currentColor" stroke-width="1.2" />
          </svg>
          目录
        </div>
        <div
          class="flex flex-1 cursor-pointer items-center justify-center gap-1.5 px-4 py-2 text-center text-[12px] font-medium transition-[background] duration-[0.08s]"
          :class="activeTab === 'outline' ? 'bg-(--t-status-bar-button-hover)' : 'hover:bg-(--t-status-bar-button-hover)'"
          @click="activeTab = 'outline'"
        >
          <svg viewBox="0 0 16 16" aria-hidden="true" class="size-[13px]">
            <circle cx="3" cy="3" r="1.4" fill="currentColor" />
            <circle cx="3" cy="8" r="1.4" fill="currentColor" />
            <circle cx="3" cy="13" r="1.4" fill="currentColor" />
            <line x1="6" y1="3" x2="14" y2="3" stroke="currentColor" stroke-width="1.2" />
            <line x1="6" y1="8" x2="12" y2="8" stroke="currentColor" stroke-width="1.2" />
            <line x1="6" y1="13" x2="10" y2="13" stroke="currentColor" stroke-width="1.2" />
          </svg>
          大纲
        </div>
      </div>

      <div class="flex-1 overflow-y-auto p-2">
        <!-- 目录：当前文件所在文件夹 -->
        <template v-if="activeTab === 'toc'">
          <template v-if="rootDir">
            <div class="t-dim truncate px-2 py-1 text-[11px]" :title="rootDir">{{ rootDir }}</div>
            <div
              v-for="item in folderItems"
              :key="item.entry.path"
              class="flex cursor-pointer items-center gap-1.5 truncate rounded px-2 py-1 text-[12px] transition-[background] duration-[0.08s]"
              :class="[
                item.entry.path === currentFilePath
                  ? 'bg-(--t-selection) font-medium'
                  : 'hover:bg-(--t-status-bar-button-hover)',
                !item.entry.isDir && !item.entry.isMarkdown ? 't-dim' : '',
              ]"
              :style="{ paddingLeft: `${8 + item.depth * 14}px` }"
              :title="item.entry.name"
              @click="onEntryClick(item.entry)"
            >
              <svg v-if="item.entry.isDir" viewBox="0 0 16 16" class="size-[12px] shrink-0" aria-hidden="true">
                <path
                  :d="dirTree.get(item.entry.path)?.expanded
                    ? 'M4 6l4 4 4-4'
                    : 'M6 4l4 4-4 4'"
                  fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"
                />
              </svg>
              <svg v-else viewBox="0 0 16 16" class="size-[12px] shrink-0" aria-hidden="true">
                <rect x="3" y="1.5" width="10" height="13" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.2" />
                <line x1="5.5" y1="5" x2="10.5" y2="5" stroke="currentColor" stroke-width="1" />
                <line x1="5.5" y1="8" x2="10.5" y2="8" stroke="currentColor" stroke-width="1" />
                <line x1="5.5" y1="11" x2="9" y2="11" stroke="currentColor" stroke-width="1" />
              </svg>
              <span class="truncate">{{ item.entry.name }}</span>
            </div>
            <div v-if="!folderItems.length" class="t-dim rounded px-2 py-1 text-[12px]">空文件夹</div>
          </template>
          <div v-else class="t-dim rounded px-2 py-1 text-[12px]">未打开文件</div>
        </template>

        <!-- 大纲：标题树（树形缩进标记 + 层级徽标） -->
        <template v-else>
          <template v-if="headings.length">
            <div
              v-for="item in outlineItems"
              :key="item.id"
              class="flex cursor-pointer items-stretch rounded text-[12px] transition-[background] duration-[0.08s]"
              :class="item.id === activeHeadingId
                ? 'bg-(--t-selection) font-medium'
                : 'hover:bg-(--t-status-bar-button-hover)'"
              :title="item.text"
              @click="emit('select-block', item.id)"
            >
              <!-- 树形缩进引导线 -->
              <span
                v-for="depth in item.indent"
                :key="depth"
                class="tree-guide"
              ></span>
              <span class="tree-badge" :class="{ 'tree-badge-root': item.indent === 0 }">H{{ item.level }}</span>
              <span class="truncate px-2 py-1">{{ item.text }}</span>
            </div>
          </template>
          <div v-else class="t-dim rounded px-2 py-1 text-[12px]">暂无大纲</div>
        </template>
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

/* 大纲树形缩进引导线与层级徽标 */
.tree-guide {
  width: 14px;
  margin-left: 4px;
  border-left: 1px solid var(--t-table-border);
}
.tree-guide:first-child {
  margin-left: 8px;
}
.tree-badge {
  flex-shrink: 0;
  align-self: center;
  font-size: 10px;
  font-weight: 600;
  color: var(--t-text-placeholder);
  margin-left: 4px;
  width: 18px;
}
.tree-badge-root {
  color: var(--t-text-link);
}
</style>
