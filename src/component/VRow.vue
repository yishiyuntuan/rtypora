<script setup vapor>
import { ref, inject, computed, watch, onMounted, onBeforeUnmount } from 'vue';

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

// 改动审阅：块级标记（新增/修改左侧条纹，删除顶部细线+徽标）；未提供时（无 Editor 装配）静默降级
const changeMarkOf = inject('changeMarkOf', null);
const changeVersion = inject('changeVersion', null);
// 「显示修改前内容」开关（useChangeReview provide；ref，模板内 .value 自动解包）
const changeShowOld = inject('changeShowOld', null);
// 无标记时为零响应式依赖的常量 null（条纹类不挂载，零额外 DOM）
const changeMark = computed(() => {
  if (!changeMarkOf) return null;
  changeVersion.value;
  return changeMarkOf(props.block.id);
});
// 旧内容块：偏好开启且标记带旧文本时渲染（删除/修改前的原文，只读展示）
const changeOldBlocks = computed(() => {
  const m = changeMark.value;
  if (!m || !changeShowOld?.value) return null;
  return m.oldBlocks?.length ? m.oldBlocks : null;
});

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
    :class="{
      'md-chg-added': changeMark?.kind === 'added',
      'md-chg-modified': changeMark?.kind === 'modified',
      'md-chg-run-start': changeMark?.runStart,
      'md-chg-run-end': changeMark?.runEnd,
      'md-chg-deleted': changeMark && changeMark.deleted !== undefined,
    }"
    :data-block-id="block.id"
    :style="!visible ? { minHeight: `${cachedHeight}px` } : undefined"
  >
    <span v-if="changeMark && changeMark.deleted !== undefined" class="md-chg-deleted-badge" aria-hidden="true">-{{ changeMark.deleted }}</span>
    <!-- 修改前内容（旧文本块，只读；渲染态不挂载行内容时也不显示） -->
    <pre v-for="(t, i) in changeOldBlocks || []" :key="i" class="md-chg-old" aria-hidden="true">{{ t }}</pre>
    <slot v-if="visible || force || printMode" />
  </div>
</template>
