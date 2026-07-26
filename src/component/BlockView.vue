<script setup vapor>
import { computed, inject, provide, ref, watch, onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import InlineView from './InlineView.vue';
import MathView from './MathView.vue';
import SectionView from './SectionView.vue';
import { plainText, numberedOrdinals } from '../utils/wysiwyg.js';
import { highlightCodeHtml } from '../utils/highlight.js';
import { resolveImageSrc } from '../utils/image.js';
import { getPref, renderVersion } from '../utils/prefs.js';

// 递归块渲染器：渲染 velotype 移植版块模型（17 种块类型）。
// 顶层块的编辑状态由 Editor.vue 管理，本组件只负责渲染与事件转发。
// 列表项在模型中是独立块（嵌套经 children），此处逐项渲染标记。
// 颜色全部走 style.css 语义类（--t-* 主题变量），此处只写布局类。
const props = defineProps({
  block: { type: Object, required: true },
  // 有序列表序号（由父级按连续 numberedListItem 兄弟计算）
  ordinal: { type: Number, default: 1 },
  // 列表嵌套层级（0 起；列表项子块 +1，引用/callout/脚注子块不变）
  depth: { type: Number, default: 0 },
});

const emit = defineEmits(['toggle-task']);

// 列表标记按层级区分（体现父子层级）：圆点 •/◦/▪，序号 1./a./i.
const bulletMarker = computed(() => ['•', '◦', '▪'][Math.min(props.depth, 2)]);
const numberMarker = computed(() => {
  if (props.depth === 0) return `${props.ordinal}.`;
  if (props.depth === 1) return `${alphaOrdinal(props.ordinal)}.`;
  return `${romanOrdinal(props.ordinal)}.`;
});

// 二级序号：a, b, ..., z, aa, ab...
function alphaOrdinal(n) {
  let s = '';
  let x = Math.max(1, n);
  while (x > 0) {
    x -= 1;
    s = String.fromCharCode(97 + (x % 26)) + s;
    x = Math.floor(x / 26);
  }
  return s;
}

// 三级及更深序号：小写罗马数字 i, ii, iii, iv, v...
function romanOrdinal(n) {
  const table = [
    [1000, 'm'], [900, 'cm'], [500, 'd'], [400, 'cd'],
    [100, 'c'], [90, 'xc'], [50, 'l'], [40, 'xl'],
    [10, 'x'], [9, 'ix'], [5, 'v'], [4, 'iv'], [1, 'i'],
  ];
  let x = Math.max(1, n);
  let s = '';
  for (const [v, sym] of table) {
    while (x >= v) {
      s += sym;
      x -= v;
    }
  }
  return s;
}

const headingClasses = {
  1: 'my-3',
  2: 'my-3',
  3: 'my-2',
  4: 'my-2',
  5: 'my-1',
  6: 'my-1',
};

// callout 变体：语义类名（配色在 style.css 的 .callout-* 规则中）
const calloutLabels = {
  note: 'Note',
  tip: 'Tip',
  important: 'Important',
  warning: 'Warning',
  caution: 'Caution',
};

const isListItem = computed(() =>
  ['bulletedListItem', 'taskListItem', 'numberedListItem'].includes(props.block.type),
);

const rawText = computed(() => props.block.rawFallback ?? plainText(props.block.title));

// 代码块语法高亮（Rust tree-sitter，异步取 span；未就绪时先按纯文本渲染）
const escapedCode = computed(() =>
  props.block.type === 'codeBlock' ? escapeHtml(plainText(props.block.title)) : '',
);
// 代码块行号（偏好 render_code_line_numbers 控制，默认显示；renderVersion 驱动响应）
const showLineNumbers = computed(() => {
  renderVersion.value;
  return props.block.type === 'codeBlock' && getPref('render_code_line_numbers');
});
const lineNumbersText = computed(() => {
  const count = (plainText(props.block.title).match(/\n/g) || []).length + 1;
  return Array.from({ length: count }, (_, i) => i + 1).join('\n');
});
const highlightedCode = ref('');
function escapeHtml(text) {
  return String(text).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
}
// 重活懒执行：高亮/Mermaid/公式经 Editor 提供的可见性回调延后到块进入视口
//（mounted 前跳过，挂载后 watcher 因 mounted 变化重触发并注册观察）
const rootEl = ref(null);
const mounted = ref(false);
const onBlockVisible = inject('onBlockVisible', (el, cb) => cb());
onMounted(() => {
  mounted.value = true;
});
watch(
  () => [props.block.id, props.block.language, props.block.title, renderVersion.value, mounted.value],
  async () => {
    // 偏好设置可关闭语法高亮（回退纯文本）
    highlightedCode.value = '';
    if (props.block.type !== 'codeBlock' || !getPref('render_code_highlight') || !mounted.value) return;
    onBlockVisible(rootEl.value, async () => {
      highlightedCode.value = await highlightCodeHtml(plainText(props.block.title), props.block.language);
    });
  },
  { immediate: true },
);

// Mermaid 图：Rust render_mermaid 渲染 SVG（围栏剥离在 Rust 端完成；失败返回源码占位）
const mermaidSvg = ref('');
watch(
  () => [props.block.id, rawText.value, renderVersion.value, mounted.value],
  async () => {
    mermaidSvg.value = '';
    // 偏好设置可关闭 Mermaid 渲染（回退源码占位）
    if (props.block.type !== 'mermaidBlock' || !getPref('render_mermaid') || !mounted.value) return;
    onBlockVisible(rootEl.value, async () => {
      const svg = await invoke('render_mermaid', { source: rawText.value }).catch(() => null);
      if (svg) mermaidSvg.value = svg;
    });
  },
  { immediate: true },
);

// 独立图片段落：相对路径以文档目录为基准解析（App provide）
const documentDir = inject('documentDir', { value: null });
const imageSrc = ref('');
watch(
  () => [props.block.image?.src, documentDir.value],
  async () => {
    imageSrc.value = '';
    if (!props.block.image) return;
    imageSrc.value = await resolveImageSrc(props.block.image.src, documentDir.value);
  },
  { immediate: true },
);

// 复制代码块内容（复制成功短暂切换为对勾图标）
const codeCopied = ref(false);
let codeCopiedTimer = null;
async function copyCodeText() {
  await navigator.clipboard.writeText(plainText(props.block.title)).catch(() => {});
  codeCopied.value = true;
  clearTimeout(codeCopiedTimer);
  codeCopiedTimer = setTimeout(() => {
    codeCopied.value = false;
  }, 1200);
}

// 子块的有序序号表（quote/callout/footnote/列表项嵌套共用）
const childOrdinals = computed(() => numberedOrdinals(props.block.children));

// 内容目录（[TOC] 段落）：渲染文档大纲（数据源与滚动定位由 Editor provide）
const allBlocks = inject('allBlocks', { value: [] });
const scrollToBlock = inject('scrollToBlock', null);
const isTocBlock = computed(
  () => props.block.type === 'paragraph' && plainText(props.block.title).trim() === '[TOC]',
);
const tocHeadings = computed(() => {
  if (!isTocBlock.value) return [];
  const list = [];
  const walk = (nodes) => {
    for (const n of nodes || []) {
      if (n.type === 'heading') list.push(n);
      if (n.children?.length) walk(n.children);
    }
  };
  walk(allBlocks.value);
  return list;
});
const tocMinLevel = computed(() =>
  tocHeadings.value.reduce((min, h) => Math.min(min, h.level ?? 1), 7),
);
// 展示公式编号（Editor provide 的编号表；无编号为空串）
const mathNumbers = inject('mathNumbers', { value: new Map() });
const mathNumberLabel = computed(() => {
  const n = mathNumbers.value.get(props.block.id);
  return n ? `(${n})` : '';
});

// 向后代 InlineView 传递当前块 id，用于脚注引用全局序号匹配
provide('currentBlockId', props.block.id);

// 脚注定义区：当前定义 id 与所有引用它的位置列表
const footnoteRefList = inject('footnoteRefList', { value: [] });
const scrollToFootnoteRef = inject('scrollToFootnoteRef', () => {});
const footnoteId = computed(() => plainText(props.block.title));
const footnoteBackRefs = computed(() =>
  props.block.type === 'footnoteDefinition'
    ? footnoteRefList.value.filter((r) => r.id === footnoteId.value)
    : [],
);

function scrollToFootnoteRefFrom(ref) {
  scrollToFootnoteRef(ref.refIndex);
}
function scrollToFirstFootnoteRef() {
  const first = footnoteBackRefs.value[0];
  if (first) scrollToFootnoteRef(first.refIndex);
}

const calloutClass = computed(() => `blk-callout callout-${props.block.variant || 'note'}`);
const listItemClass = computed(() => `blk-${props.block.type.replace(/[A-Z]/g, (ch) => '-' + ch.toLowerCase())}`);
const calloutLabelStyle = computed(() => ({
  color: `var(--t-callout-${props.block.variant || 'note'}-border)`,
}));

function alignStyle(alignments, index) {
  const align = alignments?.[index];
  return align && align !== 'default' ? { textAlign: align } : {};
}
</script>

<template>
  <!-- 独立图片段落：渲染图片，加载失败显示占位 -->
  <div v-if="block.type === 'paragraph' && block.image" class="my-2">
    <img
      v-if="imageSrc"
      :src="imageSrc"
      :alt="block.image.alt"
      :title="block.image.title || undefined"
      class="mx-auto block max-w-full rounded-lg"
    />
    <div v-else class="md-placeholder rounded border p-3 text-center text-[12px]">
      {{ block.image.alt || '图片' }}（无法加载 {{ block.image.src }}）
    </div>
  </div>

  <!-- 内容目录（[TOC] 段落）：渲染文档大纲，点击标题定位（须在通用段落分支之前） -->
  <div v-else-if="isTocBlock" class="md-toc my-2">
    <div
      v-for="h in tocHeadings"
      :key="h.id"
      class="md-toc-item"
      :style="{ paddingLeft: `${(h.level - tocMinLevel) * 16}px` }"
      :title="plainText(h.title)"
      @click="scrollToBlock?.(h.id)"
    >{{ plainText(h.title) }}</div>
    <div v-if="!tocHeadings.length" class="t-dim text-[12px]">暂无标题</div>
  </div>

  <p v-else-if="block.type === 'paragraph'" class="blk-paragraph my-2 whitespace-pre-wrap">
    <!-- 空段落也要占位一行高度，否则 Enter 新建的空块不可见 -->
    <InlineView v-if="block.title?.fragments?.length" :tree="block.title" />
    <br v-else />
  </p>

  <component
    :is="'h' + block.level"
    v-else-if="block.type === 'heading'"
    class="blk-heading whitespace-pre-wrap"
    :class="headingClasses[block.level] || 'my-2'"
  >
    <InlineView :tree="block.title" />
  </component>

  <hr v-else-if="block.type === 'separator'" class="blk-separator my-4" />

  <!-- 列表项：模型中是独立块，逐项渲染标记 + 标题 + 嵌套子块 -->
  <div v-else-if="isListItem" class="my-0.5 flex items-start gap-1.5" :class="listItemClass">
    <input
      v-if="block.type === 'taskListItem'"
      type="checkbox"
      class="mt-[5px] shrink-0 cursor-pointer"
      :checked="block.checked"
      @click.stop
      @change="emit('toggle-task', block)"
    />
    <span v-else-if="block.type === 'numberedListItem'" class="md-marker shrink-0 select-none">{{ numberMarker }}</span>
    <span v-else class="md-marker shrink-0 select-none">{{ bulletMarker }}</span>
    <div class="min-w-0 flex-1">
      <div class="whitespace-pre-wrap"><InlineView :tree="block.title" /></div>
      <div v-if="block.children?.length" class="pl-4">
        <BlockView
          v-for="child in block.children"
          :key="child.id"
          :block="child"
          :ordinal="childOrdinals.get(child.id) || 1"
          :depth="depth + 1"
          @toggle-task="emit('toggle-task', $event)"
        />
      </div>
    </div>
  </div>

  <blockquote v-else-if="block.type === 'quote'" class="blk-quote my-2 border-l-4 pl-3">
    <p v-if="plainText(block.title)" class="my-1 whitespace-pre-wrap">
      <InlineView :tree="block.title" />
    </p>
    <BlockView
      v-for="child in block.children || []"
      :key="child.id"
      :block="child"
      :ordinal="childOrdinals.get(child.id) || 1"
      :depth="depth"
      @toggle-task="emit('toggle-task', $event)"
    />
  </blockquote>

  <div v-else-if="block.type === 'callout'" class="my-2 border-l-4 p-3" :class="calloutClass">
    <div class="mb-1 text-[12px] font-semibold uppercase tracking-wide" :style="calloutLabelStyle">
      {{ calloutLabels[block.variant] || 'Note' }}
    </div>
    <p v-if="plainText(block.title)" class="my-1 whitespace-pre-wrap">
      <InlineView :tree="block.title" />
    </p>
    <BlockView
      v-for="child in block.children || []"
      :key="child.id"
      :block="child"
      :ordinal="childOrdinals.get(child.id) || 1"
      :depth="depth"
      @toggle-task="emit('toggle-task', $event)"
    />
  </div>

  <div
    v-else-if="block.type === 'footnoteDefinition'"
    class="blk-footnote-definition"
    :data-footnote-def="footnoteId"
  >
    <div class="md-footnote-id">
      <a
        href="javascript:void(0)"
        class="md-footnote-ref"
        @click.stop.prevent="scrollToFirstFootnoteRef"
      >[{{ footnoteId }}]:</a>
    </div>
    <div class="md-footnote-content">
      <BlockView
        v-for="child in block.children || []"
        :key="child.id"
        :block="child"
        :ordinal="childOrdinals.get(child.id) || 1"
        :depth="depth"
        @toggle-task="emit('toggle-task', $event)"
      />
    </div>
    <div class="md-footnote-back">
      <sup
        v-for="ref in footnoteBackRefs"
        :key="ref.blockId + ':' + ref.occurrenceIndex"
        class="md-footnote-back-item"
      >
        <a
          href="javascript:void(0)"
          @click.stop.prevent="scrollToFootnoteRefFrom(ref)"
        >[{{ ref.refIndex }}]</a>
      </sup>
    </div>
  </div>

  <div v-else-if="block.type === 'codeBlock'" ref="rootEl" class="my-2">
    <div class="group relative">
      <!-- 右上角角标区：悬停/聚焦时浮现（语言徽章 + 复制按钮），平时隐藏 -->
      <div class="md-code-corner">
        <span v-if="block.language" class="md-code-lang-badge">{{ block.language }}</span>
        <button
          type="button"
          class="md-code-copy"
          :title="codeCopied ? '已复制' : '复制代码'"
          @click.stop="copyCodeText"
        >
          <svg v-if="!codeCopied" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">
            <rect x="5" y="5" width="8.5" height="9" rx="1.2" />
            <path d="M10.5 5V3.5A1.5 1.5 0 009 2H4.5A1.5 1.5 0 003 3.5V11a1.5 1.5 0 001.5 1.5H5" />
          </svg>
          <svg v-else viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
            <path d="M3 8.5l3.5 3.5L13 4.5" />
          </svg>
        </button>
      </div>
      <pre
        class="md-pre blk-code-block overflow-x-auto rounded p-3 font-mono text-[13px]"
        :class="[showLineNumbers ? 'flex' : '']"
      ><code
          v-if="showLineNumbers"
          class="md-code-gutter mr-3 shrink-0 select-none border-r pr-2 text-right"
          aria-hidden="true"
        >{{ lineNumbersText }}</code><code class="min-w-0 flex-1" v-html="highlightedCode || escapedCode"></code></pre>
    </div>
  </div>

  <table v-else-if="block.type === 'table' && block.table" class="blk-table my-2 border-collapse text-[13px]">
    <thead>
      <tr>
        <th
          v-for="(cell, ci) in block.table.header"
          :key="ci"
          class="border px-2 py-1 font-semibold"
          :style="alignStyle(block.table.alignments, ci)"
        ><InlineView :tree="cell" /></th>
      </tr>
    </thead>
    <tbody>
      <tr v-for="(row, ri) in block.table.rows" :key="ri">
        <td
          v-for="(cell, ci) in row"
          :key="ci"
          class="border px-2 py-1"
          :style="alignStyle(block.table.alignments, ci)"
        ><InlineView :tree="cell" /></td>
      </tr>
    </tbody>
  </table>

  <!-- 数学公式：Rust ratex 渲染 SVG（失败回退源码占位）；渲染态不显示背景块；
       编号按偏好 math_numbering 显示（AMS 判定在 Rust mathNumbered 字段） -->
  <div v-else-if="block.type === 'mathBlock'" class="blk-math-block relative my-2">
    <MathView :source="rawText" display />
    <span v-if="mathNumberLabel" class="md-eq-number absolute right-2 top-1/2 -translate-y-1/2">{{ mathNumberLabel }}</span>
  </div>

  <!-- Mermaid 图：Rust 渲染 SVG（加载中/失败时回退源码占位） -->
  <div v-else-if="block.type === 'mermaidBlock'" ref="rootEl" class="md-placeholder blk-mermaid-block my-2 rounded border p-3">
    <div v-if="mermaidSvg" class="mermaid-diagram" v-html="mermaidSvg"></div>
    <template v-else>
      <div class="t-dim mb-1 text-[11px] font-medium">Mermaid 图表</div>
      <pre class="overflow-x-auto font-mono text-[13px]">{{ rawText }}</pre>
    </template>
  </div>

  <!-- 注释块（可见） -->
  <pre
    v-else-if="block.type === 'comment'"
    class="md-pre md-comment blk-comment my-2 overflow-x-auto rounded p-3 font-mono text-[13px]"
  >{{ rawText }}</pre>

  <!-- section 图文排版块：grid 布局渲染（关闭渲染开关时回退源码展示） -->
  <div v-else-if="block.type === 'sectionBlock'" class="blk-section-block my-2">
    <SectionView :raw="rawText" />
    <pre
      v-if="!getPref('render_html_block')"
      class="md-pre my-2 overflow-x-auto rounded p-3 font-mono text-[13px]"
    >{{ rawText }}</pre>
  </div>

  <!-- htmlBlock：独立 <img> HTML 行按图片渲染（含 zoom 缩放，src 经文档目录解析）；
       其余 htmlBlock / rawMarkdown 源码展示，避免注入 -->
  <div v-else-if="block.type === 'htmlBlock' && block.image" class="my-2">
    <img
      v-if="imageSrc"
      :src="imageSrc"
      :alt="block.image.alt"
      :style="block.image.zoom ? { zoom: block.image.zoom } : undefined"
      class="mx-auto block max-w-full rounded-lg"
    />
    <div v-else class="md-placeholder rounded border p-3 text-center text-[12px]">
      {{ block.image.alt || '图片' }}（无法加载 {{ block.image.src }}）
    </div>
  </div>
  <pre
    v-else-if="block.type === 'htmlBlock' || block.type === 'rawMarkdown'"
    class="md-pre my-2 overflow-x-auto rounded p-3 font-mono text-[13px]"
    :class="block.type === 'htmlBlock' ? 'blk-html-block' : 'blk-raw-markdown'"
  >{{ rawText }}</pre>
</template>
