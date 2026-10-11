import { ref, computed, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import {
  SLASH_ITEMS,
  SLASH_TEXT_GROUPS,
  CALLOUT_TYPES,
  FONT_COLORS,
  FONT_SIZES,
  applySlashCommand,
  placeCursorAtEnd,
  rangeBeforePoint,
  textBeforePoint,
} from '../utils/wysiwyg.js';
import {
  buildFontSpan,
  buildFontTemplate,
  placeCaretInFontTemplate,
  caretFontSpan,
  mergeStyleIntoSpanWithMarkSync,
} from '../utils/fontStyle.js';
import { flipMenuTop, caretAnchorOutsidePre } from '../utils/menuPos.js';

// 斜杠命令菜单（/ 召唤 Markdown 语法菜单，任意位置触发）+ 字体（行内色板 + RGB 输入 + 字号行）
//
// ctx：Editor.vue 共享上下文（currentEditable/editingId/blocks/content/syncing 为 ref，
// suppressBlurCommit/pendingTableFocus/editableEl 经访问器共享；
// appendBlocksAtEnd/reparseRegion/publishBlocks/updateLangMenu 运行时解析）。
export function useSlashMenu(ctx) {
  const slashOpen = ref(false);
  const slashQuery = ref('');
  const slashIndex = ref(0);
  // 文本行当前徽章行列（三行：行内格式 / 标题 / 列表；←/→ 移列、↑/↓ 移行、悬停徽章同步）
  const slashTextRow = ref(0);
  const slashTextCol = ref(0);
  // 表格行行列数量与当前调节字段（'item' 表格项本身 / 'rows' 行数 / 'cols' 列数；
  // 'item' 时 ↑/↓ 为菜单导航，←/→ 在 项→行→列 间循环，进入字段后 ↑/↓ 增减）
  const slashTableRows = ref(2);
  const slashTableCols = ref(2);
  const slashTableField = ref('item');
  // 警告框当前类型（CALLOUT_TYPES 下标；警告框菜单项 ←/→ 切换、Enter 应用、悬停徽章同步）
  const slashCalloutType = ref(0);
  const slashPos = ref({ left: 0, top: 0 });

  // ---------- 字体（/ 菜单「字体」行内色板 + RGB 输入 + 字号行） ----------
  // 色板/字号当前行列（row 0=颜色、1=字号；←/→ 移列、↑/↓ 换行、悬停同步）
  const slashFontRow = ref(0);
  const slashFontColorIndex = ref(0);
  const slashFontSizeIndex = ref(0);
  // RGB 输入非法值标记（菜单内输入框红框提示）
  const rgbError = ref(false);

  // 应用字体样式（{ color } 或 { fontSize }）。支持多选：首次删除 / 触发文本并插入样式 span，
  // 菜单保持打开，再次选择直接在当前样式 span 上按字段合并（颜色与字号可叠加）
  let fontMenuSession = false;
  watch(slashOpen, (open) => {
    if (!open) fontMenuSession = false;
  });

  function applyFontStyle(style) {
    const el = ctx.currentEditable();
    if (!el) return;
    let midBlockSlash = false; // / 在块中触发（触发段前有正文）：仅插入模板，不整段套用
    ctx.suppressBlurCommit = true;
    try {
      if (fontMenuSession) {
        // 多选会话：直接在当前样式 span 上合并（不重复删除触发符/重建结构）
        const existing = caretFontSpan(el);
        if (existing) {
          mergeStyleIntoSpanWithMarkSync(existing, style);
          slashOpen.value = true; // 菜单保持打开，可继续选择
          return;
        }
        // 光标不在样式 span 内（如整段套用后移出）：按新样式插入
      } else {
        // 首次：删除触发文本——光标在编辑器内时只删 /query 触发段（块中触发保留前文）；
        // 焦点在菜单输入框（RGB）时按块首的 /query 删除
        const sel = window.getSelection();
        if (sel.rangeCount && el.contains(sel.anchorNode)) {
          const caret = sel.getRangeAt(0);
          const slashIdx = textBeforePoint(el, caret.startContainer, caret.startOffset).lastIndexOf('/');
          const point = slashIdx > 0 ? textOffsetPoint(el, slashIdx) : null;
          const trigger = document.createRange();
          if (point) trigger.setStart(point.node, point.offset);
          else trigger.selectNodeContents(el);
          trigger.setEnd(caret.startContainer, caret.startOffset);
          trigger.deleteContents();
          midBlockSlash = slashIdx > 0;
        } else {
          deleteSlashTriggerAtBlockStart(el);
        }
      }
      const span = buildFontSpan(style);
      const sel2 = window.getSelection();
      // 目标容器：块的首个块级子元素（p/h1-h6 等）——样式 span 只能包块「内容」，
      // 把块级元素本身包进内联 span 是非法嵌套（浏览器会拆散，样式丢失、光标异常）
      const target = el.firstElementChild || el;
      if (sel2.rangeCount && !sel2.isCollapsed && el.contains(sel2.anchorNode)) {
        // 有选区：选区内容上色
        const range = sel2.getRangeAt(0);
        span.append(range.extractContents());
        range.insertNode(span);
        placeCursorAtEnd(span);
      } else if (midBlockSlash) {
        // 块中触发：光标处插入字体模板（前文保留，不整段套用）
        const { frag, zwsp } = buildFontTemplate(style);
        const r = sel2.rangeCount && el.contains(sel2.anchorNode) ? sel2.getRangeAt(0) : null;
        if (r) r.insertNode(frag);
        else target.append(frag);
        placeCaretInFontTemplate(zwsp);
      } else if (target.textContent.trim()) {
        // 块内已有文字：整段套用
        while (target.firstChild) span.append(target.firstChild);
        target.append(span);
        placeCursorAtEnd(span);
      } else {
        // 空块：插入字体模板（淡灰 <font> 标签 + 样式 span），光标在标签内文字位，
        // 输入即带选择的颜色/字号；Esc 退出该样式（光标移到模板之后）
        target.innerHTML = '';
        const { frag, zwsp } = buildFontTemplate(style);
        target.append(frag);
        placeCaretInFontTemplate(zwsp);
      }
      fontMenuSession = true;
      slashOpen.value = true; // 菜单保持打开，颜色与字号可叠加选择
    } finally {
      ctx.suppressBlurCommit = false;
    }
    if (document.activeElement !== el) el.focus({ preventScroll: true });
  }

  function applyFontColor(color) {
    applyFontStyle({ color });
  }
  function applyFontSize(fontSize) {
    applyFontStyle({ fontSize });
  }

  // RGB 输入应用：Rust 解析色值，有效则应用并回编辑器输入（带样式）；
  // 颜色值错误：按默认普通文本处理——清掉 / 触发文本，焦点回编辑器继续普通输入
  async function applyRgbInput(text) {
    const color = await invoke('parse_html_color', { text: (text || '').trim() }).catch(() => null);
    rgbError.value = false;
    if (!color) {
      slashOpen.value = false;
      if (fontMenuSession) {
        // 已应用过样式（多选会话）：不删除任何内容，直接回编辑器
        ctx.currentEditable()?.focus({ preventScroll: true });
      } else {
        clearSlashTriggerAndFocus();
      }
      return;
    }
    applyFontColor(color);
  }

  // 焦点在菜单输入框（RGB）时按块首 /query 删除触发文本
  function deleteSlashTriggerAtBlockStart(el) {
    const query = '/' + slashQuery.value;
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    const firstText = walker.nextNode();
    if (firstText && query.length > 1 && firstText.textContent.startsWith(query)) {
      firstText.textContent = firstText.textContent.slice(query.length);
    } else if (firstText?.textContent.startsWith('/')) {
      firstText.textContent = firstText.textContent.slice(1);
    }
  }
  // 无效 RGB 输入的回落：清掉 / 触发文本并聚焦编辑器（普通文本输入）
  function clearSlashTriggerAndFocus() {
    const el = ctx.currentEditable();
    if (!el) return;
    ctx.suppressBlurCommit = true;
    try {
      const sel = window.getSelection();
      if (sel.rangeCount && el.contains(sel.anchorNode)) {
        const caret = sel.getRangeAt(0);
        rangeBeforePoint(el, caret.startContainer, caret.startOffset).deleteContents();
      } else {
        deleteSlashTriggerAtBlockStart(el);
      }
    } finally {
      ctx.suppressBlurCommit = false;
    }
    el.focus({ preventScroll: true });
  }

  // 菜单内 RGB 输入框获得焦点：编辑容器 blur 的提交需抑制（/ 触发文本保持未提交态）
  function onRgbFocus() {
    ctx.suppressBlurCommit = true;
  }
  // RGB 输入框 Esc：关闭菜单并恢复编辑焦点与提交通路
  function onRgbCancel() {
    slashOpen.value = false;
    ctx.suppressBlurCommit = false;
    ctx.currentEditable()?.focus({ preventScroll: true });
  }

  // RGB 输入框内 ↑/↓：↑ 回字号区下行（列保持），↓ 进到下一菜单项
  function onRgbNav(dir) {
    if (dir < 0) {
      slashFontRow.value = 1;
      if (slashFontSizeIndex.value < 5) slashFontSizeIndex.value += 5;
    } else {
      slashFontRow.value = 0;
      slashIndex.value = (slashIndex.value + 1) % slashItems.value.length;
    }
    ctx.suppressBlurCommit = false;
    ctx.currentEditable()?.focus({ preventScroll: true });
  }

  // 按查询串过滤菜单项（匹配中文名 / 拼音与英文别名 / id）
  const slashItems = computed(() => {
    const q = slashQuery.value.trim().toLowerCase();
    if (!q) return SLASH_ITEMS;
    return SLASH_ITEMS.filter(
      (item) =>
        item.label.toLowerCase().includes(q) || item.keywords.includes(q) || item.id.toLowerCase().includes(q),
    );
  });

  // 编辑容器内光标前的纯文本（Range 探针；slashContext/fenceContext 等共用）
  function textBeforeCaretIn(el) {
    const sel = window.getSelection();
    if (!sel.rangeCount || !sel.isCollapsed) return null;
    const caret = sel.getRangeAt(0);
    if (!el.contains(caret.startContainer)) return null;
    return { text: textBeforePoint(el, caret.startContainer, caret.startOffset), caret };
  }

  // 光标处的斜杠上下文：光标前文本中最后一个 / 位于块首或空白/换行/哨兵字符之后时，
  // 返回查询串与菜单位置（避免路径、URL、分数等误触发）；代码/原文 pre 编辑内不触发
  function slashContext() {
    const el = ctx.currentEditable();
    if (!el || ctx.editingId.value === null) return null;
    const probed = textBeforeCaretIn(el);
    if (!probed) return null;
    const { text, caret } = probed;
    if (!caretAnchorOutsidePre(caret)) return null;
    const idx = text.lastIndexOf('/');
    if (idx < 0) return null;
    if (idx > 0 && !/[\s\u00A0\uFEFF]/.test(text[idx - 1])) return null;
    const rect = caret.getBoundingClientRect();
    // 视口底部空间不足时向上翻（菜单最大高度 264 + 间隙）；文本行徽章较宽，左侧防溢出
    const top = flipMenuTop(rect, 270);
    const left = Math.max(8, Math.min(rect.left, window.innerWidth - 380));
    return { query: text.slice(idx + 1), pos: { left, top } };
  }

  // 输入后刷新菜单：触发文本被删改（不再是 / 开头、含空格/换行）则关闭
  function updateSlashMenu() {
    const c = slashContext();
    if (!c || c.query.includes('\n') || c.query.includes(' ')) {
      slashOpen.value = false;
      return;
    }
    slashQuery.value = c.query;
    if (slashIndex.value >= slashItems.value.length) slashIndex.value = 0;
    slashTextRow.value = 0;
    slashTextCol.value = 0;
    slashTableField.value = 'item';
    slashCalloutType.value = 0;
    slashFontRow.value = 0;
    slashFontColorIndex.value = 0;
    slashFontSizeIndex.value = 0;
    rgbError.value = false;
    slashPos.value = c.pos;
    slashOpen.value = true;
  }

  async function applySlashItem(item, option) {
    const el = ctx.currentEditable();
    if (!el || !item) return;
    slashOpen.value = false;
    // 文本行按当前/点选徽章应用；表格带行列数量；其余项直接应用
    let id = item.id;
    let opts;
    if (id === 'text') {
      const cell = option ?? { row: slashTextRow.value, col: slashTextCol.value };
      id = SLASH_TEXT_GROUPS[cell.row][cell.col].id;
    } else if (id === 'table') opts = { rows: slashTableRows.value, cols: slashTableCols.value };
    else if (id === 'callout') {
      // 警告框类型：徽章点选带类型，Enter 应用当前类型（←/→ 或悬停切换）
      opts = { variant: option?.calloutVariant ?? CALLOUT_TYPES[slashCalloutType.value].id };
    }
    // 字体：色板/字号点选带样式，Enter 应用当前行列（←/→ 区内循环、↑/↓ 网格移动）
    if (item.id === 'fontColor') {
      if (option?.fontColor) applyFontColor(option.fontColor);
      else if (option?.fontSize) applyFontSize(option.fontSize);
      else if (slashFontRow.value === 1) applyFontSize(FONT_SIZES[slashFontSizeIndex.value].fontSize);
      else if (slashFontRow.value === 2) return; // RGB 输入框行：Enter 由输入框自身处理
      else applyFontColor(FONT_COLORS[slashFontColorIndex.value].color);
      return;
    }
    // 表格：模板即时提交渲染，不进入原文编辑（随后聚焦首个数据单元格）
    if (id === 'table') {
      await applyTableTemplate(opts);
      return;
    }
    // 任意位置 / 触发：先把 /query 触发段前后的内容摘出（应用时容器会被清空重建），
    // 应用完成后放回原处——前文不丢
    const frags = extractSlashFrags(el);
    // 替换 DOM 期间抑制 blur 误提交：清空聚焦中的容器可能触发同步 blur，
    // 不抑制则 onEditableBlur 抢先提交空内容（块被删，新内容插到已卸载节点上）
    ctx.suppressBlurCommit = true;
    try {
      await applySlashCommand(el, id, opts);
    } finally {
      ctx.suppressBlurCommit = false;
    }
    if (frags) spliceSlashFrags(el, frags, id);
    // 替换导致失焦时恢复焦点（光标已由构建函数放置）
    if (document.activeElement !== el) el.focus({ preventScroll: true });
    // 代码块：围栏已插入（```），立即弹出语言补全，先编辑语言
    if (id === 'codeBlock') ctx.updateLangMenu();
  }

  // 摘出 /query 触发段的前后内容（任意位置 / 触发时调用；/ 在块首返回 null——无前文需保留）。
  // 返回 DocumentFragment 对（before/after），触发段本身被移除
  // 容器内纯文本偏移 → DOM 点（{node, offset}）
  function textOffsetPoint(root, target) {
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    let acc = 0;
    let cur = walker.nextNode();
    while (cur) {
      const len = cur.textContent.length;
      if (target <= acc + len) return { node: cur, offset: target - acc };
      acc += len;
      cur = walker.nextNode();
    }
    return null;
  }

  function extractSlashFrags(el) {
    const probed = textBeforeCaretIn(el);
    if (!probed) return null;
    const { text, caret } = probed;
    const slashIdx = text.lastIndexOf('/');
    if (slashIdx <= 0) return null;
    // 文本偏移 → DOM 点
    const point = textOffsetPoint(el, slashIdx);
    if (!point) return null;
    const beforeFrag = rangeBeforePoint(el, point.node, point.offset).extractContents();
    const after = document.createRange();
    after.selectNodeContents(el);
    after.setStart(caret.startContainer, caret.startOffset);
    const afterFrag = after.extractContents();
    return { before: beforeFrag, after: afterFrag };
  }

  // 应用完成后把摘除的前后内容放回：模板类（data-raw pre/p）放回容器首尾（不混入模板原文），
  // 行内格式放回样式元素两侧，块级转换放回新结构文本宿主的首尾（光标落末尾）

  // 摘录片段 → 行内内容：extractContents 从容器起点摘录会克隆部分选中的块级外壳
  //（p/h1-h6/li/ul/…），直接插进行内上下文会形成块套块——样式元素（及 md-src 源标记）
  // 被挤到下一行。递归展开块级克隆，剔除 contenteditable=false 的交互控件
  //（任务勾选框/callout 标签等），相邻块内容之间补 <br> 保住换行
  function slashFragInline(frag) {
    const BLOCK_SHELLS = new Set([
      'P', 'H1', 'H2', 'H3', 'H4', 'H5', 'H6',
      'LI', 'UL', 'OL', 'BLOCKQUOTE', 'DIV', 'PRE', 'TABLE', 'SECTION',
    ]);
    const out = document.createDocumentFragment();
    const walk = (node) => {
      for (const child of [...node.childNodes]) {
        if (child.nodeType !== Node.ELEMENT_NODE) {
          out.append(child);
          continue;
        }
        if (child.getAttribute('contenteditable') === 'false') continue;
        if (BLOCK_SHELLS.has(child.tagName)) {
          // 与已有内容之间补换行（首块、或前导已是 <br> 时不补）
          if (out.childNodes.length && out.lastChild.nodeName !== 'BR') {
            out.append(document.createElement('br'));
          }
          walk(child);
        } else {
          out.append(child);
        }
      }
    };
    walk(frag);
    return out;
  }

  function spliceSlashFrags(el, frags, id) {
    const TEMPLATE_IDS = new Set([
      'link', 'image', 'inlineMath', 'mathBlock', 'mermaidBlock', 'callout', 'sectionBlock',
      'codeBlock', 'footnoteDef', 'linkRef', 'toc',
    ]);
    if (TEMPLATE_IDS.has(id)) {
      el.prepend(frags.before);
      el.append(frags.after);
      return;
    }
    const INLINE_IDS = new Set(['bold', 'italic', 'underline', 'strikethrough', 'highlight', 'inlineCode']);
    if (INLINE_IDS.has(id)) {
      const wrap = el.querySelector('strong, em, u, s, mark, code');
      if (wrap) {
        // 片段含块级外壳克隆，须展开为行内内容，否则样式元素被挤到下一行
        wrap.parentNode.insertBefore(slashFragInline(frags.before), wrap);
        wrap.parentNode.insertBefore(slashFragInline(frags.after), wrap.nextSibling);
      }
      return;
    }
    const host = el.querySelector('p, h1, h2, h3, h4, h5, h6, li');
    if (!host) return;
    // 空结构占位 br 先移除（避免夹在前后文之间产生换行）
    if (host.childNodes.length === 1 && host.firstChild.tagName === 'BR') host.innerHTML = '';
    host.prepend(slashFragInline(frags.before));
    host.append(slashFragInline(frags.after));
    placeCursorAtEnd(host);
  }

  // 斜杠表格：取 Rust block_template 模板后立即提交渲染为表格块，并进入首个数据单元格编辑
  async function applyTableTemplate(opts) {
    let tpl = null;
    try {
      tpl = await invoke('block_template', { kind: 'table', rows: opts?.rows, cols: opts?.cols });
    } catch (e) {
      console.error('block_template 调用失败:', e);
      return;
    }
    if (!tpl) return;
    ctx.syncing.value = true;
    try {
      // 聚焦新表格块的首个数据单元格（reparse 后旧 id 已失效，取替换位首块）
      const isAppend = ctx.editingId.value === '__append__';
      let tableBlock = null;
      if (isAppend) {
        const anchored = await ctx.appendBlocksAtEnd(tpl.markdown);
        if (!anchored) return; // 解析失败：全文重解析自愈（appendBlocksAtEnd 内处理）
        ctx.publishBlocks();
        tableBlock = anchored[0] ?? null;
      } else {
        const index = ctx.blocks.value.findIndex((b) => b.id === ctx.editingId.value);
        const block = ctx.blocks.value[index];
        if (block) {
          ctx.content.value = ctx.content.value.slice(0, block.start) + tpl.markdown + ctx.content.value.slice(block.end);
          await ctx.reparseRegion(index, block, tpl.markdown);
          tableBlock = ctx.blocks.value[index] ?? null;
        }
      }
      ctx.editableEl = null;
      ctx.suppressBlurCommit = true;
      ctx.pendingTableFocus = true;
      ctx.editingId.value = tableBlock ? tableBlock.id : null;
      if (!tableBlock) ctx.suppressBlurCommit = false;
    } finally {
      ctx.syncing.value = false;
    }
  }

  // 斜杠菜单打开时的键盘优先路由：上下选择、Enter 应用、Esc 关闭。
  // 已处理返回 true（调用方结束本次按键分发）；←/→ 未命中特定项类型时不拦截（返回 false 继续分发）
  function routeSlashKeys(e) {
    if (!slashOpen.value) return false;
    const items = slashItems.value;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const item = items[slashIndex.value];
      // 表格行且已进入行/列字段：↑/↓ 增减数量；字段为表格项本身时照常菜单导航
      if (item?.id === 'table' && slashTableField.value !== 'item') {
        const delta = e.key === 'ArrowUp' ? 1 : -1;
        if (slashTableField.value === 'rows') {
          slashTableRows.value = Math.min(50, Math.max(1, slashTableRows.value + delta));
        } else {
          slashTableCols.value = Math.min(20, Math.max(1, slashTableCols.value + delta));
        }
        return true;
      }
      // 文本行：↑/↓ 在徽章子行间移动（行内格式→标题→列表），边缘行再进出菜单导航
      if (item?.id === 'text') {
        const lastRow = SLASH_TEXT_GROUPS.length - 1;
        const down = e.key === 'ArrowDown';
        if (down && slashTextRow.value < lastRow) {
          slashTextRow.value += 1;
        } else if (!down && slashTextRow.value > 0) {
          slashTextRow.value -= 1;
        } else {
          slashIndex.value = down
            ? (slashIndex.value + 1) % items.length
            : (slashIndex.value - 1 + items.length) % items.length;
        }
        slashTextCol.value = Math.min(slashTextCol.value, SLASH_TEXT_GROUPS[slashTextRow.value].length - 1);
        return true;
      }
      // 字体行：↑/↓ 网格内上下移动（色板 6 列两行、字号 5 列两行），边界换区/回菜单导航
      if (item?.id === 'fontColor') {
        const down = e.key === 'ArrowDown';
        if (slashFontRow.value === 0) {
          // 色板区（12 格，6 列两行）：↓ 上行→下行，下行→字号区；↑ 下行→上行，上行→菜单上一项
          const idx = slashFontColorIndex.value;
          if (down) {
            if (idx < 6) slashFontColorIndex.value = idx + 6;
            else {
              slashFontRow.value = 1;
              slashFontSizeIndex.value = Math.min(idx - 6, 4);
            }
          } else if (idx >= 6) {
            slashFontColorIndex.value = idx - 6;
          } else {
            slashIndex.value = (slashIndex.value - 1 + items.length) % items.length;
          }
        } else {
          // 字号区（10 格，5 列两行）：↑ 下行→上行，上行→色板下行；↓ 上行→下行，下行→RGB 输入框
          const idx = slashFontSizeIndex.value;
          if (down) {
            if (idx < 5) slashFontSizeIndex.value = idx + 5;
            else slashFontRow.value = 2;
          } else if (idx >= 5) {
            slashFontSizeIndex.value = idx - 5;
          } else {
            slashFontRow.value = 0;
            slashFontColorIndex.value = 6 + Math.min(idx, 5);
          }
        }
        return true;
      }
      if (items.length) {
        // 循环导航（到底/到顶回绕），滚动跟随由菜单面板负责
        slashIndex.value =
          e.key === 'ArrowDown'
            ? (slashIndex.value + 1) % items.length
            : (slashIndex.value - 1 + items.length) % items.length;
      }
      return true;
    }
    // 行内 ←/→：文本行移动徽章列 / 表格在 项→行→列 字段间循环
    if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
      const item = items[slashIndex.value];
      if (item?.id === 'text') {
        e.preventDefault();
        const cols = SLASH_TEXT_GROUPS[slashTextRow.value].length;
        slashTextCol.value =
          e.key === 'ArrowRight'
            ? Math.min(cols - 1, slashTextCol.value + 1)
            : Math.max(0, slashTextCol.value - 1);
        return true;
      }
      if (item?.id === 'table') {
        e.preventDefault();
        const fields = ['item', 'rows', 'cols'];
        const step = e.key === 'ArrowRight' ? 1 : -1;
        const next = (fields.indexOf(slashTableField.value) + step + fields.length) % fields.length;
        slashTableField.value = fields[next];
        return true;
      }
      // 警告框行：←/→ 循环切换类型（Enter 应用当前类型）
      if (item?.id === 'callout') {
        e.preventDefault();
        const step = e.key === 'ArrowRight' ? 1 : -1;
        slashCalloutType.value =
          (slashCalloutType.value + step + CALLOUT_TYPES.length) % CALLOUT_TYPES.length;
        return true;
      }
      // 字体行：←/→ 在当前区内循环（色板区 12 格 / 字号区 10 格；RGB 输入框聚焦时按键在框内）
      if (item?.id === 'fontColor') {
        e.preventDefault();
        const step = e.key === 'ArrowRight' ? 1 : -1;
        if (slashFontRow.value === 1) {
          slashFontSizeIndex.value =
            (slashFontSizeIndex.value + step + FONT_SIZES.length) % FONT_SIZES.length;
        } else if (slashFontRow.value === 0) {
          slashFontColorIndex.value =
            (slashFontColorIndex.value + step + FONT_COLORS.length) % FONT_COLORS.length;
        }
        return true;
      }
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      applySlashItem(items[slashIndex.value]);
      return true;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      slashOpen.value = false;
      return true;
    }
    return false;
  }

  return {
    // 状态（模板绑定）
    slashOpen, slashIndex, slashPos, slashItems, slashTextRow, slashTextCol,
    slashTableRows, slashTableCols, slashTableField, slashCalloutType,
    slashFontRow, slashFontColorIndex, slashFontSizeIndex, rgbError,
    // 菜单与字体动作（模板 / 输入链路）
    updateSlashMenu, applySlashItem, applyRgbInput, onRgbFocus, onRgbCancel, onRgbNav,
    // 键盘优先路由（onEditableKeydown）
    routeSlashKeys,
    // 共享：编辑容器内光标前文本探针（撤销历史/围栏补全共用）+ 斜杠表格模板（右键插入表格共用）
    textBeforeCaretIn, applyTableTemplate,
  };
}
