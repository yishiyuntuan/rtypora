import { htmlStyleCss } from './wysiwyg.js';

// 字体样式 DOM 构建/查询助手（斜杠菜单与右键菜单共用；htmlStyle JSON 落 data 属性无损往返）

// 字体样式 span 构建
export function buildFontSpan(style) {
  const span = document.createElement('span');
  span.setAttribute('style', htmlStyleCss(style));
  span.setAttribute('data-html-style', JSON.stringify(style));
  return span;
}

// 字体样式模板：淡灰 <font> 标签 + 中间样式 span（FEFF 哨兵光标位）——源码形态编辑，
// 标签仅作显示且不可编辑（contenteditable=false：否则光标可落入标签文本内，
// 输入被当作标签装饰、提交提取时剔除——表现为「标签后输入的文字被吞」）
export function buildFontTemplate(style) {
  const open = document.createElement('span');
  open.className = 'md-font-mark';
  open.setAttribute('contenteditable', 'false');
  open.textContent = `<font style="${htmlStyleCss(style)}">`;
  const inner = buildFontSpan(style);
  const zwsp = document.createTextNode('\uFEFF');
  inner.append(zwsp);
  const close = document.createElement('span');
  close.className = 'md-font-mark';
  close.setAttribute('contenteditable', 'false');
  close.textContent = '</font>';
  const frag = document.createDocumentFragment();
  frag.append(open, inner, close);
  return { frag, zwsp };
}

// 光标放入字体模板的文字位（哨兵之后）
export function placeCaretInFontTemplate(zwsp) {
  const sel = window.getSelection();
  const range = document.createRange();
  range.setStart(zwsp, 1);
  range.collapse(true);
  sel.removeAllRanges();
  sel.addRange(range);
}

// 光标所在的字体样式 span（无则 null）
export function caretFontSpan(el) {
  const sel = window.getSelection();
  if (!sel.rangeCount || !el.contains(sel.anchorNode)) return null;
  const node =
    sel.anchorNode.nodeType === Node.TEXT_NODE ? sel.anchorNode.parentElement : sel.anchorNode;
  return node?.closest?.('span[data-html-style]') ?? null;
}

// 在既有样式 span 上按字段合并（color/fontSize 各自独立覆盖）
function mergeStyleIntoSpan(span, style) {
  let current = {};
  try {
    current = JSON.parse(span.getAttribute('data-html-style') || '{}');
  } catch {
    current = {};
  }
  const merged = { ...current, ...style };
  span.setAttribute('data-html-style', JSON.stringify(merged));
  span.setAttribute('style', htmlStyleCss(merged));
}

// 合并样式并同步模板开标签显示文本（颜色/字号变化后标签内容与样式一致）；
// 斜杠菜单与右键菜单的「光标在既有字体 span 内」路径共用
export function mergeStyleIntoSpanWithMarkSync(span, style) {
  mergeStyleIntoSpan(span, style);
  const openMark = span.previousSibling;
  if (openMark?.nodeType === Node.ELEMENT_NODE && openMark.classList.contains('md-font-mark')) {
    openMark.textContent = `<font style="${span.getAttribute('style')}">`;
  }
}
