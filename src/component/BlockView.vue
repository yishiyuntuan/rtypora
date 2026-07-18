<script setup>
import { computed } from 'vue';
import InlineView from './InlineView.vue';

// 递归块渲染器：按块类型渲染 Rust 解析出的 Block
// 顶层块的编辑状态由 Editor.vue 管理，本组件只负责渲染与事件转发
const props = defineProps({
  block: { type: Object, required: true },
});

const emit = defineEmits(['toggle-task']);

const headingClasses = {
  1: 'my-3 text-2xl',
  2: 'my-3 text-xl',
  3: 'my-2 text-lg',
  4: 'my-2 text-base',
  5: 'my-1 text-sm',
  6: 'my-1 text-xs',
};

// 任务列表（所有项都带勾选框）不显示列表符号
const isTaskList = computed(
  () =>
    props.block.type === 'list' &&
    props.block.items.length > 0 &&
    props.block.items.every((item) => item.checked !== null && item.checked !== undefined),
);

function alignStyle(alignments, index) {
  const align = alignments?.[index];
  return align && align !== 'none' ? { textAlign: align } : {};
}
</script>

<template>
  <p v-if="block.type === 'paragraph'" class="my-2 whitespace-pre-wrap leading-relaxed">
    <InlineView :inlines="block.inlines" />
  </p>

  <component
    :is="'h' + block.level"
    v-else-if="block.type === 'heading'"
    class="whitespace-pre-wrap font-semibold"
    :class="headingClasses[block.level] || 'my-2 text-base'"
  >
    <InlineView :inlines="block.inlines" />
  </component>

  <div v-else-if="block.type === 'codeBlock'" class="my-2">
    <div
      v-if="block.language"
      class="rounded-t bg-black/10 px-3 py-1 font-mono text-[11px] text-[#666] dark:bg-white/15 dark:text-[#aaa]"
    >{{ block.language }}</div>
    <pre
      class="overflow-x-auto bg-black/5 p-3 font-mono text-[13px] leading-relaxed dark:bg-white/10"
      :class="block.language ? 'rounded-b' : 'rounded'"
    ><code>{{ block.code }}</code></pre>
  </div>

  <blockquote
    v-else-if="block.type === 'blockQuote'"
    class="my-2 border-l-4 border-black/15 pl-3 text-[#555] dark:border-white/20 dark:text-[#aaa]"
  >
    <BlockView
      v-for="child in block.children"
      :key="child.id"
      :block="child"
      @toggle-task="emit('toggle-task', $event)"
    />
  </blockquote>

  <component
    :is="block.ordered ? 'ol' : 'ul'"
    v-else-if="block.type === 'list'"
    class="my-2 pl-6"
    :class="isTaskList ? 'list-none pl-4' : block.ordered ? 'list-decimal' : 'list-disc'"
  >
    <li v-for="(item, i) in block.items" :key="i" class="my-0.5">
      <div class="flex items-start gap-1.5">
        <input
          v-if="item.checked !== null && item.checked !== undefined"
          type="checkbox"
          class="mt-[5px] shrink-0 cursor-pointer"
          :checked="item.checked"
          @click.stop
          @change="emit('toggle-task', item)"
        />
        <div class="min-w-0 flex-1">
          <BlockView
            v-for="child in item.children"
            :key="child.id"
            :block="child"
            @toggle-task="emit('toggle-task', $event)"
          />
        </div>
      </div>
    </li>
  </component>

  <table v-else-if="block.type === 'table'" class="my-2 border-collapse text-[13px]">
    <thead>
      <tr>
        <th
          v-for="(cell, ci) in block.head"
          :key="ci"
          class="border border-black/15 bg-black/5 px-2 py-1 font-semibold dark:border-white/20 dark:bg-white/10"
          :style="alignStyle(block.alignments, ci)"
        ><InlineView :inlines="cell" /></th>
      </tr>
    </thead>
    <tbody>
      <tr v-for="(row, ri) in block.rows" :key="ri">
        <td
          v-for="(cell, ci) in row"
          :key="ci"
          class="border border-black/15 px-2 py-1 dark:border-white/20"
          :style="alignStyle(block.alignments, ci)"
        ><InlineView :inlines="cell" /></td>
      </tr>
    </tbody>
  </table>

  <hr v-else-if="block.type === 'thematicBreak'" class="my-4 border-black/15 dark:border-white/20" />

  <!-- HTML 块展示源码，避免注入 -->
  <pre
    v-else-if="block.type === 'html'"
    class="my-2 overflow-x-auto rounded bg-black/5 p-3 font-mono text-[13px] text-[#888] dark:bg-white/10"
  >{{ block.html }}</pre>
</template>
