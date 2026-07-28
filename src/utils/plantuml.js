// PlantUML 渲染：两种模式（偏好 plantuml_renderer）——
// local：内置官方 TeaVM 引擎（@plantuml/core，纯 JS 本地渲染，离线可用，无需服务器/Java；
//         引擎 ~8MB 动态 import 分包，首个 PlantUML 块可见时才加载；SVG 结果缓存回填）；
// server：<img> 直连渲染服务器（无需加载引擎，但需要网络；webview 跨域 fetch 会被 CORS 拦截故不用 fetch）。
import plantumlEncoder from 'plantuml-encoder';
import { getPref } from './prefs.js';

export function plantumlRenderer() {
  return getPref('plantuml_renderer') || 'local';
}

function serverBase() {
  return (getPref('plantuml_server') || 'https://www.plantuml.com/plantuml').replace(/\/+$/, '');
}

// 剥离线围栏（块按 codeBlock+plantuml 存储，源码可能带 ```plantuml 围栏）
function stripFence(source) {
  const m = String(source).match(/^\s*~{3,}[^\n]*\n([\s\S]*?)~{3,}\s*$|^\s*`{3,}[^\n]*\n([\s\S]*?)`{3,}\s*$/);
  return m ? (m[1] ?? m[2]) : source;
}

// ---------- server 模式：渲染服务器 URL ----------
export function plantumlUrl(source) {
  const src = stripFence(source);
  if (!src || !src.trim()) return null;
  return `${serverBase()}/svg/${plantumlEncoder(src)}`;
}

// ---------- local 模式：内置引擎 ----------
let enginePromise = null;
async function getEngine() {
  if (!enginePromise) {
    enginePromise = (async () => {
      // Graphviz 布局引擎（Viz.js，先加载并挂到 globalThis.Viz），再加载 TeaVM 引擎本体
      await import('@plantuml/core/viz-global.js');
      return import('@plantuml/core');
    })();
    enginePromise.catch(() => {
      enginePromise = null; // 加载失败允许重试
    });
  }
  return enginePromise;
}

// SVG 结果缓存（与 Mermaid 同策略）：滚动重挂载经 peekPlantuml 同步回填，不重复渲染
const svgCache = new Map();
const SVG_CACHE_MAX = 60;
function cacheKey(source) {
  return `${plantumlRenderer()}|${stripFence(source)}`;
}
export function peekPlantuml(source) {
  return svgCache.get(cacheKey(source)) || null;
}

// 本地渲染 PlantUML 源为 SVG 字符串；失败（引擎加载/语法错误）返回 null
export async function renderPlantumlLocal(source) {
  const src = stripFence(source);
  if (!src || !src.trim()) return null;
  const key = cacheKey(source);
  const hit = svgCache.get(key);
  if (hit) return hit;
  const { renderToString } = await getEngine().catch(() => null);
  if (!renderToString) return null;
  const svg = await new Promise((resolve) => {
    try {
      renderToString(src.split('\n'), resolve, () => resolve(null));
    } catch {
      resolve(null);
    }
  });
  if (!svg || !svg.includes('<svg')) return null;
  if (svgCache.size >= SVG_CACHE_MAX) svgCache.delete(svgCache.keys().next().value);
  svgCache.set(key, svg);
  return svg;
}
