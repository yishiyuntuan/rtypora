// 偏好设置存取（localStorage 持久化）。
// 编辑器排版覆盖在主题应用之后再写入同名 CSS 变量（优先于主题值）。
import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';

// 偏好版本号：setPref 时递增，渲染层据此响应开关变化
export const prefsVersion = ref(0);
// 渲染开关版本号：仅 render_* 键变化时递增——数学/Mermaid/高亮/图文排版
// 的 watcher 依赖它而非全局版本号，避免改字号等非渲染偏好时全文重渲染
export const renderVersion = ref(0);
// 结构版本号：仅影响解析结构的偏好（html_to_md）变化时递增，Editor 据此重解析
export const structureVersion = ref(0);

const STORAGE_KEY = 'tauri-editor.prefs';

const DEFAULTS = {
  // 编辑器
  text_size: null,           // 正文字号覆盖（px）
  text_line_height: null,    // 行高覆盖
  content_max_width: null,   // 内容列最大宽度覆盖（px）
  editor_padding: null,      // 编辑区内边距覆盖（px）
  first_line_indent: false,  // 段落首行缩进（2em）
  // 滚动条自动隐藏：平时隐藏，悬停或滚动时显示
  scrollbar_auto_hide: true,
  // 侧边栏底部操作栏自动隐藏：平时隐藏，鼠标滑过侧栏时显现
  sidebar_toolbar_auto_hide: true,
  // 公式中文回落字体覆盖（ratex 字体规格：路径 或 路径#字体族名；留空跟随主题 token，重启后生效）
  math_cjk_font: null,
  // 编辑器字体 / 代码块字体覆盖（字体族名；留空跟随主题 typography.font_family/code_font_family）
  editor_font_family: null,
  code_font_family: null,
  // 图像
  image_paste_behavior: 'document', // 'off' | 'document'（文档目录）| 'assets'（assets 子目录）
  // Markdown 渲染开关
  render_code_highlight: true,
  render_code_line_numbers: true,
  render_mermaid: true,
  render_plantuml: true,
  // PlantUML 渲染方式：'local' 内置引擎（离线）| 'server' 渲染服务器
  plantuml_renderer: 'local',
  // PlantUML 渲染服务器（/svg/<encoded> 端点；server 模式用，可换自部署实例）
  plantuml_server: 'https://www.plantuml.com/plantuml',
  render_math: true,
  render_html_block: true,
  // 公式编号：'off' 不启用 | 'ams' 按 AMS 规则 | 'all' 所有展示公式
  math_numbering: 'ams',
  // 自动保存（默认关闭，状态栏开关）：触发方式 'blur' 焦点切换 | 'delay' 输入停止 N 秒
  auto_save_enabled: false,
  auto_save_trigger: 'blur',
  auto_save_delay_seconds: 3,
  // HTML 标签转换：h1-h6/p/div/center 容器与内联样式标签转换为 Markdown 结构
  html_to_md: true,
  // 警告框扩展语法统一转换：Obsidian 别名（[!hint] 等）与 :::type / !!! type
  // 容器按 GitHub 五变体解析，保存时落源为标准 [!TYPE] 引用格式
  callout_unify: true,
  // macOS 专用：平时隐藏原生红绿灯窗口按钮，鼠标滑过左上角时显示
  traffic_light_autohide: false,
  // 主题跟随系统外观：开启后按系统明暗自动切换下述两个主题
  theme_follow_system: false,
  // 跟随系统时暗色/亮色外观各自使用的主题 id
  theme_dark_id: 'velotype',
  theme_light_id: 'velotype-light',
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
  if (key === 'html_to_md' || key === 'callout_unify') {
    structureVersion.value += 1;
    syncRustPrefs();
  }
}

// 把影响 Rust 解析的偏好同步到 Rust 端（应用启动与 html_to_md/callout_unify 变更时调用）
export function syncRustPrefs() {
  invoke('set_html_to_md', { enabled: !!load().html_to_md }).catch((e) =>
    console.error('set_html_to_md 失败:', e),
  );
  invoke('set_callout_unify', { enabled: !!load().callout_unify }).catch((e) =>
    console.error('set_callout_unify 失败:', e),
  );
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
  // 字体族覆盖（字符串值，带空格族名加引号；CSS 以 var(--t-x-pref, var(--t-x)) 回落主题）
  const applyFont = (name, value) => {
    if (value === null || value === undefined || value === '') style.removeProperty(name);
    else style.setProperty(name, JSON.stringify(value));
  };
  applyFont('--t-font-family-pref', prefs.editor_font_family);
  applyFont('--t-code-font-family-pref', prefs.code_font_family);
  // 滚动条自动隐藏开关（html.sb-auto-hide 驱动 CSS 显隐规则）
  document.documentElement.classList.toggle('sb-auto-hide', !!prefs.scrollbar_auto_hide);
  // 公式中文回落字体覆盖（ratex 字体规格；空值跟随主题 token，重启后生效）
  if (prefs.math_cjk_font) {
    invoke('set_math_unicode_font', { spec: prefs.math_cjk_font }).catch((e) =>
      console.error('set_math_unicode_font 失败:', e),
    );
  }
  // macOS 红绿灯自动隐藏（仅 mac 生效）
  applyTrafficLightAutohide(!!prefs.traffic_light_autohide);
}

// ---------- macOS 红绿灯自动隐藏 ----------
// 平时经 Rust 命令隐藏原生红绿灯按钮，鼠标进入左上角热区时显示、移出后隐藏。
// 仅在 macOS（原生 Overlay 标题栏）生效；其余平台为空操作。
import { isMac as IS_MAC } from './platform.js';
const tlState = { enabled: false, visible: true, listener: null };
// 红绿灯当前是否可见（响应式）：标题栏/侧边栏据此收起避让留白，
// 隐藏时目录/大纲标签顶到上边框、菜单按钮贴近左缘
export const trafficLightsVisible = ref(true);
// 左上角热区（覆盖红绿灯区域及其周边，像素）
const TL_HOT_X = 96;
const TL_HOT_Y = 40;

function setTrafficLights(visible) {
  if (tlState.visible === visible) return;
  tlState.visible = visible;
  trafficLightsVisible.value = visible;
  invoke('set_traffic_lights_visible', { visible }).catch((e) =>
    console.error('set_traffic_lights_visible 失败:', e),
  );
}

export function applyTrafficLightAutohide(enabled) {
  if (!IS_MAC) return;
  if (enabled === tlState.enabled) return;
  tlState.enabled = enabled;
  if (enabled) {
    const onMove = (e) => setTrafficLights(e.clientX <= TL_HOT_X && e.clientY <= TL_HOT_Y);
    window.addEventListener('mousemove', onMove);
    tlState.listener = onMove;
    // 启用后立即隐藏，等首次滑入热区再显示
    setTrafficLights(false);
  } else {
    if (tlState.listener) window.removeEventListener('mousemove', tlState.listener);
    tlState.listener = null;
    setTrafficLights(true);
  }
}
