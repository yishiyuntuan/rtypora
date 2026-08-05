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
// 打印模式：Editor 打印时强制所有行挂载（虚拟滚动下未挂载行不会被打印）
const printMode = inject('printMode', ref(false));

const el = ref(null);
const visible = ref(props.force);
const cachedHeight = ref(props.estimate);

function onVisible(isVisible) {
  if (isVisible) {
    // 重新进入视口：取消待执行的延迟卸载
    clearTimeout(hideTimer);
    hideTimer = null;
    visible.value = true;
  } else if (!props.force && visible.value && !hideTimer) {
    // 延迟卸载（3s 宽限）：快速来回滚动不重建内容——
    // 图片重解码、Mermaid 重渲染的闪烁主要来自频繁的挂载/卸载
    hideTimer = setTimeout(() => {
      hideTimer = null;
      // 卸载前缓存实测高度（估计值被修正为真值，滚动几乎不跳）
      if (el.value) cachedHeight.value = el.value.offsetHeight;
      visible.value = false;
    }, 3000);
  }
}
let hideTimer = null;

// 程序性进入编辑（如大纲/目录定位）时立即渲染
watch(
  () => props.force,
  (v) => {
    if (v) visible.value = true;
  },
);

// 记录被观察元素（卸载回调时 ref 可能已为 null，须用挂载时捕获的元素注销）
let observedEl = null;
onMounted(() => {
  observedEl = el.value;
  observeRow(observedEl, onVisible);
});
onBeforeUnmount(() => {
  clearTimeout(hideTimer);
  hideTimer = null;
  if (observedEl) {
    unobserveRow(observedEl);
    observedEl = null;
  }
});
</script>

<template>
  <div
    ref="el"
    class="md-vrow"
    :data-block-id="block.id"
    :style="!visible ? { minHeight: `${cachedHeight}px` } : undefined"
  >
    <slot v-if="visible || force || printMode" />
  </div>
</template>
