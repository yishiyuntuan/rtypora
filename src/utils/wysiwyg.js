// Typora 式就地编辑辅助：Block 模型 → 可编辑 HTML 字符串，contenteditable DOM → Markdown。
// 编辑态 HTML 的结构与样式和 BlockView 渲染保持一致（BlockView 类名调整时需同步此处）：
// 用户在渲染结果上直接编辑，提交时序列化回 Markdown，再交给 Rust 统一重新解析。

const headingClasses = {
  1: 'my-3 text-2xl',
  2: 'my-3 text-xl',
  3: 'my-2 text-lg',
  4: 'my-2 text-base',
  5: 'my-1 text-sm',
  6: 'my-1 text-xs',
};

const P_CLASS = 'my-2 whitespace-pre-wrap leading-relaxed';
const INLINE_CODE_CLASS = 'rounded bg-black/5 px-1 py-0.5 font-mono text-[0.88em] dark:bg-white/10';
const LINK_CLASS = 'text-blue-600 underline decoration-blue-600/40 underline-offset-2 dark:text-blue-400';
const PRE_CLASS = 'my-2 overflow-x-auto rounded bg-black/5 p-3 font-mono text-[13px] leading-relaxed dark:bg-white/10';
const QUOTE_CLASS = 'my-2 border-l-4 border-black/15 pl-3 text-[#555] dark:border-white/20 dark:text-[#aaa]';
const TABLE_CLASS = 'my-2 border-collapse text-[13px]';
const TH_CLASS = 'border border-black/15 bg-black/5 px-2 py-1 font-semibold dark:border-white/20 dark:bg-white/10';
const TD_CLASS = 'border border-black/15 px-2 py-1 dark:border-white/20';

const HTML_ESCAPES = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' };

function escapeHtml(text) {
  return String(text).replace(/[&<>"]/g, (c) => HTML_ESCAPES[c]);
}

export function emptyParagraphHtml() {
  return `<p class="${P_CLASS}"><br></p>`;
}

// ---------- Block 模型 → 可编辑 HTML ----------

export function inlineToHtml(nodes) {
  return (nodes || [])
    .map((node) => {
      switch (node.type) {
        case 'text':
          return escapeHtml(node.text);
        case 'bold':
          return `<strong class="font-semibold">${inlineToHtml(node.children)}</strong>`;
        case 'italic':
          return `<em>${inlineToHtml(node.children)}</em>`;
        case 'strikethrough':
          return `<s>${inlineToHtml(node.children)}</s>`;
        case 'code':
          return `<code class="${INLINE_CODE_CLASS}">${escapeHtml(node.code)}</code>`;
        case 'link':
          return `<a class="${LINK_CLASS}" href="${escapeHtml(node.dest)}" title="${escapeHtml(node.title)}">${inlineToHtml(node.children)}</a>`;
        case 'image':
          return `<img src="${escapeHtml(node.src)}" alt="${escapeHtml(node.alt)}" title="${escapeHtml(node.title)}">`;
        case 'softBreak':
        case 'hardBreak':
          return '<br>';
        default:
          return '';
      }
    })
    .join('');
}

export function blockToHtml(block) {
  switch (block.type) {
    case 'paragraph':
      return `<p class="${P_CLASS}">${inlineToHtml(block.inlines)}</p>`;
    case 'heading':
      return `<h${block.level} class="whitespace-pre-wrap font-semibold ${headingClasses[block.level] || 'my-2 text-base'}">${inlineToHtml(block.inlines)}</h${block.level}>`;
    case 'codeBlock':
      return `<pre class="${PRE_CLASS}" data-language="${escapeHtml(block.language || '')}"><code>${escapeHtml(block.code)}</code></pre>`;
    case 'blockQuote':
      return `<blockquote class="${QUOTE_CLASS}">${block.children.map(blockToHtml).join('')}</blockquote>`;
    case 'list': {
      const tag = block.ordered ? 'ol' : 'ul';
      const isTask = block.items.length > 0 && block.items.every((i) => i.checked !== null && i.checked !== undefined);
      const cls = isTask ? 'my-2 list-none pl-4' : block.ordered ? 'my-2 pl-6 list-decimal' : 'my-2 pl-6 list-disc';
      const items = block.items
        .map((item) => {
          const body = item.children.map(blockToHtml).join('');
          if (item.checked === null || item.checked === undefined) {
            return `<li class="my-0.5">${body}</li>`;
          }
          // 勾选框不可编辑、不可点击（勾选在渲染态完成）
          const checkbox = `<input type="checkbox" class="pointer-events-none mt-[5px] shrink-0" data-checked="${item.checked ? 'x' : ' '}" ${item.checked ? 'checked' : ''} contenteditable="false">`;
          return `<li class="my-0.5"><div class="flex items-start gap-1.5">${checkbox}<div class="min-w-0 flex-1">${body}</div></div></li>`;
        })
        .join('');
      return `<${tag} class="${cls}">${items}</${tag}>`;
    }
    case 'table': {
      const head = `<thead><tr>${block.head.map((c) => `<th class="${TH_CLASS}">${inlineToHtml(c)}</th>`).join('')}</tr></thead>`;
      const body = `<tbody>${block.rows.map((r) => `<tr>${r.map((c) => `<td class="${TD_CLASS}">${inlineToHtml(c)}</td>`).join('')}</tr>`).join('')}</tbody>`;
      return `<table class="${TABLE_CLASS}">${head}${body}</table>`;
    }
    case 'thematicBreak':
      return '<hr class="my-4 border-black/15 dark:border-white/20">';
    case 'html':
      return `<pre class="${PRE_CLASS} text-[#888]" data-html=""><code>${escapeHtml(block.html)}</code></pre>`;
    default:
      return '';
  }
}

// ---------- contenteditable DOM → Markdown ----------

const BLOCK_TAGS = new Set(['P', 'DIV', 'H1', 'H2', 'H3', 'H4', 'H5', 'H6', 'PRE', 'BLOCKQUOTE', 'UL', 'OL', 'TABLE', 'HR']);

function isBlockElement(node) {
  return node.nodeType === Node.ELEMENT_NODE && BLOCK_TAGS.has(node.tagName);
}

// 把 el 的子节点按块分组：连续的文本/行内节点合并为一段，块级元素单独序列化
function childrenToBlocks(el) {
  const blocks = [];
  let inlineBuffer = null;
  const flush = () => {
    if (!inlineBuffer) return;
    const md = inlineToMarkdown(inlineBuffer).trim();
    inlineBuffer = null;
    if (md) blocks.push(md);
  };
  for (const child of el.childNodes) {
    if (isBlockElement(child)) {
      flush();
      const md = blockToMarkdown(child).trimEnd();
      if (md) blocks.push(md);
    } else if (child.nodeType === Node.ELEMENT_NODE && child.tagName === 'INPUT') {
      continue; // 任务列表勾选框由 marker 字段表达
    } else {
      if (!inlineBuffer) inlineBuffer = document.createElement('span');
      inlineBuffer.append(child.cloneNode(true));
    }
  }
  flush();
  return blocks;
}

export function editableToMarkdown(root) {
  return childrenToBlocks(root).join('\n\n');
}

function blockToMarkdown(el) {
  const tag = el.tagName;
  if (/^H[1-6]$/.test(tag)) return `${'#'.repeat(Number(tag[1]))} ${inlineToMarkdown(el)}`;
  if (tag === 'P' || tag === 'DIV') return inlineToMarkdown(el);
  if (tag === 'PRE') {
    // HTML 块按原文保留
    if (el.hasAttribute('data-html')) return el.textContent;
    const language = el.getAttribute('data-language') || '';
    return '```' + language + '\n' + el.textContent.replace(/\n+$/, '') + '\n```';
  }
  if (tag === 'BLOCKQUOTE') {
    return childrenToBlocks(el)
      .map((b) => b.split('\n').map((line) => '> ' + line).join('\n'))
      .join('\n>\n');
  }
  if (tag === 'UL' || tag === 'OL') return listToMarkdown(el, tag === 'OL');
  if (tag === 'TABLE') return tableToMarkdown(el);
  if (tag === 'HR') return '---';
  return inlineToMarkdown(el);
}

function listToMarkdown(el, ordered) {
  const items = [];
  el.querySelectorAll(':scope > li').forEach((li, i) => {
    const checkbox = li.querySelector(':scope input[type="checkbox"]');
    let marker = ordered ? `${i + 1}. ` : '- ';
    if (checkbox) marker += `[${checkbox.getAttribute('data-checked') || ' '}] `;
    const body = childrenToBlocks(li).join('\n\n');
    if (!body) {
      items.push(marker.trimEnd());
      return;
    }
    // 首行接标记，后续行缩进对齐
    const indented = body.split('\n').map((line, idx) => (idx === 0 ? line : '  ' + line)).join('\n');
    items.push(marker + indented);
  });
  return items.join('\n');
}

function tableToMarkdown(el) {
  const rows = [];
  el.querySelectorAll('tr').forEach((tr) => {
    const cells = [];
    tr.querySelectorAll('th, td').forEach((cell) => {
      cells.push(inlineToMarkdown(cell).replace(/\|/g, '\\|').replace(/\n/g, ' ').trim());
    });
    rows.push('| ' + cells.join(' | ') + ' |');
  });
  if (rows.length === 0) return '';
  const columnCount = rows[0].split('|').length - 3;
  const separator = '| ' + Array(Math.max(columnCount, 1)).fill('---').join(' | ') + ' |';
  return [rows[0], separator, ...rows.slice(1)].join('\n');
}

function inlineToMarkdown(el) {
  let out = '';
  for (const node of el.childNodes) {
    if (node.nodeType === Node.TEXT_NODE) {
      out += node.textContent;
      continue;
    }
    if (node.nodeType !== Node.ELEMENT_NODE) continue;
    const tag = node.tagName;
    const inner = inlineToMarkdown(node);
    if (tag === 'STRONG' || tag === 'B') out += `**${inner}**`;
    else if (tag === 'EM' || tag === 'I') out += `*${inner}*`;
    else if (tag === 'S' || tag === 'DEL') out += `~~${inner}~~`;
    else if (tag === 'CODE') out += '`' + node.textContent + '`';
    else if (tag === 'A') out += `[${inner}](${node.getAttribute('href') || ''})`;
    else if (tag === 'IMG') out += `![${node.getAttribute('alt') || ''}](${node.getAttribute('src') || ''})`;
    else if (tag === 'BR') out += '\n';
    else if (tag === 'INPUT') continue;
    else out += inner;
  }
  return out;
}

// ---------- Markdown 快捷输入（Typora 式即时转换） ----------

function styled(tag, text, className) {
  const el = document.createElement(tag);
  if (className) el.className = className;
  el.textContent = text;
  return el;
}

// 行内快捷转换：光标前文本匹配闭合标记时替换为样式元素
const INLINE_SHORTCUTS = [
  {
    re: /!\[([^\]]*)\]\(([^)\s]+)\)$/,
    make: (m) => {
      const img = document.createElement('img');
      img.setAttribute('src', m[2]);
      img.setAttribute('alt', m[1]);
      return img;
    },
  },
  {
    re: /\[([^\]]+)\]\(([^)\s]+)\)$/,
    make: (m) => {
      const a = styled('a', m[1], LINK_CLASS);
      a.setAttribute('href', m[2]);
      return a;
    },
  },
  { re: /\*\*([^*]+)\*\*$/, make: (m) => styled('strong', m[1], 'font-semibold') },
  { re: /(?<!\*)\*([^*]+)\*$/, make: (m) => styled('em', m[1]) },
  { re: /~~([^~]+)~~$/, make: (m) => styled('s', m[1]) },
  { re: /`([^`]+)`$/, make: (m) => styled('code', m[1], INLINE_CODE_CLASS) },
];

function transformInlineShortcut() {
  const sel = window.getSelection();
  if (!sel.rangeCount || !sel.isCollapsed) return false;
  const node = sel.anchorNode;
  if (node.nodeType !== Node.TEXT_NODE) return false;
  const parent = node.parentElement;
  // 代码内不触发行内转换
  if (parent && (parent.closest('pre') || parent.closest('code'))) return false;
  const offset = sel.anchorOffset;
  const before = node.textContent.slice(0, offset);
  for (const shortcut of INLINE_SHORTCUTS) {
    const m = before.match(shortcut.re);
    if (!m) continue;
    const range = document.createRange();
    range.setStart(node, offset - m[0].length);
    range.setEnd(node, offset);
    range.deleteContents();
    const el = shortcut.make(m);
    range.insertNode(el);
    placeCursorAfter(el);
    return true;
  }
  return false;
}

// 块级快捷转换：段落开头输入 # / > / - / 1. / ``` / --- 等标记时整块转换
function transformBlockShortcut() {
  const sel = window.getSelection();
  if (!sel.rangeCount) return false;
  const node = sel.anchorNode;
  const el = node.nodeType === Node.TEXT_NODE ? node.parentElement : node;
  const p = el && el.closest('p');
  if (!p) return false;
  const text = p.textContent;

  let replacement = null;
  const marker = text.match(/^(#{1,6}|>|[-+]|\d+\.)\s/);
  if (marker) {
    const rest = text.slice(marker[0].length);
    if (marker[1].startsWith('#')) {
      replacement = styled(`h${marker[1].length}`, rest, `whitespace-pre-wrap font-semibold ${headingClasses[marker[1].length]}`);
    } else if (marker[1] === '>') {
      replacement = document.createElement('blockquote');
      replacement.className = QUOTE_CLASS;
      replacement.append(styled('p', rest, P_CLASS));
    } else if (/\d+\./.test(marker[1])) {
      replacement = styled('ol', '', 'my-2 pl-6 list-decimal');
      replacement.append(styled('li', rest, 'my-0.5'));
    } else {
      replacement = styled('ul', '', 'my-2 pl-6 list-disc');
      replacement.append(styled('li', rest, 'my-0.5'));
    }
  } else if (/^`{3,}$/.test(text.trim())) {
    replacement = document.createElement('pre');
    replacement.className = PRE_CLASS;
    replacement.append(styled('code', ''));
  } else if (/^-{3,}$/.test(text.trim())) {
    // 分割线：替换为新段落，前方插入 hr，光标落在空段落里
    const hr = styled('hr', '', 'my-4 border-black/15 dark:border-white/20');
    const next = styled('p', '', P_CLASS);
    next.innerHTML = '<br>';
    p.replaceWith(hr, next);
    placeCursorAtEnd(next);
    return true;
  }

  if (!replacement) return false;
  p.replaceWith(replacement);
  placeCursorAtEnd(replacement);
  return true;
}

// 在一次 input 后尝试即时转换；返回是否有结构变化
export function applyMarkdownShortcuts() {
  return transformBlockShortcut() || transformInlineShortcut();
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
