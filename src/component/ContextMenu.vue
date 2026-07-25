<script setup>
import { ref } from 'vue';
import { SLASH_ICON, FONT_COLORS } from '../utils/wysiwyg.js';

// 编辑器右键菜单（Typora 式）：顶部剪贴板图标行 + 复制/粘贴为、行内格式与
// 块类型图标行 + 段落/插入子菜单（hover 展开）。只负责展示与事件转发，
// 动作语义由 Editor 处理；样式走 --t-* 主题变量。
const props = defineProps({
  x: { type: Number, default: 0 },
  y: { type: Number, default: 0 },
  // 当前块类型（段落子菜单勾选态；heading 块传 h1-h6）
  currentType: { type: String, default: 'paragraph' },
});
const emit = defineEmits(['action']);

// hover 展开的子菜单（'copy' | 'para' | 'insert' | null）
const openSub = ref(null);

// 菜单估算尺寸（视口边缘防溢出翻转）
const MENU_W = 210;
const MENU_H = 300;
const pos = {
  left: `${Math.max(8, Math.min(props.x, window.innerWidth - MENU_W - 8))}px`,
  top: `${Math.max(8, Math.min(props.y, window.innerHeight - MENU_H - 8))}px`,
};
// 子菜单靠右展开，右侧空间不足时翻转到左侧
const subFlip = props.x > window.innerWidth - MENU_W - 230;

const COPY_SUB = [
  { id: 'copyMarkdown', label: '复制为 Markdown', shortcut: 'Ctrl+Shift+C' },
  { id: 'copyHtml', label: '复制为 HTML 代码' },
  { id: 'copySimplified', label: '复制内容并简化格式' },
  { id: 'copyPlain', label: '复制为纯文本' },
  'sep',
  { id: 'pastePlain', label: '粘贴为纯文本', shortcut: 'Ctrl+Shift+V' },
];
const PARA_SUB = [
  { id: 'h1', label: '一级标题', shortcut: 'Ctrl+1' },
  { id: 'h2', label: '二级标题', shortcut: 'Ctrl+2' },
  { id: 'h3', label: '三级标题', shortcut: 'Ctrl+3' },
  { id: 'h4', label: '四级标题', shortcut: 'Ctrl+4' },
  { id: 'h5', label: '五级标题', shortcut: 'Ctrl+5' },
  { id: 'h6', label: '六级标题', shortcut: 'Ctrl+6' },
  'sep',
  { id: 'paragraph', label: '段落', shortcut: 'Ctrl+0' },
];
const INSERT_SUB = [
  { id: 'image', label: '图像', shortcut: 'Ctrl+Shift+I' },
  { id: 'footnote', label: '脚注' },
  { id: 'linkRef', label: '链接引用' },
  { id: 'separator', label: '水平分割线' },
  { id: 'table', label: '表格', shortcut: 'Ctrl+T' },
  { id: 'codeBlock', label: '代码块', shortcut: 'Ctrl+Shift+K' },
  { id: 'mathBlock', label: '公式块', shortcut: 'Ctrl+Shift+M' },
  { id: 'toc', label: '内容目录' },
  { id: 'yamlFrontMatter', label: 'YAML Front Matter' },
  'sep',
  { id: 'paraAbove', label: '段落（上方）' },
  { id: 'paraBelow', label: '段落（下方）' },
];

// 剪贴板图标（剪切/复制/粘贴/删除）
const ICON_CUT =
  '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"><circle cx="4.5" cy="4.5" r="2"/><circle cx="4.5" cy="11.5" r="2"/><path d="M6 6l7 6.5M6 10L13 3.5"/></svg>';
const ICON_COPY =
  '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"><rect x="5.5" y="5.5" width="8" height="9" rx="1.2"/><path d="M10.5 5.5v-2a1.2 1.2 0 00-1.2-1.2H3.7A1.2 1.2 0 002.5 3.5v7.6a1.2 1.2 0 001.2 1.2h1.8"/></svg>';
const ICON_PASTE =
  '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="10" height="10.5" rx="1.2"/><path d="M5.5 4V3a1.5 1.5 0 011.5-1.5h2A1.5 1.5 0 0110.5 3v1M5.5 8h5M5.5 11h3.5"/></svg>';
const ICON_TRASH =
  '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"><path d="M2.5 4h11M6.3 4V2.9c0-.5.4-.9.9-.9h1.6c.5 0 .9.4.9.9V4M4.2 4l.6 9.1c0 .6.5 1 1 1h4.4c.5 0 1-.4 1-1L11.8 4M6.8 7v4M9.2 7v4"/></svg>';

const CLIP_ACTIONS = [
  { id: 'cut', label: '剪切', icon: ICON_CUT, color: '#c0392b' },
  { id: 'copy', label: '复制', icon: ICON_COPY, color: '#2f6dbb' },
  { id: 'paste', label: '粘贴', icon: ICON_PASTE, color: '#03b736' },
  { id: 'delete', label: '删除', icon: ICON_TRASH, color: '#e02424' },
];
const FORMAT_ROW1 = [
  { id: 'bold', label: '加粗', text: 'B', color: '#d35d2e' },
  { id: 'italic', label: '斜体', text: 'I', color: '#8250df' },
  { id: 'underline', label: '下划线', text: 'U', color: '#3e69d7' },
  { id: 'strikethrough', label: '删除线', text: 'S', color: '#c0392b' },
];
const FORMAT_ROW2 = [
  { id: 'inlineCode', label: '行内代码', icon: SLASH_ICON.inlineCode, color: '#03b736' },
  { id: 'link', label: '超链接', icon: SLASH_ICON.link, color: '#2f6dbb' },
  { id: 'highlight', label: '高亮', text: '==', color: '#c9a227' },
];
const fontColors = FONT_COLORS;
const fontColorIcon = SLASH_ICON.fontColor;
const BLOCK_ACTIONS = [
  { id: 'quote', label: '引用', icon: SLASH_ICON.quote, color: '#f59102' },
  { id: 'numberedListItem', label: '有序列表', icon: SLASH_ICON.numberedListItem, color: '#03b736' },
  { id: 'bulletedListItem', label: '无序列表', icon: SLASH_ICON.bulletedListItem, color: '#2f9dbb' },
  { id: 'taskListItem', label: '任务列表', icon: SLASH_ICON.taskListItem, color: '#f59102' },
];
</script>

<template>
  <Teleport to="body">
    <div class="t-app">
      <div class="md-ctx-menu" :style="pos" @mousedown.prevent @contextmenu.prevent>
        <!-- 剪贴板 -->
        <div class="md-ctx-icons">
          <button
            v-for="a in CLIP_ACTIONS"
            :key="a.id"
            type="button"
            :title="a.label"
            :style="{ color: a.color }"
            @click="emit('action', a.id)"
            v-html="a.icon"
          ></button>
        </div>
        <div class="md-ctx-sep"></div>
        <!-- 复制/粘贴为... -->
        <div class="md-ctx-item" :class="{ open: openSub === 'copy' }" @mouseenter="openSub = 'copy'">
          <span>复制 / 粘贴为...</span>
          <span class="md-ctx-arrow">▸</span>
          <div v-if="openSub === 'copy'" class="md-ctx-sub" :class="{ flip: subFlip }">
            <template v-for="(it, i) in COPY_SUB" :key="i">
              <div v-if="it === 'sep'" class="md-ctx-sep"></div>
              <div v-else class="md-ctx-sub-item" @click="emit('action', it.id)">
                <span>{{ it.label }}</span>
                <span v-if="it.shortcut" class="md-ctx-shortcut">{{ it.shortcut }}</span>
              </div>
            </template>
          </div>
        </div>
        <div class="md-ctx-sep"></div>
        <!-- 行内格式（B/I/U/S） -->
        <div class="md-ctx-icons" @mouseenter="openSub = null">
          <button
            v-for="a in FORMAT_ROW1"
            :key="a.id"
            type="button"
            :title="a.label"
            :style="{ color: a.color }"
            @click="emit('action', a.id)"
          >{{ a.text }}</button>
        </div>
        <!-- 行内格式（代码/链接/高亮/文字颜色） -->
        <div class="md-ctx-icons">
          <button
            v-for="a in FORMAT_ROW2"
            :key="a.id"
            type="button"
            :title="a.label"
            :style="{ color: a.color }"
            @mouseenter="openSub = null"
            @click="emit('action', a.id)"
          >
            <span v-if="a.icon" v-html="a.icon"></span>
            <template v-else>{{ a.text }}</template>
          </button>
          <!-- 文字颜色：hover 展开色板（与 / 菜单同一色板常量） -->
          <div class="md-ctx-icon-fly" @mouseenter="openSub = 'color'">
            <button type="button" title="文字颜色" style="color: #d35d2e" v-html="fontColorIcon"></button>
            <div v-if="openSub === 'color'" class="md-ctx-sub md-ctx-color-sub" :class="{ flip: subFlip }">
              <div class="md-color-swatches">
                <button
                  v-for="c in fontColors"
                  :key="c.label"
                  type="button"
                  class="md-color-swatch"
                  :title="c.label"
                  :style="{ background: c.css }"
                  @click="emit('action', 'fontColor', c.color)"
                ></button>
              </div>
            </div>
          </div>
        </div>
        <!-- 块类型 -->
        <div class="md-ctx-icons" @mouseenter="openSub = null">
          <button
            v-for="a in BLOCK_ACTIONS"
            :key="a.id"
            type="button"
            :title="a.label"
            :style="{ color: a.color }"
            @click="emit('action', a.id)"
            v-html="a.icon"
          ></button>
        </div>
        <div class="md-ctx-sep"></div>
        <!-- 段落 -->
        <div class="md-ctx-item" :class="{ open: openSub === 'para' }" @mouseenter="openSub = 'para'">
          <span>段落</span>
          <span class="md-ctx-arrow">▸</span>
          <div v-if="openSub === 'para'" class="md-ctx-sub" :class="{ flip: subFlip }">
            <template v-for="(it, i) in PARA_SUB" :key="i">
              <div v-if="it === 'sep'" class="md-ctx-sep"></div>
              <div v-else class="md-ctx-sub-item" @click="emit('action', it.id)">
                <span class="md-ctx-label">
                  <span class="md-ctx-check">{{ currentType === it.id ? '✓' : '' }}</span>
                  {{ it.label }}
                </span>
                <span v-if="it.shortcut" class="md-ctx-shortcut">{{ it.shortcut }}</span>
              </div>
            </template>
          </div>
        </div>
        <!-- 插入 -->
        <div class="md-ctx-item" :class="{ open: openSub === 'insert' }" @mouseenter="openSub = 'insert'">
          <span>插入</span>
          <span class="md-ctx-arrow">▸</span>
          <div v-if="openSub === 'insert'" class="md-ctx-sub" :class="{ flip: subFlip }">
            <template v-for="(it, i) in INSERT_SUB" :key="i">
              <div v-if="it === 'sep'" class="md-ctx-sep"></div>
              <div v-else class="md-ctx-sub-item" @click="emit('action', it.id)">
                <span>{{ it.label }}</span>
                <span v-if="it.shortcut" class="md-ctx-shortcut">{{ it.shortcut }}</span>
              </div>
            </template>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>
