<script setup vapor>
import { ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { themeVersion } from '../themes/index.js';
import { getPref, renderVersion } from '../utils/prefs.js';

// 数学公式渲染：Rust ratex 渲染 SVG（display 传 $$..$$ 原文，inline 传 LaTeX 正文）。
// 颜色/字号随当前主题（themeVersion 变化时重渲染）；语法错误回退源码展示。
const props = defineProps({
  source: { type: String, required: true },
  display: { type: Boolean, default: false },
});

const svg = ref('');

watch(
  () => [props.source, props.display, themeVersion.value, renderVersion.value],
  async () => {
    svg.value = '';
    // 偏好设置可关闭公式渲染（回退源码展示）
    if (!getPref('render_math')) return;
    const style = getComputedStyle(document.documentElement);
    const color = style.getPropertyValue('--t-text-default').trim() || '#000000';
    const baseFontSize = parseFloat(style.getPropertyValue('--t-text-size')) || 16;
    const args = props.display
      ? { raw: props.source, color, baseFontSize }
      : { body: props.source, color, baseFontSize };
    const command = props.display ? 'render_display_math' : 'render_inline_math';
    const result = await invoke(command, args).catch(() => null);
    svg.value = result || '';
  },
  { immediate: true },
);
</script>

<template>
  <span v-if="svg" class="math-view" :class="{ 'math-display': display }" v-html="svg"></span>
  <pre v-else-if="display" class="md-pre overflow-x-auto rounded p-3 font-mono text-[13px]">{{ source }}</pre>
  <code v-else class="md-code px-1 py-0.5 font-mono">{{ source }}</code>
</template>
