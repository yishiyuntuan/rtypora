<script setup>
import { computed } from 'vue';
import InlineView from './InlineView.vue';
import { plainText, numberedOrdinals } from '../utils/wysiwyg.js';

// 递归块渲染器：渲染 velotype 移植版块模型（16 种块类型）。
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

// 子块的有序序号表（quote/callout/footnote/列表项嵌套共用）
const childOrdinals = computed(() => numberedOrdinals(props.block.children));

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
  <p v-if="block.type === 'paragraph'" class="blk-paragraph my-2 whitespace-pre-wrap">
    <InlineView :tree="block.title" />
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
      :disabled="block.start == null"
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
      :class="block.language ? 'rounded-b' : 'rounded'"
    ><code>{{ plainText(block.title) }}</code></pre>
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

  <!-- 数学公式占位块（KaTeX 渲染为后续阶段） -->
  <div v-else-if="block.type === 'mathBlock'" class="md-placeholder blk-math-block my-2 rounded border p-3">
    <div class="t-dim mb-1 text-[11px] font-medium">数学公式</div>
    <pre class="overflow-x-auto font-mono text-[13px]">{{ rawText }}</pre>
  </div>

  <!-- Mermaid 图占位块（mermaid.js 渲染为后续阶段） -->
  <div v-else-if="block.type === 'mermaidBlock'" class="md-placeholder blk-mermaid-block my-2 rounded border p-3">
    <div class="t-dim mb-1 text-[11px] font-medium">Mermaid 图表</div>
    <pre class="overflow-x-auto font-mono text-[13px]">{{ rawText }}</pre>
  </div>

  <!-- 注释块（可见） -->
  <pre
    v-else-if="block.type === 'comment'"
    class="md-pre md-comment blk-comment my-2 overflow-x-auto rounded p-3 font-mono text-[13px]"
  >{{ rawText }}</pre>

  <!-- htmlBlock / rawMarkdown：源码展示，避免注入 -->
  <pre
    v-else-if="block.type === 'htmlBlock' || block.type === 'rawMarkdown'"
    class="md-pre my-2 overflow-x-auto rounded p-3 font-mono text-[13px]"
    :class="block.type === 'htmlBlock' ? 'blk-html-block' : 'blk-raw-markdown'"
  >{{ rawText }}</pre>
</template>
