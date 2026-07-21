// 偏好设置存取（localStorage 持久化）。
// 编辑器排版覆盖在主题应用之后再写入同名 CSS 变量（优先于主题值）。
import { ref } from 'vue';

// 偏好版本号：setPref 时递增，渲染层据此响应开关变化
export const prefsVersion = ref(0);

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
  render_mermaid: true,
  render_math: true,
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
}

// 把编辑器排版覆盖写入 CSS 变量（空值删除，回退主题值）
export function applyEditorOverrides(prefs = load()) {
  const style = document.documentElement.style;
  const apply = (name, value) => {
    if (value === null || value === undefined || value === '') style.removeProperty(name);
    else style.setProperty(name, `${value}px`);
  };
  apply('--t-text-size', prefs.text_size);
  apply('--t-text-line-height', prefs.text_line_height);
  apply('--t-content-max-width', prefs.content_max_width);
  apply('--t-editor-padding', prefs.editor_padding);
}
