<script setup vapor>
import { ref, inject, watch, onMounted, onBeforeUnmount } from 'vue';

// 虚拟滚动行：所有行常驻 DOM（占位保证文档总高正确、滚动位置稳定），
// 仅视口余量（Editor 观察器 rootMargin 800px）内挂载内容；
// 远离后卸载内容并按实测高度占位，避免滚动条跳动。
// 首次挂载前按源码行数估计高度（Editor 传入 estimate），卸载时替换为实测值。
const props = defineProps({
  block: { type: Object, required: true },
  estimate: { type: Number, default: 32 },
  // 编辑块强制渲染（编辑容器不可卸载，否则丢失光标与未提交内容）
  force: { type: Boolean, default: false },
});

const observeRow = inject('observeRow');
const unobserveRow = inject('unobserveRow');

const el = ref(null);
const visible = ref(props.force);
const cachedHeight = ref(props.estimate);

function onVisible(isVisible) {
  if (isVisible) {
    visible.value = true;
  } else if (!props.force && visible.value) {
    // 卸载前缓存实测高度（估计值被修正为真值，滚动几乎不跳）
    if (el.value) cachedHeight.value = el.value.offsetHeight;
    visible.value = false;
  }
}

// 程序性进入编辑（如大纲/目录定位）时立即渲染
watch(
  () => props.force,
  (v) => {
    if (v) visible.value = true;
  },
);

onMounted(() => observeRow(el.value, onVisible));
onBeforeUnmount(() => unobserveRow(el.value));
</script>

<template>
  <div
    ref="el"
    class="md-vrow"
    :data-block-id="block.id"
    :style="!visible ? { minHeight: `${cachedHeight}px` } : undefined"
  >
    <slot v-if="visible || force" />
  </div>
</template>
