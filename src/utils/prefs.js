// 偏好设置存取（localStorage 持久化）。
// 编辑器排版覆盖在主题应用之后再写入同名 CSS 变量（优先于主题值）。
import { ref } from 'vue';

// 偏好版本号：setPref 时递增，渲染层据此响应开关变化
export const prefsVersion = ref(0);
// 渲染开关版本号：仅 render_* 键变化时递增——数学/Mermaid/高亮/图文排版
// 的 watcher 依赖它而非全局版本号，避免改字号等非渲染偏好时全文重渲染
export const renderVersion = ref(0);

const STORAGE_KEY = 'tauri-editor.prefs';

const DEFAULTS = {
  // 编辑器
  text_size: null,           // 正文字号覆盖（px）
  text_line_height: null,    // 行高覆盖
  content_max_width: null,   // 内容列最大宽度覆盖（px）
  editor_padding: null,      // 编辑区内边距覆盖（px）
  // 图像
  image_paste_behavior: 'document', // 'off' | 'document'（文档目录）| 'assets'（assets 子目录）
  // Markdown 渲染开关
  render_code_highlight: true,
  render_code_line_numbers: true,
  render_mermaid: true,
  render_math: true,
  render_html_block: true,
  // 公式编号：'off' 不启用 | 'ams' 按 AMS 规则 | 'all' 所有展示公式
  math_numbering: 'ams',
  // 自动保存（默认关闭，状态栏开关）：触发方式 'blur' 焦点切换 | 'delay' 输入停止 N 秒
  auto_save_enabled: false,
  auto_save_trigger: 'blur',
  auto_save_delay_seconds: 3,
};

let cache = null;

function load() {
  if (cache) return cache;
  try {
    cache = { ...DEFAULTS, ...JSON.parse(localStorage.getItem(STORAGE_KEY) || '{}') };
  } catch {
    cache = { ...DEFAULTS };
  }
  return cache;
}

export function getPrefs() {
  return load();
}

export function getPref(key) {
  return load()[key];
}

export function setPref(key, value) {
  const prefs = load();
  prefs[key] = value;
  localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs));
  applyEditorOverrides(prefs);
  prefsVersion.value += 1;
  if (key.startsWith('render_')) renderVersion.value += 1;
}

// 把编辑器排版覆盖写入独立的 --t-*-pref 覆盖变量（CSS 以
// var(--t-x-pref, var(--t-x)) 回落到主题值，覆盖与主题互不干扰）。
// 空值仅删除覆盖键，主题值不受影响。
export function applyEditorOverrides(prefs = load()) {
  const style = document.documentElement.style;
  const apply = (name, value) => {
    if (value === null || value === undefined || value === '') style.removeProperty(name);
    else style.setProperty(name, `${value}px`);
  };
  apply('--t-text-size-pref', prefs.text_size);
  apply('--t-text-line-height-pref', prefs.text_line_height);
  apply('--t-content-max-width-pref', prefs.content_max_width);
  apply('--t-editor-padding-pref', prefs.editor_padding);
}
