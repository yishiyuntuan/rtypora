 <script setup>
import { ref, computed, watch } from 'vue';

const emit = defineEmits(['update:stats']);

const content = ref('');
const cursorLine = ref(1);
const cursorColumn = ref(1);

const wordCount = computed(() => {
  const text = content.value.trim();
  return text ? text.split(/\s+/).length : 0;
});

const charCount = computed(() => content.value.length);

const lineCount = computed(() => {
  return content.value ? content.value.split('\n').length : 1;
});

watch([wordCount, charCount, lineCount, cursorLine, cursorColumn], () => {
  emit('update:stats', {
    wordCount: wordCount.value,
    charCount: charCount.value,
    lineCount: lineCount.value,
    cursorLine: cursorLine.value,
    cursorColumn: cursorColumn.value,
  });
}, { immediate: true });

function onInput(e) {
  const textarea = e.target;
  const text = textarea.value.substring(0, textarea.selectionStart);
  cursorLine.value = (text.match(/\n/g) || []).length + 1;
  cursorColumn.value = text.length - text.lastIndexOf('\n');
}
</script>

<template>
  <div class="flex h-full flex-col">
    <textarea
      class="flex-1 resize-none border-none bg-transparent p-4 font-mono text-[14px] leading-relaxed text-inherit outline-none"
      placeholder="开始写作..."
      v-model="content"
      @input="onInput"
      @keyup="onInput"
      @click="onInput"
    ></textarea>
  </div>
</template>
