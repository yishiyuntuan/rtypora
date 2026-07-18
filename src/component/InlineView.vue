<script setup>
// 递归行内渲染器：渲染 Rust 解析出的 Inline 节点数组
defineProps({
  inlines: { type: Array, default: () => [] },
});
</script>

<template>
  <template v-for="(node, i) in inlines" :key="i">
    <strong v-if="node.type === 'bold'" class="font-semibold">
      <InlineView :inlines="node.children" />
    </strong>
    <em v-else-if="node.type === 'italic'">
      <InlineView :inlines="node.children" />
    </em>
    <s v-else-if="node.type === 'strikethrough'">
      <InlineView :inlines="node.children" />
    </s>
    <code
      v-else-if="node.type === 'code'"
      class="rounded bg-black/5 px-1 py-0.5 font-mono text-[0.88em] dark:bg-white/10"
    >{{ node.code }}</code>
    <a
      v-else-if="node.type === 'link'"
      :href="node.dest"
      :title="node.title || undefined"
      class="text-blue-600 underline decoration-blue-600/40 underline-offset-2 dark:text-blue-400"
      @click.prevent
    ><InlineView :inlines="node.children" /></a>
    <img
      v-else-if="node.type === 'image'"
      :src="node.src"
      :alt="node.alt"
      :title="node.title || undefined"
      class="my-1 inline-block max-w-full"
    />
    <br v-else-if="node.type === 'hardBreak'" />
    <template v-else-if="node.type === 'softBreak'">{{ '\n' }}</template>
    <template v-else>{{ node.text }}</template>
  </template>
</template>
