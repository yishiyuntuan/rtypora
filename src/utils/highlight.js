// 代码块语法高亮（tree-sitter，Rust 端 highlight_code 命令）。
// 命令返回扁平的「UTF-16 区间 + 类名」span，前端拼 HTML；
// 类名与主题 token code_syntax_* 后缀一致（ts-keyword 等，颜色由 --t-code-syntax-* 变量驱动）。
import { invoke } from '@tauri-apps/api/core';

const HTML_ESCAPES = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' };

function escapeHtml(text) {
  return String(text).replace(/[&<>"]/g, (c) => HTML_ESCAPES[c]);
}

// 结果缓存：key = language + 代码文本（上限 200 条，先进先出）
const CACHE_LIMIT = 200;
const cache = new Map();

function cached(key, loader) {
  if (cache.has(key)) return cache.get(key);
  const value = loader();
  if (cache.size >= CACHE_LIMIT) cache.delete(cache.keys().next().value);
  cache.set(key, value);
  return value;
}

// 异步获取代码块的高亮 HTML（语言未知/失败时返回转义纯文本）
export function highlightCodeHtml(code, language) {
  const key = `${language ?? ''}\n${code}`;
  return cached(key, async () => {
    try {
      const spans = await invoke('highlight_code', { language: language ?? null, code });
      return spansToHtml(code, spans);
    } catch {
      return escapeHtml(code);
    }
  });
}

// 扁平 span 数组 → HTML（span 已按起点排序且互不重叠）
function spansToHtml(code, spans) {
  if (!spans?.length) return escapeHtml(code);
  let html = '';
  let pos = 0;
  for (const span of spans) {
    if (span.start > pos) html += escapeHtml(code.slice(pos, span.start));
    html += `<span class="ts-${span.class}">${escapeHtml(code.slice(span.start, span.end))}</span>`;
    pos = span.end;
  }
  html += escapeHtml(code.slice(pos));
  return html;
}
