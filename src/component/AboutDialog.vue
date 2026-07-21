<script setup>
// 关于对话框
defineProps({
  visible: { type: Boolean, default: false },
});
const emit = defineEmits(['close']);
</script>

<template>
  <Teleport to="body">
    <div v-if="visible" class="about-backdrop" @click.self="emit('close')">
      <div class="about-panel t-app" @click.stop>
        <div class="flex h-9 items-center justify-between border-b border-(--t-table-border) px-4">
          <span class="text-[13px] font-medium">关于</span>
          <span class="t-btn cursor-pointer rounded px-1.5 text-[15px]" @click="emit('close')">×</span>
        </div>
        <div class="p-5 text-[13px] leading-relaxed">
          <div class="mb-1 text-[16px] font-semibold">tauri-editor</div>
          <div class="t-dim mb-3 text-[12px]">版本 0.1.0</div>
          <p>基于 Tauri 2 + Vue 3 的桌面 Markdown 编辑器，支持所见即所得与源码双模式编辑。</p>
          <p class="mt-2">
            Markdown 解析/序列化核心移植自
            <a href="https://github.com/manyougz/velotype" class="underline" target="_blank" rel="noreferrer">velotype</a>（Apache-2.0），
            公式渲染基于 ratex，图表渲染基于 mermaid-rs-renderer，代码高亮基于 tree-sitter。
          </p>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.about-backdrop {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.3);
  z-index: 95;
  display: flex;
  align-items: center;
  justify-content: center;
}
.about-panel {
  width: 420px;
  border: 1px solid var(--t-table-border);
  border-radius: 10px;
  overflow: hidden;
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.28);
}
</style>
