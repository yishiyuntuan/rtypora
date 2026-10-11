import { ref, computed } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import {
  LANG_BADGES,
  placeCursorAtStart,
  placeCaretAtTextOffset,
  textBeforePoint,
  markSpanFactory,
} from '../utils/wysiwyg.js';
import { flipMenuTop, caretAnchorOutsidePre } from '../utils/menuPos.js';

// 围栏语言自动补全（``` 后弹出语言菜单，含 mermaid/math）
// 与图片 () 内 ./ 路径补全共用 SlashMenu 面板（两套补全互斥触发）。
//
// ctx：Editor.vue 共享上下文（currentEditable/editingId/textBeforeCaretIn/
// scheduleCodeHighlight/documentDir，运行时解析）。
// 注意：函数内「补全上下文」局部变量命名为 c，避免遮蔽共享上下文参数 ctx。
export function useCompletion(ctx) {
  const langOpen = ref(false);
  const langQuery = ref('');
  const langIndex = ref(0);
  const langPos = ref({ left: 0, top: 0 });
  // 语言清单（Rust code_languages：mermaid/math + tree-sitter 高亮语言），首次打开时拉取
  const langList = ref([]);

  // 前缀匹配过滤（清单由 Rust code_languages 按字典序返回）
  const langItems = computed(() => {
    const q = langQuery.value.trim().toLowerCase();
    if (!q) return langList.value;
    return langList.value.filter((lang) => lang.toLowerCase().startsWith(q));
  });

  // 菜单展示项（每语言专属缩写徽章 + 品牌色）
  const langMenuItems = computed(() =>
    langItems.value.map((lang) => {
      const badge = LANG_BADGES[lang] || { text: lang.slice(0, 2), color: 'currentColor' };
      return { id: lang, label: lang, icon: `<span class="md-lang-badge" style="color:${badge.color}">${badge.text}</span>` };
    }),
  );

  // 光标前文本为 ```/~~~ 加部分语言名（无空格无换行）时返回补全上下文；否则 null
  function fenceContext() {
    const el = ctx.currentEditable();
    if (!el || ctx.editingId.value === null) return null;
    const probed = ctx.textBeforeCaretIn(el);
    if (!probed) return null;
    const { text, caret } = probed;
    if (!caretAnchorOutsidePre(caret)) return null;
    const match = /^(`{3,}|~{3,})([a-zA-Z0-9#+_-]*)$/.exec(text);
    if (!match) return null;
    const rect = caret.getBoundingClientRect();
    return { fence: match[1], query: match[2], pos: { left: rect.left, top: flipMenuTop(rect, 270) } };
  }

  async function updateLangMenu() {
    const c = fenceContext();
    if (!c) {
      langOpen.value = false;
      return;
    }
    if (!langList.value.length) {
      langList.value = await invoke('code_languages').catch(() => []);
    }
    langMenuInput = null;
    langQuery.value = c.query;
    if (langIndex.value >= langItems.value.length) langIndex.value = 0;
    langPos.value = c.pos;
    langOpen.value = true;
  }

  // 语言输入框（代码块右上角）补全：与围栏补全共用菜单；
  // langMenuInput 非空表示菜单来自输入框（应用时写回输入框而非围栏文本）
  let langMenuInput = null;
  async function updateLangInputMenu(input) {
    if (!langList.value.length) {
      langList.value = await invoke('code_languages').catch(() => []);
    }
    langMenuInput = input;
    langQuery.value = input.value.trim();
    if (langIndex.value >= langItems.value.length) langIndex.value = 0;
    const rect = input.getBoundingClientRect();
    langPos.value = {
      // 右缘钳制（语言框在代码块右上角，菜单默认向左伸出会溢出视口）
      left: Math.max(8, Math.min(rect.left, window.innerWidth - 340)),
      top: flipMenuTop(rect, 270),
    };
    langOpen.value = true;
  }

  // 应用语言补全：围栏后的查询文本替换为所选语言，光标落在语言之后
  //（此时仍是普通文本行，继续输入或 Enter 经既有流程转为代码块）；
  // 来自语言输入框时：写回输入框并同步到 pre，焦点回代码区开头
  function applyLangItem(lang) {
    if (langMenuInput) {
      const input = langMenuInput;
      langMenuInput = null;
      langOpen.value = false;
      if (!lang || !input.isConnected) return;
      input.value = lang;
      const pre = input.closest('pre');
      pre?.setAttribute('data-language', lang);
      ctx.scheduleCodeHighlight(); // 语言补全应用后重排高亮
      const code = pre?.querySelector('code');
      if (code) placeCursorAtStart(code);
      pre?.closest('[contenteditable]')?.focus({ preventScroll: true });
      return;
    }
    const el = ctx.currentEditable();
    const c = fenceContext();
    if (!el || !c || !lang) return;
    langOpen.value = false;
    const after = el.textContent.slice((c.fence + c.query).length);
    const p = el.querySelector('p') || el;
    p.textContent = c.fence + lang + after;
    placeCaretAtTextOffset(p, (c.fence + lang).length);
  }

  // ---------- 图片 () 内 ./ 路径补全（与语言补全共用 SlashMenu 面板） ----------
  const pathOpen = ref(false);
  const pathIndex = ref(0);
  const pathPos = ref({ left: 0, top: 0 });
  const pathCandidates = ref([]);
  let pathCtx = null; // { node, offset, prefix }：应用时据此替换前缀文本

  // 光标位于图片 () 内的 ./ 路径中时返回补全上下文；否则 null。
  // 按「段落整体文本 + 光标偏移」判定（图片模板/图片段落再编辑均为 data-raw 单段），
  // 与光标落在哪个节点（标记 span / 文本节点）无关
  function imagePathContext() {
    const el = ctx.currentEditable();
    if (!el || ctx.editingId.value === null) return null;
    const sel = window.getSelection();
    if (!sel.rangeCount || !sel.isCollapsed) return null;
    const anchor = sel.anchorNode;
    const anchorEl = anchor.nodeType === Node.TEXT_NODE ? anchor.parentElement : anchor;
    const p = anchorEl?.closest?.('p');
    if (!p || !el.contains(p) || !p.hasAttribute('data-raw')) return null;
    // 光标在段落内的纯文本偏移（探针 range；\uFEFF 哨兵不计入）
    const before = textBeforePoint(p, anchor, sel.anchorOffset).replace(/\uFEFF/g, '');
    const m = /^!\[[^\]]*\]\((\.\/[^)\s]*)$/.exec(before);
    if (!m) return null;
    const rect = sel.getRangeAt(0).getBoundingClientRect();
    return { p, prefix: m[1], pos: { left: Math.max(8, rect.left), top: flipMenuTop(rect, 240) } };
  }

  async function updatePathMenu() {
    const c = imagePathContext();
    if (!c || !ctx.documentDir.value) {
      pathOpen.value = false;
      pathCtx = null;
      return;
    }
    const list = await invoke('complete_path', { baseDir: ctx.documentDir.value, prefix: c.prefix }).catch(() => []);
    // 等待期间光标/文本变化则放弃（下次输入再触发）
    const cur = imagePathContext();
    if (!cur || cur.p !== c.p || cur.prefix !== c.prefix) return;
    if (!list.length) {
      pathOpen.value = false;
      pathCtx = null;
      return;
    }
    pathCtx = c;
    pathCandidates.value = list;
    if (pathIndex.value >= list.length) pathIndex.value = 0;
    pathPos.value = c.pos;
    pathOpen.value = true;
  }

  // 菜单展示项：目录带 📁、图片带 🖼
  const pathMenuItems = computed(() =>
    pathCandidates.value.map((p) => ({
      id: p,
      label: p,
      icon: p.endsWith('/') ? '<span>📁</span>' : '<span>🖼</span>',
    })),
  );

  // 应用路径补全：重建图片模板段落为所选路径（保留 alt 与标记结构），光标落在路径之后；
  // 选中目录（尾随 /）时下次输入会自动重新触发补全（列出该目录内容）
  function applyPathItem(path) {
    const c = pathCtx;
    pathCtx = null;
    pathOpen.value = false;
    if (!c || !path || !c.p.isConnected) return;
    const p = c.p;
    const text = p.textContent.replace(/\uFEFF/g, '');
    const alt = /^!\[([^\]]*)\]/.exec(text)?.[1] ?? '';
    const mark = markSpanFactory('md-link-mark');
    const altSpan = document.createElement('span');
    altSpan.className = 'md-link-text';
    altSpan.textContent = alt;
    // 路径用 md-link-url span 包裹（浅蓝显示，与链接文字同色）
    const pathSpan = document.createElement('span');
    pathSpan.className = 'md-link-url';
    pathSpan.textContent = path;
    const pathNode = pathSpan.firstChild;
    p.innerHTML = '';
    p.append(mark('!'), mark('['), altSpan, mark(']'), mark('('), pathSpan, mark(')'));
    const sel = window.getSelection();
    sel.collapse(pathNode, pathNode.textContent.length);
    ctx.currentEditable()?.focus({ preventScroll: true });
    // 选中目录（尾随 /）：立即列出其内容继续补全，直至选中图片（无需再敲字符）
    if (path.endsWith('/')) updatePathMenu();
  }

  // 语言输入框按键回落（Enter/Tab/Esc 回代码区）时解除输入框模式
  function dismissLangInput() {
    langMenuInput = null;
  }

  return {
    langOpen, langIndex, langPos, langItems, langMenuItems,
    pathOpen, pathIndex, pathPos, pathCandidates, pathMenuItems,
    updateLangMenu, updateLangInputMenu, applyLangItem, dismissLangInput,
    updatePathMenu, applyPathItem,
  };
}
