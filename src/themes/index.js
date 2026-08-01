// 主题系统：参照 velotype 的主题自定义接口（主题包 + 基主题继承）。
//
// 自定义主题接口（JSON/JSONC 或 YAML 文件，经状态栏「导入主题…」导入，
// 格式按内容自动识别——以 { 开头为 JSON，否则按 YAML 解析）：
//   {
//     "name": "主题显示名（必填）",
//     "creator": "作者",
//     "baseThemeId": "velotype | velotype-light",   // 缺省继承当前主题的内置基主题
//     "description" / "version" / "homepage" / "license": "可选元数据",
//     "theme": {
//       "colors":     { "editor_background": "#191919", ... },  // 缺省字段继承基主题
//       "typography": { "text_size": 17, "h1_weight": "bold", ... },
//       "dimensions": { "editor_padding": 24, "content_max_width": 860, ... }
//     }
//   }
// token 名称与内置主题（themes/velotype*.js）一致；colors 为任意 CSS 颜色，
// typography/dimensions 为数值（px），weight 取 thin|light|normal|medium|semibold|bold|extrabold|black。
// null / 空字符串字段视为未设置，继承基主题（同 velotype 的归一化规则）。

import velotype from './velotype.js';
import velotypeLight from './velotype-light.js';
import { ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { applyEditorOverrides, getPref, prefsVersion } from '../utils/prefs.js';

// 主题版本号：applyTheme 时递增，依赖主题色的异步渲染（公式 SVG 等）据此重渲染
export const themeVersion = ref(0);

const STORAGE_THEME_ID = 'tauri-editor.theme.id';
// 上次主题的编辑器背景色：index.html 首帧防白闪脚本读取（样式加载前设色）
export const STORAGE_THEME_BG = 'tauri-editor.theme.bg';
const STORAGE_CUSTOM = 'tauri-editor.theme.custom';
const DEFAULT_THEME_ID = 'velotype-light';

const FONT_WEIGHTS = {
  thin: 100,
  light: 300,
  normal: 400,
  medium: 500,
  semibold: 600,
  bold: 700,
  extrabold: 800,
  black: 900,
};

const builtinThemes = [velotype, velotypeLight];

function isPlainObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

// 深合并：override 中的 null/空字符串跳过（继承 base）
function mergeTheme(base, override) {
  const out = { ...base };
  for (const [key, value] of Object.entries(override || {})) {
    if (value === null || value === '') continue;
    if (isPlainObject(value) && isPlainObject(out[key])) {
      out[key] = mergeTheme(out[key], value);
    } else {
      out[key] = value;
    }
  }
  return out;
}

function loadCustomThemes() {
  try {
    const parsed = JSON.parse(localStorage.getItem(STORAGE_CUSTOM) || '[]');
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function saveCustomThemes(themes) {
  localStorage.setItem(STORAGE_CUSTOM, JSON.stringify(themes));
}

/** 全部可选主题（内置 + 用户导入的自定义主题）。 */
export function listThemes() {
  return [...builtinThemes, ...loadCustomThemes()];
}

function findTheme(id) {
  return listThemes().find((theme) => theme.id === id);
}

// 解析基主题继承链，返回合并后的完整 theme（colors/typography/dimensions）
function resolveTheme(pack) {
  const baseId = pack.baseThemeId || DEFAULT_THEME_ID;
  const base = builtinThemes.find((theme) => theme.id === baseId) || velotypeLight;
  return mergeTheme(base.theme, pack.theme);
}

function kebab(name) {
  return name.replace(/_/g, '-');
}

// ---------- 按块自定义样式（theme.blocks） ----------
//
// theme.blocks 以块类型为键、CSS 属性表为值，作用于该块类型的根元素：
//   "blocks": {
//     "heading": { "h1": { "borderBottom": "2px solid" }, "letterSpacing": "0.02em" },
//     "codeBlock": { "borderRadius": "8px", "padding": "16px" },
//     "quote": { "background": "#00000008" }
//   }
// 键为块类型（paragraph/heading/separator/bulletedListItem/taskListItem/numberedListItem/
// quote/callout/footnoteDefinition/table/codeBlock/comment/htmlBlock/mathBlock/mermaidBlock/
// rawMarkdown）；heading 的值里可再嵌 h1~h6 分级覆盖。属性名 camelCase 或 kebab 均可，
// 数值自动补 px（fontWeight/lineHeight/opacity 等无单位属性除外）。

const BLOCK_SELECTORS = {
  paragraph: '.blk-paragraph',
  heading: '.blk-heading',
  separator: '.blk-separator',
  bulletedListItem: '.blk-bulleted-list-item',
  taskListItem: '.blk-task-list-item',
  numberedListItem: '.blk-numbered-list-item',
  quote: '.blk-quote',
  callout: '.blk-callout',
  footnoteDefinition: '.blk-footnote-definition',
  table: '.blk-table',
  codeBlock: '.blk-code-block',
  comment: '.blk-comment',
  htmlBlock: '.blk-html-block',
  sectionBlock: '.blk-section-block',
  mathBlock: '.blk-math-block',
  mermaidBlock: '.blk-mermaid-block',
  rawMarkdown: '.blk-raw-markdown',
  // 行内选择器：定制链接/行内代码/粗斜体等行内表现
  link: 'a',
  inlineCode: '.md-code',
  strong: 'strong',
  em: 'em',
  strikethrough: 's',
  mark: 'mark',
  footnoteRef: '.md-footnote-ref',
  listMarker: '.md-marker',
  taskCheckbox: 'input[type="checkbox"]',
  kbd: 'kbd',
  superscript: 'sup',
  subscript: 'sub',
  // 界面区域（blocks 作用域为 .t-app，覆盖编辑区与侧栏/状态栏）
  sidebarTab: '.t-tab',
  sidebarTabActive: '.t-tab-active',
  // 斜杠命令菜单（/ 召唤）与选中项
  slashMenu: '.md-slash-menu',
  slashMenuItemActive: '.md-slash-item-active',
};

const UNITLESS_PROPS = new Set([
  'font-weight', 'line-height', 'opacity', 'z-index', 'flex', 'flex-grow', 'flex-shrink', 'order',
]);

function cssPropName(name) {
  return name.replace(/[A-Z]/g, (ch) => '-' + ch.toLowerCase());
}

function cssValue(prop, value) {
  if (value === null || value === '') return null;
  // 防止跳出样式块（SVG data URI 等含 <> 的合法值不受影响）
  const text = String(value);
  if (text.includes('}') || text.toLowerCase().includes('</style')) return null;
  if (typeof value === 'number' && !UNITLESS_PROPS.has(prop)) return `${value}px`;
  return text;
}

function cssDeclarations(props) {
  const decls = [];
  for (const [rawName, value] of Object.entries(props || {})) {
    const prop = cssPropName(rawName);
    const val = cssValue(prop, value);
    if (val !== null) decls.push(`${prop}: ${val}`);
  }
  return decls.length ? decls.join('; ') : null;
}

// 键是否按后代选择器处理（属性名是标识符；含空白、括号、组合器等特征即选择器）
function isSelectorKey(key) {
  return /[\s>+~()[\]#.*,]/.test(key);
}

// 选择器键净化：不允许跳出规则/样式块
function sanitizeSelector(key) {
  return /[{}<;]/.test(key) || key.toLowerCase().includes('</style') ? null : key;
}

// 一个选择器上下文（块或标题级别）的属性表 → CSS 规则：
// - `:` 开头键：伪类/伪元素（:hover / :focus / ::before / ::after / :nth-child 等），直接拼接在选择器后
// - 含选择器特征的键：后代选择器（如 "tbody tr:nth-child(even) td"，实现斑马纹）
// - 其余为直属 CSS 属性
function buildRulesFor(selector, props, rules) {
  const direct = {};
  for (const [key, value] of Object.entries(props || {})) {
    if (!isPlainObject(value)) {
      direct[key] = value;
      continue;
    }
    if (key.startsWith(':')) {
      const suffix = sanitizeSelector(key);
      const decls = cssDeclarations(value);
      if (suffix && decls) rules.push(`${selector}${suffix} { ${decls} }`);
    } else if (isSelectorKey(key)) {
      const descendant = sanitizeSelector(key);
      const decls = cssDeclarations(value);
      if (descendant && decls) rules.push(`${selector} ${descendant} { ${decls} }`);
    }
    // 非对象值已在上面归入 direct；对象值但非选择器键的忽略（防御）
  }
  const decls = cssDeclarations(direct);
  if (decls) rules.push(`${selector} { ${decls} }`);
}

// blocks 配置 → CSS 文本（作用域 .t-app）
function buildBlockCss(blocks) {
  const rules = [];
  for (const [type, value] of Object.entries(blocks || {})) {
    const selector = BLOCK_SELECTORS[type];
    if (!selector || !isPlainObject(value)) continue;
    if (type === 'heading') {
      // heading 直属属性作用于全部标题级别；h1~h6 子表分级覆盖
      const { h1, h2, h3, h4, h5, h6, ...rest } = value;
      buildRulesFor(`.t-app .blk-heading`, rest, rules);
      for (const [level, props] of Object.entries({ h1, h2, h3, h4, h5, h6 })) {
        buildRulesFor(`.t-app ${level}.blk-heading`, props, rules);
      }
      continue;
    }
    buildRulesFor(`.t-app ${selector}`, value, rules);
  }
  return rules.join('\n');
}

// 把按块样式注入/替换到专用 <style> 元素
function applyBlockCss(blocks) {
  let el = document.getElementById('t-block-styles');
  if (!el) {
    el = document.createElement('style');
    el.id = 't-block-styles';
    document.head.append(el);
  }
  el.textContent = buildBlockCss(blocks);
}

// 把解析后的主题写入 documentElement 的 --t-* CSS 变量
function applyCssVars(theme) {
  const style = document.documentElement.style;
  for (const [key, value] of Object.entries(theme.colors || {})) {
    style.setProperty(`--t-${kebab(key)}`, String(value));
  }
  const typography = theme.typography || {};
  for (const [key, value] of Object.entries(typography)) {
    // math_cjk_font 是 ratex 字体规格（非 CSS 变量），经 set_math_unicode_font 命令下发
    if (key === 'math_cjk_font') continue;
    if (key.endsWith('_weight')) {
      style.setProperty(`--t-${kebab(key)}`, String(FONT_WEIGHTS[value] || 400));
    } else if (key === 'text_line_height' || key === 'script_scale' || key.endsWith('_family')) {
      style.setProperty(`--t-${kebab(key)}`, String(value));
    } else {
      style.setProperty(`--t-${kebab(key)}`, `${value}px`);
    }
  }
  for (const [key, value] of Object.entries(theme.dimensions || {})) {
    style.setProperty(`--t-${kebab(key)}`, value === null ? 'none' : `${value}px`);
  }
}

/** 解析主题继承链后的完整主题（外观页主题卡片缩略图取色用）。 */
export function previewTheme(pack) {
  return resolveTheme(pack);
}

/** 应用指定 id 的主题；返回是否成功（id 不存在则回退默认主题）。 */
export function applyTheme(id) {  let pack = findTheme(id);
  if (!pack) {
    pack = findTheme(DEFAULT_THEME_ID);
    id = DEFAULT_THEME_ID;
  }
  const resolved = resolveTheme(pack);
  applyCssVars(resolved);
  applyBlockCss(resolved.blocks);
  // 持久化解析后的背景色，供下次启动首帧防白闪
  const bg = resolved.colors?.editor_background;
  if (bg) localStorage.setItem(STORAGE_THEME_BG, String(bg));
  else localStorage.removeItem(STORAGE_THEME_BG);
  // 主题值写入后再叠加编辑器排版偏好覆盖（主题切换时覆盖不丢失）
  applyEditorOverrides();
  // 公式中文回落字体随主题下发（ratex 首次加载后进程内锁定，运行中切换需重启生效）
  const mathFont = resolved.typography?.math_cjk_font;
  if (mathFont) {
    invoke('set_math_unicode_font', { spec: mathFont }).catch((e) => console.error('set_math_unicode_font 失败:', e));
  }
  localStorage.setItem(STORAGE_THEME_ID, id);
  themeVersion.value += 1;
  return id;
}

/** 当前主题 id（未设置时为默认主题）。 */
export function currentThemeId() {
  return localStorage.getItem(STORAGE_THEME_ID) || DEFAULT_THEME_ID;
}

/** 启动时恢复上次选择的主题。 */
export function initTheme() {
  initFollowSystem();
  if (getPref('theme_follow_system')) applySystemTheme();
  else applyTheme(currentThemeId());
}

// ---------- 主题跟随系统外观 ----------
// 开启偏好 theme_follow_system 后：按系统明暗在 theme_dark_id / theme_light_id
// 之间切换；系统外观变化与偏好变化均即时生效。
const systemDarkMedia = window.matchMedia('(prefers-color-scheme: dark)');
let followSystemInited = false;

/** 当前系统是否为暗色外观。 */
export function isSystemDark() {
  return systemDarkMedia.matches;
}

/** 按系统外观应用偏好中设置的暗/亮色主题。 */
export function applySystemTheme() {
  const dark = systemDarkMedia.matches;
  const id = dark ? getPref('theme_dark_id') : getPref('theme_light_id');
  applyTheme(id || (dark ? 'velotype' : 'velotype-light'));
}

function initFollowSystem() {
  if (followSystemInited) return;
  followSystemInited = true;
  // 系统外观切换时跟随换主题
  systemDarkMedia.addEventListener('change', () => {
    if (getPref('theme_follow_system')) applySystemTheme();
  });
  // 跟随开关/暗色主题/亮色主题偏好变更时即时应用（applyTheme 不动 prefsVersion，无循环）。
  // 仅在主题相关键变化时应用——无关偏好（字号等）变更不得把手动导入/选择的主题换回槽位主题
  let lastThemeSig = '';
  watch(prefsVersion, () => {
    const sig = [getPref('theme_follow_system'), getPref('theme_dark_id'), getPref('theme_light_id')].join('|');
    if (sig === lastThemeSig) return;
    lastThemeSig = sig;
    if (getPref('theme_follow_system')) applySystemTheme();
  });
}

// 剥离 JSONC 注释（跳过字符串内部的 // 与 /*）
function stripJsonComments(text) {
  let out = '';
  let inString = false;
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    const next = text[i + 1];
    if (inString) {
      out += ch;
      if (ch === '\\') {
        out += next ?? '';
        i++;
      } else if (ch === '"') {
        inString = false;
      }
      continue;
    }
    if (ch === '"') {
      inString = true;
      out += ch;
    } else if (ch === '/' && next === '/') {
      while (i < text.length && text[i] !== '\n') i++;
      out += '\n';
    } else if (ch === '/' && next === '*') {
      i += 2;
      while (i < text.length && !(text[i] === '*' && text[i + 1] === '/')) i++;
      i++;
    } else {
      out += ch;
    }
  }
  return out;
}

// 去掉 JSONC 尾逗号（跳过字符串内部），使 JSON.parse 兼容
function stripJsonTrailingCommas(text) {
  let out = '';
  let inString = false;
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (inString) {
      out += ch;
      if (ch === '\\') {
        out += text[i + 1] ?? '';
        i++;
      } else if (ch === '"') {
        inString = false;
      }
      continue;
    }
    if (ch === '"') {
      inString = true;
      out += ch;
      continue;
    }
    if (ch === ',') {
      // 下一个非空白字符是 } 或 ] 时该逗号为尾逗号，丢弃
      let j = i + 1;
      while (j < text.length && /\s/.test(text[j])) j++;
      if (text[j] === '}' || text[j] === ']') continue;
    }
    out += ch;
  }
  return out;
}

// ---------- 受限 YAML 子集解析（主题包用） ----------
// 支持：嵌套映射（空格缩进）、`#` 注释（仅 `# ` 开头或行尾，避免吃掉 #ff0000 颜色）、
// 单/双引号与纯量、数字/布尔/null。不支持列表、flow 语法、多行字符串与锚点
// （主题包结构仅为嵌套映射，足够覆盖）。
function stripYamlComment(line) {
  let quote = null;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (quote) {
      if (ch === quote && line[i - 1] !== '\\') quote = null;
      continue;
    }
    if (ch === '"' || ch === "'") {
      quote = ch;
      continue;
    }
    // 宽松规则：`# ` 或行尾 `#` 才是注释（颜色 #ff0000 不受影响）
    if (ch === '#' && (i === 0 || line[i - 1] === ' ' || line[i - 1] === '\t')) {
      if (i + 1 >= line.length || line[i + 1] === ' ') {
        return line.slice(0, i);
      }
    }
  }
  return line;
}

function parseYamlScalar(raw) {
  const text = raw.trim();
  if (text === '' || text === 'null' || text === 'Null' || text === 'NULL' || text === '~') return null;
  if (text === 'true' || text === 'True' || text === 'TRUE') return true;
  if (text === 'false' || text === 'False' || text === 'FALSE') return false;
  if (text.startsWith('"') && text.endsWith('"') && text.length >= 2) {
    // 双引号：处理常见转义
    return text.slice(1, -1).replace(/\\(["\\/nrt])/g, (_, ch) => {
      return { '"': '"', '\\': '\\', '/': '/', n: '\n', r: '\r', t: '\t' }[ch] ?? ch;
    });
  }
  if (text.startsWith("'") && text.endsWith("'") && text.length >= 2) {
    return text.slice(1, -1).replace(/''/g, "'");
  }
  const num = Number(text);
  if (text !== '' && !Number.isNaN(num) && /^-?[\d.]+(e[+-]?\d+)?$/i.test(text)) return num;
  return text;
}

// YAML 键值切分：键结束于「后随空格或行尾」的冒号（跳过引号内与 nth-child/:hover 等键内冒号）
function splitYamlKeyValue(content) {
  let quote = null;
  for (let i = 0; i < content.length; i++) {
    const ch = content[i];
    if (quote) {
      if (ch === quote && content[i - 1] !== '\\') quote = null;
      continue;
    }
    if (ch === '"' || ch === "'") {
      quote = ch;
      continue;
    }
    if (ch === ':' && (i + 1 >= content.length || content[i + 1] === ' ')) {
      const key = content.slice(0, i).trim();
      const value = i + 1 >= content.length ? undefined : content.slice(i + 1).trim();
      return [key, value];
    }
  }
  return null;
}

function parseYamlSubset(text) {
  const root = {};
  const stack = [{ indent: -1, value: root }];
  for (const rawLine of text.split('\n')) {
    const line = stripYamlComment(rawLine);
    if (!line.trim()) continue;
    if (/\t/.test(rawLine.slice(0, rawLine.length - rawLine.trimStart().length))) {
      throw new Error('YAML 缩进请使用空格（不支持 Tab）');
    }
    const indent = line.length - line.trimStart().length;
    const content = line.trim();
    const pair = splitYamlKeyValue(content);
    if (!pair) throw new Error(`无法解析的 YAML 行: ${content}`);
    let [key, rawValue] = pair;
    if (key.startsWith('"') && key.endsWith('"')) key = key.slice(1, -1);
    else if (key.startsWith("'") && key.endsWith("'")) key = key.slice(1, -1);
    while (stack.length > 1 && indent <= stack[stack.length - 1].indent) stack.pop();
    const parent = stack[stack.length - 1].value;
    if (rawValue === undefined) {
      // 无值键：嵌套映射（子行缩进更深时入栈生效）
      const child = {};
      parent[key] = child;
      stack.push({ indent, value: child });
    } else {
      parent[key] = parseYamlScalar(rawValue);
    }
  }
  return root;
}

/**
 * 导入自定义主题包（JSON/JSONC 或 YAML 文本），注册、持久化并应用。
 * 格式按内容自动识别（跳过前导注释后以 { 开头为 JSON/JSONC，否则按 YAML 解析）。
 * 返回新主题 id；格式非法时抛错。
 */
export function importThemeJson(text) {
  const trimmed = text.trimStart();
  // JSONC 常以注释开头（如示例文件），跳过前导注释再判定格式——否则会被误判为 YAML
  const head = trimmed.replace(/^(\/\/[^\n]*\n?|\/\*[\s\S]*?\*\/\s*)+/, '');
  const pack = head.startsWith('{')
    ? JSON.parse(stripJsonTrailingCommas(stripJsonComments(text)))
    : parseYamlSubset(text);
  if (!pack || typeof pack.name !== 'string' || !pack.name.trim()) {
    throw new Error('主题包缺少必填字段 name');
  }
  const custom = loadCustomThemes();
  const id = `custom-${Date.now().toString(36)}`;
  custom.push({ ...pack, id, name: pack.name.trim() });
  saveCustomThemes(custom);
  return applyTheme(id);
}

/** 删除一个自定义主题；若正在使用则回退默认主题。 */
export function removeCustomTheme(id) {
  saveCustomThemes(loadCustomThemes().filter((theme) => theme.id !== id));
  if (currentThemeId() === id) applyTheme(DEFAULT_THEME_ID);
}
