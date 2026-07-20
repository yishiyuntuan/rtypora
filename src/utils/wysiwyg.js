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

// 连续 numberedListItem 兄弟块的序号表（id -> 1 基序号），用于有序列表渲染
export function numberedOrdinals(blocks) {
  const map = new Map();
  let n = 0;
  for (const b of blocks || []) {
    if (b.type === 'numberedListItem') {
      n += 1;
      map.set(b.id, n);
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
  if (s.script === 'superscript') html = `<sup>${html}</sup>`;
  if (s.script === 'subscript') html = `<sub>${html}</sub>`;
  if (f.link) {
    html = `<a class="${LINK_CLASS}" href="${escapeHtml(linkHref(f.link))}">${html}</a>`;
  }
  return html;
}

// 块 → 可编辑 HTML。rawSource 为该块在全文中的原始 Markdown 切片（原子块按原文编辑）。
// 富编辑块带与渲染态一致的 blk-* 语义类（按块自定义样式在编辑态同样生效）。
export function blockToHtml(block, rawSource) {
  switch (block.type) {
    case 'paragraph':
      return `<p class="blk-paragraph ${P_CLASS}">${inlineToHtml(block.title)}</p>`;
    case 'heading':
      return `<h${block.level} class="blk-heading whitespace-pre-wrap ${headingClasses[block.level] || 'my-2'}">${inlineToHtml(block.title)}</h${block.level}>`;
    case 'bulletedListItem':
      return `<ul class="blk-bulleted-list-item my-2 pl-6 list-disc"><li class="my-0.5">${listItemBodyHtml(block)}</li></ul>`;
    case 'numberedListItem':
      return `<ol class="blk-numbered-list-item my-2 pl-6 list-decimal"><li class="my-0.5">${listItemBodyHtml(block)}</li></ol>`;
    case 'taskListItem': {
      // 勾选框不可编辑、不可点击（勾选在渲染态完成）
      const checkbox = `<input type="checkbox" class="pointer-events-none mt-[5px] shrink-0" data-checked="${block.checked ? 'x' : ' '}" ${block.checked ? 'checked' : ''} contenteditable="false">`;
      const body = `<div class="flex items-start gap-1.5">${checkbox}<div class="min-w-0 flex-1">${listItemBodyHtml(block)}</div></div>`;
      return `<ul class="blk-task-list-item my-2 list-none pl-4"><li class="my-0.5">${body}</li></ul>`;
    }
    case 'quote': {
      const title = plainText(block.title)
        ? `<p class="${P_CLASS}">${inlineToHtml(block.title)}</p>`
        : '';
      return `<blockquote class="blk-quote ${QUOTE_CLASS}">${title}${(block.children || []).map((c) => blockToHtml(c)).join('')}</blockquote>`;
    }
    case 'codeBlock':
      return `<pre class="${PRE_CLASS} blk-code-block" data-language="${escapeHtml(block.language || '')}"><code>${escapeHtml(plainText(block.title))}</code></pre>`;
    // 原子/保留类块：编辑原始 Markdown 切片，保证不丢内容
    case 'separator':
    case 'table':
    case 'callout':
    case 'footnoteDefinition':
    case 'mathBlock':
    case 'mermaidBlock':
    case 'htmlBlock':
    case 'comment':
    case 'rawMarkdown':
      return `<pre class="${PRE_CLASS}" data-raw="">${escapeHtml(rawSource ?? block.rawFallback ?? plainText(block.title))}</pre>`;
    default:
      return '';
  }
}

function listItemBodyHtml(block) {
  const title = plainText(block.title) ? inlineToHtml(block.title) : '<br>';
  return title + (block.children || []).map((c) => blockToHtml(c)).join('');
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

function elementToBlocks(el) {
  const tag = el.tagName;
  if (/^H[1-6]$/.test(tag)) {
    return [makeBlock({ type: 'heading', level: Number(tag[1]) }, { title: makeTree(domToInlines(el)) })];
  }
  if (tag === 'P' || tag === 'DIV') {
    return [makeBlock({ type: 'paragraph' }, { title: makeTree(domToInlines(el)) })];
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
    const checkbox = li.querySelector(':scope input[type="checkbox"]');
    const blocks = domChildrenToBlocks(li);
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

// 围栏行（```lang / ~~~lang）按 Enter 时调用：把当前编辑容器转换为代码块编辑
// （pre[data-language]，光标进入块内），返回是否已转换。
// 语言标记由 Rust 命令 detect_block_shortcut 判定。
export async function convertFenceToCodeBlock(container) {
  const text = container.textContent.trim();
  // 仅围栏起始符才调用 Rust 判定
  if (!/^[`~]/.test(text)) return false;
  const hit = await invoke('detect_block_shortcut', { line: text }).catch(() => null);
  if (!hit || hit.type !== 'codeBlock') return false;
  const pre = document.createElement('pre');
  pre.className = PRE_CLASS;
  pre.setAttribute('data-language', hit.language || '');
  pre.append(styled('code', ''));
  container.innerHTML = '';
  container.append(pre);
  placeCursorAtEnd(pre);
  return true;
}

function styled(tag, text, className) {
  const el = document.createElement(tag);
  if (className) el.className = className;
  el.textContent = text;
  return el;
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
  // 可能的行内结尾才调用 Rust 判定
  if (!/[*`)\]]$/.test(before)) return false;
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

  // 可能的块标记前缀才调用 Rust 判定
  if (!/^[#>\-+*0-9~_]/.test(text)) return false;
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
    replacement = document.createElement('pre');
    replacement.className = PRE_CLASS;
    replacement.setAttribute('data-language', hit.language || '');
    replacement.append(styled('code', ''));
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
