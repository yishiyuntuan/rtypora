<script setup>
// 未保存更改确认框：保存 / 不保存 / 取消
defineProps({
  visible: { type: Boolean, default: false },
  fileName: { type: String, default: '' },
});
const emit = defineEmits(['save', 'discard', 'cancel']);
</script>

<template>
  <Teleport to="body">
    <div v-if="visible" class="confirm-backdrop" @click.self="emit('cancel')">
      <div class="confirm-panel t-app" @click.stop>
        <div class="p-5 text-[13px] leading-relaxed">
          <div class="mb-2 text-[14px] font-semibold">未保存的更改</div>
          <p>文档{{ fileName ? `「${fileName}」` : ''}}有未保存的更改，是否保存？</p>
        </div>
        <div class="flex justify-end gap-2 border-t border-(--t-table-border) px-4 py-3">
          <button class="t-btn rounded border border-(--t-table-border) px-3 py-1 text-[12px]" @click="emit('save')">保存</button>
          <button class="t-btn rounded border border-(--t-table-border) px-3 py-1 text-[12px]" @click="emit('discard')">不保存</button>
          <button class="t-btn rounded border border-(--t-table-border) px-3 py-1 text-[12px]" @click="emit('cancel')">取消</button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.confirm-backdrop {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.3);
  z-index: 96;
  display: flex;
  align-items: center;
  justify-content: center;
}
.confirm-panel {
  width: 360px;
  border: 1px solid var(--t-table-border);
  border-radius: 10px;
  overflow: hidden;
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.28);
}
</style>
