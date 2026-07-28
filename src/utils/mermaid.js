// Mermaid 渲染：官方 mermaid.js（支持全部高级语法与新特性）。
// 动态 import 按需加载（首屏无负担；Vite 自动分包）；
// 主题随应用明暗切换（重新 initialize 后由调用方重渲染）。

let mermaidModule = null;
let initializedThemeKey = null;
let renderSeq = 0;

// 当前应用主题为暗色？（按编辑器背景亮度判定）
function isDarkTheme() {
  const bg = getComputedStyle(document.documentElement).getPropertyValue('--t-editor-background').trim();
  const m = bg.match(/#([0-9a-f]{6})/i) || bg.match(/rgba?\((\d+)[,\s]+(\d+)[,\s]+(\d+)/);
  if (!m) return false;
  const [r, g, b] =
    m.length === 2
      ? [parseInt(m[1].slice(0, 2), 16), parseInt(m[1].slice(2, 4), 16), parseInt(m[1].slice(4, 6), 16)]
      : [Number(m[1]), Number(m[2]), Number(m[3])];
  return (r * 299 + g * 587 + b * 114) / 1000 < 128;
}

async function getMermaid() {
  if (!mermaidModule) mermaidModule = (await import('mermaid')).default;
  const themeKey = isDarkTheme() ? 'dark' : 'default';
  if (initializedThemeKey !== themeKey) {
    initializedThemeKey = themeKey;
    mermaidModule.initialize({
      startOnLoad: false,
      // 本地文档也不放行脚本（strict 级别，图内 HTML/脚本不执行）
      securityLevel: 'strict',
      theme: themeKey,
      fontFamily: getComputedStyle(document.body).fontFamily || undefined,
    });
  }
  return mermaidModule;
}

// 剥离线围栏（块 rawFallback 可能含 ```mermaid 围栏；mermaid.js 只接受图源）
function stripFence(source) {
  const m = String(source).match(/^\s*~{3,}[^\n]*\n([\s\S]*?)~{3,}\s*$|^\s*`{3,}[^\n]*\n([\s\S]*?)`{3,}\s*$/);
  return m ? (m[1] ?? m[2]) : source;
}

// 渲染结果缓存：key = 主题|图源。滚动重挂载/来回滚动时同步回填，
// 不再重复跑布局渲染（滚动闪烁的主要来源）；FIFO 上限防无限增长
const svgCache = new Map();
const SVG_CACHE_MAX = 120;
function cacheKey(source) {
  return `${isDarkTheme() ? 'dark' : 'default'}|${stripFence(source)}`;
}
function cachePut(key, svg) {
  if (svgCache.size >= SVG_CACHE_MAX) svgCache.delete(svgCache.keys().next().value);
  svgCache.set(key, svg);
}

// 同步取缓存（未命中返回 null）；供重挂载时先行回填，避免空白帧
export function peekMermaid(source) {
  return svgCache.get(cacheKey(source)) || null;
}

// 渲染图源为 SVG 字符串；语法错误返回 null（调用方回退源码占位）
export async function renderMermaid(source) {
  const src = stripFence(source);
  if (!src || !src.trim()) return null;
  const key = cacheKey(source);
  const hit = svgCache.get(key);
  if (hit) return hit;
  const mermaid = await getMermaid();
  const id = `mmd-${Date.now()}-${++renderSeq}`;
  try {
    const { svg } = await mermaid.render(id, src);
    cachePut(key, svg);
    return svg;
  } catch {
    // mermaid 渲染失败时会把错误图形塞进 body（#d{id}），清理掉
    document.getElementById(id)?.remove();
    document.getElementById(`d${id}`)?.remove();
    return null;
  }
}
