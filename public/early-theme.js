// 首帧防白闪：样式/脚本加载前，按上次主题的编辑器背景色设置画布底色
// （暗色主题下窗口不再由亮转暗）；颜色值由 applyTheme 持久化。
// 独立文件（非内联脚本）：生产 CSP 的 script-src 'self' 不允许内联脚本。
(function () {
  try {
    var bg = localStorage.getItem('tauri-editor.theme.bg');
    if (!bg) return;
    document.documentElement.style.background = bg;
    var m = bg.match(/#([0-9a-f]{6})/i);
    if (m) {
      var v = parseInt(m[1], 16);
      var lum = ((v >> 16) * 299 + ((v >> 8) & 255) * 587 + (v & 255) * 114) / 1000;
      document.documentElement.style.colorScheme = lum < 128 ? 'dark' : 'light';
    }
  } catch (e) {}
})();
