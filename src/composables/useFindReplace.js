import { ref, onBeforeUnmount } from 'vue';

// 文档内查找/替换（Ctrl+F 查找 / Ctrl+H 替换；FindBar 浮动条）
// 匹配在正文源码上做（大小写不敏感），滚动+闪烁定位到所在块；替换走 content → reparse，
// 历史 watch 自动记录（撤销/重做可回退替换）；
// 已挂载内容经 CSS Custom Highlight API 高亮全部匹配 + 当前匹配（不改 DOM）
//
// ctx：Editor.vue 共享上下文（content/blocks/scrollRoot/editingId/flashId/
// reparse/scrollToBlock 与 editableEl 访问器）。
export function useFindReplace(ctx) {
  const findVisible = ref(false);
  const findWithReplace = ref(false);
  const findMatch = ref({ index: 0, total: 0 });
  let findMatches = []; // 匹配起点偏移（升序）
  let findIndex = 0;
  let lastKeyword = '';
  // 当前匹配定位：findIndex 是源码 Markdown 中的匹配序号，与渲染 DOM 中的 Range 序号
  // 并不一致（源码含语法字符、行未挂载、标记节点被过滤），直接用同一序号索引会指错匹配。
  // 改用「目标块 id + 块内第几处匹配」定位当前高亮（块内源码匹配与 DOM 匹配一一对应，
  // 命中语法字符的极少数情况按块内末处钳制）。
  let findCurrentBlockId = null;
  let findCurrentOrdinalInBlock = 0;

  // 打开查找/替换浮动条（菜单入口；withReplace 决定是否展开替换行）
  function openFind(withReplace = false) {
    findWithReplace.value = withReplace;
    findVisible.value = true;
  }

  // 关闭查找条：清掉高亮组
  function closeFind() {
    findVisible.value = false;
    clearTimeout(findHlTimer);
    clearTimeout(findSearchTimer);
    CSS.highlights.delete('find-all');
    CSS.highlights.delete('find-current');
  }

  // 查找高亮：已挂载行的文本节点逐段建 Range，注册 find-all/find-current 两个高亮组
  function applyFindHighlights() {
    CSS.highlights.delete('find-all');
    CSS.highlights.delete('find-current');
    if (!lastKeyword || !findVisible.value) return;
    const root = ctx.scrollRoot.value;
    if (!root) return;
    const kw = lastKeyword.toLowerCase();
    const all = [];
    // 目标块内的匹配单独收集，按块内序号取当前高亮
    const inCurrentBlock = [];
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, (n) =>
      n.parentElement?.closest('input, [contenteditable="true"], .md-code-lang-badge, .md-callout-label, .md-link-mark, .md-font-mark')
        ? NodeFilter.FILTER_REJECT
        : NodeFilter.FILTER_ACCEPT);
    let node = walker.nextNode();
    while (node) {
      const inTarget = node.parentElement?.closest('[data-block-id]')?.getAttribute('data-block-id') === findCurrentBlockId;
      const text = node.textContent.toLowerCase();
      let i = text.indexOf(kw);
      while (i >= 0) {
        const r = new Range();
        r.setStart(node, i);
        r.setEnd(node, i + kw.length);
        all.push(r);
        if (inTarget) inCurrentBlock.push(r);
        i = text.indexOf(kw, i + kw.length);
      }
      node = walker.nextNode();
    }
    if (all.length) CSS.highlights.set('find-all', new Highlight(...all));
    const cur = inCurrentBlock.length
      ? inCurrentBlock[Math.min(findCurrentOrdinalInBlock, inCurrentBlock.length - 1)]
      : null;
    if (cur) CSS.highlights.set('find-current', new Highlight(cur));
  }

  let findHlTimer = null;
  function scheduleFindHighlights() {
    clearTimeout(findHlTimer);
    findHlTimer = setTimeout(applyFindHighlights, 120);
  }

  // 查找输入防抖：每键都全量复制文档 + 扫描，大文档下输入卡顿——150ms 合帧
  //（替换后重扫（replace 链路）仍直接调 searchInDocument 保持即时）
  let findSearchTimer = null;
  function onFindSearchInput(keyword) {
    clearTimeout(findSearchTimer);
    findSearchTimer = setTimeout(() => searchInDocument(keyword), 150);
  }

  function searchInDocument(keyword) {
    lastKeyword = keyword;
    findMatches = [];
    findIndex = 0;
    findMatch.value = { index: 0, total: 0 };
    findCurrentBlockId = null;
    findCurrentOrdinalInBlock = 0;
    if (!keyword) return;
    const lower = ctx.content.value.toLowerCase();
    const kw = keyword.toLowerCase();
    let i = 0;
    while (i <= lower.length - kw.length) {
      const hit = lower.indexOf(kw, i);
      if (hit < 0) break;
      findMatches.push(hit);
      i = hit + kw.length;
    }
    if (findMatches.length) {
      findMatch.value = { index: 1, total: findMatches.length };
      scrollToMatch(0);
    } else {
      applyFindHighlights(); // 无匹配也要清掉旧高亮
    }
  }

  // 定位到第 i 个匹配（循环）：滚动到所在块并短暂闪烁
  function scrollToMatch(i) {
    if (!findMatches.length) return;
    findIndex = ((i % findMatches.length) + findMatches.length) % findMatches.length;
    findMatch.value = { index: findIndex + 1, total: findMatches.length };
    const offset = findMatches[findIndex];
    const block = ctx.blocks.value.find((b) => b.start != null && b.end != null && offset >= b.start && offset < b.end);
    // 当前高亮按「块 id + 块内第几处」定位（先于 applyFindHighlights 设置）
    findCurrentBlockId = block?.id ?? null;
    findCurrentOrdinalInBlock = 0;
    if (block) {
      findCurrentOrdinalInBlock = findMatches.reduce((n, m) => (m >= block.start && m < offset ? n + 1 : n), 0);
    }
    applyFindHighlights();
    if (block) {
      ctx.scrollToBlock(block.id);
      ctx.flashId.value = block.id;
      setTimeout(() => {
        if (ctx.flashId.value === block.id) ctx.flashId.value = null;
      }, 1200);
    }
  }

  function navFind(step) {
    scrollToMatch(findIndex + step);
  }

  // 替换当前匹配项，替换后重搜刷新计数（同一槽位即下一处匹配）
  async function replaceInDocument(text) {
    if (!findMatches.length || !lastKeyword) return;
    const offset = findMatches[findIndex];
    ctx.content.value = ctx.content.value.slice(0, offset) + text + ctx.content.value.slice(offset + lastKeyword.length);
    if (ctx.editingId.value !== null) {
      ctx.editingId.value = null;
      ctx.editableEl = null;
    }
    await ctx.reparse();
    searchInDocument(lastKeyword);
    if (findMatches.length) scrollToMatch(Math.min(findIndex, findMatches.length - 1));
  }

  // 全部替换（大小写不敏感，与搜索一致；正则转义后全局替换）
  async function replaceAllInDocument(text) {
    if (!lastKeyword || !findMatches.length) return;
    const re = new RegExp(lastKeyword.replace(/[.*+?^${}()[\]\\]/g, '\\$&'), 'gi');
    ctx.content.value = ctx.content.value.replace(re, text);
    if (ctx.editingId.value !== null) {
      ctx.editingId.value = null;
      ctx.editableEl = null;
    }
    await ctx.reparse();
    searchInDocument(lastKeyword);
  }

  // 组件卸载时清理定时器（多标签/多窗口复用防泄漏）
  onBeforeUnmount(() => {
    clearTimeout(findHlTimer);
    clearTimeout(findSearchTimer);
  });

  return {
    findVisible,
    findWithReplace,
    findMatch,
    openFind,
    closeFind,
    onFindSearchInput,
    navFind,
    replaceInDocument,
    replaceAllInDocument,
    scheduleFindHighlights,
    // 输入/滚动链路的高亮刷新闸门（lastKeyword 不直接外露）
    hasKeyword: () => !!lastKeyword,
  };
}
