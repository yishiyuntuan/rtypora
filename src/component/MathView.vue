<script setup vapor>
import { ref, watch, inject, onMounted } from 'vue';
import { themeVersion } from '../themes/index.js';
import { getPref, renderVersion } from '../utils/prefs.js';
import { peekMath, renderMath } from '../utils/math.js';

// 数学公式渲染：Rust ratex 渲染 SVG（display 传 $$..$$ 原文，inline 传 LaTeX 正文）。
// 颜色/字号随当前主题（themeVersion 变化时重渲染）；语法错误回退源码展示。
// SVG 结果经 math.js 缓存：虚拟滚动重挂载同步回填（peekMath），不重复发 IPC。
// 重活懒执行：进入视口才渲染（Editor 提供的可见性回调，长文档挂载不打满 IPC）
const props = defineProps({
  source: { type: String, required: true },
  display: { type: Boolean, default: false },
});

const svg = ref('');
const rootEl = ref(null);
const mounted = ref(false);
const onBlockVisible = inject('onBlockVisible', (el, cb) => cb());
onMounted(() => {
  mounted.value = true;
});

watch(
  () => [props.source, props.display, themeVersion.value, renderVersion.value, mounted.value],
  async () => {
    svg.value = '';
    // 偏好设置可关闭公式渲染（回退源码展示）
    if (!getPref('render_math') || !mounted.value) return;
    const style = getComputedStyle(document.documentElement);
    const color = style.getPropertyValue('--t-text-default').trim() || '#000000';
    const baseFontSize = parseFloat(style.getPropertyValue('--t-text-size')) || 16;
    // 缓存命中同步回填（重挂载零成本）；未命中进视口后再渲染
    const hit = peekMath(props.display, props.source, color, baseFontSize);
    if (hit) {
      svg.value = hit;
      return;
    }
    onBlockVisible(rootEl.value, async () => {
      const result = await renderMath(props.display, props.source, color, baseFontSize);
      svg.value = result || '';
    });
  },
  { immediate: true },
);
</script>

<template>
  <span v-if="svg" ref="rootEl" class="math-view" :class="{ 'math-display': display }" v-html="svg"></span>
  <pre v-else-if="display" ref="rootEl" class="md-pre overflow-x-auto rounded p-3 font-mono text-[13px]">{{ source }}</pre>
  <code v-else ref="rootEl" class="md-code px-1 py-0.5 font-mono">{{ source }}</code>
</template>
