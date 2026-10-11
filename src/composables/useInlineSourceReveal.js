import {
  LINK_CLASS,
  markSpanFactory,
  textBeforePoint,
  placeCaretAtTextOffset,
} from '../utils/wysiwyg.js';

// 行内源码显现（Typora 式）：光标进入已渲染的行内样式（加粗/斜体/下划线/删除线/高亮/行内代码）时，
// 给该元素挂 md-src 类显示其 Markdown 源标记；纯 CSS 视觉标记（::before/::after），不改 DOM 结构、不参与提交。
// 光标离开即移除。链接/字体样式不走 md-src：光标进入即展开为可编辑的源标记结构（expandLinkAtCaret/expandFontAtCaret）。
//
// ctx：Editor.vue 共享上下文（currentEditable 运行时解析）。
export function useInlineSourceReveal(ctx) {
  let srcRevealEl = null;

  // 光标移出字体模板（Esc 退出/方向键/点击到标记之外）：剔除淡灰标签装饰，只留样式文字
  //（渲染态）。焦点在编辑器外（菜单/RGB 输入框）或光标仍在模板内时不动作。
  // 「仍在模板内」含紧邻标签的边界位（标记 contenteditable=false，光标不会落入标记文本，
  // 只会停在其左右邻位）——移到标签位置不渲染，移出标签（进入正文）才渲染
  function collapseFontTemplateOutside(el) {
    const marks = el.querySelectorAll('.md-font-mark');
    if (!marks.length) return;
    const sel = window.getSelection();
    const anchor = sel.rangeCount ? sel.anchorNode : null;
    let anchorEl = null;
    if (anchor) anchorEl = anchor.nodeType === Node.TEXT_NODE ? anchor.parentElement : anchor;
    if (anchorEl && !el.contains(anchorEl)) return; // 焦点在编辑器外（菜单等）：保持现状
    if (anchorEl?.closest?.('.md-font-mark, span[data-html-style], font')) return; // 光标仍在模板内
    // 元素边界锚点：光标紧邻任一标签标记（标签左右邻位）视为仍在标签位置，不渲染
    if (anchor && anchor.nodeType === Node.ELEMENT_NODE) {
      const isMark = (n) => n?.nodeType === Node.ELEMENT_NODE && n.classList?.contains('md-font-mark');
      if (isMark(anchor.childNodes[sel.anchorOffset - 1]) || isMark(anchor.childNodes[sel.anchorOffset])) return;
    }
    marks.forEach((m) => m.remove());
  }

  // 光标移出展开的链接结构（[文字](地址) 整体之外）：折叠回渲染态 <a>
  //（与字体模板折叠同触发；pre[data-raw] 原文段不参与——那是整段原文编辑形态。
  // 斜杠插入的链接模板段 p[data-raw] 参与折叠：折叠时连带摘掉 data-raw 转常规段落，
  // 否则提交走 textContent 原文回写，标记移除后括号丢失、链接退化为纯文字）
  function collapseLinkTemplateOutside(el) {
    const sel = window.getSelection();
    const anchor = sel.rangeCount ? sel.anchorNode : null;
    let anchorEl = null;
    if (anchor) anchorEl = anchor.nodeType === Node.TEXT_NODE ? anchor.parentElement : anchor;
    if (!anchor || (anchorEl && !el.contains(anchorEl))) return; // 焦点在编辑器外：保持现状
    const isMark = (n, t) =>
      n?.nodeType === Node.ELEMENT_NODE && n.classList?.contains('md-link-mark') && n.textContent === t;
    for (const startMark of el.querySelectorAll('.md-link-mark')) {
      if (startMark.textContent !== '[') continue;
      // 图片模板（! 标记紧邻 [ 前）：保持原文编辑形态，不参与折叠
      if (isMark(startMark.previousSibling, '!')) continue;
      // 斜杠链接模板段（p[data-raw]）：折叠为渲染态 <a> 并摘除 data-raw（提交改走常规段落提取）；
      // 其余 data-raw 宿主（pre 原子块等）不参与
      const rawHost = startMark.closest('[data-raw]');
      if (rawHost && rawHost.tagName !== 'P' && rawHost.tagName !== 'DIV') continue;
      const textEl = startMark.nextSibling;
      const m2 = textEl?.nextSibling;
      const m3 = m2?.nextSibling;
      const headOk =
        textEl?.nodeType === Node.ELEMENT_NODE &&
        textEl.classList.contains('md-link-text') &&
        isMark(m2, ']') &&
        isMark(m3, '(');
      if (!headOk) continue; // 结构被改残：按普通文本保留，不折叠
      // 地址区扫描到 ) 标记为止（容忍变形：( 后多裸文本节点、空地址 span 等——
      // 逐位取节点会在变形时永不折叠，表现为「光标移到 ) 外也不渲染」）
      const urlNodes = [];
      let closeMark = null;
      for (let n = m3.nextSibling; n; n = n.nextSibling) {
        if (n.nodeType === Node.ELEMENT_NODE && n.classList?.contains('md-link-mark')) {
          if (isMark(n, ')')) closeMark = n;
          break; // 其他标记先到：结构已残
        }
        urlNodes.push(n);
      }
      if (!closeMark) continue;
      // 「仍在结构内」= 光标落在结构节点内，或处于首标记之后、末标记之前（含之间）的
      // 父元素边界位；末标记之后（) 外）算外部——折叠渲染（Typora 式：括号外即还原链接）
      // 例外：Chromium 常把「移到 ) 后」的光标锚在 ) 标记文本末尾（视觉已在括号外），
      // 这种形态必须算外部，否则永不折叠；对称地，[ 标记文本开头（视觉在 [ 前）也算外部
      const atMarkEnd = (mark) => {
        if (!mark.contains(anchor)) return false;
        if (anchor.nodeType === Node.TEXT_NODE) return sel.anchorOffset >= anchor.textContent.length;
        // 元素锚点：(mark, 子节点数) = 视觉在标记字符之后
        return anchor === mark && sel.anchorOffset >= mark.childNodes.length;
      };
      const atMarkStart = (mark) => {
        if (!mark.contains(anchor)) return false;
        if (anchor.nodeType === Node.TEXT_NODE) return sel.anchorOffset === 0;
        return anchor === mark && sel.anchorOffset === 0;
      };
      const afterClose = atMarkEnd(closeMark);
      const beforeOpen = atMarkStart(startMark);
      const nodes = [startMark, textEl, m2, m3, ...urlNodes, closeMark];
      let inside = nodes.some((n) => n.contains(anchor)) && !afterClose && !beforeOpen;
      const parent = startMark.parentNode;
      if (!inside && anchor === parent) {
        const kids = [...parent.childNodes];
        const i0 = kids.indexOf(startMark);
        const i1 = kids.indexOf(closeMark);
        inside = i0 >= 0 && sel.anchorOffset > i0 && sel.anchorOffset <= i1;
      }
      if (inside) continue;
      // 折叠回 <a>；父元素边界光标在结构起点之后、或锚在 ) 标记末尾时，
      // 折叠后钉到新 <a> 之后（移除节点会平移偏移）；锚在 [ 标记开头则钉到 <a> 之前
      const pinCaret =
        afterClose ||
        (anchor === parent && sel.anchorOffset > [...parent.childNodes].indexOf(startMark));
      const pinBefore = !pinCaret && beforeOpen;
      const a = document.createElement('a');
      a.className = LINK_CLASS;
      a.setAttribute(
        'href',
        urlNodes.map((n) => n.textContent).join('').replace(/\uFEFF/g, '').trim(),
      );
      a.textContent = textEl.textContent.replace(/\uFEFF/g, '');
      parent.insertBefore(a, startMark);
      for (const n of nodes) n.remove();
      rawHost?.removeAttribute('data-raw');
      if (pinCaret || pinBefore) {
        const range = document.createRange();
        if (pinCaret) range.setStartAfter(a);
        else range.setStartBefore(a);
        range.collapse(true);
        sel.removeAllRanges();
        sel.addRange(range);
      }
    }
  }

  function updateInlineSourceReveal() {
    const el = ctx.currentEditable();
    if (el) collapseFontTemplateOutside(el);
    if (el) collapseLinkTemplateOutside(el);
    let host = null;
    if (el) {
      const sel = window.getSelection();
      if (sel.rangeCount && sel.isCollapsed) {
        const anchor = sel.anchorNode;
        const anchorEl = anchor.nodeType === Node.TEXT_NODE ? anchor.parentElement : anchor;
        // 链接：光标进入时展开为源标记编辑形态（文字/地址都可编辑），不打 md-src
        const a = anchorEl?.closest?.('a');
        // 字体样式（颜色/字号 span、font 标签）：光标进入时展开 <font …> 原始标签编辑
        const fontEl = anchorEl?.closest?.('span[data-html-style], font');
        if (a && el.contains(a) && !a.closest('pre')) {
          expandLinkAtCaret(a);
        } else if (fontEl && el.contains(fontEl) && !fontEl.closest('pre')) {
          expandFontAtCaret(fontEl);
        } else {
          host = anchorEl?.closest?.('strong, em, u, s, mark, code') || null;
          // pre 内的 code（代码块）不算行内样式
          if (host && (!el.contains(host) || host.closest('pre'))) host = null;
        }
      }
    }
    if (host === srcRevealEl) return;
    srcRevealEl?.classList?.remove('md-src');
    srcRevealEl = host;
    srcRevealEl?.classList?.add('md-src');
  }

  // 光标进入字体样式（颜色/字号 span、font 标签）：展开为原始标签编辑形态
  //（<font …>文字</font>，标签淡灰），文字可编辑；提交时标签装饰不入内容
  //（domToInlines 跳过 .md-font-mark），样式 span 照常回写
  function expandFontAtCaret(fontEl) {
    // 已有标签结构（刚插入的模板/已展开）不重复展开
    const prev = fontEl.previousSibling;
    if (prev?.nodeType === Node.ELEMENT_NODE && prev.classList?.contains('md-font-mark')) return;
    const sel = window.getSelection();
    const raw = fontEl.textContent;
    // 光标在文字内的偏移（展开后恢复）；非文本锚点（元素边界）落到文字末尾
    let caretOffset = raw.length;
    if (sel.rangeCount && sel.isCollapsed) {
      const anchor = sel.anchorNode;
      if (anchor.nodeType === Node.TEXT_NODE && fontEl.contains(anchor)) {
        caretOffset = textBeforePoint(fontEl, anchor, sel.anchorOffset).length;
      }
    }
    const tag = fontEl.tagName.toLowerCase() === 'font' ? 'font' : 'span';
    const attrs = [];
    const color = fontEl.getAttribute('color');
    if (color) attrs.push(`color="${color}"`);
    const styleText = fontEl.getAttribute('style');
    if (styleText) attrs.push(`style="${styleText}"`);
    const mark = markSpanFactory('md-font-mark');
    const openMark = mark(`<${tag}${attrs.length ? ' ' + attrs.join(' ') : ''}>`);
    const closeMark = mark(`</${tag}>`);
    // 标签仅作显示、不可编辑：否则光标可落入标签文本内，输入被当作装饰剔除（文字被吞）
    openMark.setAttribute('contenteditable', 'false');
    closeMark.setAttribute('contenteditable', 'false');
    fontEl.parentNode.insertBefore(openMark, fontEl);
    fontEl.parentNode.insertBefore(closeMark, fontEl.nextSibling);
    // 恢复光标到文字内同一偏移
    placeCaretAtTextOffset(fontEl, caretOffset);
  }

  // 光标进入链接：展开为原始标记编辑形态（[文字](url)，括号淡灰/文字与地址浅蓝），
  // 文字与地址都可编辑；提交时按标记结构折叠回链接（domToInlines 的 md-link-mark 分支）。
  // 展开链接为源标记结构（共用）：替换 <a> 为标记片段，返回恢复光标所需信息
  function expandLinkTemplate(a) {
    const sel = window.getSelection();
    // 光标在链接文字内的偏移（展开后恢复）；非文本锚点（元素边界）落到文字末尾
    const raw = a.textContent;
    let caretOffset = raw.length;
    if (sel.rangeCount && sel.isCollapsed) {
      const anchor = sel.anchorNode;
      if (anchor.nodeType === Node.TEXT_NODE && a.contains(anchor)) {
        caretOffset = textBeforePoint(a, anchor, sel.anchorOffset).length;
      }
    }
    const href = a.getAttribute('href') || '';
    const mark = markSpanFactory('md-link-mark');
    const textSpan = document.createElement('span');
    textSpan.className = 'md-link-text';
    textSpan.textContent = raw;
    // 地址用 md-link-url span 包裹（浅蓝显示，与链接文字同色；提取兼容文本/元素两种形态）
    const urlSpan = document.createElement('span');
    urlSpan.className = 'md-link-url';
    urlSpan.textContent = href;
    const frag = document.createDocumentFragment();
    frag.append(mark('['), textSpan, mark(']'), mark('('), urlSpan, mark(')'));
    a.parentNode.replaceChild(frag, a);
    return { sel, textSpan, urlSpan, caretOffset };
  }

  // 光标进入链接：展开为原始标记编辑形态（[文字](url)，括号淡灰/文字与地址浅蓝），
  // 文字与地址都可编辑；提交时按标记结构折叠回链接（domToInlines 的 md-link-mark 分支）
  function expandLinkAtCaret(a) {
    const { sel, textSpan, caretOffset } = expandLinkTemplate(a);
    const t = textSpan.firstChild;
    if (t) {
      const r = document.createRange();
      r.setStart(t, Math.min(caretOffset, t.textContent.length));
      r.collapse(true);
      sel.removeAllRanges();
      sel.addRange(r);
    }
  }

  // 渲染链接右缘按 → 的变体：展开后光标落在地址末尾
  //（空地址落 urlSpan 元素位——仍在结构内，不会立即折叠）
  function expandLinkAtCaretInUrl(a) {
    const { sel, urlSpan } = expandLinkTemplate(a);
    const r = document.createRange();
    if (urlSpan.firstChild) r.setStart(urlSpan.firstChild, urlSpan.textContent.length);
    else r.setStart(urlSpan, 0);
    r.collapse(true);
    sel.removeAllRanges();
    sel.addRange(r);
  }

  // expandFontAtCaret/collapse*TemplateOutside 仅模块内部调用（updateInlineSourceReveal 路径），不进 ctx
  return {
    updateInlineSourceReveal,
    expandLinkAtCaretInUrl,
  };
}
