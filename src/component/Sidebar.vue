<script setup>
import { ref } from 'vue';

defineProps({
  visible: { type: Boolean, default: true },
});

const emit = defineEmits(['update:visible']);

const activeTab = ref('toc');
</script>

<template>
  <Transition name="sidebar">
    <div
      v-show="visible"
      class="flex h-full w-60 flex-col border-r border-black/10 bg-[#f6f6f6] text-[13px] text-[#333] dark:border-white/10 dark:bg-[#2f2f2f] dark:text-[#ccc]"
    >
      <div class="flex border-b border-black/10 dark:border-white/10">
        <div
          class="flex-1 cursor-pointer px-4 py-2 text-center text-[12px] font-medium transition-[background] duration-[0.08s]"
          :class="activeTab === 'toc' ? 'bg-black/10 dark:bg-white/10' : 'hover:bg-black/5 dark:hover:bg-white/5'"
          @click="activeTab = 'toc'"
        >
          目录
        </div>
        <div
          class="flex-1 cursor-pointer px-4 py-2 text-center text-[12px] font-medium transition-[background] duration-[0.08s]"
          :class="activeTab === 'outline' ? 'bg-black/10 dark:bg-white/10' : 'hover:bg-black/5 dark:hover:bg-white/5'"
          @click="activeTab = 'outline'"
        >
          大纲
        </div>
      </div>

      <div class="flex-1 overflow-y-auto p-3">
        <div v-if="activeTab === 'toc'" class="space-y-1">
          <div class="rounded px-2 py-1 text-[12px] text-[#888] dark:text-[#888]">暂无目录</div>
        </div>
        <div v-else class="space-y-1">
          <div class="rounded px-2 py-1 text-[12px] text-[#888] dark:text-[#888]">暂无大纲</div>
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.sidebar-enter-active,
.sidebar-leave-active {
  transition: transform 0.2s ease, opacity 0.2s ease;
}
.sidebar-enter-from,
.sidebar-leave-to {
  transform: translateX(-100%);
  opacity: 0;
}
</style>
