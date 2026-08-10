<script setup vapor>
import { ref, watch } from 'vue';
import PrefsDialog from './PrefsDialog.vue';
import { isMac } from '../utils/platform.js';

// 一体化菜单（Typora 风格）：左侧深色菜单列 + 右侧内容面板。
// 动作项（新建/保存/另存为/打印/关闭）直接执行；内容项（打开/导出/偏好设置/关于）在右侧展示。
// 主题管理在 偏好设置→外观 页（主题卡片），菜单不再单设主题页。
const props = defineProps({
  visible: { type: Boolean, default: false },
  recentFiles: { type: Array, default: () => [] },
});

const emit = defineEmits(['close', 'action']);

// macOS 使用原生标题栏（Overlay 样式），需为左上角红绿灯留白

// 当前选中的内容页（默认「打开」，与参考一致）
const selected = ref('open');
watch(() => props.visible, (v) => {
  if (v) selected.value = 'open';
});

const fileInput = ref(null);
const themeFilter = ref('');

// 最近文件过滤
const filteredRecent = () => {
  const kw = themeFilter.value.trim().toLowerCase();
  if (!kw) return props.recentFiles;
  return props.recentFiles.filter((p) => p.toLowerCase().includes(kw));
};

function fileName(path) {
  return path.split(/[\\/]/).pop() || path;
}
function fileDir(path) {
  const parts = path.split(/[\\/]/);
  parts.pop();
  // 拼接统一用正斜杠（macOS/Linux 原生，Windows 侧 Rust 路径 API 同样接受）
  return parts.join('/') || path;
}

const prefsPages = [
  { id: 'editor', label: '编辑器', desc: '字号、行高、内容宽度、内边距' },
  { id: 'image', label: '图像', desc: '粘贴图片保存行为' },
  { id: 'markdown', label: 'Markdown', desc: '高亮、图表、公式渲染开关' },
  { id: 'appearance', label: '外观', desc: '主题与字体' },
];

// 菜单项：彩色线性图标 + 标签（图标风格与编辑器角标/工具栏一致）
const MENU_ICONS = {
  new: '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M9.5 2H4a1 1 0 00-1 1v10a1 1 0 001 1h8a1 1 0 001-1V5.5L9.5 2z"/><path d="M9 2v4h4M8 8v4M6 10h4"/></svg>',
  open: '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round"><path d="M2 4.5A1.5 1.5 0 013.5 3h2.6l1.4 1.6h5A1.5 1.5 0 0114 6.1v5.4a1.5 1.5 0 01-1.5 1.5h-9A1.5 1.5 0 012 11.5v-7z"/></svg>',
  save: '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M3 3h9l1 1v9a1 1 0 01-1 1H4a1 1 0 01-1-1V3z"/><path d="M5 3v4h6V3M5.5 14V9.5h5V14"/></svg>',
  saveAs: '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M3 3h9l1 1v9a1 1 0 01-1 1H4a1 1 0 01-1-1V3z"/><path d="M5 3v4h6V3M8 7.5v5M6 10.5l2 2 2-2"/></svg>',
  export: '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M6 3H3v10h10v-3"/><path d="M9 2h5v5M14 2L7.5 8.5"/></svg>',
  print: '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"><path d="M4 6V2h8v4M4 12H2V6h12v6h-2M4 10h8v4H4v-4z"/></svg>',
  prefs: '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"><circle cx="8" cy="8" r="2.2"/><path d="M8 1.8v1.8M8 12.4v1.8M1.8 8h1.8M12.4 8h1.8M3.6 3.6l1.3 1.3M11.1 11.1l1.3 1.3M12.4 3.6l-1.3 1.3M4.9 11.1l-1.3 1.3"/></svg>',
  about: '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><circle cx="8" cy="8" r="6.2" stroke-width="1.3"/><path d="M8 7.2v3.6M8 4.8v.2"/></svg>',
  close: '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="M4 4l8 8M12 4l-8 8"/></svg>',
};

const menuItems = [
  { action: 'new', label: '新建', kind: 'action', icon: MENU_ICONS.new, iconColor: '#03b736' },
  { action: 'open', label: '打开', kind: 'content', icon: MENU_ICONS.open, iconColor: '#f59102' },
  { action: 'save', label: '保存', kind: 'action', icon: MENU_ICONS.save, iconColor: '#2f6dbb' },
  { action: 'save-as', label: '另存为', kind: 'action', icon: MENU_ICONS.saveAs, iconColor: '#2f9dbb' },
  { action: 'export', label: '导出', kind: 'content', icon: MENU_ICONS.export, iconColor: '#8a5cf6' },
  { action: 'print', label: '打印', kind: 'action', icon: MENU_ICONS.print, iconColor: '#0e9f6e' },
  { action: 'prefs', label: '偏好设置', kind: 'content', icon: MENU_ICONS.prefs, iconColor: '#667c89' },
  { action: 'about', label: '关于', kind: 'content', icon: MENU_ICONS.about, iconColor: '#3e69d7' },
  { action: 'close', label: '关闭', kind: 'action', icon: MENU_ICONS.close, iconColor: '#e30f2e' },
];

function onItem(item) {
  if (item.kind === 'action') {
    emit('action', item.action);
  } else {
    selected.value = item.action;
    // 切离偏好设置时收起第三列
    if (item.action !== 'prefs') prefsColumn.value = null;
  }
}

// 偏好设置第三列：点击卡片在右侧新增一列显示该页设置表单（不整窗替换）
const prefsColumn = ref(null);
function openPrefsColumn(pageId) {
  prefsColumn.value = pageId;
}
</script>

<template>
  <Teleport to="body">
    <Transition name="menu-slide">
      <div v-if="visible" class="menu-overlay" @click.self="emit('close')">
        <!-- 全宽顶部拖拽带（菜单打开时整个上缘均可拖动窗口；16px 不遮挡下方按钮） -->
        <div class="menu-drag-top" :class="{ 'menu-drag-top-mac': isMac }" data-tauri-drag-region></div>
        <!-- 左侧深色菜单列 -->
        <div class="menu-sidebar">
          <div class="menu-header">
            <span class="menu-title" data-tauri-drag-region>
              <span class="hb-icon" aria-hidden="true"><span></span><span></span><span></span></span>
              菜单
            </span>
            <span class="menu-back" title="返回" @click="emit('close')">
              <svg viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <path d="M14 8H4M8.5 3.5L4 8l4.5 4.5" />
              </svg>
              返回
            </span>
          </div>
          <div
            v-for="item in menuItems"
            :key="item.action"
            class="menu-item"
            :class="[{ active: item.kind === 'content' && selected === item.action }, `mi-${item.action}`]"
            @click="onItem(item)"
          >
            <span class="menu-item-icon" :style="{ color: item.iconColor }" v-html="item.icon"></span>
            <span>{{ item.label }}</span>
          </div>
        </div>

        <!-- 右侧内容面板 -->
        <div class="menu-content t-app" @click.stop>
          <!-- 打开 -->
          <div v-if="selected === 'open'" class="menu-page">
            <h2 class="menu-page-title" data-tauri-drag-region>打开</h2>
            <button class="menu-btn" @click="emit('action', 'open')">
              <span class="menu-btn-icon">📂</span> 打开…
            </button>

            <h3 class="menu-page-subtitle">最近使用的文件</h3>
            <div class="menu-filter-row">
              <input v-model="themeFilter" class="menu-filter" placeholder="查找…" />
            </div>
            <div v-if="filteredRecent().length" class="menu-recent">
              <div
                v-for="path in filteredRecent()"
                :key="path"
                class="menu-recent-item"
                :title="path"
                @click="emit('action', { type: 'open-recent', path })"
              >
                <span class="menu-recent-name">{{ fileName(path) }}</span>
                <span class="menu-recent-dir">{{ fileDir(path) }}</span>
              </div>
            </div>
            <p v-else class="menu-dim">暂无最近文件</p>
          </div>

          <!-- 导出 -->
          <div v-else-if="selected === 'export'" class="menu-page">
            <h2 class="menu-page-title" data-tauri-drag-region>导出</h2>
            <button class="menu-btn" @click="emit('action', 'export')">
              <span class="menu-btn-icon">📄</span> HTML（含主题样式与渲染结果）
            </button>
            <button class="menu-btn" @click="emit('action', 'print')">
              <span class="menu-btn-icon">🖨</span> PDF（经系统打印对话框）
            </button>
          </div>

          <!-- 偏好设置 -->
          <div v-else-if="selected === 'prefs'" class="menu-page">
            <h2 class="menu-page-title" data-tauri-drag-region>偏好设置</h2>
            <div
              v-for="page in prefsPages"
              :key="page.id"
              class="menu-prefs-item"
              :class="{ 'menu-prefs-item-active': prefsColumn === page.id }"
              @click="openPrefsColumn(page.id)"
            >
              <div class="menu-prefs-label">{{ page.label }}</div>
              <div class="menu-prefs-desc">{{ page.desc }}</div>
            </div>
          </div>

          <!-- 关于 -->
          <div v-else-if="selected === 'about'" class="menu-page">
            <h2 class="menu-page-title" data-tauri-drag-region>tauri-editor</h2>
            <p class="menu-dim mb-3">版本 0.1.0</p>
            <p>基于 Tauri 2 + Vue 3 的桌面 Markdown 编辑器，支持所见即所得与源码双模式编辑。</p>
            <p class="mt-2">
              Markdown 核心移植自 velotype（Apache-2.0）；公式渲染基于 ratex，图表基于 mermaid-rs-renderer，代码高亮基于 tree-sitter。
            </p>
          </div>
        </div>

        <!-- 偏好设置第三列：选中卡片后在右侧显示该页设置表单（返回键收起本列） -->
        <div v-if="prefsColumn" class="menu-prefs-column t-app" @click.stop>
          <PrefsDialog :visible="true" :page="prefsColumn" embedded @close="prefsColumn = null" />
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.menu-overlay {
  position: fixed;
  inset: 0;
  z-index: 90;
  display: flex;
  /* 极窄窗口（三列最小宽之和仍超出）时横向滚动，不再挤占列宽 */
  overflow-x: auto;
}
/* 全宽顶部拖拽带（菜单打开时整个上缘可拖动窗口） */
.menu-drag-top {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  height: 16px;
  z-index: 95;
}
/* macOS 原生红绿灯在左上角，拖拽带让出该区域 */
.menu-drag-top-mac {
  left: 76px;
}
/* 左侧菜单列：跟随主题（副背景色，亮主题为浅灰、暗主题为深灰），不再固定黑色 */
.menu-sidebar {
  width: 240px;
  height: 100%;
  background: var(--t-source-mode-block-bg);
  color: var(--t-text-default);
  display: flex;
  flex-direction: column;
  overflow-y: auto;
  flex-shrink: 0;
}
.menu-header {
  position: relative;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 18px 20px 10px;
}
/* 默认显示 汉堡图标+「菜单」标题；悬停左上角区域时：
   标题左滑淡出（汉堡条逐根收拢）→「← 返回」错峰右滑淡入（箭头描边自绘），
   两组动画首尾衔接（0.16s 退场 / 0.22s+60ms 延迟进场），原位交叉无布局跳动 */
.menu-title {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 17px;
  font-weight: 600;
  color: var(--t-text-default);
  letter-spacing: 0.5px;
  transition: opacity 0.16s ease-in, transform 0.16s ease-in;
}
.menu-title .hb-icon {
  color: #3e69d7;
}
/* 汉堡条：悬停收拢（逐根错峰），移出还原 */
.menu-title .hb-icon > span {
  transition: transform 0.16s ease-in, opacity 0.16s ease-in;
}
.menu-header:hover .menu-title .hb-icon > span:nth-child(1),
.menu-drag-top:hover ~ .menu-sidebar .menu-title .hb-icon > span:nth-child(1) {
  transform: scaleX(0.25);
  opacity: 0.4;
}
.menu-header:hover .menu-title .hb-icon > span:nth-child(2),
.menu-drag-top:hover ~ .menu-sidebar .menu-title .hb-icon > span:nth-child(2) {
  transform: scaleX(0.25);
  opacity: 0.4;
  transition-delay: 0.03s;
}
.menu-header:hover .menu-title .hb-icon > span:nth-child(3),
.menu-drag-top:hover ~ .menu-sidebar .menu-title .hb-icon > span:nth-child(3) {
  transform: scaleX(0.25);
  opacity: 0.4;
  transition-delay: 0.06s;
}
.menu-back {
  position: absolute;
  /* 起点与菜单标题同一左缘；下移 4px 使箭头与标题（汉堡+菜单）垂直中线对齐 */
  left: 20px;
  top: calc(50% + 4px);
  transform: translateY(-50%) translateX(12px);
  display: inline-flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
  font-size: 17px;
  font-weight: 600;
  padding: 4px 10px 4px 6px;
  border-radius: 5px;
  opacity: 0;
  transition:
    opacity 0.22s ease-out 0.06s,
    transform 0.24s cubic-bezier(0.2, 0.9, 0.3, 1.15) 0.06s,
    background 0.12s ease;
}
.menu-back svg {
  width: 15px;
  height: 15px;
  display: block;
  color: #3e69d7;
}
/* 返回箭头：进场时描边自绘（画出来的感觉）；路径全长 ≈22.7，dasharray 取 24 */
.menu-back svg path {
  stroke-dasharray: 24;
  stroke-dashoffset: 24;
  transition: stroke-dashoffset 0.26s ease-out 0.1s;
}
.menu-header:hover .menu-back svg path,
.menu-drag-top:hover ~ .menu-sidebar .menu-back svg path {
  stroke-dashoffset: 0;
}
.menu-header:hover .menu-title,
.menu-drag-top:hover ~ .menu-sidebar .menu-title {
  opacity: 0;
  transform: translateX(-10px) scale(0.95);
}
.menu-header:hover .menu-back,
.menu-drag-top:hover ~ .menu-sidebar .menu-back,
.menu-back:focus-visible {
  opacity: 1;
  transform: translateY(-50%) translateX(0);
}
/* 返回键：滑过箭头向左一探（transform 须保留垂直居中分量；
   选择器优先级与上方显现规则持平且位置靠后，悬停时胜出） */
.menu-header .menu-back:hover {
  background: var(--t-status-bar-button-hover);
  transform: translateY(-50%) translateX(-3px);
}
.menu-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 11px 20px;
  font-size: 14px;
  cursor: pointer;
  border-left: 3px solid transparent;
}
.menu-item-icon {
  display: inline-flex;
  width: 16px;
  height: 16px;
  flex-shrink: 0;
}
.menu-item-icon :deep(svg) {
  width: 16px;
  height: 16px;
  display: block;
}
.menu-item:hover {
  background: var(--t-status-bar-button-hover);
}
.menu-item.active {
  background: var(--t-status-bar-button-hover);
  border-left-color: var(--t-tab-indicator);
}
/* 菜单导航项滑过动画：整行缩进右移（指示条仍钉在左缘）+ 图标弹跳；
   逐图标细节见下方 mi-* 规则 */
.menu-item {
  transition: background 0.15s ease, padding-left 0.2s cubic-bezier(0.25, 0.8, 0.3, 1.15);
}
.menu-item-icon {
  transition: transform 0.22s cubic-bezier(0.2, 0.9, 0.3, 1.45);
}
.menu-item:hover {
  padding-left: 27px;
}
.menu-item:hover .menu-item-icon {
  transform: scale(1.22) rotate(-7deg);
}
/* 逐图标细节：齿轮旋转、关闭旋成 ×、保存下沉、打开上挑 */
.menu-item.mi-prefs:hover .menu-item-icon {
  transform: rotate(55deg) scale(1.15);
}
.menu-item.mi-close:hover .menu-item-icon {
  transform: rotate(90deg) scale(1.15);
}
.menu-item.mi-save:hover .menu-item-icon,
.menu-item.mi-save-as:hover .menu-item-icon {
  transform: translateY(1.5px) scale(1.15);
}
.menu-item.mi-open:hover .menu-item-icon,
.menu-item.mi-new:hover .menu-item-icon,
.menu-item.mi-export:hover .menu-item-icon,
.menu-item.mi-print:hover .menu-item-icon,
.menu-item.mi-about:hover .menu-item-icon {
  transform: translateY(-1.5px) scale(1.18);
}
/* 右侧内容面板（主题色）；最小宽度保证可用，避免被第三列挤没 */
.menu-content {
  flex: 1;
  min-width: 320px;
  height: 100%;
  overflow-y: auto;
}
/* 偏好设置第三列（选中卡片后出现）：独立表单列，内容自滚动；
   与中间列同一主题底色 + 发丝分隔线，整列统一观感；
   空间不足时优先收缩本列（640 → 400），而非挤占中间内容列 */
.menu-prefs-column {
  width: 640px;
  flex: 0 1 640px;
  min-width: 400px;
  height: 100%;
  overflow: hidden;
  background: var(--t-editor-background);
  border-left: 1px solid var(--t-table-border);
}
.menu-prefs-item-active {
  background: color-mix(in srgb, var(--t-tab-indicator) 14%, transparent);
  border-color: color-mix(in srgb, var(--t-tab-indicator) 40%, transparent);
}
.menu-slide-enter-active,
.menu-slide-leave-active {
  transition: transform 0.2s ease, opacity 0.2s ease;
}
.menu-slide-enter-from,
.menu-slide-leave-to {
  transform: translateX(-6%);
  opacity: 0;
}
/* 页面元素 */
.menu-page {
  max-width: 720px;
  padding: 40px 48px;
}
.menu-page-title {
  font-size: 26px;
  font-weight: 600;
  margin-bottom: 20px;
}
.menu-page-subtitle {
  font-size: 17px;
  font-weight: 600;
  margin: 28px 0 10px;
}
.menu-btn {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  max-width: 460px;
  padding: 10px 14px;
  margin-bottom: 10px;
  border: 1px solid var(--t-table-border);
  border-radius: 8px;
  background: transparent;
  color: inherit;
  font-size: 14px;
  cursor: pointer;
  text-align: left;
  transition: background 0.12s ease, transform 0.18s ease, box-shadow 0.18s ease, border-color 0.18s ease;
}
/* 卡片按钮：滑过浮起 + 投影 + 边框点亮 + 图标弹跳 */
.menu-btn:hover {
  background: var(--t-status-bar-button-hover);
  transform: translateY(-2px);
  box-shadow: 0 6px 16px rgb(0 0 0 / 0.18);
  border-color: color-mix(in srgb, var(--t-tab-indicator) 40%, transparent);
}
.menu-btn:active {
  transform: translateY(0);
  box-shadow: none;
  transition-duration: 0.06s;
}
.menu-btn-icon {
  font-size: 15px;
  display: inline-block;
  transition: transform 0.22s cubic-bezier(0.2, 0.9, 0.3, 1.5);
}
.menu-btn:hover .menu-btn-icon {
  transform: scale(1.3) rotate(-10deg);
}
.menu-filter-row {
  margin-bottom: 10px;
}
.menu-filter {
  width: 100%;
  max-width: 460px;
  padding: 7px 10px;
  border: 1px solid var(--t-table-border);
  border-radius: 6px;
  background: transparent;
  color: inherit;
  outline: none;
}
.menu-recent {
  max-width: 640px;
}
.menu-recent-item {
  display: flex;
  justify-content: space-between;
  gap: 16px;
  padding: 8px 10px;
  border-radius: 6px;
  cursor: pointer;
}
.menu-recent-item:hover {
  background: var(--t-status-bar-button-hover);
}
.menu-recent-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.menu-recent-dir {
  color: var(--t-text-placeholder);
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  direction: rtl;
  text-align: right;
  flex-shrink: 0;
  max-width: 60%;
}
.menu-prefs-item {
  max-width: 460px;
  padding: 10px 12px;
  border: 1px solid var(--t-table-border);
  border-radius: 8px;
  margin-bottom: 10px;
  cursor: pointer;
  transition: background 0.1s, border-color 0.1s;
}
.menu-prefs-item:hover {
  background: var(--t-status-bar-button-hover);
}
.menu-prefs-label {
  font-weight: 600;
  margin-bottom: 2px;
}
.menu-prefs-desc,
.menu-dim {
  color: var(--t-text-placeholder);
  font-size: 12px;
}
</style>
