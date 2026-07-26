<script setup>
import { computed, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { plainText } from '../utils/wysiwyg.js';

// 侧边栏：「目录」（文件面板）与「大纲」（标题嵌套树）两个标签页。
// 目录页结构：中部文件列表/树 + 底部工具栏（文件夹名操作菜单、新建文件、列表/树切换）。
const props = defineProps({
  visible: { type: Boolean, default: true },
  blocks: { type: Array, default: () => [] },
  activeHeadingId: { type: String, default: null },
  currentFilePath: { type: String, default: null },
  workspaceDir: { type: String, default: null },
});

const emit = defineEmits(['update:visible', 'select-block', 'open-file', 'open-folder', 'create-file', 'show-in-explorer']);

const activeTab = ref('toc');

// ---------- 宽度（默认 280，右缘拖拽调整，localStorage 持久化） ----------
const SIDEBAR_WIDTH_KEY = 'tauri-editor.sidebar-width';
const width = ref(Math.min(480, Math.max(200, Number(localStorage.getItem(SIDEBAR_WIDTH_KEY)) || 280)));

function startResize(e) {
  e.preventDefault();
  const startX = e.clientX;
  const startWidth = width.value;
  const onMove = (ev) => {
    width.value = Math.min(480, Math.max(200, startWidth + ev.clientX - startX));
  };
  const onUp = () => {
    document.removeEventListener('pointermove', onMove);
    document.removeEventListener('pointerup', onUp);
    document.body.style.cursor = '';
    document.body.style.userSelect = '';
    localStorage.setItem(SIDEBAR_WIDTH_KEY, String(width.value));
  };
  // 拖拽期间全局接管光标与选中，避免经过编辑器时选中文本
  document.body.style.cursor = 'col-resize';
  document.body.style.userSelect = 'none';
  document.addEventListener('pointermove', onMove);
  document.addEventListener('pointerup', onUp);
}

// ---------- 目录：文件面板 ----------

// 视图模式：tree（树形）| list（列表）
const viewMode = ref('tree');
// 操作菜单显隐 / 搜索框显隐 / 搜索关键字 / 排序（asc 名称升序、desc 名称降序）
const opsMenuOpen = ref(false);
const searchOpen = ref(false);
const searchKeyword = ref('');
const sortMode = ref('asc');

// 懒加载树：path -> { expanded, children }
const dirTree = ref(new Map());
const folderItems = ref([]);

async function loadChildren(path) {
  // 排序由 Rust list_dir 完成（文件夹在前、文件在后，按名称升/降序）
  const children = await invoke('list_dir', { path, sort: sortMode.value }).catch(() => []);
  dirTree.value.set(path, { ...(dirTree.value.get(path) || {}), children });
  return children;
}

// 搜索：在已加载的目录数据中按名称过滤（含子目录路径提示）
const searchResults = computed(() => {
  const kw = searchKeyword.value.trim().toLowerCase();
  if (!kw) return [];
  const results = [];
  for (const [parent, node] of dirTree.value) {
    for (const child of node.children || []) {
      if (child.name.toLowerCase().includes(kw)) {
        results.push({ ...child, parent });
      }
    }
  }
  return results;
});

function rebuildFolderItems() {
  const items = [];
  const walk = (path, depth) => {
    const node = dirTree.value.get(path);
    for (const child of node?.children || []) {
      items.push({ entry: child, depth });
      if (child.isDir && dirTree.value.get(child.path)?.expanded) {
        walk(child.path, depth + 1);
      }
    }
  };
  if (props.workspaceDir) walk(props.workspaceDir, 0);
  folderItems.value = items;
}

// 列表模式：根目录下文件平铺（不含子目录）
const listItems = computed(() =>
  (dirTree.value.get(props.workspaceDir)?.children || []).filter((e) => !e.isDir),
);

async function refreshTree() {
  dirTree.value = new Map();
  if (!props.workspaceDir) {
    folderItems.value = [];
    return;
  }
  await loadChildren(props.workspaceDir);
  rebuildFolderItems();
}

// 最近使用的目录（localStorage，最多 8 条）
const RECENT_DIR_KEY = 'tauri-editor.recent-dirs';
const recentDirs = ref(JSON.parse(localStorage.getItem(RECENT_DIR_KEY) || '[]'));
watch(
  () => props.workspaceDir,
  (dir) => {
    refreshTree();
    if (!dir) return;
    const list = [dir, ...recentDirs.value.filter((d) => d !== dir)].slice(0, 8);
    recentDirs.value = list;
    localStorage.setItem(RECENT_DIR_KEY, JSON.stringify(list));
  },
  { immediate: true },
);

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

function dirName(path) {
  return path?.split(/[\\/]/).pop() || path || '';
}

function onOps(action) {
  opsMenuOpen.value = false;
  if (action === 'create') emit('create-file');
  else if (action === 'open-folder') emit('open-folder');
  else if (action === 'refresh') refreshTree();
  else if (action === 'explorer') emit('show-in-explorer', props.workspaceDir);
  else if (action === 'search') {
    searchOpen.value = true;
  }
}

// 排序切换：已加载目录按新方向经 Rust 重新拉取
async function onSort(mode) {
  if (mode === sortMode.value) return;
  sortMode.value = mode;
  await Promise.all([...dirTree.value.keys()].map((path) => loadChildren(path)));
  rebuildFolderItems();
}

function openRecentDir(dir) {
  opsMenuOpen.value = false;
  emit('open-folder', dir);
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
      class="t-app relative flex h-full flex-col border-r border-(--t-table-border) text-[13px]"
      :style="{ width: `${width}px` }"
    >
      <!-- 右缘拖拽调宽手柄 -->
      <div class="sidebar-resizer" title="拖拽调整宽度" @pointerdown="startResize"></div>
      <div class="flex border-b border-(--t-table-border)">
        <div
          class="t-tab flex flex-1 cursor-pointer items-center justify-center gap-1.5 px-4 pb-2 pt-3 text-center text-[12px] font-medium"
          :class="activeTab === 'toc' ? 't-tab-active' : 't-dim hover:bg-(--t-status-bar-button-hover)'"
          @click="activeTab = 'toc'"
        >
          <svg viewBox="0 0 16 16" class="size-[13px]" aria-hidden="true" style="color: #f59102">
            <path d="M2 3.5A1.5 1.5 0 0 1 3.5 2h2.6l1.4 1.6h5A1.5 1.5 0 0 1 14 5.1v6.4a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5v-8z" fill="none" stroke="currentColor" stroke-width="1.2" />
          </svg>
          目录
        </div>
        <div
          class="t-tab flex flex-1 cursor-pointer items-center justify-center gap-1.5 px-4 pb-2 pt-3 text-center text-[12px] font-medium"
          :class="activeTab === 'outline' ? 't-tab-active' : 't-dim hover:bg-(--t-status-bar-button-hover)'"
          @click="activeTab = 'outline'"
        >
          <svg viewBox="0 0 16 16" aria-hidden="true" class="size-[13px]" style="color: #3e69d7">
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

      <!-- 目录：文件面板 -->
      <template v-if="activeTab === 'toc'">
        <!-- 搜索框 -->
        <div v-if="searchOpen" class="flex items-center gap-1 border-b border-(--t-table-border) px-2 py-1.5">
          <input
            v-model="searchKeyword"
            class="min-w-0 flex-1 rounded border border-(--t-table-border) bg-transparent px-2 py-1 text-[12px] outline-none"
            placeholder="搜索文件名…"
            autofocus
          />
          <span class="t-btn shrink-0 cursor-pointer rounded px-1 text-[13px]" title="关闭搜索" @click="searchOpen = false; searchKeyword = ''">×</span>
        </div>

        <div class="flex-1 overflow-y-auto p-2">
          <!-- 搜索结果 -->
          <template v-if="searchKeyword.trim()">
            <template v-if="searchResults.length">
              <div
                v-for="entry in searchResults"
                :key="entry.path"
                class="flex cursor-pointer items-center gap-1.5 truncate rounded px-2 py-1 text-[12px] transition-[background] duration-[0.08s]"
                :class="[
                  entry.path === currentFilePath ? 'bg-(--t-selection) font-medium' : 'hover:bg-(--t-status-bar-button-hover)',
                  !entry.isDir && !entry.isMarkdown ? 't-dim' : '',
                ]"
                :title="entry.parent"
                @click="onEntryClick(entry)"
              >
                <span class="truncate">{{ entry.name }}</span>
                <span class="t-dim truncate text-[10px]">{{ entry.parent }}</span>
              </div>
            </template>
            <div v-else class="t-dim rounded px-2 py-1 text-[12px]">无匹配文件（仅搜索已展开的目录）</div>
          </template>

          <template v-else-if="workspaceDir">
            <template v-if="viewMode === 'tree' && folderItems.length">
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
                    :d="dirTree.get(item.entry.path)?.expanded ? 'M4 6l4 4 4-4' : 'M6 4l4 4-4 4'"
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
            </template>
            <template v-else-if="viewMode === 'list' && listItems.length">
              <div
                v-for="entry in listItems"
                :key="entry.path"
                class="flex cursor-pointer items-center gap-1.5 truncate rounded px-2 py-1 text-[12px] transition-[background] duration-[0.08s]"
                :class="[
                  entry.path === currentFilePath ? 'bg-(--t-selection) font-medium' : 'hover:bg-(--t-status-bar-button-hover)',
                  !entry.isMarkdown ? 't-dim' : '',
                ]"
                :title="entry.name"
                @click="onEntryClick(entry)"
              >
                <svg viewBox="0 0 16 16" class="size-[12px] shrink-0" aria-hidden="true">
                  <rect x="3" y="1.5" width="10" height="13" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.2" />
                  <line x1="5.5" y1="5" x2="10.5" y2="5" stroke="currentColor" stroke-width="1" />
                  <line x1="5.5" y1="8" x2="10.5" y2="8" stroke="currentColor" stroke-width="1" />
                  <line x1="5.5" y1="11" x2="9" y2="11" stroke="currentColor" stroke-width="1" />
                </svg>
                <span class="truncate">{{ entry.name }}</span>
              </div>
            </template>
            <div v-else class="t-dim flex h-full items-center justify-center text-[12px]">文件列表为空</div>
          </template>
          <div v-else class="t-dim flex h-full items-center justify-center text-[12px]">文件列表为空</div>
        </div>

        <!-- 底部工具栏（与状态栏同高 h-7，图标文字垂直居中） -->
        <div class="relative flex h-7 items-center justify-between border-t border-(--t-table-border) px-2">
          <template v-if="workspaceDir">
            <div class="t-btn flex shrink-0 cursor-pointer items-center rounded px-1.5 py-1 text-[14px] leading-none" style="color: #03b736" title="在当前文件夹新建文件" @click="emit('create-file')">+</div>
            <div
              class="t-btn flex min-w-0 flex-1 cursor-pointer items-center justify-center rounded px-1.5 py-1 text-[12px]"
              :title="workspaceDir"
              @click="opsMenuOpen = !opsMenuOpen"
            >
              <svg viewBox="0 0 16 16" class="mr-1 size-[12px] shrink-0" aria-hidden="true" style="color: #f59102">
                <path d="M2 4.5A1.5 1.5 0 0 1 3.5 3h2.6l1.4 1.6h5A1.5 1.5 0 0 1 14 6.1v5.4a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5v-7z" fill="none" stroke="currentColor" stroke-width="1.2" />
              </svg>
              <span class="truncate font-medium">{{ dirName(workspaceDir) }}</span>
            </div>
            <div
              class="t-btn flex shrink-0 cursor-pointer items-center rounded px-1.5 py-1 text-[13px] leading-none tracking-wider"
              title="操作"
              @click="opsMenuOpen = !opsMenuOpen"
            >
              ⋮
            </div>
            <div
              class="t-btn flex shrink-0 cursor-pointer items-center rounded px-1.5 py-1"
              :title="viewMode === 'tree' ? '树形显示' : '列表显示'"
              @click="viewMode = viewMode === 'tree' ? 'list' : 'tree'"
            >
              <svg v-if="viewMode === 'tree'" viewBox="0 0 16 16" class="size-[13px]" aria-hidden="true" style="color: #3e69d7">
                <!-- 文件树图标（当前为树形显示） -->
                <path d="M2 2.5h4l1.2 1.4H8v9.6H2v-11z" fill="none" stroke="currentColor" stroke-width="1.2" />
                <path d="M9.5 6.5h4.5v2h-4.5v-2zM11 9.5h3v2h-3v-2zM11 12.5h3v2h-3v-2z" fill="none" stroke="currentColor" stroke-width="1.1" />
              </svg>
              <svg v-else viewBox="0 0 16 16" class="size-[13px]" aria-hidden="true" style="color: #3e69d7">
                <!-- 列表图标（当前为列表显示） -->
                <line x1="2" y1="4" x2="14" y2="4" stroke="currentColor" stroke-width="1.3" />
                <line x1="2" y1="8" x2="14" y2="8" stroke="currentColor" stroke-width="1.3" />
                <line x1="2" y1="12" x2="14" y2="12" stroke="currentColor" stroke-width="1.3" />
              </svg>
            </div>
          </template>
          <template v-else>
            <div class="t-btn flex min-w-0 flex-1 cursor-pointer items-center justify-center rounded px-1.5 py-1 text-[12px]" @click="emit('open-folder')">
              打开文件夹…
            </div>
            <div
              class="t-btn flex shrink-0 cursor-pointer items-center rounded px-1.5 py-1 text-[13px] leading-none tracking-wider"
              title="操作"
              @click="opsMenuOpen = !opsMenuOpen"
            >
              ⋮
            </div>
          </template>

          <!-- 文件夹操作菜单（操作/排序/最近使用的目录 分区） -->
          <div
            v-if="opsMenuOpen"
            class="t-statusbar absolute bottom-full left-2 z-50 mb-1 w-52 overflow-hidden rounded-md border border-(--t-table-border) py-1 text-[12px] shadow-lg"
          >
            <div class="flex items-center justify-between px-3 pb-1 pt-0.5">
              <span class="font-medium">操作</span>
              <span class="t-btn cursor-pointer rounded px-1 text-[13px]" @click="opsMenuOpen = false">×</span>
            </div>
            <div v-if="workspaceDir" class="t-btn cursor-pointer px-3 py-1.5" @click="onOps('create')">新建文件</div>
            <div class="t-btn cursor-pointer px-3 py-1.5" @click="onOps('search')">搜索</div>
            <div v-if="workspaceDir" class="t-btn cursor-pointer px-3 py-1.5" @click="onOps('explorer')">在资源管理器中显示</div>
            <div class="t-btn cursor-pointer px-3 py-1.5" @click="onOps('open-folder')">打开文件夹…</div>
            <div v-if="workspaceDir" class="t-btn cursor-pointer px-3 py-1.5" @click="onOps('refresh')">刷新</div>

            <div class="mt-1 flex items-center justify-between border-t border-(--t-table-border) px-3 pb-1 pt-1.5">
              <span class="t-dim text-[11px]">排序</span>
              <div class="flex items-center gap-1">
                <span
                  class="t-btn cursor-pointer rounded px-1.5 py-0.5 text-[12px]"
                  :class="{ 'bg-(--t-status-bar-button-hover) font-semibold': sortMode === 'asc' }"
                  title="名称升序"
                  @click="onSort('asc')"
                >A↓Z</span>
                <span
                  class="t-btn cursor-pointer rounded px-1.5 py-0.5 text-[12px]"
                  :class="{ 'bg-(--t-status-bar-button-hover) font-semibold': sortMode === 'desc' }"
                  title="名称降序"
                  @click="onSort('desc')"
                >Z↓A</span>
              </div>
            </div>

            <template v-if="recentDirs.length">
              <div class="t-dim border-t border-(--t-table-border) px-3 pb-1 pt-1.5 text-[11px]">最近使用的目录</div>
              <div
                v-for="dir in recentDirs"
                :key="dir"
                class="t-btn cursor-pointer truncate px-3 py-1"
                :title="dir"
                @click="openRecentDir(dir)"
              >
                {{ dirName(dir) }}
              </div>
            </template>
          </div>
          <div v-if="opsMenuOpen" class="fixed inset-0 z-40" @click="opsMenuOpen = false"></div>
        </div>
      </template>

      <!-- 大纲：标题树（树形缩进标记 + 层级徽标） -->
      <div v-else class="flex-1 overflow-y-auto p-2">
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
            <span v-for="depth in item.indent" :key="depth" class="tree-guide"></span>
            <span class="tree-badge" :class="{ 'tree-badge-root': item.indent === 0 }">H{{ item.level }}</span>
            <span class="truncate px-2 py-1">{{ item.text }}</span>
          </div>
        </template>
        <div v-else class="t-dim rounded px-2 py-1 text-[12px]">暂无大纲</div>
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

/* 右缘拖拽调宽手柄：窄热区 + 悬停高亮（主题指示色） */
.sidebar-resizer {
  position: absolute;
  top: 0;
  right: -3px;
  width: 6px;
  height: 100%;
  cursor: col-resize;
  z-index: 10;
}
.sidebar-resizer:hover,
.sidebar-resizer:active {
  background: color-mix(in srgb, var(--t-tab-indicator) 35%, transparent);
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
