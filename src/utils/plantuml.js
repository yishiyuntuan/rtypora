// PlantUML 渲染：两种模式（偏好 plantuml_renderer）——
// local：内置官方 TeaVM 引擎（@plantuml/core，纯 JS 本地渲染，离线可用，无需服务器/Java；
//         引擎 ~8MB 动态 import 分包，首个 PlantUML 块可见时才加载；SVG 结果缓存回填）；
// server：<img> 直连渲染服务器（无需加载引擎，但需要网络；webview 跨域 fetch 会被 CORS 拦截故不用 fetch）。
import plantumlEncoder from 'plantuml-encoder';
// Viz.js（Graphviz 布局引擎）内置为项目资源：经典脚本注入（见 loadVizScript 注释）
import vizGlobalUrl from '../assets/vendor/viz-global.js?url';
// TeaVM 引擎本体同样内置为资源，按原始文件 URL 加载（见 getEngine 注释）
import plantumlCoreUrl from '../assets/vendor/plantuml-core.js?url';
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
let vizScriptPromise = null;
function loadVizScript() {
  if (!vizScriptPromise) {
    vizScriptPromise = new Promise((resolve, reject) => {
      const s = document.createElement('script');
      s.src = vizGlobalUrl;
      s.onload = () => resolve();
      s.onerror = () => reject(new Error('viz-global.js 加载失败'));
      document.head.append(s);
    });
    vizScriptPromise.catch(() => {
      vizScriptPromise = null; // 失败允许重试
    });
  }
  return vizScriptPromise;
}
async function getEngine() {
  if (!enginePromise) {
    enginePromise = (async () => {
      // Viz.js 必须按经典脚本加载：引擎里是对 Viz 的裸引用（TeaVM 编译产物），
      // 若把 viz-global.js 当 ES 模块导入，打包器会给它包上 CJS exports，
      // UMD 全局检测失真，Viz 不会挂到 globalThis → 引擎 ReferenceError 静默失败
      await loadVizScript();
      // 引擎本体按原始文件 URL 加载（@vite-ignore 绕过打包器）：TeaVM 的 CPS 编译产物
      // 被 Vite8/rolldown 依赖预构建重写后运行时状态机会崩（监视锁 bGH/IllegalMonitorState），
      // 该文件零依赖、自带 export，让浏览器按原生 ES 模块执行原文即可。
      // 回归点：15f42a3 将 vite ^8.0.16 升到 ^8.2.0 后预构建行为变化导致此故障
      return import(/* @vite-ignore */ plantumlCoreUrl);
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

// TeaVM 引擎内部状态全页共享，官方要求同页渲染必须串行（见包内 GITHUB_INTEGRATION.md）：
// 并发 renderToString 会同时写共享全局变量，崩在引擎监视锁（bGH/IllegalMonitorState）。
// 故所有调用经此队列逐个执行：前一个回调返回（或超时兜底）后下一个才开始。
const RENDER_TIMEOUT_MS = 20000;
let renderTail = Promise.resolve();
const pendingJobs = new Map(); // cacheKey → 在途 Promise（相同源码的并发请求去重）

function invokeEngine(src) {
  return getEngine()
    .then((engine) => {
      if (!engine?.renderToString) {
        console.warn('[plantuml] 引擎缺少 renderToString:', engine && Object.keys(engine));
        return null;
      }
      return new Promise((resolve) => {
        let settled = false;
        const timer = setTimeout(() => {
          console.warn('[plantuml] 渲染超时（引擎可能已进入异常状态）');
          finish(null);
        }, RENDER_TIMEOUT_MS);
        const finish = (v) => {
          if (settled) return;
          settled = true;
          clearTimeout(timer);
          resolve(v);
        };
        try {
          engine.renderToString(
            src.split('\n'),
            (s) => finish(s),
            (err) => {
              console.warn('[plantuml] 渲染错误回调:', err);
              finish(null);
            },
          );
        } catch (e) {
          console.warn('[plantuml] 渲染同步异常:', e);
          finish(null);
        }
      });
    })
    .catch((e) => {
      console.warn('[plantuml] 引擎加载失败:', e);
      return null;
    });
}

// 本地渲染 PlantUML 源为 SVG 字符串；失败（引擎加载/语法错误/超时）返回 null
export function renderPlantumlLocal(source) {
  const src = stripFence(source);
  if (!src || !src.trim()) return Promise.resolve(null);
  const key = cacheKey(source);
  const hit = svgCache.get(key);
  if (hit) return Promise.resolve(hit);
  const dup = pendingJobs.get(key);
  if (dup) return dup;
  const job = renderTail
    .then(() => invokeEngine(src))
    .then((svg) => {
      if (svg && svg.includes('<svg')) {
        if (svgCache.size >= SVG_CACHE_MAX) svgCache.delete(svgCache.keys().next().value);
        svgCache.set(key, svg);
        return svg;
      }
      return null;
    });
  pendingJobs.set(key, job);
  job.finally(() => pendingJobs.delete(key));
  renderTail = job.catch(() => {}); // 单个任务失败不阻塞队列后续任务
  return job;
}
