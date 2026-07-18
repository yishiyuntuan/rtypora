<script setup>
const emit = defineEmits(['toggle-sidebar', 'toggle-source']);

defineProps({
  wordCount: { type: Number, default: 0 },
  charCount: { type: Number, default: 0 },
  lineCount: { type: Number, default: 0 },
  cursorLine: { type: Number, default: 1 },
  cursorColumn: { type: Number, default: 1 },
  sourceMode: { type: Boolean, default: false },
  sidebarVisible: { type: Boolean, default: true },
});
</script>

<template>
  <div
    class="flex h-7 items-center justify-between border-t border-black/10 bg-[#f0f0f0] text-[12px] text-[#555] select-none dark:border-white/10 dark:bg-[#252526] dark:text-[#ccc]"
  >
    <div class="flex h-full items-center">
      <div
        class="inline-flex h-full cursor-pointer items-center gap-1 px-3 transition-[background] duration-[0.08s] ease-in-out hover:bg-black/10 active:bg-black/15 dark:hover:bg-white/10 dark:active:bg-white/15"
        :class="{ 'bg-black/10 dark:bg-white/10': sidebarVisible }"
        @click="emit('toggle-sidebar')"
      >
        <svg viewBox="0 0 16 16" aria-hidden="true" class="size-[14px]">
          <rect x="1" y="2" width="5" height="12" rx="1" fill="currentColor" />
          <rect :x="sidebarVisible ? 9 : 7" y="2" :width="sidebarVisible ? 6 : 8" height="12" rx="1" fill="none" stroke="currentColor" stroke-width="1" />
        </svg>
      </div>

      <div
        class="inline-flex h-full cursor-pointer items-center gap-1 px-3 transition-[background] duration-[0.08s] ease-in-out hover:bg-black/10 active:bg-black/15 dark:hover:bg-white/10 dark:active:bg-white/15"
        :class="{ 'bg-black/10 dark:bg-white/10': sourceMode }"
        @click="emit('toggle-source')"
      >
        <span>&lt;/&gt;</span>
      </div>
    </div>

    <div class="flex h-full items-center">
      <div class="inline-flex h-full items-center px-2">
        Ln {{ cursorLine }}, Col {{ cursorColumn }}
      </div>
      <div class="inline-flex h-full items-center px-2">
        {{ lineCount }} 行
      </div>
      <div class="inline-flex h-full items-center px-2">
        {{ wordCount }} 词
      </div>
      <div class="inline-flex h-full items-center px-2">
        {{ charCount }} 字符
      </div>
    </div>
  </div>
</template>
