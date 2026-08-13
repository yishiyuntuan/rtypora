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

// 渲染前净化：section 原文来自文档本身，v-html 上屏时 <script> 虽不执行，
// 但 on* 事件属性（img onerror 等）、javascript: 链接、iframe/srcdoc 会真正生效——
// 打开恶意文档即在 webview 上下文执行任意脚本（可经 IPC 读写任意文件），必须剔除。
const BLOCKED_TAGS = new Set([
  'SCRIPT', 'IFRAME', 'OBJECT', 'EMBED', 'FORM', 'INPUT', 'BUTTON', 'TEXTAREA', 'SELECT',
  'LINK', 'META', 'BASE', 'AUDIO', 'VIDEO', 'SOURCE', 'TRACK', 'FRAME', 'FRAMESET', 'NOSCRIPT',
]);
const URL_ATTRS = new Set(['href', 'src', 'xlink:href', 'formaction', 'action']);
function sanitizeSectionDom(root) {
  for (const el of [root, ...root.querySelectorAll('*')]) {
    if (BLOCKED_TAGS.has(el.tagName)) {
      el.remove();
      continue;
    }
    for (const attr of [...el.attributes]) {
      const name = attr.name.toLowerCase();
      if (name.startsWith('on') || name === 'srcdoc') {
        el.removeAttribute(attr.name);
        continue;
      }
      if (URL_ATTRS.has(name)) {
        // DOMParser 已解码字符引用；剥离空白/控制字符后判定危险协议
        const v = attr.value.replace(/[\s\u0000-\u001F\u007F]+/g, '').toLowerCase();
        if (v.startsWith('javascript:') || v.startsWith('vbscript:')) el.removeAttribute(attr.name);
      }
    }
  }
}

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
    sanitizeSectionDom(section);
    html.value = section.outerHTML;
  },
  { immediate: true },
);
</script>

<template>
  <!-- 渲染用户自己文档中的 <section> 布局（v-html 不执行 script） -->
  <div v-if="html" class="md-section" v-html="html"></div>
</template>
