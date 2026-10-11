import { ref, computed } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import {
  insertInlineWrapper,
  applySlashCommand,
  placeCursorAtStart,
  placeCursorAtEnd,
  insertTextAtCursor,
  plainText,
  rangeBeforePoint,
  domToBlockDtos,
} from '../utils/wysiwyg.js';
import {
  buildFontSpan,
  buildFontTemplate,
  placeCaretInFontTemplate,
  caretFontSpan,
  mergeStyleIntoSpanWithMarkSync,
} from '../utils/fontStyle.js';

// 右键菜单（Typora 式：剪贴板 / 复制粘贴为 / 格式 / 段落 / 插入）
// 含：选区暂存/恢复（WKWebView 右键会塌陷选区）、格式化动作、块类型转换、插入类动作。
//
// ctx：Editor.vue 共享上下文（blocks/content/editingId/editingBlock/footnotes/syncing 为 ref，
// suppressBlurCommit/pendingCaretOffset/cursorAtStart 经访问器共享；
// startEdit/commitEdit/reparse/splitAndCommit/appendBlocksAtEnd/publishBlocks/
// undoHistory/redoHistory/openFind/applyTableTemplate/updateLangMenu 运行时解析）。
export function useContextMenu(ctx) {
  const ctxMenu = ref(null); // { x, y }

  function onContextMenu(e) {
    if (ctx.isSourceMode()) return;
    e.preventDefault();
    // 已处于编辑态：保留当前选区；否则进入被点击块的编辑
    const el = ctx.currentEditable();
    if (!(el && el.contains(e.target))) {
      const blockEl = e.target.closest?.('[data-block-id]');
      const block = ctx.blocks.value.find((b) => b.id === blockEl?.getAttribute('data-block-id'));
      if (!block) return;
      // 有非空选区时 startEdit 会拒入编辑（避免替换 DOM 销毁选区）；此处选区已按
      // 纯文本偏移暂存（mousedown/contextmenu 时），直接进入编辑，动作应用时按偏移重建选区
      if (ctx.editingId.value === null) ctx.editingId.value = block.id;
      else ctx.startEdit(block);
    }
    // 此处仅尝试覆盖暂存：若 WKWebView 已在右键 mousedown 后塌陷选区，
    // 本次 stash 无效，保留 mousedown 时的暂存（不得先行清空）
    stashCtxSelection();
    // 编辑态下平台默认行为可能已塌陷选区：立即恢复，保持菜单打开期间高亮连续
    if (el) restoreCtxSelection(el);
    ctxMenu.value = { x: e.clientX, y: e.clientY };
  }

  // 右键菜单选区暂存：WKWebView 中右键 mousedown 的默认行为会把选区塌陷为光标，
  // 在默认行为发生前的 mousedown/contextmenu 里暂存选区，菜单动作应用前恢复。
  // 注意：动作处理以 closeCtxMenu 开头，暂存不得随菜单关闭清理；
  // 以时间戳防陈旧（仅菜单打开后短时间内的动作才允许恢复）
  let ctxSavedRange = null;

  // 暂存/重建选区时要跳过的非内容文本（渲染态列表标记、代码块语言角标、callout 标签等；
  // 可编辑容器内无这些元素或为 contenteditable=false 的非内容标签，两侧按同一规则跳过保证偏移一致）
  const CTX_SKIP_SEL = '.md-marker, .md-code-lang-badge, .md-callout-label';

  // 子树内 (node, offset) 的纯文本偏移（Range 探针 - 被跳过子树的文本长度）
  function domTextOffset(root, node, offset) {
    const probe = rangeBeforePoint(root, node, offset);
    let len = probe.toString().length;
    for (const skip of root.querySelectorAll(CTX_SKIP_SEL)) {
      if (probe.intersectsNode(skip)) len -= skip.textContent.length;
    }
    return len;
  }

  // 纯文本偏移 → DOM Range（在可编辑容器内按同一跳过规则逐文本节点累计）
  function rangeFromTextOffsets(root, start, end) {
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, (n) =>
      n.parentElement?.closest(CTX_SKIP_SEL) ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT);
    let acc = 0;
    let s = null;
    let e = null;
    let cur = walker.nextNode();
    while (cur) {
      const len = cur.textContent.length;
      if (!s && start <= acc + len) s = { node: cur, offset: start - acc };
      if (!e && end <= acc + len) {
        e = { node: cur, offset: end - acc };
        break;
      }
      acc += len;
      cur = walker.nextNode();
    }
    if (!s || !e) return null;
    const r = document.createRange();
    r.setStart(s.node, s.offset);
    r.setEnd(e.node, e.offset);
    return r;
  }

  // 渲染态暂存 → 可编辑容器 Range：先按偏移换算并用暂存原文校验
  //（公式 SVG/脚注引用等载体两侧文本可能不一致），不一致时退化为在容器纯文本中
  // 搜索暂存原文定位；都找不到则尽力返回偏移结果
  function reifyCtxRange(root, saved) {
    const byOffset = rangeFromTextOffsets(root, saved.start, saved.end);
    if (!saved.text || (byOffset && byOffset.toString() === saved.text)) return byOffset;
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, (n) =>
      n.parentElement?.closest(CTX_SKIP_SEL) ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT);
    const parts = [];
    let cur = walker.nextNode();
    while (cur) {
      parts.push(cur.textContent);
      cur = walker.nextNode();
    }
    const idx = parts.join('').indexOf(saved.text);
    if (idx < 0) return byOffset;
    return rangeFromTextOffsets(root, idx, idx + saved.text.length) || byOffset;
  }

  function stashCtxSelection() {
    const el = ctx.currentEditable();
    const sel = window.getSelection();
    if (!sel.rangeCount) return;
    const r = sel.getRangeAt(0);
    if (r.collapsed) return;
    // 编辑态：DOM 结构在提交前保持稳定，直接暂存 Range
    if (el && el.contains(r.startContainer)) {
      ctxSavedRange = { range: r.cloneRange(), at: Date.now() };
      return;
    }
    // 非编辑态（无光标直接框选渲染态文字）：渲染 DOM 将随进入编辑被替换，
    // 选区按「块内纯文本偏移」暂存（渲染态与编辑态行内文本一致——同一 InlineTextTree 数据源）；
    // 同时暂存选区原文，重建时校验/搜索回退（公式 SVG 等载体两侧文本可能不一致）
    const anchorEl = r.startContainer.nodeType === Node.ELEMENT_NODE ? r.startContainer : r.startContainer.parentElement;
    const row = anchorEl?.closest('[data-block-id]');
    if (!row || !row.contains(r.endContainer)) return; // 跨块选区不在此转移
    const start = domTextOffset(row, r.startContainer, r.startOffset);
    const end = domTextOffset(row, r.endContainer, r.endOffset);
    if (end <= start) return;
    ctxSavedRange = { blockId: row.getAttribute('data-block-id'), start, end, text: r.toString(), at: Date.now() };
  }

  // 编辑区 mousedown：右键（button 2）按下时抢先暂存选区
  function onEditorMouseDown(e) {
    if (e.button === 2) stashCtxSelection();
  }

  // Ctrl/Cmd+点击链接：系统默认浏览器打开（Typora 式）。仅放行 http(s)/mailto 协议
  //（相对路径/锚点/危险协议如 javascript: 不处理）；脚注引用（javascript:void(0)）排除。
  // mousedown 拦截：阻止落光标/焦点变化；click 拦截：阻止进编辑/展开等后续分发
  function ctrlLinkTarget(e) {
    if (!(e.ctrlKey || e.metaKey) || e.button !== 0) return null;
    const a = e.target.closest?.('a[href]');
    if (!a || a.hasAttribute('data-footnote-ref') || a.classList.contains('md-footnote-ref')) return null;
    const href = a.getAttribute('href') || '';
    return /^(https?:|mailto:)/i.test(href) ? href : null;
  }
  function onCtrlLinkMousedown(e) {
    if (!ctrlLinkTarget(e)) return;
    e.preventDefault();
    e.stopPropagation();
  }
  function onCtrlLinkClick(e) {
    const href = ctrlLinkTarget(e);
    if (!href) return;
    e.preventDefault();
    e.stopPropagation();
    openUrl(href).catch(() => {});
  }

  // 菜单动作应用前恢复暂存选区（当前选区仍在编辑区内且未塌陷则无需恢复）
  function restoreCtxSelection(el) {
    const saved = ctxSavedRange;
    if (!saved || !el) return;
    if (Date.now() - saved.at > 5000) return; // 陈旧暂存不恢复
    const sel = window.getSelection();
    if (sel.rangeCount) {
      const cur = sel.getRangeAt(0);
      if (!cur.collapsed && el.contains(cur.startContainer)) return;
    }
    // 编辑态暂存：DOM Range 直接恢复
    if (saved.range) {
      const r = saved.range;
      if (!r.startContainer.isConnected || !el.contains(r.startContainer)) return;
      sel.removeAllRanges();
      sel.addRange(r);
      return;
    }
    // 渲染态暂存（纯文本偏移 + 原文校验/搜索回退）：块须与当前编辑块一致
    if (saved.blockId && saved.blockId !== ctx.editingId.value) return;
    const r = reifyCtxRange(el, saved);
    if (!r) return;
    sel.removeAllRanges();
    sel.addRange(r);
  }

  function closeCtxMenu() {
    ctxMenu.value = null;
  }

  // 右键进入编辑携带的渲染态框选：可编辑容器挂载后立即按暂存的纯文本偏移重建选区，
  // 保持高亮视觉连续（否则第一次右键时选区高亮随 DOM 替换消失）
  function restoreCtxSelectionOnMount(el) {
    const saved = ctxSavedRange;
    if (!saved || saved.range || !saved.blockId) return false;
    if (saved.blockId !== ctx.editingId.value) return false;
    if (Date.now() - saved.at > 5000) return false;
    const r = reifyCtxRange(el, saved);
    if (!r) return false;
    const sel = window.getSelection();
    sel.removeAllRanges();
    sel.addRange(r);
    return true;
  }

  // 段落子菜单勾选态（heading 块映射为 h1-h6）
  const ctxCurrentType = computed(() => {
    const b = ctx.editingBlock.value;
    if (!b) return 'paragraph';
    return b.type === 'heading' ? `h${b.level}` : b.type;
  });

  async function onMenuAction(id, payload) {
    closeCtxMenu();
    const el = ctx.currentEditable();
    switch (id) {
      // 撤销/重做/查找替换（右键菜单顶部项）
      case 'undo':
        return ctx.undoHistory();
      case 'redo':
        return ctx.redoHistory();
      case 'find':
        return ctx.openFind(true);
      case 'cut':
        el?.focus({ preventScroll: true });
        document.execCommand('cut');
        return;
      case 'copy':
        document.execCommand('copy');
        return;
      case 'delete':
        document.execCommand('delete');
        return;
      case 'paste':
      case 'pastePlain':
        return pastePlainText();
      case 'copyMarkdown':
        return copySelection('markdown');
      case 'copyHtml':
        return copySelection('html');
      case 'copySimplified':
        return copySelection('simplified');
      case 'copyPlain':
        return copySelection('plain');
      // 行内格式：统一走 insertInlineWrapper（有选区包选区，无选区插空元素光标入内）
      case 'bold':
      case 'italic':
      case 'underline':
      case 'strikethrough':
      case 'highlight':
      case 'inlineCode':
      case 'link':
        if (el) {
          ctx.suppressBlurCommit = true;
          try {
            restoreCtxSelection(el);
            insertInlineWrapper(el, id);
          } finally {
            ctx.suppressBlurCommit = false;
          }
          if (document.activeElement !== el) el.focus({ preventScroll: true });
        }
        return;
      // 文字颜色/字号：色板与字号行选择（payload 为 htmlStyle 的 color/fontSize JSON）
      case 'fontColor':
        return ctxApplyFontStyle({ color: payload });
      case 'fontSize':
        return ctxApplyFontStyle({ fontSize: payload });
      case 'quote':
      case 'bulletedListItem':
      case 'numberedListItem':
      case 'taskListItem':
      case 'paragraph':
      case 'h1':
      case 'h2':
      case 'h3':
      case 'h4':
      case 'h5':
      case 'h6':
        return convertBlockType(id);
      case 'image':
      case 'separator':
      case 'table':
      case 'codeBlock':
      case 'mathBlock':
      case 'linkRef':
      case 'toc':
        return insertBlockFromMenu(id);
      case 'footnote':
        return insertFootnote();
      case 'yamlFrontMatter':
        return insertYamlFrontMatter();
      case 'paraAbove':
        return insertParagraphSibling(true);
      case 'paraBelow':
        return insertParagraphSibling(false);
    }
  }

  // 右键菜单字体样式：有选区包选区；光标在既有字体 span 内则叠加更新（换色/换号）；
  // 否则插入空 span 光标入内（FEFF 哨兵占位），输入即带样式文字，Esc 退出该样式
  function ctxApplyFontStyle(style) {
    const el = ctx.currentEditable();
    if (!el || !style) return;
    ctx.suppressBlurCommit = true;
    try {
      restoreCtxSelection(el);
      const sel = window.getSelection();
      if (sel.rangeCount && !sel.isCollapsed && el.contains(sel.anchorNode)) {
        // 有选区：选区内容套用样式
        const span = buildFontSpan(style);
        const range = sel.getRangeAt(0);
        span.append(range.extractContents());
        range.insertNode(span);
        placeCursorAtEnd(span);
      } else if (sel.rangeCount && el.contains(sel.anchorNode) && caretFontSpan(el)) {
        // 光标在既有字体 span 内：叠加更新该 span（color/fontSize 独立覆盖），不嵌套新 span
        const existing = caretFontSpan(el);
        mergeStyleIntoSpanWithMarkSync(existing, style);
      } else {
        // 无选区：插入字体模板（淡灰 <font> 标签 + 样式 span），光标在标签内文字位，
        // 输入即带选择的颜色/字号；Esc 退出该样式（光标移到模板之后）
        const { frag, zwsp } = buildFontTemplate(style);
        const range = sel.rangeCount && el.contains(sel.anchorNode) ? sel.getRangeAt(0) : null;
        if (range) {
          range.insertNode(frag);
        } else {
          // 无光标：放入块的首个块级子元素内（模板不能包块级元素）
          (el.firstElementChild || el).append(frag);
        }
        placeCaretInFontTemplate(zwsp);
      }
    } finally {
      ctx.suppressBlurCommit = false;
    }
    if (document.activeElement !== el) el.focus({ preventScroll: true });
  }

  // 插入 YAML Front Matter：文档头插入（已有则不重复）；模板由 Rust block_template 生成
  async function insertYamlFrontMatter() {
    if (ctx.content.value.startsWith('---\n')) return;
    const tpl = await invoke('block_template', { kind: 'yamlFrontMatter' }).catch(() => null);
    if (!tpl) return;
    if (ctx.editingId.value !== null) await ctx.commitEdit();
    ctx.syncing.value = true;
    try {
      ctx.content.value = tpl.markdown + '\n' + ctx.content.value;
      await ctx.reparse();
      const target = ctx.blocks.value[0];
      ctx.suppressBlurCommit = true;
      ctx.pendingCaretOffset = tpl.caretOffset; // 光标落 title: 之后
      ctx.editingId.value = target ? target.id : null;
      if (!target) {
        ctx.suppressBlurCommit = false;
        ctx.pendingCaretOffset = null;
      }
    } finally {
      ctx.syncing.value = false;
    }
  }

  // 粘贴为纯文本（剪贴板 → 光标处；浏览器 insertText 自动处理多行分段，换行符先统一为 LF）
  async function pastePlainText() {
    const text = await navigator.clipboard.readText().catch(() => null);
    const el = ctx.currentEditable();
    if (text == null || !el) return;
    el.focus({ preventScroll: true });
    document.execCommand('insertText', false, text.replace(/\r\n?/g, '\n'));
  }

  // 复制选区（无选区取当前整块）：markdown=Rust 序列化 / html=DOM HTML /
  // simplified=按块分段的纯文本 / plain=纯文本
  async function copySelection(kind) {
    const el = ctx.currentEditable();
    if (!el) return;
    const sel = window.getSelection();
    const div = document.createElement('div');
    if (sel.rangeCount && !sel.isCollapsed && el.contains(sel.anchorNode)) {
      div.append(sel.getRangeAt(0).cloneContents());
    } else {
      // 无选区取当前整块：Range 克隆整段子树（不落 innerHTML 字符串）
      const all = document.createRange();
      all.selectNodeContents(el);
      div.append(all.cloneContents());
    }
    let text = '';
    if (kind === 'markdown') {
      text = await invoke('serialize_markdown', { blocks: domToBlockDtos(div) }).catch(() => '');
    } else if (kind === 'html') {
      text = div.innerHTML;
    } else if (kind === 'simplified') {
      text = [...div.childNodes]
        .map((n) => n.textContent)
        .filter((t) => t.trim())
        .join('\n\n');
    } else {
      text = sel.rangeCount && !sel.isCollapsed ? sel.toString() : div.textContent;
    }
    if (text) await navigator.clipboard.writeText(text).catch(() => {});
  }

  // 段落/块类型转换：保留行内结构（节点迁移而非纯文本——粗斜体/链接/颜色等格式不丢）
  async function convertBlockType(id) {
    const el = ctx.currentEditable();
    if (!el) return;
    // 先取出块内容：单块时取其内部行内容（避免块套块），多块时取全部子节点
    const sourceChildren =
      el.firstElementChild && el.childElementCount === 1
        ? [...el.firstElementChild.childNodes]
        : [...el.childNodes];
    const fragment = document.createDocumentFragment();
    for (const node of sourceChildren) fragment.append(node);
    const hasContent = fragment.textContent.trim() !== '';
    ctx.suppressBlurCommit = true;
    try {
      await applySlashCommand(el, id);
      if (hasContent) {
        // applySlashCommand 已把光标放在内容槽位（h1-h6/li/quote p 内）：原内容节点插回
        const sel = window.getSelection();
        if (sel.rangeCount && el.contains(sel.anchorNode)) {
          sel.getRangeAt(0).insertNode(fragment);
          placeCursorAtEnd(el.lastElementChild || el);
        }
      }
    } finally {
      ctx.suppressBlurCommit = false;
    }
    if (document.activeElement !== el) el.focus({ preventScroll: true });
  }

  // 插入类块（图像/分割线/表格/代码块/公式块/链接引用）：
  // 当前块有内容时先在末尾拆出新空块（保住原内容），再按斜杠模板插入
  async function insertBlockFromMenu(id) {
    let el = ctx.currentEditable();
    if (!el) return;
    if (el.textContent.trim()) {
      placeCursorAtEnd(el);
      await ctx.splitAndCommit();
      el = ctx.currentEditable();
      if (!el) return;
    }
    if (id === 'table') {
      await ctx.applyTableTemplate({ rows: 2, cols: 2 });
      return;
    }
    ctx.suppressBlurCommit = true;
    try {
      await applySlashCommand(el, id);
    } finally {
      ctx.suppressBlurCommit = false;
    }
    if (document.activeElement !== el) el.focus({ preventScroll: true });
    // 代码块：围栏已插入（```），立即弹出语言补全，先编辑语言
    if (id === 'codeBlock') ctx.updateLangMenu();
  }

  // 插入脚注：光标处插入 [^n] 引用（编号 = 现有最大编号 +1），提交后在文末追加定义块并进入编辑
  async function insertFootnote() {
    const el = ctx.currentEditable();
    if (!el) return;
    let max = 0;
    for (const b of ctx.footnotes.value) {
      const n = parseInt(plainText(b.title), 10);
      if (!Number.isNaN(n)) max = Math.max(max, n);
    }
    const id = max + 1;
    insertTextAtCursor(`[^${id}]`);
    await ctx.commitEdit();
    // 模板（含脚注 id）由 Rust 生成，前端不做 Markdown 文本拼接
    const tpl = await invoke('block_template', { kind: 'footnoteDef', footnoteId: String(id) }).catch(() => null);
    if (!tpl) return;
    const def = tpl.markdown;
    ctx.syncing.value = true;
    try {
      // 解析失败：全文重解析自愈（appendBlocksAtEnd 内处理）
      const parsed = await ctx.appendBlocksAtEnd(def);
      if (!parsed) return;
      ctx.publishBlocks();
      const target = parsed[parsed.length - 1];
      ctx.suppressBlurCommit = true;
      ctx.cursorAtStart = false; // 光标落块尾（定义文本后）
      ctx.editingId.value = target ? target.id : null;
      if (!target) ctx.suppressBlurCommit = false;
    } finally {
      ctx.syncing.value = false;
    }
  }

  // 段落（上方/下方）：上方 = 块首拆分后进入前一个空段落；下方 = 块尾拆分（默认进入新空段落）
  async function insertParagraphSibling(above) {
    const el = ctx.currentEditable();
    if (!el || !ctx.editingBlock.value) return;
    if (above) {
      placeCursorAtStart(el);
      await ctx.splitAndCommit();
      const idx = ctx.blocks.value.findIndex((b) => b.id === ctx.editingId.value);
      const prev = ctx.blocks.value[idx - 1];
      if (prev) {
        ctx.suppressBlurCommit = true;
        ctx.cursorAtStart = true;
        ctx.editingId.value = prev.id;
      }
    } else {
      placeCursorAtEnd(el);
      await ctx.splitAndCommit();
    }
  }

  return {
    // 状态（模板绑定）
    ctxMenu, ctxCurrentType,
    // 事件处理（模板绑定）
    onContextMenu, onEditorMouseDown, onCtrlLinkMousedown, onCtrlLinkClick, onMenuAction,
    // 编辑链路共用（setEditableEl 挂载恢复选区 / Esc 与文档级点击关闭菜单）
    closeCtxMenu, restoreCtxSelectionOnMount,
    // 文档级快捷键（复制为 Markdown 等）与列表组光标映射（domTextOffset）共用
    copySelection, pastePlainText, insertBlockFromMenu, convertBlockType, domTextOffset,
  };
}
