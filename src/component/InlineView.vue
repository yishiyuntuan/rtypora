<script setup vapor>
// 行内渲染器：按 fragment 样式标志递归包裹渲染（粗体/斜体/删除线/下划线/
// 行内代码/上下标/链接/脚注引用/行内公式占位），叶子为 MathView 或纯文本。
// 纯文本叶子按 #rrggbb/#rrggbbaa 命中切分，色值文本后紧跟预览色块（行内代码/链接除外）。
import { computed, inject } from 'vue';
import MathView from './MathView.vue';
import { htmlStyleCss } from '../utils/wysiwyg.js';
import { getPref, renderVersion } from '../utils/prefs.js';

// 行内渲染器：渲染 velotype InlineTextTree 的 fragments。
// 每个 fragment 的样式为标志位组合，通过递归逐层包裹元素（link → 粗 → 斜 → 删 → 下划线 → 代码 → 上下标）。
const props = defineProps({
  tree: { type: Object, default: null },
  // 递归内部使用：直接渲染单个去掉了部分样式的 fragment
  fragment: { type: Object, default: null },
});

const currentBlockId = inject('currentBlockId', null);
const getFootnoteRefIndex = inject('getFootnoteRefIndex', () => 0);
const scrollToFootnote = inject('scrollToFootnote', () => {});

const LINK_CLASS = 'underline underline-offset-2';
const INLINE_CODE_CLASS = 'md-code px-1 py-0.5 font-mono';

function linkHref(link) {
  return link?.destination || link?.target || '';
}

function refIndexOf(fragment) {
  if (!fragment?.footnote || !currentBlockId) return 0;
  return getFootnoteRefIndex(fragment.footnote.id, fragment.footnote.occurrenceIndex, currentBlockId);
}

function onFootnoteClick(fragment) {
  if (fragment?.footnote) scrollToFootnote(fragment.footnote.id);
}

// 递归剥离一层样式包装
function strip(fragment, key) {
  return {
    ...fragment,
    style: { ...fragment.style, [key]: key === 'script' ? 'normal' : false },
  };
}
function withoutLink(fragment) {
  // 链接片段不渲染色值预览块：_noSwatch 随 strip/withoutHtmlStyle 的对象展开逐层传播到内部叶子
  return { ...fragment, link: null, _noSwatch: true };
}
function withoutHtmlStyle(fragment) {
  return { ...fragment, htmlStyle: null };
}

// 行内 hex 色值预览（对齐 MEO，仅 6/8 位）：\B# 要求 # 前非单词字符（避免词中误命中），
// 尾部 \b 保证恰好 6/8 位（7/9 位不命中）；8 位优先于 6 位匹配。
// 色块类名 md-hex-swatch（md-color-swatch 已被字体颜色面板的 22px 色板占用）
const HEX_COLOR_RE = /\B#(?:[0-9a-fA-F]{8}|[0-9a-fA-F]{6})\b/g;

// 文本按色值命中切分为片段序列：[{ text }] 或 [{ text, color }…]（color 段渲染时紧跟色块）。
// 偏好 render_color_swatch 关闭、链接片段（_noSwatch）时不切分原样返回；
// 行内代码在 style.code 分支已被拦截，不会到达文本叶子
function colorTextParts(fragment) {
  renderVersion.value; // 模板渲染期读取版本号即建立响应依赖，切换偏好即时重渲染
  const text = fragment.text || '';
  if (!text || fragment._noSwatch || !getPref('render_color_swatch')) return [{ text }];
  const parts = [];
  let last = 0;
  for (const m of text.matchAll(HEX_COLOR_RE)) {
    if (m.index > last) parts.push({ text: text.slice(last, m.index) });
    parts.push({ text: m[0], color: m[0] });
    last = m.index + m[0].length;
  }
  if (last < text.length) parts.push({ text: text.slice(last) });
  return parts.length ? parts : [{ text }];
}
</script>

<template>
  <!-- 渲染整棵 InlineTextTree -->
  <template v-if="tree">
    <template v-for="(f, i) in tree.fragments || []" :key="i">
      <InlineView :fragment="f" />
    </template>
  </template>

  <!-- 单个 fragment：特殊载体优先，之后逐层剥样式 -->
  <template v-else-if="fragment">
    <a
      v-if="fragment.footnote"
      :data-footnote-ref="refIndexOf(fragment)"
      href="javascript:void(0)"
      class="md-footnote-ref align-super text-[0.75em]"
      @click.stop.prevent="onFootnoteClick(fragment)"
    ><sup>[{{ fragment.footnote.id }}]</sup></a>
    <MathView v-else-if="fragment.math" :source="fragment.math.body" />
    <!-- HTML 行内样式（<span style>/<font>）：最外层包裹，颜色/背景/字号作用于内部全部样式 -->
    <span v-else-if="fragment.htmlStyle" :style="htmlStyleCss(fragment.htmlStyle)"><InlineView :fragment="withoutHtmlStyle(fragment)" /></span>
    <a
      v-else-if="fragment.link"
      :href="linkHref(fragment.link)"
      :title="fragment.link.title || undefined"
      :class="LINK_CLASS"
      @click.prevent
    ><InlineView :fragment="withoutLink(fragment)" /></a>
    <strong v-else-if="fragment.style?.bold" class="font-semibold"><InlineView :fragment="strip(fragment, 'bold')" /></strong>
    <em v-else-if="fragment.style?.italic"><InlineView :fragment="strip(fragment, 'italic')" /></em>
    <s v-else-if="fragment.style?.strikethrough"><InlineView :fragment="strip(fragment, 'strikethrough')" /></s>
    <u v-else-if="fragment.style?.underline"><InlineView :fragment="strip(fragment, 'underline')" /></u>
    <mark v-else-if="fragment.style?.highlight" class="rounded px-0.5"><InlineView :fragment="strip(fragment, 'highlight')" /></mark>
    <kbd v-else-if="fragment.style?.kbd"><InlineView :fragment="strip(fragment, 'kbd')" /></kbd>
    <code v-else-if="fragment.style?.code" :class="INLINE_CODE_CLASS">{{ fragment.text }}</code>
    <!-- 文本叶子（含上下标）：色值命中段文本后紧跟预览色块；无命中时 DOM 与纯文本渲染一致 -->
    <sup v-else-if="fragment.style?.script === 'superscript'"><template v-for="(p, i) in colorTextParts(fragment)" :key="i"><span v-if="p.color">{{ p.text }}<span class="md-hex-swatch" :style="{ backgroundColor: p.color }"></span></span><template v-else>{{ p.text }}</template></template></sup>
    <sub v-else-if="fragment.style?.script === 'subscript'"><template v-for="(p, i) in colorTextParts(fragment)" :key="i"><span v-if="p.color">{{ p.text }}<span class="md-hex-swatch" :style="{ backgroundColor: p.color }"></span></span><template v-else>{{ p.text }}</template></template></sub>
    <template v-else><template v-for="(p, i) in colorTextParts(fragment)" :key="i"><span v-if="p.color">{{ p.text }}<span class="md-hex-swatch" :style="{ backgroundColor: p.color }"></span></span><template v-else>{{ p.text }}</template></template></template>
  </template>
</template>
