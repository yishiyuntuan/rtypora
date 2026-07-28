// Typora 式就地编辑辅助：Block 模型 → 可编辑 HTML 字符串，contenteditable DOM → BlockDto JSON。
// 编辑态 HTML 的结构与样式和 BlockView 渲染保持一致（BlockView 类名调整时需同步此处）：
// 用户在渲染结果上直接编辑；前端只提取 DOM 结构，Markdown 的解析与序列化全部由
// Rust 端完成（parse_markdown / parse_blocks / serialize_markdown / detect_block_shortcut 等命令）。

import { invoke } from '@tauri-apps/api/core';
//
// 数据模型为 velotype 移植版：块 = { id, type, title(InlineTextTree), table, rawFallback, children, start/end }；
// 行内 = InlineTextTree { fragments: [{ text, style{bold,italic,underline,strikethrough,code,script}, link, footnote, math }] }。
// 行内定界符与 Rust 端 serialize_markdown 对齐：粗 **、斜 *、删除 ~~、下划线 <u>、上标 ^x^、下标 ~x~、
// 行内代码 `、脚注 [^id]、行内公式 $..$（回写用原 source）。

const headingClasses = {
  1: 'my-3',
  2: 'my-3',
  3: 'my-2',
  4: 'my-2',
  5: 'my-1',
  6: 'my-1',
};

const P_CLASS = 'my-2 whitespace-pre-wrap';
const INLINE_CODE_CLASS = 'md-code px-1 py-0.5 font-mono';
const LINK_CLASS = 'underline underline-offset-2';
const PRE_CLASS = 'md-pre my-2 overflow-x-auto rounded p-3 font-mono text-[13px]';
const QUOTE_CLASS = 'my-2 border-l-4 pl-3';
const FOOTNOTE_REF_CLASS = 'md-footnote-ref align-super text-[0.75em]';

const HTML_ESCAPES = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' };

function escapeHtml(text) {
  return String(text).replace(/[&<>"]/g, (c) => HTML_ESCAPES[c]);
}

export function emptyParagraphHtml() {
  return `<p class="${P_CLASS}"><br></p>`;
}

// InlineTextTree 的纯文本（代码块内容等场景使用）
export function plainText(tree) {
  return (tree?.fragments || []).map((f) => f.text).join('');
}

// 链接目标的统一取值（inline: destination；reference: destination；autolink: target）
function linkHref(link) {
  return link?.destination || link?.target || '';
}

// 连续 numberedListItem 兄弟块的序号表（id -> 1 基序号），用于有序列表渲染。
// 块模型会把列表项之间的空行保留为空段落块（velotype 编辑模型），空段落不打断
// 编号（对应 CommonMark 松散列表）；实质内容块（含独立图片段落）才重置序号。
export function numberedOrdinals(blocks) {
  const map = new Map();
  let n = 0;
  for (const b of blocks || []) {
    if (b.type === 'numberedListItem') {
      n += 1;
      map.set(b.id, n);
    } else if (b.type === 'paragraph' && !plainText(b.title).trim() && !b.image) {
      continue;
    } else {
      n = 0;
    }
  }
  return map;
}

// ---------- Block 模型 → 可编辑 HTML ----------

// InlineTextTree → HTML；与 InlineView.vue 的渲染结构保持一致
export function inlineToHtml(tree) {
  return (tree?.fragments || []).map(fragmentToHtml).join('');
}

function fragmentToHtml(f) {
  // 脚注引用：整体为不可拆分的上标记号，回写为 [^id]
  if (f.footnote) {
    return `<sup data-footnote-id="${escapeHtml(f.footnote.id)}" class="${FOOTNOTE_REF_CLASS}">[${escapeHtml(f.footnote.id)}]</sup>`;
  }
  // 行内公式：占位渲染，回写原 source
  if (f.math) {
    return `<code data-math-source="${escapeHtml(f.math.source)}" class="${INLINE_CODE_CLASS}">${escapeHtml(f.math.body)}</code>`;
  }
  let html = escapeHtml(f.text);
  const s = f.style || {};
  if (s.code) html = `<code class="${INLINE_CODE_CLASS}">${html}</code>`;
  if (s.bold) html = `<strong class="font-semibold">${html}</strong>`;
  if (s.italic) html = `<em>${html}</em>`;
  if (s.strikethrough) html = `<s>${html}</s>`;
  if (s.underline) html = `<u>${html}</u>`;
  if (s.highlight) html = `<mark>${html}</mark>`;
  if (s.kbd) html = `<kbd>${html}</kbd>`;
  if (s.script === 'superscript') html = `<sup>${html}</sup>`;
  if (s.script === 'subscript') html = `<sub>${html}</sub>`;
  if (f.link) {
    html = `<a class="${LINK_CLASS}" href="${escapeHtml(linkHref(f.link))}">${html}</a>`;
  }
  // HTML 行内样式（<span style>/<font>）：样式上屏 + JSON 存 data 属性无损往返
  if (f.htmlStyle) {
    html = `<span style="${htmlStyleCss(f.htmlStyle)}" data-html-style="${escapeHtml(JSON.stringify(f.htmlStyle))}">${html}</span>`;
  }
  return html;
}

// HTML 行内样式（颜色/背景/字号白名单）→ CSS 字符串（渲染态 :style 与编辑态 span 共用）
export function htmlStyleCss(hs) {
  const colorCss = (c) => {
    if (!c) return undefined;
    if (c === 'currentColor') return 'currentColor';
    // var(--x) 主题变量引用原样输出
    if (c.var) return c.var;
    const r = c.rgba;
    return r ? `rgba(${r.red},${r.green},${r.blue},${r.alpha})` : undefined;
  };
  const sizeCss = (f) => {
    if (!f) return undefined;
    if (f.px != null) return `${f.px}px`;
    if (f.em != null) return `${f.em}em`;
    if (f.rem != null) return `${f.rem}rem`;
    if (f.percent != null) return `${f.percent}%`;
    // 关键字（xxSmall → xx-small 等）
    if (f.keyword) return f.keyword.replace(/[A-Z]/g, (m) => '-' + m.toLowerCase());
    return undefined;
  };
  const parts = [];
  const color = colorCss(hs.color);
  const backgroundColor = colorCss(hs.backgroundColor);
  const fontSize = sizeCss(hs.fontSize);
  if (color) parts.push(`color: ${color}`);
  if (backgroundColor) parts.push(`background-color: ${backgroundColor}`);
  if (fontSize) parts.push(`font-size: ${fontSize}`);
  return parts.join('; ');
}

// 嵌套 htmlStyle 合并：内层字段优先（外层补空缺）
function mergeHtmlStyle(outer, inner) {
  if (!outer) return inner || null;
  if (!inner) return outer;
  return {
    color: inner.color ?? outer.color ?? null,
    backgroundColor: inner.backgroundColor ?? outer.backgroundColor ?? null,
    fontSize: inner.fontSize ?? outer.fontSize ?? null,
  };
}

// 块 → 可编辑 HTML。rawSource 为该块在全文中的原始 Markdown 切片（原子块按原文编辑）。
// 富编辑块带与渲染态一致的 blk-* 语义类（按块自定义样式在编辑态同样生效）。
// depth 为列表嵌套层级（0 起）：ul/ol 带 lst-d{1..3} 层级类，编辑态标记随层级
// 区分（圆点 disc/circle/square、序号 decimal/lower-alpha/lower-roman，与渲染态一致）。
export function blockToHtml(block, rawSource, depth = 0) {
  const lstClass = `lst-d${Math.min(depth + 1, 3)}`;
  switch (block.type) {
    case 'paragraph':
      return `<p class="blk-paragraph ${P_CLASS}">${inlineToHtml(block.title)}</p>`;
    case 'heading':
      return `<h${block.level} class="blk-heading whitespace-pre-wrap ${headingClasses[block.level] || 'my-2'}">${inlineToHtml(block.title)}</h${block.level}>`;
    case 'bulletedListItem':
      return `<ul class="blk-bulleted-list-item ${lstClass} my-2 pl-6 list-disc"><li class="my-0.5">${listItemBodyHtml(block, depth)}</li></ul>`;
    case 'numberedListItem':
      return `<ol class="blk-numbered-list-item ${lstClass} my-2 pl-6 list-decimal"><li class="my-0.5">${listItemBodyHtml(block, depth)}</li></ol>`;
    case 'taskListItem': {
      // 勾选框不可编辑、不可点击（勾选在渲染态完成）
      const checkbox = `<input type="checkbox" class="pointer-events-none mt-[5px] shrink-0" data-checked="${block.checked ? 'x' : ' '}" ${block.checked ? 'checked' : ''} contenteditable="false">`;
      const body = `<div class="flex items-start gap-1.5">${checkbox}<div class="min-w-0 flex-1">${listItemBodyHtml(block, depth)}</div></div>`;
      return `<ul class="blk-task-list-item ${lstClass} my-2 list-none pl-4"><li class="my-0.5">${body}</li></ul>`;
    }
    case 'quote': {
      const title = plainText(block.title)
        ? `<p class="${P_CLASS}">${inlineToHtml(block.title)}</p>`
        : '';
      return `<blockquote class="blk-quote ${QUOTE_CLASS}">${title}${(block.children || []).map((c) => blockToHtml(c, undefined, depth)).join('')}</blockquote>`;
    }
    case 'table': {
      // 表格富编辑：thead/tbody 单元格就地编辑（对齐信息写在单元格 textAlign，
      // 提交时经 tableToBlock 提取为 DTO 对齐列，与渲染/序列化同一数据源）
      const table = block.table || { header: [], rows: [], alignments: [] };
      const cellHtml = (tree, tag, i) => {
        const align = table.alignments?.[i];
        const style = align && align !== 'default' ? ` style="text-align:${align}"` : '';
        const inner = plainText(tree) ? inlineToHtml(tree) : '<br>';
        return `<${tag}${style}>${inner}</${tag}>`;
      };
      const thead = `<thead><tr>${table.header.map((cell, i) => cellHtml(cell, 'th', i)).join('')}</tr></thead>`;
      const tbody = `<tbody>${table.rows
        .map((row) => `<tr>${row.map((cell, i) => cellHtml(cell, 'td', i)).join('')}</tr>`)
        .join('')}</tbody>`;
      return `<table class="blk-table">${thead}${tbody}</table>`;
    }
    case 'codeBlock':
      // 编辑态不内嵌高亮（避免 contenteditable 拆分 span），渲染态经 Rust tree-sitter 高亮；
      // 右上角语言输入框（输入同步到 data-language，提交时随块序列化；
      // contenteditable="false" 必须——否则 Chromium 下键入由编辑宿主接管，不产生 input 事件）
      return `<pre class="${PRE_CLASS} blk-code-block relative" data-language="${escapeHtml(block.language || '')}"><input class="md-code-lang-input" data-lang-input contenteditable="false" value="${escapeHtml(block.language || '')}" placeholder="text" spellcheck="false" /><code>${escapeHtml(plainText(block.title))}</code></pre>`;
    // 原子/保留类块：编辑原始 Markdown 切片，保证不丢内容
    case 'separator':
    case 'callout':
    case 'footnoteDefinition':
    case 'mathBlock':
    case 'mermaidBlock':
    case 'htmlBlock':
    case 'sectionBlock':
    case 'comment':
    case 'rawMarkdown':
      return `<pre class="${PRE_CLASS}" data-raw="">${escapeHtml(rawSource ?? block.rawFallback ?? plainText(block.title))}</pre>`;
    default:
      return '';
  }
}

function listItemBodyHtml(block, depth = 0) {
  const title = plainText(block.title) ? inlineToHtml(block.title) : '<br>';
  return title + (block.children || []).map((c) => blockToHtml(c, undefined, depth + 1)).join('');
}

// ---------- contenteditable DOM → BlockDto JSON ----------
// 前端只从 DOM 提取结构（块类型 + 行内样式标志），Markdown 文本的生成全部交给
// Rust 命令 serialize_markdown（定界符选择、转义、引用前缀、围栏长度等规则由 Rust 统一负责）。

let dtoSeq = 0;

function makeTree(fragments) {
  return { fragments };
}

function makeFragment(text, style, extra = {}) {
  return {
    text,
    style: {
      bold: false,
      italic: false,
      underline: false,
      strikethrough: false,
      highlight: false,
      kbd: false,
      code: false,
      script: 'normal',
      ...style,
    },
    htmlStyle: null,
    link: null,
    footnote: null,
    math: null,
    ...extra,
  };
}

function makeBlock(kindFields, { title, table, rawFallback, children } = {}) {
  return {
    id: `dto-${++dtoSeq}`,
    ...kindFields,
    title: title ?? makeTree([]),
    ...(table ? { table } : {}),
    ...(rawFallback != null ? { rawFallback } : {}),
    ...(children?.length ? { children } : {}),
  };
}

// DOM 子树 → 行内 fragments（样式标志沿元素嵌套累积）
function domToInlines(el, style = {}, out = []) {
  for (const node of el.childNodes) {
    if (node.nodeType === Node.TEXT_NODE) {
      if (node.textContent) out.push(makeFragment(node.textContent, style));
      continue;
    }
    if (node.nodeType !== Node.ELEMENT_NODE) continue;
    const tag = node.tagName;
    if (tag === 'INPUT') continue; // 任务勾选框由 checked 字段表达
    if (tag === 'BR') {
      out.push(makeFragment('\n', style));
      continue;
    }
    // 脚注引用：整体为一个 fragment，回写 [^id]
    if (tag === 'SUP' && node.hasAttribute('data-footnote-id')) {
      const id = node.getAttribute('data-footnote-id');
      out.push(makeFragment(`[^${id}]`, style, { footnote: { id } }));
      continue;
    }
    // 行内公式占位：回写原 source
    if (tag === 'CODE' && node.hasAttribute('data-math-source')) {
      out.push(
        makeFragment(node.textContent, style, {
          math: {
            source: node.getAttribute('data-math-source'),
            body: node.textContent,
            delimiter: 'dollar',
          },
        }),
      );
      continue;
    }
    // 行内模型无图片节点：回写图片语法文本，交由 Rust 重新解析
    if (tag === 'IMG') {
      out.push(makeFragment(`![${node.getAttribute('alt') || ''}](${node.getAttribute('src') || ''})`, style));
      continue;
    }
    // HTML 行内样式 span（<span style>/<font> 编辑态占位）：data-html-style JSON 无损往返
    if (tag === 'SPAN' && node.hasAttribute('data-html-style')) {
      const inner = domToInlines(node, style, []);
      let hs = null;
      try {
        hs = JSON.parse(node.getAttribute('data-html-style'));
      } catch {
        hs = null; // 损坏数据按无样式处理
      }
      for (const f of inner) f.htmlStyle = mergeHtmlStyle(hs, f.htmlStyle);
      out.push(...inner);
      continue;
    }
    if (tag === 'CODE') {
      out.push(makeFragment(node.textContent, { ...style, code: true }));
      continue;
    }
    if (tag === 'A') {
      // 链接：内部 fragments 统一挂 link 元数据
      const inner = domToInlines(node, style, []);
      const href = node.getAttribute('href') || '';
      for (const f of inner) f.link = { type: 'inline', destination: href };
      out.push(...inner);
      continue;
    }
    const next = { ...style };
    if (tag === 'STRONG' || tag === 'B') next.bold = true;
    else if (tag === 'EM' || tag === 'I') next.italic = true;
    else if (tag === 'S' || tag === 'DEL') next.strikethrough = true;
    else if (tag === 'U') next.underline = true;
    else if (tag === 'MARK') next.highlight = true;
    else if (tag === 'KBD') next.kbd = true;
    else if (tag === 'SUP') next.script = 'superscript';
    else if (tag === 'SUB') next.script = 'subscript';
    domToInlines(node, next, out);
  }
  return out;
}

const BLOCK_TAGS = new Set(['P', 'DIV', 'H1', 'H2', 'H3', 'H4', 'H5', 'H6', 'PRE', 'BLOCKQUOTE', 'UL', 'OL', 'TABLE', 'HR']);

function isBlockElement(node) {
  return node.nodeType === Node.ELEMENT_NODE && BLOCK_TAGS.has(node.tagName);
}

// 把 el 的子节点按块分组：连续的文本/行内节点合并为段落，块级元素单独转换
function domChildrenToBlocks(el) {
  const blocks = [];
  let buffer = null;
  const flush = () => {
    if (!buffer) return;
    const fragments = domToInlines(buffer);
    buffer = null;
    if (fragments.some((f) => f.text.trim() || f.footnote || f.math)) {
      blocks.push(makeBlock({ type: 'paragraph' }, { title: makeTree(fragments) }));
    }
  };
  for (const child of el.childNodes) {
    if (isBlockElement(child)) {
      flush();
      blocks.push(...elementToBlocks(child));
    } else if (child.nodeType === Node.ELEMENT_NODE && child.tagName === 'INPUT') {
      continue;
    } else {
      if (!buffer) buffer = document.createElement('span');
      buffer.append(child.cloneNode(true));
    }
  }
  flush();
  return blocks;
}

// 去掉首尾由占位 <br> 产生的换行 fragment（空结构占位或输入残留；
// 中间的 Shift+Enter 软换行保留）
function trimEdgeNewlines(fragments) {
  const out = [...fragments];
  while (out.length && out[0].text === '\n') out.shift();
  while (out.length && out[out.length - 1].text === '\n') out.pop();
  return out;
}

function elementToBlocks(el) {
  const tag = el.tagName;
  if (/^H[1-6]$/.test(tag)) {
    const fragments = trimEdgeNewlines(domToInlines(el));
    // 标题删空后降级为段落（与 Typora 行为一致，不再保留空标题渲染）
    if (!fragments.some((f) => f.text.trim() || f.footnote || f.math)) {
      return [makeBlock({ type: 'paragraph' })];
    }
    return [makeBlock({ type: 'heading', level: Number(tag[1]) }, { title: makeTree(fragments) })];
  }
  if (tag === 'P' || tag === 'DIV') {
    // 包装 div（渲染态 .md-block 等）仅含单个块级子元素且无其他文本时解包递归——
    // 粘贴本应用渲染 HTML 时保留标题/引用/表格等块类型
    if (tag === 'DIV' && el.childElementCount === 1 && isBlockElement(el.firstElementChild)) {
      const child = el.firstElementChild;
      const textOutside = [...el.childNodes].some((n) => n !== child && n.textContent.trim() !== '');
      if (!textOutside) return elementToBlocks(child);
    }
    return [makeBlock({ type: 'paragraph' }, { title: makeTree(trimEdgeNewlines(domToInlines(el))) })];
  }
  if (tag === 'PRE') {
    // 原子/保留类块按原文回写
    if (el.hasAttribute('data-raw')) {
      const raw = el.textContent.replace(/\n+$/, '');
      return [makeBlock({ type: 'rawMarkdown' }, { title: makeTree([makeFragment(raw)]), rawFallback: raw })];
    }
    const language = el.getAttribute('data-language') || '';
    const code = el.textContent.replace(/\n+$/, '');
    return [makeBlock({ type: 'codeBlock', language: language || null }, { title: makeTree([makeFragment(code)]) })];
  }
  if (tag === 'BLOCKQUOTE') {
    const blocks = domChildrenToBlocks(el);
    let title = makeTree([]);
    let children = blocks;
    // 模型中引用的 title 为首行内容，其余为嵌套子块
    if (blocks[0]?.type === 'paragraph') {
      title = blocks[0].title;
      children = blocks.slice(1);
    }
    return [makeBlock({ type: 'quote' }, { title, children })];
  }
  if (tag === 'UL' || tag === 'OL') return listToBlocks(el, tag === 'OL');
  if (tag === 'TABLE') return tableToBlock(el);
  if (tag === 'HR') return [makeBlock({ type: 'separator' })];
  return [makeBlock({ type: 'paragraph' }, { title: makeTree(domToInlines(el)) })];
}

function listToBlocks(el, ordered) {
  const items = [];
  el.querySelectorAll(':scope > li').forEach((li) => {
    // 只认本项自己的勾选框：input 最近的祖先 li 必须是本 li
    // （后代选择器会误中嵌套任务项的勾选框，把父项错判成任务项）
    let checkbox = null;
    for (const input of li.querySelectorAll('input[type="checkbox"]')) {
      if (input.closest('li') === li) {
        checkbox = input;
        break;
      }
    }
    // 任务项的正文在勾选框旁的 body 容器里（含嵌套子块），从该容器提取，
    // 否则整个行 div 会被当作段落、嵌套列表文字被 domToInlines 吸进标题而拍平
    const container = checkbox ? checkbox.nextElementSibling || li : li;
    const blocks = domChildrenToBlocks(container);
    let title = makeTree([]);
    let children = blocks;
    if (blocks[0]?.type === 'paragraph') {
      title = blocks[0].title;
      children = blocks.slice(1);
    }
    const kind = checkbox
      ? { type: 'taskListItem', checked: checkbox.getAttribute('data-checked') === 'x' }
      : ordered
        ? { type: 'numberedListItem' }
        : { type: 'bulletedListItem' };
    items.push(makeBlock(kind, { title, children }));
  });
  return items;
}

function tableToBlock(el) {
  const header = [];
  const rows = [];
  el.querySelectorAll('thead tr').forEach((tr) => {
    tr.querySelectorAll('th').forEach((cell) => header.push(makeTree(domToInlines(cell))));
  });
  el.querySelectorAll('tbody tr').forEach((tr) => {
    const row = [];
    tr.querySelectorAll('td').forEach((cell) => row.push(makeTree(domToInlines(cell))));
    if (row.length) rows.push(row);
  });
  // 对齐信息来自渲染态写入的 textAlign 样式
  let alignments = [];
  const firstRow = el.querySelector('tr');
  if (firstRow) {
    alignments = Array.from(firstRow.children).map((cell) =>
      ['left', 'center', 'right'].includes(cell.style.textAlign) ? cell.style.textAlign : 'default',
    );
  }
  return [makeBlock({ type: 'table' }, { table: { header, rows, alignments } })];
}

// contenteditable 根元素 → BlockDto JSON 数组（交给 Rust serialize_markdown）
export function domToBlockDtos(root) {
  return domChildrenToBlocks(root);
}

// ---------- Markdown 快捷输入（Typora 式即时转换） ----------

// 代码块编辑 DOM：pre + 右上角语言输入框 + code（结构与 blockToHtml 一致；
// 输入框 contenteditable="false" 否则 Chromium 下键入不产生 input 事件）
export function createCodePre(language) {
  const pre = document.createElement('pre');
  pre.className = `${PRE_CLASS} relative`;
  pre.setAttribute('data-language', language || '');
  const input = document.createElement('input');
  input.className = 'md-code-lang-input';
  input.setAttribute('data-lang-input', '');
  input.setAttribute('contenteditable', 'false');
  input.placeholder = 'text';
  input.spellcheck = false;
  input.value = language || '';
  pre.append(input, styled('code', ''));
  return pre;
}

// 围栏行（```lang / ~~~lang）按 Enter 时调用：把当前编辑容器转换为代码块编辑
// （pre[data-language]，光标进入块内），返回是否已转换。
// 语言标记由 Rust 命令 detect_block_shortcut 判定。
export async function convertFenceToCodeBlock(container) {
  const text = container.textContent.trim();
  // 仅围栏起始符才调用 Rust 判定
  if (!/^[`~]/.test(text)) return false;
  const hit = await invoke('detect_block_shortcut', { line: text }).catch(() => null);
  if (!hit || hit.type !== 'codeBlock') return false;
  const pre = createCodePre(hit.language);
  container.innerHTML = '';
  container.append(pre);
  placeCursorAtEnd(pre);
  return true;
}

// `<section>` 行按 Enter：转换为 section 图文排版块的原文编辑
// （模板由 Rust block_template 生成，光标在中间空行；类型判定由 Rust detect_block_shortcut 完成）。
export async function convertSectionToHtmlBlock(container) {
  const text = container.textContent.trim();
  // 仅开标签才调用 Rust 判定
  if (!/^<section/i.test(text)) return false;
  const hit = await invoke('detect_block_shortcut', { line: text }).catch(() => null);
  if (!hit || hit.type !== 'sectionBlock') return false;
  const tpl = await invoke('block_template', { kind: 'sectionBlock' }).catch(() => null);
  if (!tpl) return false;
  const pre = document.createElement('pre');
  pre.className = PRE_CLASS;
  pre.setAttribute('data-raw', '');
  pre.textContent = tpl.markdown;
  container.innerHTML = '';
  container.append(pre);
  placeCaretAtTextOffset(pre, tpl.caretOffset);
  return true;
}

function styled(tag, text, className) {
  const el = document.createElement(tag);
  if (className) el.className = className;
  el.textContent = text;
  return el;
}

// HTML 标签自动闭合：输入 > 完成开始标签时，在光标后补闭合标签（光标停在开闭之间）。
// 触发判定在 Rust（inline_html_autoclose，已知标签白名单），这里只做文本插入。
export async function applyHtmlAutoclose() {
  const sel = window.getSelection();
  if (!sel.rangeCount || !sel.isCollapsed) return false;
  const node = sel.anchorNode;
  if (node.nodeType !== Node.TEXT_NODE) return false;
  // 代码/pre 内不触发
  if (node.parentElement?.closest('pre, code')) return false;
  const offset = sel.anchorOffset;
  const before = node.textContent.slice(0, offset);
  // 性能闸门：刚输入的字符是 > 才可能触发
  if (!before.endsWith('>')) return false;
  const closing = await invoke('inline_html_autoclose', { text: before }).catch(() => null);
  if (!closing) return false;
  // 等待期间光标/文本变化则放弃
  const currentSel = window.getSelection();
  if (!currentSel.rangeCount || currentSel.anchorNode !== node || currentSel.anchorOffset !== offset) {
    return false;
  }
  node.textContent = node.textContent.slice(0, offset) + closing + node.textContent.slice(offset);
  // 光标保持原位（开闭标签之间）
  const range = document.createRange();
  range.setStart(node, offset);
  range.collapse(true);
  currentSel.removeAllRanges();
  currentSel.addRange(range);
  return true;
}

// HTML 块级容器 Ctrl+Enter 展开：光标位于 <name> 与 </name> 之间时，拆为
// 开标签/空行/闭标签 三行原文编辑（与 section 模板同一形态），光标落中间行。
// 配对与容器判定在 Rust（html_container_tag_between）。
export async function expandHtmlTagAtCaret(container) {
  const sel = window.getSelection();
  if (!sel.rangeCount || !sel.isCollapsed) return false;
  const node = sel.anchorNode;
  if (node.nodeType !== Node.TEXT_NODE) return false;
  if (node.parentElement?.closest('pre, code')) return false;
  const offset = sel.anchorOffset;
  const text = node.textContent;
  const before = text.slice(0, offset);
  const after = text.slice(offset);
  // 性能闸门：光标恰在闭合标签 </ 之前（标签内可有内容）
  if (!after.startsWith('</')) return false;
  // 容器内还有其他文本则不展开（交给常规拆分）
  if (container.textContent !== before + after) return false;
  const name = await invoke('html_container_tag_between', { before, after }).catch(() => null);
  if (!name) return false;
  const pre = document.createElement('pre');
  pre.className = PRE_CLASS;
  pre.setAttribute('data-raw', '');
  pre.textContent = `${before}\n\n${after}`;
  container.innerHTML = '';
  container.append(pre);
  placeCaretAtTextOffset(pre, before.length + 1);
  return true;
}

// `<tag>|</tag>` 之间按 Enter：光标跳到闭合标签之后（不展开结构、不拆分块）。
// 配对判定在 Rust（html_closing_tag_at；行内标签同样适用）。
export async function skipHtmlClosingTag(container) {
  const sel = window.getSelection();
  if (!sel.rangeCount || !sel.isCollapsed) return false;
  const node = sel.anchorNode;
  if (node.nodeType !== Node.TEXT_NODE) return false;
  if (node.parentElement?.closest('pre, code')) return false;
  const offset = sel.anchorOffset;
  const text = node.textContent;
  const before = text.slice(0, offset);
  const after = text.slice(offset);
  // 性能闸门：光标恰在闭合标签 </ 之前（标签内可有内容，如 <a>文字|</a>）
  if (!after.startsWith('</')) return false;
  // 容器内还有其他文本则不处理（交给常规拆分）
  if (container.textContent !== before + after) return false;
  const closing = await invoke('html_closing_tag_at', { before, after }).catch(() => null);
  if (!closing) return false;
  // 光标移到闭合标签之后（同一文本节点内）
  const range = document.createRange();
  range.setStart(node, offset + closing.length);
  range.collapse(true);
  sel.removeAllRanges();
  sel.addRange(range);
  return true;
}

// 行内包装（右键菜单/行内格式统一入口）：有选区包选区，无选区插入空元素并把光标放入其中。
// 与斜杠行内格式同一元素映射（bold/italic/underline/strikethrough/highlight/inlineCode/link）。
const INLINE_WRAPPERS = {
  bold: { tag: 'strong', cls: 'font-semibold' },
  italic: { tag: 'em' },
  underline: { tag: 'u' },
  strikethrough: { tag: 's' },
  highlight: { tag: 'mark' },
  inlineCode: { tag: 'code', cls: INLINE_CODE_CLASS },
  link: { tag: 'a', cls: LINK_CLASS, href: '' },
};

export function insertInlineWrapper(el, id) {
  const conf = INLINE_WRAPPERS[id];
  if (!conf) return false;
  const sel = window.getSelection();
  if (!sel.rangeCount || !el.contains(sel.getRangeAt(0).startContainer)) return false;
  const node = document.createElement(conf.tag);
  if (conf.cls) node.className = conf.cls;
  if (conf.href != null) node.setAttribute('href', conf.href);
  const range = sel.getRangeAt(0);
  if (sel.isCollapsed) {
    range.insertNode(node);
    placeCursorAtStart(node);
  } else {
    node.append(range.extractContents());
    range.insertNode(node);
    placeCursorAtEnd(node);
  }
  return true;
}

// 行内快捷转换：光标前文本以完整行内结构结尾时替换为样式元素。
// 结构识别由 Rust 命令 inline_shortcut 完成，这里只负责 DOM 替换。
async function transformInlineShortcut() {
  const sel = window.getSelection();
  if (!sel.rangeCount || !sel.isCollapsed) return false;
  const node = sel.anchorNode;
  if (node.nodeType !== Node.TEXT_NODE) return false;
  const parent = node.parentElement;
  // 代码内不触发行内转换
  if (parent && (parent.closest('pre') || parent.closest('code'))) return false;
  const offset = sel.anchorOffset;
  const before = node.textContent.slice(0, offset);
  // 可能的行内结尾才继续（性能闸门，判定在 Rust）
  if (!/[*`)\]~]$/.test(before)) return false;
  // 结尾符须有对应起始符才可能是完整行内结构（否则散文中的 ) ` ] ~ 每键一次 IPC）
  const last = before.at(-1);
  if (last === '*' && !before.slice(0, -1).includes('*')) return false;
  if (last === '`' && !before.slice(0, -1).includes('`')) return false;
  if ((last === ')' || last === ']') && !before.includes('[')) return false;
  if (last === '~' && !before.slice(0, -1).includes('~')) return false;
  const hit = await invoke('inline_shortcut', { text: before }).catch(() => null);
  // 等待期间光标/文本变化则放弃
  const currentSel = window.getSelection();
  if (!hit || !currentSel.rangeCount || currentSel.anchorNode !== node || currentSel.anchorOffset !== offset) {
    return false;
  }

  const range = document.createRange();
  range.setStart(node, offset - hit.matchLen);
  range.setEnd(node, offset);
  range.deleteContents();
  let el;
  if (hit.kind === 'image') {
    el = document.createElement('img');
    el.setAttribute('src', hit.dest || '');
    el.setAttribute('alt', hit.text);
  } else if (hit.kind === 'link') {
    el = styled('a', hit.text, LINK_CLASS);
    el.setAttribute('href', hit.dest || '');
  } else if (hit.kind === 'bold') {
    el = styled('strong', hit.text, 'font-semibold');
  } else if (hit.kind === 'italic') {
    el = styled('em', hit.text);
  } else if (hit.kind === 'strikethrough') {
    el = styled('s', hit.text);
  } else if (hit.kind === 'code') {
    el = styled('code', hit.text, INLINE_CODE_CLASS);
  } else {
    return false;
  }
  range.insertNode(el);
  placeCursorAfter(el);
  return true;
}

// 块级快捷转换：段落开头输入 # / > / - / 1. / - [ ] / ``` / --- 等标记时整块转换。
// 标记 → 块类型的判定全部由 Rust 命令 detect_block_shortcut 完成（含 fence 与分割线），
// 这里只负责 DOM 结构替换。
async function transformBlockShortcut() {
  const sel = window.getSelection();
  if (!sel.rangeCount) return false;
  const node = sel.anchorNode;
  const el = node.nodeType === Node.TEXT_NODE ? node.parentElement : node;
  const p = el && el.closest('p');
  if (!p) return false;
  const text = p.textContent;

  // 可能的块标记前缀才继续（性能闸门，判定在 Rust）：
  // 块标记都在前几个字符内完成（`###### `=7、`- [ ] `=6、`1. `=3），
  // 超出标记形态或为围栏/分割线才调用判定，避免「2024年计划」这类每键一次 IPC
  if (!/^[#>\-+*0-9~_]/.test(text)) return false;
  const plausible = text.length <= 7 || /^(`{3,}|~{3,})/.test(text) || /^([-*_]\s*){3,}$/.test(text);
  if (!plausible) return false;
  const hit = await invoke('detect_block_shortcut', { line: text }).catch(() => null);
  // 等待期间用户继续输入则放弃本次转换
  if (!hit || p.textContent !== text || !p.isConnected) return false;
  const rest = text.slice(hit.prefixLen);

  let replacement = null;
  if (hit.type === 'heading') {
    replacement = styled(`h${hit.level}`, rest, `whitespace-pre-wrap ${headingClasses[hit.level]}`);
  } else if (hit.type === 'quote') {
    replacement = document.createElement('blockquote');
    replacement.className = QUOTE_CLASS;
    replacement.append(styled('p', rest, P_CLASS));
  } else if (hit.type === 'numberedListItem') {
    replacement = styled('ol', '', 'my-2 pl-6 list-decimal');
    replacement.append(styled('li', rest, 'my-0.5'));
  } else if (hit.type === 'bulletedListItem') {
    replacement = styled('ul', '', 'my-2 pl-6 list-disc');
    replacement.append(styled('li', rest, 'my-0.5'));
  } else if (hit.type === 'codeBlock') {
    replacement = createCodePre(hit.language);
  } else if (hit.type === 'separator') {
    // 分割线：替换为新段落，前方插入 hr，光标落在空段落里
    const hr = styled('hr', '', 'my-4');
    const next = styled('p', '', P_CLASS);
    next.innerHTML = '<br>';
    p.replaceWith(hr, next);
    placeCursorAtEnd(next);
    return true;
  } else if (hit.type === 'taskListItem') {
    replacement = styled('ul', '', 'my-2 list-none pl-4');
    const checkbox = document.createElement('input');
    checkbox.type = 'checkbox';
    checkbox.className = 'pointer-events-none mt-[5px] shrink-0';
    checkbox.setAttribute('data-checked', hit.checked ? 'x' : ' ');
    checkbox.checked = !!hit.checked;
    checkbox.setAttribute('contenteditable', 'false');
    const li = styled('li', '', 'my-0.5');
    const row = styled('div', '', 'flex items-start gap-1.5');
    const body = styled('div', '', 'min-w-0 flex-1');
    body.textContent = rest;
    row.append(checkbox, body);
    li.append(row);
    replacement.append(li);
  }

  if (!replacement) return false;
  p.replaceWith(replacement);
  placeCursorAtEnd(replacement);
  return true;
}

// 在一次 input 后尝试即时转换（先行内后块级；判定在 Rust 端异步完成）
export function applyMarkdownShortcuts() {
  transformInlineShortcut().then((done) => {
    if (!done) transformBlockShortcut();
  });
}

// ---------- 光标操作 ----------

// ---------- 斜杠命令（/ 召唤语法菜单） ----------

// 菜单项：id 对应 applySlashCommand 的 DOM 构建；icon 为内联 SVG；keywords 供过滤
//（拼音/英文别名）。标题合并为一项（菜单内 H1-H6 徽章选级别）。
// 菜单展示与构建为视图层职责；块结构的 Markdown 序列化在提交时由 Rust 统一完成。
export const SLASH_ICON = {
  heading:
    '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><path d="M3 3.5v9M9 3.5v9M3 8h6M12.8 6v6.5M11 6l1.8-1.5"/></svg>',
  paragraph:
    '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><path d="M3 4.5h10M3 8h10M3 11.5h7"/></svg>',
  bulletedListItem:
    '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><circle cx="3.2" cy="4.5" r="1" fill="currentColor" stroke="none"/><circle cx="3.2" cy="8" r="1" fill="currentColor" stroke="none"/><circle cx="3.2" cy="11.5" r="1" fill="currentColor" stroke="none"/><path d="M6.5 4.5h7M6.5 8h7M6.5 11.5h7"/></svg>',
  numberedListItem:
    '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><text x="1.2" y="5.6" font-size="5.5" fill="currentColor" stroke="none">1</text><text x="1.2" y="9.8" font-size="5.5" fill="currentColor" stroke="none">2</text><text x="1.2" y="14" font-size="5.5" fill="currentColor" stroke="none">3</text><path d="M6.5 4.5h7M6.5 8.7h7M6.5 12.8h7"/></svg>',
  taskListItem:
    '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><circle cx="4.6" cy="5" r="2.6"/><path d="M3.3 5l1.1 1.1L6.5 4"/><path d="M9 5h4.5M3 11.5h10M3 14h7"/></svg>',
  quote:
    '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><path d="M3 3v10" stroke-width="2.2"/><path d="M6.5 4.5h7M6.5 8h7M6.5 11.5h5"/></svg>',
  codeBlock:
    '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M4.5 4.5L1 8l3.5 3.5M11.5 4.5L15 8l-3.5 3.5M9.5 2.5l-3 11"/></svg>',
  table:
    '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3"><rect x="2.5" y="3" width="11" height="10" rx="1"/><path d="M2.5 6.3h11M2.5 9.6h11M8 3v10"/></svg>',
  image:
    '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round"><rect x="2.5" y="3" width="11" height="10" rx="1"/><circle cx="5.6" cy="6.1" r="1.1"/><path d="M3.5 12l3-3.4 2.4 2.4 2.1-2.4 1.5 1.7"/></svg>',
  mathBlock:
    '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M11.5 3H6L10.5 8 6 13h5.5"/></svg>',
  mermaidBlock:
    '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3"><rect x="2" y="3" width="4.5" height="3" rx="0.8"/><rect x="9.5" y="10" width="4.5" height="3" rx="0.8"/><path d="M6.5 4.5h3.2a2 2 0 012 2V10"/></svg>',
  callout:
    '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round"><path d="M8 2.6L14 13H2z"/><path d="M8 6.4v3.4" stroke-linecap="round"/><circle cx="8" cy="11.3" r="0.6" fill="currentColor" stroke="none"/></svg>',
  sectionBlock:
    '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3"><rect x="2" y="4" width="5" height="8" rx="0.8"/><path d="M8.5 5h5M8.5 8h5M8.5 11h3.5" stroke-linecap="round"/></svg>',
  separator:
    '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><path d="M2.5 8h11" stroke-dasharray="2.5 2"/></svg>',
  link: '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M6.7 8.7a3.3 3.3 0 005 .3l2-2a3.33 3.33 0 00-4.7-4.7l-1.2 1.1"/><path d="M9.3 7.3a3.3 3.3 0 00-5-.3l-2 2a3.33 3.33 0 004.7 4.7l1.2-1.1"/></svg>',
  fontColor:
    '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M4.5 12L8 3.5 11.5 12M5.9 9h4.2"/><path d="M3 14.5h10" stroke-width="2.2"/></svg>',
  // 行内代码（<>）与普通公式（fx）的文本菜单徽章图标
  inlineCode:
    '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M5.5 3.5L2 8l3.5 4.5M10.5 3.5L14 8l-3.5 4.5"/></svg>',
  mathDisplay:
    '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M12.1873 4.1404C11.2229 3.41705 9.84236 4.0694 9.78883 5.2738L9.71211 6.99991H12C12.5523 6.99991 13 7.44762 13 7.99991C13 8.55219 12.5523 8.99991 12 8.99991H9.62322L9.22988 17.85C9.0996 20.7814 5.63681 22.2609 3.42857 20.3287L3.34151 20.2525C2.92587 19.8888 2.88375 19.257 3.24743 18.8414C3.61112 18.4258 4.24288 18.3836 4.65852 18.7473L4.74558 18.8235C5.69197 19.6516 7.17602 19.0175 7.23186 17.7612L7.62125 8.99991H6C5.44772 8.99991 5 8.55219 5 7.99991C5 7.44762 5.44772 6.99991 6 6.99991H7.71014L7.7908 5.185C7.9157 2.37474 11.1369 0.852583 13.3873 2.5404L13.6 2.6999C14.0418 3.03127 14.1314 3.65807 13.8 4.0999C13.4686 4.54173 12.8418 4.63127 12.4 4.2999L12.1873 4.1404Z"/><path d="M13.082 13.0461C13.3348 12.907 13.6525 13.0102 13.7754 13.2713L14.5879 14.9978L11.2928 18.2928C10.9023 18.6834 10.9023 19.3165 11.2928 19.707C11.6834 20.0976 12.3165 20.0976 12.707 19.707L15.493 16.9211L16.2729 18.5785C16.9676 20.0548 18.8673 20.4807 20.1259 19.4424L20.6363 19.0213C21.0623 18.6698 21.1228 18.0396 20.7713 17.6136C20.4198 17.1875 19.7896 17.1271 19.3636 17.4786L18.8531 17.8997C18.6014 18.1073 18.2215 18.0221 18.0825 17.7269L16.996 15.4181L19.707 12.707C20.0976 12.3165 20.0976 11.6834 19.707 11.2928C19.3165 10.9023 18.6834 10.9023 18.2928 11.2928L16.0909 13.4947L15.585 12.4197C14.9708 11.1143 13.3822 10.5984 12.1182 11.2936L11.518 11.6237C11.0341 11.8899 10.8576 12.4979 11.1237 12.9819C11.3899 13.4658 11.998 13.6423 12.4819 13.3761L13.082 13.0461Z"/></svg>',
};

export const SLASH_ITEMS = [
  { id: 'text', label: '文本', icon: SLASH_ICON.paragraph, iconColor: '#3e69d7', keywords: 'h1 h2 h3 h4 h5 h6 heading biaoti bt list liebiao lb task todo renwu rw code daima wenben wb bold jiacu italic xieti underline xiahuaxian strikethrough shanchuxian highlight gaoliang link lianjie lj quote yinyong yy math gongshi gs' },
  { id: 'quote', label: '引用', icon: SLASH_ICON.quote, iconColor: '#f59102', keywords: 'quote yinyong yy' },
  { id: 'codeBlock', label: '代码块', icon: SLASH_ICON.codeBlock, iconColor: '#03b736', keywords: 'code daima dm' },
  { id: 'table', label: '表格', icon: SLASH_ICON.table, iconColor: '#2f6dbb', keywords: 'table biaoge bg' },
  { id: 'image', label: '图片', icon: SLASH_ICON.image, iconColor: '#d35d2e', keywords: 'img image tupian tp' },
  { id: 'fontColor', label: '字体', icon: SLASH_ICON.fontColor, iconColor: '#d35d2e', keywords: 'color font yanse ys ziti rgb size zhao ziti' },
  { id: 'mathBlock', label: '数学公式', icon: SLASH_ICON.mathBlock, iconColor: '#8250df', keywords: 'math latex gongshi gs' },
  { id: 'callout', label: '警告框（Callout）', icon: SLASH_ICON.callout, iconColor: '#c9a227', keywords: 'callout note warning gaoliang gl jinggao jgk' },
  { id: 'sectionBlock', label: '图文排版（section）', icon: SLASH_ICON.sectionBlock, iconColor: '#2f9dbb', keywords: 'section tuwen tw paiban pb' },
  { id: 'separator', label: '分割线', icon: SLASH_ICON.separator, iconColor: '#8a8a8a', keywords: 'hr fenge fg' },
];

// 语言补全菜单的徽章图标（按语言缩写 + 品牌色，mermaid/math 为特殊渲染围栏）
export const LANG_BADGES = {
  mermaid: { text: 'Mm', color: '#3e69d7' },
  plantuml: { text: 'Pu', color: '#2f9dbb' },
  math: { text: '∑', color: '#8250df' },
  rust: { text: 'Rs', color: '#b7410e' },
  javascript: { text: 'JS', color: '#c9a227' },
  jsx: { text: 'JSX', color: '#2f9dbb' },
  typescript: { text: 'TS', color: '#2f6dbb' },
  tsx: { text: 'TSX', color: '#2f6dbb' },
  json: { text: '{}', color: '#8a8a8a' },
  markdown: { text: 'M↓', color: '#3e69d7' },
  bash: { text: '>_', color: '#4e9a3d' },
  c: { text: 'C', color: '#2f6dbb' },
  cpp: { text: 'C++', color: '#2f6dbb' },
  csharp: { text: 'C#', color: '#8250df' },
  css: { text: 'CSS', color: '#2f9dbb' },
  go: { text: 'Go', color: '#2f9dbb' },
  html: { text: '<>', color: '#d35d2e' },
  java: { text: 'Jv', color: '#b7410e' },
  php: { text: 'php', color: '#6c7fb7' },
  python: { text: 'Py', color: '#3a6ea5' },
  ruby: { text: 'Rb', color: '#c0392b' },
  yaml: { text: 'Y', color: '#8a8a8a' },
  toml: { text: 'T', color: '#8a8a8a' },
};

// 列表行的类型徽章（无序/有序/任务，菜单内 ←/→ 或点选）
export const SLASH_LIST_TYPES = [
  { id: 'bulletedListItem', icon: SLASH_ICON.bulletedListItem, label: '无序列表' },
  { id: 'numberedListItem', icon: SLASH_ICON.numberedListItem, label: '有序列表' },
  { id: 'taskListItem', icon: SLASH_ICON.taskListItem, label: '任务列表' },
];

// 警告框类型徽章（GitHub 风格 [!TYPE]，与 Rust CalloutVariant 一致；
// 警告框菜单项内 ←/→ 切换、Enter 应用当前类型、点选直用）
export const CALLOUT_TYPES = [
  { id: 'NOTE', label: '注意（Note）', color: '#0969da' },
  { id: 'TIP', label: '提示（Tip）', color: '#1a7f37' },
  { id: 'IMPORTANT', label: '重要（Important）', color: '#8250df' },
  { id: 'WARNING', label: '警告（Warning）', color: '#9a6700' },
  { id: 'CAUTION', label: '谨慎（Caution）', color: '#cf222e' },
];

// 字体颜色面板的常见色板（12 色轮：红/橙红/橙/橙黄/黄/黄绿/绿/蓝绿/蓝/蓝紫/紫/紫红；
// color 为 htmlStyle.color 的 JSON 形状，直接落 data-html-style）
export const FONT_COLORS = [
  { label: '红', css: '#ff0000', color: { rgba: { red: 255, green: 0, blue: 0, alpha: 1 } } },
  { label: '橙红', css: '#ff4500', color: { rgba: { red: 255, green: 69, blue: 0, alpha: 1 } } },
  { label: '橙', css: '#ff8c00', color: { rgba: { red: 255, green: 140, blue: 0, alpha: 1 } } },
  { label: '橙黄', css: '#ffc000', color: { rgba: { red: 255, green: 192, blue: 0, alpha: 1 } } },
  { label: '黄', css: '#ffd800', color: { rgba: { red: 255, green: 216, blue: 0, alpha: 1 } } },
  { label: '黄绿', css: '#9acd32', color: { rgba: { red: 154, green: 205, blue: 50, alpha: 1 } } },
  { label: '绿', css: '#00b050', color: { rgba: { red: 0, green: 176, blue: 80, alpha: 1 } } },
  { label: '蓝绿', css: '#00b0a0', color: { rgba: { red: 0, green: 176, blue: 160, alpha: 1 } } },
  { label: '蓝', css: '#0070c0', color: { rgba: { red: 0, green: 112, blue: 192, alpha: 1 } } },
  { label: '蓝紫', css: '#5b5bd6', color: { rgba: { red: 91, green: 91, blue: 214, alpha: 1 } } },
  { label: '紫', css: '#7030a0', color: { rgba: { red: 112, green: 48, blue: 160, alpha: 1 } } },
  { label: '紫红', css: '#c00070', color: { rgba: { red: 192, green: 0, blue: 112, alpha: 1 } } },
];

// 字号档（px；fontSize 为 htmlStyle.fontSize 的 JSON 形状）
export const FONT_SIZES = [12, 13, 14, 15, 16, 18, 20, 24, 28, 32].map((px) => ({
  label: String(px),
  fontSize: { px },
}));

// 「文本」行的徽章分组（四行展示：标题级别 / 行内格式 / 列表·引用·链接 / 行内代码·代码块·公式；
// ←/→ 移列、↑/↓ 移行、点选直用）。color 为徽章多彩配色（文字与图标着色，品牌色不走主题变量）。
export const SLASH_TEXT_GROUPS = [
  // 标题级别（蓝色系渐变）
  [
    { id: 'h1', text: 'H1', label: '一级标题', color: '#2f56c7' },
    { id: 'h2', text: 'H2', label: '二级标题', color: '#3e69d7' },
    { id: 'h3', text: 'H3', label: '三级标题', color: '#4f7ce0' },
    { id: 'h4', text: 'H4', label: '四级标题', color: '#6090e9' },
    { id: 'h5', text: 'H5', label: '五级标题', color: '#70a3f1' },
    { id: 'h6', text: 'H6', label: '六级标题', color: '#81b7f9' },
  ],
  // 行内格式
  [
    { id: 'bold', text: 'B', label: '加粗', color: '#d35d2e' },
    { id: 'italic', text: 'I', label: '斜体', color: '#8250df' },
    { id: 'underline', text: 'U', label: '下划线', color: '#3e69d7' },
    { id: 'strikethrough', text: 'S', label: '删除线', color: '#c0392b' },
    { id: 'highlight', text: '==', label: '高亮', color: '#c9a227' },
  ],
  // 列表·引用·链接（图标徽章）
  [
    { id: 'bulletedListItem', icon: SLASH_ICON.bulletedListItem, label: '无序列表', color: '#2f9dbb' },
    { id: 'numberedListItem', icon: SLASH_ICON.numberedListItem, label: '有序列表', color: '#03b736' },
    { id: 'taskListItem', icon: SLASH_ICON.taskListItem, label: '任务列表', color: '#f59102' },
    { id: 'quote', icon: SLASH_ICON.quote, label: '引用', color: '#f59102' },
    { id: 'link', icon: SLASH_ICON.link, label: '超链接', color: '#2f6dbb' },
  ],
  // 行内代码（<_>）·代码块（<>）·行内公式（∑）·普通公式（$$）（图标徽章，与其他行同尺寸对齐）
  [
    { id: 'inlineCode', icon: SLASH_ICON.inlineCode, label: '行内代码', color: '#03b736' },
    { id: 'codeBlock', icon: SLASH_ICON.codeBlock, label: '代码块', color: '#03b736' },
    { id: 'inlineMath', icon: SLASH_ICON.mathBlock, label: '行内公式', color: '#8250df' },
    { id: 'mathBlock', icon: SLASH_ICON.mathDisplay, label: '普通公式', color: '#8250df' },
  ],
];

// 应用斜杠命令：清空触发文本（/query），按所选语法构建编辑态 DOM 并放置光标；
// 原子类结构（表格/公式/Mermaid/callout/section/图片）的原文模板由 Rust
// block_template 生成（表格经 TableData 序列化，无平行实现），提交时经 Rust 解析。
// opts.rows/cols 仅表格使用（数据行数/列数）。
export async function applySlashCommand(el, id, opts) {
  el.innerHTML = '';
  const rawPre = (text, caretOffset) => {
    const pre = document.createElement('pre');
    pre.className = PRE_CLASS;
    pre.setAttribute('data-raw', '');
    pre.textContent = text;
    el.append(pre);
    placeCaretAtTextOffset(pre, caretOffset);
    return true;
  };
  // 原子类模板统一走 Rust block_template（Markdown 生成规则在 Rust 维护）；
  // 链接插入原始 Markdown 文本编辑（渲染态 <a> 不便就地编辑），与图片同一交互；
  // opts.variant 仅警告框使用（NOTE/TIP/IMPORTANT/WARNING/CAUTION）
  if (['table', 'mathBlock', 'inlineMath', 'mermaidBlock', 'callout', 'sectionBlock', 'image', 'link', 'footnoteDef', 'linkRef', 'toc'].includes(id)) {
    const tpl = await invoke('block_template', {
      kind: id,
      rows: opts?.rows,
      cols: opts?.cols,
      variant: opts?.variant,
    }).catch(() => null);
    if (tpl) return rawPre(tpl.markdown, tpl.caretOffset);
    return false;
  }
  if (/^h[1-6]$/.test(id)) {
    const level = Number(id[1]);
    const h = styled(`h${level}`, '', `whitespace-pre-wrap ${headingClasses[level]}`);
    el.append(h);
    placeCursorAtEnd(h);
    return true;
  }
  switch (id) {
    case 'paragraph': {
      const p = styled('p', '', P_CLASS);
      p.innerHTML = '<br>';
      el.append(p);
      placeCursorAtEnd(p);
      return true;
    }
    case 'bulletedListItem':
    case 'numberedListItem': {
      const ordered = id === 'numberedListItem';
      const list = styled(ordered ? 'ol' : 'ul', '', ordered ? 'my-2 pl-6 list-decimal' : 'my-2 pl-6 list-disc');
      list.append(styled('li', '', 'my-0.5'));
      el.append(list);
      placeCursorAtEnd(list.querySelector('li'));
      return true;
    }
    case 'taskListItem': {
      const ul = styled('ul', '', 'my-2 list-none pl-4');
      const checkbox = document.createElement('input');
      checkbox.type = 'checkbox';
      checkbox.className = 'pointer-events-none mt-[5px] shrink-0';
      checkbox.setAttribute('data-checked', ' ');
      checkbox.setAttribute('contenteditable', 'false');
      const li = styled('li', '', 'my-0.5');
      const row = styled('div', '', 'flex items-start gap-1.5');
      const body = styled('div', '', 'min-w-0 flex-1');
      row.append(checkbox, body);
      li.append(row);
      ul.append(li);
      el.append(ul);
      placeCursorAtEnd(body);
      return true;
    }
    case 'quote': {
      const quote = document.createElement('blockquote');
      quote.className = QUOTE_CLASS;
      quote.append(styled('p', '', P_CLASS));
      el.append(quote);
      placeCursorAtEnd(quote.querySelector('p'));
      return true;
    }
    case 'codeBlock': {
      // 插入围栏起始行，光标在 ``` 之后：先编辑语言（语言补全菜单立即弹出，
      // 选定后 Enter 经 convertFenceToCodeBlock 转为代码块，与手动输入同一流程）
      const p = styled('p', '', P_CLASS);
      p.textContent = '```';
      el.append(p);
      placeCaretAtTextOffset(p, 3);
      return true;
    }
    case 'separator': {
      const hr = styled('hr', '', 'my-4');
      const next = styled('p', '', P_CLASS);
      next.innerHTML = '<br>';
      el.append(hr, next);
      placeCursorAtEnd(next);
      return true;
    }
    case 'inlineCode': {
      // 行内代码：插入空 code 元素并补 <br> 占位，光标放在占位 br 之前——
      // 若放在 br 之后，浏览器会把输入插到元素外（样式丢失、渲染成普通文本）；
      // 尾部占位 br 产生的换行 fragment 在提交时由 trimEdgeNewlines 修剪。
      const p = styled('p', '', P_CLASS);
      const code = styled('code', '', INLINE_CODE_CLASS);
      p.append(code);
      el.append(p);
      placeCursorAtStart(code);
      return true;
    }
    // 行内格式（加粗/斜体/下划线/删除线/高亮）：插入空样式元素，光标在其中输入
    case 'bold':
    case 'italic':
    case 'underline':
    case 'strikethrough':
    case 'highlight': {
      const tag = { bold: 'strong', italic: 'em', underline: 'u', strikethrough: 's', highlight: 'mark' }[id];
      const cls = id === 'bold' ? 'font-semibold' : '';
      const p = styled('p', '', P_CLASS);
      const inner = styled(tag, '', cls);
      p.append(inner);
      el.append(p);
      placeCursorAtStart(inner);
      return true;
    }
    default:
      return false;
  }
}

// ---------- 列表 Tab 缩进 / Shift+Tab 取消缩进 ----------

// 光标所在列表项按 Tab：缩进为前一项的子项（前一项没有同类型子列表则新建）；
// Shift+Tab：取消缩进，提升为父项的后续兄弟（后续兄弟一并带走作其子项）。
// 首项无缩进目标、顶层项无可提升时返回 false（保持原位）。移动节点不丢光标。
export function handleListTab(el, outdent) {
  const sel = window.getSelection();
  if (!sel.rangeCount) return false;
  const caret = sel.getRangeAt(0);
  if (!el.contains(caret.startContainer)) return false;
  const anchor = caret.startContainer.nodeType === Node.TEXT_NODE ? caret.startContainer.parentElement : caret.startContainer;
  const li = anchor?.closest('li');
  if (!li || !el.contains(li)) return false;
  return outdent ? outdentListItem(li) : indentListItem(li);
}

// 列表项的内容宿主：任务项为正文 div，普通项为 li 自身
function liContentHost(li) {
  return taskItemBody(li) || li;
}

// 宿主内最后一个同类型子列表（没有则新建并挂在宿主末尾）
function ensureNestedList(host, tag, className) {
  let nested = null;
  for (const child of host.children) {
    if (child.tagName === tag) nested = child;
  }
  if (!nested) {
    nested = document.createElement(tag.toLowerCase());
    nested.className = className;
    host.append(nested);
  }
  return nested;
}

// 光标在容器内的纯文本偏移（<br> 计 1，与 placeCaretAtTextOffset 对齐）；
// 列表项移动（缩进/提升）前后据此恢复光标，避免浏览器把光标吸附到相邻项末尾
function caretOffsetIn(el) {
  const sel = window.getSelection();
  if (!sel.rangeCount || !sel.isCollapsed) return null;
  const caret = sel.getRangeAt(0);
  if (!el.contains(caret.startContainer)) return null;
  const range = document.createRange();
  range.selectNodeContents(el);
  range.setEnd(caret.startContainer, caret.startOffset);
  const div = document.createElement('div');
  div.append(range.cloneContents());
  return div.textContent.length + div.querySelectorAll('br').length;
}

function indentListItem(li) {
  const list = li.parentElement;
  const prev = li.previousElementSibling;
  if (!prev || prev.tagName !== 'LI') return false;
  const offset = caretOffsetIn(li);
  const nested = ensureNestedList(liContentHost(prev), list.tagName, list.className);
  nested.append(li);
  if (offset != null) placeCaretAtTextOffset(li, offset);
  return true;
}

function outdentListItem(li) {
  const list = li.parentElement;
  const parentLi = list.parentElement?.closest('li');
  if (!parentLi) return false; // 已是顶层列表项，无可提升
  const offset = caretOffsetIn(li);
  // 后续兄弟一并带走，作为当前项的子列表（标准 outdent 行为）
  const followers = [];
  let n = li.nextElementSibling;
  while (n) {
    followers.push(n);
    n = n.nextElementSibling;
  }
  if (followers.length) {
    const nested = ensureNestedList(liContentHost(li), list.tagName, list.className);
    followers.forEach((f) => nested.append(f));
  }
  parentLi.after(li);
  if (!list.children.length) list.remove();
  if (offset != null) placeCaretAtTextOffset(li, offset);
  return true;
}

// ---------- 列表内 Enter（Typora 式续项/退出列表） ----------

// 任务项的正文容器（勾选框旁的 div）；非任务项返回 null
function taskItemBody(li) {
  const row = li.querySelector(':scope > div');
  const input = row?.firstElementChild;
  if (row && input?.tagName === 'INPUT' && input.getAttribute('type') === 'checkbox' && input.closest('li') === li) {
    return input.nextElementSibling || null;
  }
  return null;
}

// 列表项标题部分是否为空（忽略嵌套子列表与勾选框）
function liTitleBlank(li, host) {
  const clone = host.cloneNode(true);
  clone.querySelectorAll('ul, ol, input').forEach((n) => n.remove());
  return clone.textContent.trim() === '';
}

// 编辑容器内、光标在列表项中按 Enter：
// 非空项 → 在光标处拆出同类型新列表项（任务项补勾选框结构），嵌套子列表留在原项；
// 空项（无嵌套子列表）→ 退出列表：空项替换为空段落，列表在该处断开，光标进入段落。
// 返回是否已处理；未处理时调用方走既有拆分逻辑。提交序列化仍由 Rust 统一完成。
export function handleListEnter(el) {
  const sel = window.getSelection();
  if (!sel.rangeCount || !sel.isCollapsed) return false;
  const caret = sel.getRangeAt(0);
  if (!el.contains(caret.startContainer)) return false;
  const anchor = caret.startContainer.nodeType === Node.TEXT_NODE ? caret.startContainer.parentElement : caret.startContainer;
  const li = anchor?.closest('li');
  if (!li || !el.contains(li)) return false;

  const body = taskItemBody(li);
  const host = body || li;
  // 含嵌套子列表的空标题项不退出（避免子列表随空项丢失），按普通拆分处理
  if (liTitleBlank(li, host) && !host.querySelector('ul, ol')) {
    // 空项回车：嵌套则提升一层（回到父项的后续兄弟）；已是顶层则退出列表转为段落
    if (!outdentListItem(li)) exitListAtEmptyItem(li);
  } else {
    splitListItemAtCaret(li, host, caret, !!body);
  }
  return true;
}

// 在光标处拆分列表项：光标后的行内内容移入新项，嵌套子列表留在原项
function splitListItemAtCaret(li, host, caret, isTask) {
  // 抽取范围终点：行内内容末尾（首个嵌套子列表之前）
  let endOffset = host.childNodes.length;
  for (let i = 0; i < host.childNodes.length; i++) {
    const n = host.childNodes[i];
    if (n.nodeType === Node.ELEMENT_NODE && (n.tagName === 'UL' || n.tagName === 'OL')) {
      endOffset = i;
      break;
    }
  }
  const range = document.createRange();
  range.setStart(caret.startContainer, caret.startOffset);
  range.setEnd(host, endOffset);
  const frag = range.extractContents();

  const newLi = document.createElement('li');
  newLi.className = 'my-0.5';
  let cursorHost = newLi;
  if (isTask) {
    // 新任务项：补上勾选框与正文结构（与 blockToHtml 的任务项 DOM 一致）
    newLi.innerHTML = `<div class="flex items-start gap-1.5"><input type="checkbox" class="pointer-events-none mt-[5px] shrink-0" data-checked=" " contenteditable="false"><div class="min-w-0 flex-1"></div></div>`;
    cursorHost = newLi.querySelector('div.min-w-0');
  }
  cursorHost.append(frag);
  if (!cursorHost.textContent && !cursorHost.querySelector('img, input, br')) cursorHost.innerHTML = '<br>';
  li.after(newLi);
  placeCursorAtStart(cursorHost);
}

// 空列表项退出列表：空项替换为空段落；列表在该处断开为前后两段（若有）
function exitListAtEmptyItem(li) {
  const ul = li.parentElement;
  const p = document.createElement('p');
  p.className = `blk-paragraph ${P_CLASS}`;
  p.innerHTML = '<br>';
  const items = [...ul.children].filter((n) => n.tagName === 'LI');
  const index = items.indexOf(li);
  if (items.length === 1) {
    ul.replaceWith(p);
  } else if (index === 0) {
    li.remove();
    ul.before(p);
  } else if (index === items.length - 1) {
    li.remove();
    ul.after(p);
  } else {
    const afterUl = ul.cloneNode(false);
    let n = li.nextElementSibling;
    while (n) {
      const next = n.nextElementSibling;
      afterUl.append(n);
      n = next;
    }
    li.remove();
    ul.after(p, afterUl);
  }
  placeCursorAtStart(p);
}

// 光标紧贴任务勾选框之后（任务行内、勾选框前无其他内容）时删除该勾选框：
// 任务项降为普通列表项，行包装展开、正文节点直接移入 li（与普通列表项 DOM 结构一致，
// 提交提取才不丢嵌套），光标位置不变。返回是否已处理——浏览器原生退格无法越过
// contenteditable=false 的勾选框，不处理则表现为「勾选框后按删除无反应」。
export function deleteTaskCheckboxBeforeCaret(el) {
  const sel = window.getSelection();
  if (!sel.rangeCount || !sel.isCollapsed) return false;
  const caret = sel.getRangeAt(0);
  if (!el.contains(caret.startContainer)) return false;
  let node = caret.startContainer.nodeType === Node.TEXT_NODE ? caret.startContainer.parentElement : caret.startContainer;
  for (let cur = node; cur && cur !== el; cur = cur.parentElement) {
    const first = cur.firstElementChild;
    if (!first || first.tagName !== 'INPUT' || first.getAttribute('type') !== 'checkbox') continue;
    // cur 是任务行容器：勾选框之后、光标之前不能有任何文本/有意义元素
    const before = document.createRange();
    before.selectNodeContents(cur);
    before.setEnd(caret.startContainer, caret.startOffset);
    const div = document.createElement('div');
    div.append(before.cloneContents());
    div.querySelectorAll('input').forEach((n) => n.remove());
    if (div.textContent !== '' || div.querySelector('img, hr, br, table, pre')) return false;
    // 删除勾选框并展开行包装（任务行 div → li 直挂正文节点）
    const li = cur.closest('li');
    const body = first.nextElementSibling;
    first.remove();
    if (li && body && cur !== li) {
      while (body.firstChild) li.insertBefore(body.firstChild, cur);
      cur.remove();
    }
    return true;
  }
  return false;
}

// 光标是否处于编辑容器内容的最前面（前方无文本、无有意义元素；任务勾选框不算内容）
export function isCaretAtStart(el) {
  const sel = window.getSelection();
  if (!sel.rangeCount || !sel.isCollapsed) return false;
  const caret = sel.getRangeAt(0);
  if (!el.contains(caret.startContainer)) return false;
  const before = document.createRange();
  before.selectNodeContents(el);
  before.setEnd(caret.startContainer, caret.startOffset);
  const div = document.createElement('div');
  div.append(before.cloneContents());
  div.querySelectorAll('input').forEach((n) => n.remove());
  return div.textContent === '' && !div.querySelector('img, hr, br, table, pre');
}

// 按纯文本偏移放置光标（<br> 记 1 个单位，与模型文本中的 \n 对齐；越界落到末尾）
export function placeCaretAtTextOffset(el, offset) {
  const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT);
  const sel = window.getSelection();
  const range = document.createRange();
  let remaining = Math.max(0, offset);
  let node;
  while ((node = walker.nextNode())) {
    if (node.nodeType === Node.ELEMENT_NODE) {
      if (node.tagName === 'BR') {
        if (remaining === 0) {
          range.setStartBefore(node);
          range.collapse(true);
          sel.removeAllRanges();
          sel.addRange(range);
          return;
        }
        remaining -= 1;
      }
      continue;
    }
    const len = node.textContent.length;
    if (remaining <= len) {
      range.setStart(node, remaining);
      range.collapse(true);
      sel.removeAllRanges();
      sel.addRange(range);
      return;
    }
    remaining -= len;
  }
  placeCursorAtEnd(el);
}

// 行内树 → 单段落 DTO（借 Rust serialize_markdown 生成行内 Markdown 文本，如块首退格合并）
export function paragraphDtoFromTree(title) {
  return makeBlock({ type: 'paragraph' }, { title });
}

// 块首退格降级：标题/引用/列表项等样式块的编辑态 DOM 替换为段落（行内内容保留），
// 嵌套子块（子列表、引用的后续段）提升为顶层块跟在段落后，光标置开头。
// 仅改 DOM；Markdown 源在提交时由 Rust 序列化统一更新。
export function demoteEditableToParagraph(el, dto) {
  const p = document.createElement('p');
  p.className = `blk-paragraph ${P_CLASS}`;
  p.innerHTML = inlineToHtml(dto.title) || '<br>';
  el.replaceChildren(p);
  for (const child of dto.children || []) {
    el.insertAdjacentHTML('beforeend', blockToHtml(child));
  }
  placeCursorAtStart(el);
}

export function placeCursorAtEnd(el) {
  // 深入到最后一个叶子元素，光标落在其内容末尾
  let target = el;
  while (target.lastElementChild && !['BR', 'IMG', 'INPUT', 'HR'].includes(target.lastElementChild.tagName)) {
    target = target.lastElementChild;
  }
  // 空元素需要一个 <br> 才能显示光标
  if (target.childNodes.length === 0) target.innerHTML = '<br>';
  const sel = window.getSelection();
  const range = document.createRange();
  range.selectNodeContents(target);
  range.collapse(false);
  sel.removeAllRanges();
  sel.addRange(range);
}

export function placeCursorAtStart(el) {
  // 深入到第一个叶子元素，光标落在其内容开头（与 placeCursorAtEnd 对称）
  let target = el;
  while (target.firstElementChild && !['BR', 'IMG', 'INPUT', 'HR'].includes(target.firstElementChild.tagName)) {
    target = target.firstElementChild;
  }
  // 空元素需要一个 <br> 才能显示光标
  if (target.childNodes.length === 0) target.innerHTML = '<br>';
  const sel = window.getSelection();
  const range = document.createRange();
  range.selectNodeContents(target);
  range.collapse(true);
  sel.removeAllRanges();
  sel.addRange(range);
}

function placeCursorAfter(el) {
  const sel = window.getSelection();
  const range = document.createRange();
  range.setStartAfter(el);
  range.collapse(true);
  sel.removeAllRanges();
  sel.addRange(range);
}

export function insertTextAtCursor(text) {
  const sel = window.getSelection();
  if (!sel.rangeCount) return;
  const range = sel.getRangeAt(0);
  range.deleteContents();
  const node = document.createTextNode(text);
  range.insertNode(node);
  range.setStartAfter(node);
  range.collapse(true);
  sel.removeAllRanges();
  sel.addRange(range);
}

export function insertLineBreakAtCursor() {
  // execCommand 处理 contenteditable 内换行的边界情况（行尾需要额外 <br> 等）更稳妥
  if (!document.execCommand('insertLineBreak')) insertTextAtCursor('\n');
}
