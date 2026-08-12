// 平台判定与 macOS 适配工具（红绿灯避让、Cmd 快捷键提示、平台字体默认值等）。
export const isMac = /Macintosh|Mac OS X/.test(navigator.userAgent);
export const isWindows = /Windows/.test(navigator.userAgent);

// 路径去重键：统一分隔符为正斜杠、去尾部分隔符；Windows 路径大小写不敏感（整体小写化）。
// 用途：最近使用文件/目录列表去重——同一路径「对话框原生反斜杠形式」与「拼接正斜杠形式」
// 字符串不等会重复入列，须按规范化键判重
export function pathKey(p) {
  const n = String(p || '').replace(/\\/g, '/').replace(/\/+$/, '');
  return isWindows ? n.toLowerCase() : n;
}

// 路径列表去重（保序，先出现者为准），用于最近使用列表的读写两侧
export function dedupPaths(list) {
  const seen = new Set();
  const out = [];
  for (const p of list || []) {
    const k = pathKey(p);
    if (seen.has(k)) continue;
    seen.add(k);
    out.push(p);
  }
  return out;
}

// 公式中文回落字体默认规格（ratex：路径 或 路径#字体族名）：
// Windows 宋体（SimSun）；macOS 华文宋体（Songti SC，同为衬线）
export const mathCjkFontSpec = isMac
  ? '/System/Library/Fonts/Supplemental/Songti.ttc#Songti SC'
  : 'C:\\Windows\\Fonts\\simsun.ttc#SimSun';

// 快捷键标签按平台格式化：macOS 显示 ⌘⇧⌥ 符号，其余平台保持 Ctrl/Shift/Alt 文本
export function formatShortcut(s) {
  if (!s || !isMac) return s;
  return s.replace(/Ctrl\+/g, '⌘').replace(/Shift\+/g, '⇧').replace(/Alt\+/g, '⌥');
}
