<script setup>
// 行内渲染器：渲染 velotype InlineTextTree 的 fragments。
// 每个 fragment 的样式为标志位组合，通过递归逐层包裹元素（link → 粗 → 斜 → 删 → 下划线 → 代码 → 上下标）。
defineProps({
  tree: { type: Object, default: null },
  // 递归内部使用：直接渲染单个去掉了部分样式的 fragment
  fragment: { type: Object, default: null },
});

const LINK_CLASS = 'underline underline-offset-2';
const INLINE_CODE_CLASS = 'md-code px-1 py-0.5 font-mono';

function linkHref(link) {
  return link?.destination || link?.target || '';
}

// 递归剥离一层样式包装
function strip(fragment, key) {
  return {
    ...fragment,
    style: { ...fragment.style, [key]: key === 'script' ? 'normal' : false },
  };
}
function withoutLink(fragment) {
  return { ...fragment, link: null };
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
    <sup v-if="fragment.footnote" class="md-footnote-ref align-super text-[0.75em]">[{{ fragment.footnote.id }}]</sup>
    <code v-else-if="fragment.math" :class="INLINE_CODE_CLASS" :title="fragment.math.source">{{ fragment.math.body }}</code>
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
    <code v-else-if="fragment.style?.code" :class="INLINE_CODE_CLASS">{{ fragment.text }}</code>
    <sup v-else-if="fragment.style?.script === 'superscript'">{{ fragment.text }}</sup>
    <sub v-else-if="fragment.style?.script === 'subscript'">{{ fragment.text }}</sub>
    <template v-else>{{ fragment.text }}</template>
  </template>
</template>
