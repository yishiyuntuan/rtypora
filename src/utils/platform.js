// 平台判定与 macOS 适配工具（红绿灯避让、Cmd 快捷键提示、平台字体默认值等）。
export const isMac = /Macintosh|Mac OS X/.test(navigator.userAgent);

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
