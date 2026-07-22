<script setup>
import { computed, inject, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import InlineView from './InlineView.vue';
import MathView from './MathView.vue';
import SectionView from './SectionView.vue';
import { plainText, numberedOrdinals } from '../utils/wysiwyg.js';
import { highlightCodeHtml } from '../utils/highlight.js';
import { resolveImageSrc } from '../utils/image.js';
import { getPref, prefsVersion } from '../utils/prefs.js';

// 递归块渲染器：渲染 velotype 移植版块模型（17 种块类型）。
// 顶层块的编辑状态由 Editor.vue 管理，本组件只负责渲染与事件转发。
// 列表项在模型中是独立块（嵌套经 children），此处逐项渲染标记。
// 颜色全部走 style.css 语义类（--t-* 主题变量），此处只写布局类。
const props = defineProps({
  block: { type: Object, required: true },
  // 有序列表序号（由父级按连续 numberedListItem 兄弟计算）
  ordinal: { type: Number, default: 1 },
});

const emit = defineEmits(['toggle-task']);

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
// 代码块行号（偏好 render_code_line_numbers 控制，默认显示；prefsVersion 驱动响应）
const showLineNumbers = computed(() => {
  prefsVersion.value;
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
watch(
  () => [props.block.id, props.block.language, props.block.title, prefsVersion.value],
  async () => {
    // 偏好设置可关闭语法高亮（回退纯文本）
    highlightedCode.value = '';
    if (props.block.type !== 'codeBlock' || !getPref('render_code_highlight')) return;
    highlightedCode.value = await highlightCodeHtml(plainText(props.block.title), props.block.language);
  },
  { immediate: true },
);

// Mermaid 图：Rust render_mermaid 渲染 SVG（围栏剥离在 Rust 端完成；失败返回源码占位）
const mermaidSvg = ref('');
watch(
  () => [props.block.id, rawText.value, prefsVersion.value],
  async () => {
    mermaidSvg.value = '';
    // 偏好设置可关闭 Mermaid 渲染（回退源码占位）
    if (props.block.type !== 'mermaidBlock' || !getPref('render_mermaid')) return;
    const svg = await invoke('render_mermaid', { source: rawText.value }).catch(() => null);
    if (svg) mermaidSvg.value = svg;
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

// 子块的有序序号表（quote/callout/footnote/列表项嵌套共用）
const childOrdinals = computed(() => numberedOrdinals(props.block.children));
// 展示公式编号（Editor provide 的编号表；无编号为空串）
const mathNumbers = inject('mathNumbers', { value: new Map() });
const mathNumberLabel = computed(() => {
  const n = mathNumbers.value.get(props.block.id);
  return n ? `(${n})` : '';
});

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
    <span v-else-if="block.type === 'numberedListItem'" class="md-marker shrink-0 select-none">{{ ordinal }}.</span>
    <span v-else class="md-marker shrink-0 select-none">•</span>
    <div class="min-w-0 flex-1">
      <div class="whitespace-pre-wrap"><InlineView :tree="block.title" /></div>
      <div v-if="block.children?.length" class="pl-4">
        <BlockView
          v-for="child in block.children"
          :key="child.id"
          :block="child"
          :ordinal="childOrdinals.get(child.id) || 1"
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
      @toggle-task="emit('toggle-task', $event)"
    />
  </div>

  <div v-else-if="block.type === 'footnoteDefinition'" class="blk-footnote-definition t-dim my-2 text-[13px]">
    <span class="align-super text-[0.75em]">[^{{ plainText(block.title) }}]:</span>
    <BlockView
      v-for="child in block.children || []"
      :key="child.id"
      :block="child"
      :ordinal="childOrdinals.get(child.id) || 1"
      @toggle-task="emit('toggle-task', $event)"
    />
  </div>

  <div v-else-if="block.type === 'codeBlock'" class="my-2">
    <div
      v-if="block.language"
      class="md-code rounded-t px-3 py-1 font-mono text-[11px]"
    >{{ block.language }}</div>
    <pre
      class="md-pre blk-code-block overflow-x-auto p-3 font-mono text-[13px]"
      :class="[block.language ? 'rounded-b' : 'rounded', showLineNumbers ? 'flex' : '']"
    ><code
        v-if="showLineNumbers"
        class="md-code-gutter mr-3 shrink-0 select-none border-r pr-2 text-right"
        aria-hidden="true"
      >{{ lineNumbersText }}</code><code class="min-w-0 flex-1" v-html="highlightedCode || escapedCode"></code></pre>
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
  <div v-else-if="block.type === 'mermaidBlock'" class="md-placeholder blk-mermaid-block my-2 rounded border p-3">
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

  <!-- htmlBlock / rawMarkdown：源码展示，避免注入 -->
  <pre
    v-else-if="block.type === 'htmlBlock' || block.type === 'rawMarkdown'"
    class="md-pre my-2 overflow-x-auto rounded p-3 font-mono text-[13px]"
    :class="block.type === 'htmlBlock' ? 'blk-html-block' : 'blk-raw-markdown'"
  >{{ rawText }}</pre>
</template>
