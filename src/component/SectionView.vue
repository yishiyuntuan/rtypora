<script setup vapor>
import { ref, watch, inject } from 'vue';
import { resolveImageSrc } from '../utils/image.js';
import { getPref, renderVersion } from '../utils/prefs.js';

// section 图文排版块（Mdmdt 式 grid 布局）：HTML 结构解析为 DOM 仅为渲染，
// 图片经 Rust read_image_data_url 解析（相对路径基于文档目录）；块本身的
// Markdown 解析/分类在 Rust 端完成（BlockKindDto::SectionBlock）。
const props = defineProps({
  raw: { type: String, default: '' },
});

const documentDir = inject('documentDir', { value: null });
const html = ref('');

watch(
  () => [props.raw, documentDir.value, renderVersion.value],
  async () => {
    // 偏好设置可关闭渲染（回退源码展示，由 BlockView 分支处理）
    if (!getPref('render_html_block')) {
      html.value = '';
      return;
    }
    const doc = new DOMParser().parseFromString(props.raw, 'text/html');
    const section = doc.querySelector('section');
    if (!section) {
      // 解析失败清空（raw 失效时残留旧渲染的 bug）
      html.value = '';
      return;
    }
    const images = [...section.querySelectorAll('img')];
    // 图片解析完成前保留旧 HTML（缓存命中仅微任务）：避免重挂载出现空白帧
    await Promise.all(
      images.map(async (img) => {
        const resolved = await resolveImageSrc(img.getAttribute('src') || '', documentDir.value);
        if (resolved) img.setAttribute('src', resolved);
        else img.removeAttribute('src');
      }),
    );
    html.value = section.outerHTML;
  },
  { immediate: true },
);
</script>

<template>
  <!-- 渲染用户自己文档中的 <section> 布局（v-html 不执行 script） -->
  <div v-if="html" class="md-section" v-html="html"></div>
</template>
