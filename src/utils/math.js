// 公式渲染客户端：Rust ratex SVG 结果的缓存层。
// 虚拟滚动重挂载时同步回填（peekMath），不再每次挂载都发 IPC——
// Rust 端虽有 LATEX_CACHE，但每次挂载仍是一次 IPC 往返与 JSON 编解码。
// key = display|颜色|字号|源（主题/字号变化自然换 key）；FIFO 上限防无限增长；
// 同 key 并发请求共享同一 Promise（在途去重，与 mermaid.js 同构）。
import { invoke } from '@tauri-apps/api/core';

const svgCache = new Map();
const CACHE_MAX = 120;
const pendingJobs = new Map();

function mathKey(display, source, color, baseFontSize) {
  return `${display ? 1 : 0}|${color}|${baseFontSize}|${source}`;
}

// 同步取缓存（未命中返回 null）；供挂载时先行回填，避免空白帧与重复 IPC
export function peekMath(display, source, color, baseFontSize) {
  return svgCache.get(mathKey(display, source, color, baseFontSize)) || null;
}

// 渲染公式为 SVG 字符串；语法错误/渲染失败返回 null（调用方回退源码展示）。
// 失败结果不缓存（瞬时失败不应留下长期降级）。
export async function renderMath(display, source, color, baseFontSize) {
  const key = mathKey(display, source, color, baseFontSize);
  const hit = svgCache.get(key);
  if (hit) return hit;
  const pending = pendingJobs.get(key);
  if (pending) return pending;
  const job = (async () => {
    const args = display ? { raw: source, color, baseFontSize } : { body: source, color, baseFontSize };
    const command = display ? 'render_display_math' : 'render_inline_math';
    const svg = await invoke(command, args).catch(() => null);
    if (svg) {
      if (svgCache.size >= CACHE_MAX) svgCache.delete(svgCache.keys().next().value);
      svgCache.set(key, svg);
    }
    return svg;
  })();
  pendingJobs.set(key, job);
  try {
    return await job;
  } finally {
    pendingJobs.delete(key);
  }
}
