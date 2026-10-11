<script setup vapor>
import { ref, computed, watch, nextTick, inject, provide, onMounted, onBeforeUnmount, markRaw } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import BlockView from './BlockView.vue';
import VRow from './VRow.vue';
import SlashMenu from './SlashMenu.vue';
import ContextMenu from './ContextMenu.vue';
import FindBar from './FindBar.vue';
import {
  blockToHtml,
  domToBlockDtos,
  emptyParagraphHtml,
  applyMarkdownShortcuts,
  convertFenceToCodeBlock,
  convertSectionToHtmlBlock,
  applyHtmlAutoclose,
  expandHtmlTagAtCaret,
  skipHtmlClosingTag,
  placeCursorAtEnd,
  placeCursorAtStart,
  placeCursorInEmptyWrapper,
  placeCaretAtTextOffset,
  insertTextAtCursor,
  insertLineBreakAtCursor,
  numberedOrdinals,
  isCaretAtStart,
  deleteTaskCheckboxBeforeCaret,
  handleListEnter,
  fenceCandidateLi,
  convertFenceInListItem,
  outdentEmptyNestedListItemToParagraph,
  outdentNestedListItemAtCaret,
  exitEmptyTopLevelListItemAtCaret,
  resumeListEnter,
  handleListTab,
  paragraphDtoFromTree,
  demoteEditableToParagraph,
  insertInlineWrapper,
  exitInlineWrapperAtCaret,
  convertCalloutToRawEdit,
  rangeBeforePoint,
  textBeforePoint,
  markSpanFactory,
  LINK_CLASS,
  plainText,
} from '../utils/wysiwyg.js';
import { highlightCodeHtml } from '../utils/highlight.js';
import { renderMermaid } from '../utils/mermaid.js';
import { renderPlantumlLocal } from '../utils/plantuml.js';
import { formatShortcut } from '../utils/platform.js';
import { getPref, setPref, prefsVersion, structureVersion } from '../utils/prefs.js';
import { shouldWrapSelection, wrapSelectionWithSymbol } from '../utils/selectionWrap.js';
import { useUndoHistory } from '../composables/useUndoHistory.js';
import { useFindReplace } from '../composables/useFindReplace.js';
import { useVirtualScroll, INITIAL_ROW_QUOTA } from '../composables/useVirtualScroll.js';
import { useCompletion } from '../composables/useCompletion.js';
import { useTableEditing } from '../composables/useTableEditing.js';
import { useSlashMenu } from '../composables/useSlashMenu.js';
import { useContextMenu } from '../composables/useContextMenu.js';
import { useOutlineScroll } from '../composables/useOutlineScroll.js';
import { useChangeReview } from '../composables/useChangeReview.js';
import { useInlineSourceReveal } from '../composables/useInlineSourceReveal.js';
import { useReadingPosition } from '../composables/useReadingPosition.js';
import { useSourceLineNumbers } from '../composables/useSourceLineNumbers.js';



// 双模式编辑器：sourceMode 为 Markdown 原文编辑，否则为所见即所得（Typora 式就地编辑）。
// 全文 Markdown 字符串是唯一数据源；块树由 Rust 命令 parse_markdown 解析得到。
// 前端只负责渲染与 DOM 结构提取：Markdown 解析、序列化、勾选切换、统计、快捷判定
// 全部由 Rust 命令完成（parse_markdown / parse_blocks / serialize_markdown /
// toggle_task_markdown / text_stats / detect_block_shortcut）。
//
// 本文件为编辑器核心：文档数据（content/blocks）、编辑提交链路（commitEdit/拆分/合并/增量重解析）、
// 编辑态事件（input/keydown/paste/blur）、列表组编辑、滚动锚点（源码/WYSIWYG 切换）、
// 文档加载/打印、选区与剪贴板、生命周期。
// 外围功能区已拆分为 src/composables/ 下的组合式函数，经共享上下文 ctx 装配（见下文「组合式函数装配」）：
//   composables/useUndoHistory.js        撤销/重做历史（快照栈，watch(content) 自动记录，1s 合并窗口，上限 200）
//   composables/useFindReplace.js        查找/替换（Ctrl+F/Ctrl+H；高亮走 CSS Custom Highlight API）
//   composables/useVirtualScroll.js      虚拟滚动与渐进挂载（VRow 占位 + IntersectionObserver，大文件秒开）
//   composables/useCompletion.js         围栏语言/图片路径补全（共用 SlashMenu 面板的两套自动补全）
//   composables/useTableEditing.js       表格编辑（就地编辑 + 悬停工具栏/四边 + 按钮/尺寸网格面板）
//   composables/useSlashMenu.js          斜杠命令菜单（任意位置触发），含字体色板与 RGB 输入
//   composables/useContextMenu.js        右键菜单（选区暂存/恢复，WKWebView 右键会塌陷选区），格式化动作
//   composables/useOutlineScroll.js      大纲联动与滚动定位（scrollToBlock/标题位置测量/滚动条悬停揭示）
//   composables/useInlineSourceReveal.js 行内源码显现（.md-src 标记层；链接/字体模板展开折叠）
// 本文件保留章节以 `// ---------- 标题 ----------` 分隔。
const props = defineProps({
  sourceMode: { type: Boolean, default: false },
});

const emit = defineEmits(['update:stats', 'update:blocks', 'update:active-heading']);

const content = ref('');
// 已保存快照：脏标记 = 当前内容与快照不一致
const savedContent = ref('');
const isDirty = computed(() => content.value !== savedContent.value);



const blocks = ref([]);
// 全部正文块（不含脚注定义；脚注集中渲染在文末）
const allBlocks = computed(() => blocks.value.filter((b) => b.type !== 'footnoteDefinition'));
const footnotes = computed(() => blocks.value.filter((b) => b.type === 'footnoteDefinition'));
// 拼写检查开关（偏好设置 spellcheck，默认关闭；prefsVersion 驱动响应）
const spellcheckOn = computed(() => {
  prefsVersion.value;
  return !!getPref('spellcheck');
});
// 根块的有序列表序号表（id -> 1 基序号；footnoteDefinition 已过滤到末尾，不中断正文列表编号）
// 无有序列表时跳过序号构建（长文档性能守卫）
const rootOrdinals = computed(() =>
  allBlocks.value.some((b) => b.type === 'numberedListItem')
    ? numberedOrdinals(allBlocks.value)
    : new Map(),
);
// 脚注引用全局序号表：按文档顺序（块顺序 + 递归子块）为每个 [^id] 分配 1 基 refIndex，
// 用于定义区返回链接与引用区锚点一一对应。项中保留 blockId 与 occurrenceIndex 以精确匹配。
// 无 [^ 的文档直接跳过整树扫描（长文档性能守卫）
const footnoteRefList = computed(() => {
  if (!content.value.includes('[^')) return [];
  const list = [];
  function scanTree(tree, blockId) {
    for (const f of tree?.fragments || []) {
      if (f.footnote) {
        list.push({
          id: f.footnote.id,
          occurrenceIndex: f.footnote.occurrenceIndex,
          blockId,
          refIndex: list.length + 1,
        });
      }
    }
  }
  function scanBlock(block) {
    scanTree(block.title, block.id);
    for (const child of block.children || []) scanBlock(child);
  }
  for (const b of allBlocks.value) scanBlock(b);
  return list;
});
provide('footnoteRefList', footnoteRefList);
provide('getFootnoteRefIndex', (id, occurrenceIndex, blockId) => {
  const idx = footnoteRefList.value.findIndex(
    (r) => r.id === id && r.blockId === blockId && r.occurrenceIndex === occurrenceIndex,
  );
  return idx >= 0 ? idx + 1 : 0;
});


// 展示公式编号表（块 id -> 1 基序号）：AMS 编号环境判定在 Rust（mathNumbered 字段），
// 编号序列按偏好 math_numbering（off/ams/all）生成（视图层），经 provide 供块渲染层取用
const mathNumbers = computed(() => {
  prefsVersion.value;
  const map = new Map();
  const mode = getPref('math_numbering');
  if (mode === 'off') return map;
  let n = 0;
  for (const b of blocks.value) {
    if (b.type !== 'mathBlock') continue;
    if (mode === 'ams' && !b.mathNumbered) continue;
    n += 1;
    map.set(b.id, n);
  }
  return map;
});
provide('mathNumbers', mathNumbers);




// 下一次挂载聚焦表格首个数据单元格（斜杠表格即时渲染）
let pendingTableFocus = false;


// 滚动定位闪烁的块 id（当前标题 id 由 useOutlineScroll 内部维护并直接 emit）
const flashId = ref(null);
// WYSIWYG 滚动容器（大纲定位用）
const scrollRoot = ref(null);
// 源码模式滚动容器
const sourceRoot = ref(null);
// 切换源码模式时保存的滚动锚点：content 字符偏移 + 元素/行距视口顶部的距离
let savedAnchorOffset = 0;
let savedTopMargin = 0;
// 切回 WYSIWYG 后需要在光标放置完成时再微调滚动，使焦点与切换前相对窗口位置一致
let restoreCaretAdjustment = null;
// 文档所在目录（粘贴图片的保存基准，App provide）
const documentDir = inject('documentDir', { value: null });
// 当前文档主名（不含扩展名）：粘贴图片「按文档名分目录」的子目录名
const currentFileStem = inject('currentFileStem', { value: null });
// 当前文件完整路径（阅读位置记忆按路径键控；null = 未保存新文档）
const currentFilePath = inject('currentFilePath', { value: null });
// 正在编辑的块 id；'__append__' 表示在文末追加新块
const editingId = ref(null);
// 当前编辑块（表格编辑工具栏显隐用）
const editingBlock = computed(() => blocks.value.find((b) => b.id === editingId.value) || null);

// ---------- 列表项组编辑 ----------
// 点击任一列表项时，连续同族兄弟项并入同一编辑容器（Typora 式整组编辑——
// 单项独立成块是模型结构，不是编辑粒度）。有序项一族（ol），无序/任务项一族
//（同为 - 标记；提取按项独立判定勾选框，混排往返不丢任务态）。
// listGroupRangeAt 返回块树下标闭区间 { i0, i1 }，非列表项/单项返回 null
const listGroupKind = (t) =>
  t === 'numberedListItem' ? 'ol' : t === 'bulletedListItem' || t === 'taskListItem' ? 'ul' : null;
function listGroupRangeAt(index) {
  const kind = listGroupKind(blocks.value[index]?.type);
  if (!kind) return null;
  let i0 = index;
  let i1 = index;
  while (i0 > 0 && listGroupKind(blocks.value[i0 - 1]?.type) === kind) i0--;
  while (i1 < blocks.value.length - 1 && listGroupKind(blocks.value[i1 + 1]?.type) === kind) i1++;
  return i0 === i1 ? null : { i0, i1 };
}
// 当前编辑的组信息（非组编辑为 null）：i0/i1 下标、宿主行 id（组首块）、其余组内行 id 集
const listEdit = computed(() => {
  if (editingId.value === null || editingId.value === '__append__') return null;
  const idx = blocks.value.findIndex((b) => b.id === editingId.value);
  const g = idx >= 0 ? listGroupRangeAt(idx) : null;
  if (!g) return null;
  const ids = blocks.value.slice(g.i0, g.i1 + 1).map((b) => b.id);
  return { ...g, hostId: ids[0], hidden: new Set(ids.slice(1)) };
});
// 模板行显隐：编辑容器宿主行（组编辑时为组首块行）与组内隐藏行
const editHostBlockId = computed(() => listEdit.value?.hostId ?? editingId.value);
const editHiddenIds = computed(() => listEdit.value?.hidden ?? null);
// 组编辑挂载时的光标映射暂存（点击点的「块内纯文本偏移」，见 startEdit）
let pendingGroupCaret = null; // { id, offset }
// 首行缩进开关（偏好设置）：WYSIWYG 容器据此挂载 .md-first-indent 类
const firstLineIndent = computed(() => {
  prefsVersion.value;
  return !!getPref('first_line_indent');
});
// 滚动条自动隐藏开关（偏好设置）：滚动容器据此挂载 .sb-auto-hide 类
const scrollbarAutoHide = computed(() => {
  prefsVersion.value;
  return !!getPref('scrollbar_auto_hide');
});

// 覆盖层滚动条（本 WebView 不渲染 ::-webkit-scrollbar 与 scrollbar-color 样式，
// 原生条用 scrollbar-width:none 隐藏，自绘彩色可拖动滑轨替代；无系统箭头）
import { useOverlayScrollbar } from '../utils/scrollbar.js';

const {
  dragging: sbDragging,
  thumb: sbThumb,
  show: sbShow,
  update: updateSbThumb,
  onScroll: sbOnScroll,
  onMouseMove: sbOnMouseMove,
  onMouseLeave: sbOnMouseLeave,
  onThumbPointerDown: sbOnThumbPointerDown,
  onTrackPointerDown: sbOnTrackPointerDown,
} = useOverlayScrollbar(() => scrollRoot.value, scrollbarAutoHide);
// 换块/提交时关闭斜杠命令菜单与语言补全菜单、表格弹出层、右键菜单
watch(editingId, () => {
  slashOpen.value = false;
  langOpen.value = false;
  tablePanel.value = null;
  tableCaretAlign.value = '';
  ctxMenu.value = null;
});
// 提交/重解析进行中时忽略点击，避免用过期的块区间切片
const syncing = ref(false);
const wordCount = ref(0);
const charCount = ref(0);
const lineCount = ref(1);

const cursorLine = ref(1);
const cursorColumn = ref(1);
let parseSeq = 0;
// 当前编辑容器元素
let editableEl = null;

// 当前编辑容器（引用丢失时从 DOM 恢复，保证提交/拆分链路不被静默早退）
function currentEditable() {
  if (editableEl && editableEl.isConnected) return editableEl;
  const el = scrollRoot.value?.querySelector('[contenteditable="true"]');
  if (el) editableEl = el;
  return editableEl;
}

// ---------- 组合式函数装配（撤销历史 / 查找替换 / 虚拟滚动，见 src/composables/） ----------
// 共享上下文 ctx：核心响应式状态（ref）按引用共享；可变原始值（let）经 get/set 访问器共享；
// 跨模块函数一律经 ctx 在运行时解析（函数声明提升，无初始化顺序依赖）
const ctx = {
  content, blocks, allBlocks, footnotes, scrollRoot, sourceRoot, editingId, flashId, documentDir, syncing, savedContent, currentFilePath,
  get editableEl() { return editableEl; },
  set editableEl(v) { editableEl = v; },
  get pendingCaretOffset() { return pendingCaretOffset; },
  set pendingCaretOffset(v) { pendingCaretOffset = v; },
  get suppressBlurCommit() { return suppressBlurCommit; },
  set suppressBlurCommit(v) { suppressBlurCommit = v; },
  get pendingTableFocus() { return pendingTableFocus; },
  set pendingTableFocus(v) { pendingTableFocus = v; },
  get cursorAtStart() { return cursorAtStart; },
  set cursorAtStart(v) { cursorAtStart = v; },
  get blurCommitTimer() { return blurCommitTimer; },
  set blurCommitTimer(v) { blurCommitTimer = v; },
  currentEditable, reparse, commitEdit, scheduleCodeHighlight,
  parseAnchoredBlocks, shiftBlockOffsets, publishBlocks, renumberOrderedMarkers,
  editingBlock, isSourceMode: () => props.sourceMode, startEdit, splitAndCommit, appendBlocksAtEnd, reparseRegion,
  emit, footnoteRefList, sbOnScroll, sbOnMouseMove, sbOnMouseLeave,
};
Object.assign(ctx, useUndoHistory(ctx));
Object.assign(ctx, useFindReplace(ctx));
Object.assign(ctx, useVirtualScroll(ctx));
Object.assign(ctx, useCompletion(ctx));
Object.assign(ctx, useTableEditing(ctx));
Object.assign(ctx, useSlashMenu(ctx));
Object.assign(ctx, useContextMenu(ctx));
Object.assign(ctx, useOutlineScroll(ctx));
Object.assign(ctx, useChangeReview(ctx));
Object.assign(ctx, useInlineSourceReveal(ctx));
Object.assign(ctx, useReadingPosition(ctx));
Object.assign(ctx, useSourceLineNumbers(ctx));
const {
  undoHistory, redoHistory, setHistoryLoading, resetHistory,
  findVisible, findWithReplace, findMatch,
  openFind, closeFind, onFindSearchInput, navFind, replaceInDocument, replaceAllInDocument,
  scheduleFindHighlights, hasKeyword: hasFindKeyword,
  renderedBlocks, visibleRows, tailPadHeight, rowQuota,
  langOpen, langIndex, langPos, langItems, langMenuItems,
  pathOpen, pathIndex, pathPos, pathCandidates, pathMenuItems,
  updateLangMenu, updateLangInputMenu, applyLangItem, dismissLangInput, updatePathMenu, applyPathItem,
  tableCaretAlign, tablePanel, tablePanelPos, hoverTableId, tableEdgeBtn,
  tableGridHover, tableGridSize, TABLE_GRID_COLS, TABLE_GRID_ROWS, TABLE_MORE_ITEMS,
  onTableBlockHover, onTableBlockLeave, onTableEdgeMove, onTableEdgeAdd,
  onToolbarOp, onToolbarDelete, onToolbarPanel, applyTableGrid, runTableMore,
  caretTableCell, moveTableCaret, moveTableRowDown, updateTableCaretAlign, takePendingTableAction, tableOp, pasteTableMatrix,
  slashOpen, slashIndex, slashPos, slashItems, slashTextRow, slashTextCol,
  slashTableRows, slashTableCols, slashTableField, slashCalloutType,
  slashFontRow, slashFontColorIndex, slashFontSizeIndex, rgbError,
  updateSlashMenu, applySlashItem, applyRgbInput, onRgbFocus, onRgbCancel, onRgbNav, routeSlashKeys,
  ctxMenu, ctxCurrentType, onContextMenu, onEditorMouseDown, onCtrlLinkMousedown, onCtrlLinkClick,
  onMenuAction, closeCtxMenu, restoreCtxSelectionOnMount,
  copySelection, pastePlainText, insertBlockFromMenu, convertBlockType, domTextOffset,
  scrollToBlock, onEditorScroll, onEditorMouseMove, onEditorMouseLeave,
  rulerTicks, counts: changeCounts, reviewEnabled,
  menuOpen: chgMenuOpen, baselineKind, baselineTitle, gitState, manualTimeLabel, manualSnapshotAvailable,
  showOld: showOldContent, setBaseline, closeReview, captureManualSnapshot, probeGit,
  updateInlineSourceReveal, expandLinkAtCaretInUrl,
  backTopVisible, scrollToTop,
  gutterRoot, gutterLines, lineNumbersOn, jumpInput, jumpOpen, jumpText, routeSourceKeys, onJumpKeydown,
} = ctx;
// ---------- 改动审阅基线菜单（MEO 式 ±按钮 + 下拉） ----------
const chgReviewOn = computed(() => {
  prefsVersion.value;
  return !!getPref('change_review');
});
// 打开菜单时探测 Git 基线可用性（非 Git 仓库/未跟踪 → 禁用项带说明）
function toggleChgMenu() {
  chgMenuOpen.value = !chgMenuOpen.value;
  if (chgMenuOpen.value && gitState.value === 'unknown') probeGit();
}
const gitSuffix = computed(() => {
  if (!currentFilePath.value) return ' · 未保存文档';
  return gitState.value === 'unavailable' ? ' · 非 Git 仓库/未跟踪' : '';
});
const gitDisabled = computed(() => !currentFilePath.value || gitState.value === 'unavailable');
// 内容高度变化（块树发布/初次挂载）后重算滑轨
watch(renderedBlocks, () => nextTick(updateSbThumb), { flush: 'post' });
onMounted(() => nextTick(updateSbThumb));

// 行/词/字符统计由 Rust text_stats 命令完成（CJK 感知词数、UTF-16 字符数），
// 内容变化后防抖更新，避免每次按键都跨进程调用；
// 大文档的全文统计 IPC 与输入/首屏渲染抢资源：更长防抖 + 浏览器空闲时执行
//（异步命令离开主线程，编辑 IPC 不再排队等待统计）
let statsTimer = null;
let statsIdleHandle = null;
const cancelStatsIdle = () => {
  if (statsIdleHandle != null && 'cancelIdleCallback' in window) cancelIdleCallback(statsIdleHandle);
  statsIdleHandle = null;
};
watch(content, () => {
  clearTimeout(statsTimer);
  cancelStatsIdle();
  const isHuge = content.value.length > 512 * 1024;
  statsTimer = setTimeout(
    () => {
      const run = async () => {
        statsIdleHandle = null;
        try {
          const stats = await invoke('text_stats_async', { markdown: content.value });
          wordCount.value = stats.words;
          charCount.value = stats.chars;
          lineCount.value = stats.lines;
        } catch (e) {
          console.error('text_stats 调用失败:', e);
        }
      };
      if (isHuge && 'requestIdleCallback' in window) {
        statsIdleHandle = requestIdleCallback(run, { timeout: 4000 });
      } else {
        run();
      }
    },
    isHuge ? 2000 : 300,
  );
}, { immediate: true });

watch([wordCount, charCount, lineCount, cursorLine, cursorColumn], () => {
  emit('update:stats', {
    wordCount: wordCount.value,
    charCount: charCount.value,
    lineCount: lineCount.value,
    cursorLine: cursorLine.value,
    cursorColumn: cursorColumn.value,
  });
}, { immediate: true });

// 块树变化时上报给 App（侧边栏目录/大纲数据源）。
// 不用 deep watch（每次提交都会触发全树遍历）：所有变更点已显式调用本函数。
function publishBlocks() {
  emit('update:blocks', blocks.value);
}

// 源码模式不解析；切回所见即所得时立即解析一次并在解析完成后恢复滚动位置
watch(() => props.sourceMode, async (source) => {
  if (!source) {
    // 源码编辑可能混入外部粘贴的 CRLF/CR：回 WYSIWYG 前统一为 LF（模型只认 \n）
    if (content.value.includes('\r')) content.value = content.value.replace(/\r\n?/g, '\n');
    await reparse();
  }
  restoreScrollPosition();
});

// 影响解析结构的偏好变化（html_to_md）时重解析
watch(structureVersion, () => {
  reparse();
});

async function reparse() {
  const seq = ++parseSeq;
  try {
    const result = await invoke('parse_markdown_async', { markdown: content.value });
    if (seq === parseSeq) {
      blocks.value = rawBlocks(result);
      publishBlocks();
    }
  } catch (e) {
    console.error('parse_markdown 调用失败:', e);
  }
}

// 平移 fromIndex 起所有根块的源码偏移（编辑增删后的增量维护；负 delta 为收缩）
function shiftBlockOffsets(fromIndex, delta) {
  for (let i = fromIndex; i < blocks.value.length; i++) {
    const b = blocks.value[i];
    if (b.start != null) {
      b.start += delta;
      b.end += delta;
    }
  }
}

// 有序列表中间插入/删除项后：把后续同组项的源标记序号改写为与显示序号一致（源即所见），
// 否则保存/源码模式下后续项仍是旧序号。编号规则与 numberedOrdinals / Rust 序列化器逐字一致：
// 组首项用源标记数字（缺省 1），组内递增；空行后遇源标记 1. 视为新列表重启。
// 只改写 fromIndex 起序号与标记不符的项；加载/全量重解析路径不要调用（原文须原样往返）。
function renumberOrderedMarkers(fromIndex) {
  const bs = blocks.value;
  if (!bs.length) return;
  // 全文无有序列表项：直接跳过（否则无列表文档每次提交都回扫找组首）
  if (!bs.some((b) => b.type === 'numberedListItem')) return;
  const isNum = (b) => b?.type === 'numberedListItem';
  const isGap = (b) => b?.type === 'paragraph' && !plainText(b.title).trim() && !b.image;
  // 回退到组首（允许跨空段落；遇实质内容块停止）
  let head = -1;
  for (let k = Math.min(fromIndex, bs.length - 1); k >= 0; k--) {
    if (isNum(bs[k])) head = k;
    else if (!isGap(bs[k])) break;
  }
  if (head < 0) return;
  // 前推整组，收集 fromIndex 起序号与源标记不符的项（标记位置基于当前原文偏移）
  const sites = []; // { index, pos, oldLen, delta, expect, newText }
  let n = 0;
  let gap = false;
  for (let k = head; k < bs.length; k++) {
    const b = bs[k];
    if (isNum(b)) {
      const expect = n === 0 ? (b.listStart ?? 1) : gap && b.listStart === 1 ? 1 : n + 1;
      if (k >= fromIndex && (b.listStart ?? 1) !== expect && b.start != null) {
        const m = /^[ \t]{0,3}(\d+)(?=[.)])/.exec(content.value.slice(b.start, b.end));
        if (!m) return; // 标记定位失败：保守放弃本次改写
        const newText = String(expect);
        sites.push({
          index: k,
          pos: b.start + (m[0].length - m[1].length),
          oldLen: m[1].length,
          delta: newText.length - m[1].length,
          expect,
          newText,
        });
      }
      n = expect;
      gap = false;
    } else if (isGap(b)) {
      gap = true;
    } else {
      break;
    }
  }
  if (!sites.length) return;
  // 从后往前改文本（高位改动不影响未处理的低位偏移），再单趟平移后续块偏移
  for (let s = sites.length - 1; s >= 0; s--) {
    const { pos, oldLen, newText } = sites[s];
    content.value = content.value.slice(0, pos) + newText + content.value.slice(pos + oldLen);
  }
  let si = 0;
  let acc = 0;
  for (const b of blocks.value) {
    if (b.start == null) continue;
    while (si < sites.length && sites[si].pos < b.start) {
      acc += sites[si].delta;
      si++;
    }
    b.start += acc;
    b.end += acc;
  }
  for (const s of sites) {
    const b = blocks.value[s.index];
    b.end += s.delta; // 自身标记在块区间内，上一趟未计入
    b.listStart = s.expect;
  }
}

// 解析片段并把相对偏移换算为锚点后的绝对偏移
async function parseAnchoredBlocks(md, anchor) {
  const parsed = await invoke('parse_blocks_async', { markdown: md });
  return rawBlocks(
    parsed.map((b) => ({
      ...b,
      start: b.start != null ? anchor + b.start : null,
      end: b.end != null ? anchor + b.end : null,
    })),
  );
}

// 块对象标记为不可响应（markRaw）：块树只按数组变更（splice/替换）驱动更新，
// 深度响应代理在大文档（数万嵌套对象）上是打开慢的主要来源之一；
// 块字段（start/end 等）只在计算时按需读取，不依赖字段级响应
function rawBlocks(list) {
  return list.map((b) => markRaw(b));
}

// 增量更新：编辑提交后只重解析受影响的片段（parse_blocks 返回相对偏移），
// 原位替换旧块并平移后续块偏移；未变化的块 id 稳定，前端仅局部重渲染。
// 与 velotype 一致：输入完成只更新当前块，不整树重解析。失败时回退全文重解析。
// oldRegion 为被替换的源码区间（{start,end}）；replaceCount 为被替换的块数（列表组编辑 >1）
async function reparseRegion(index, oldRegion, md, replaceCount = 1) {
  try {
    const replacements = await parseAnchoredBlocks(md, oldRegion.start);
    // JS 字符串 length 即 UTF-16 码元数，与 Rust 端偏移约定一致
    const delta = md.length - (oldRegion.end - oldRegion.start);
    blocks.value.splice(index, replaceCount, ...replacements);
    shiftBlockOffsets(index + replacements.length, delta);
    renumberOrderedMarkers(index);
    publishBlocks();
  } catch (e) {
    console.error('parse_blocks 调用失败，回退全文重解析:', e);
    await reparse();
  }
}

// 文末追加提交（__append__ 提交、脚注定义/斜杠表格等插入共用）：片段 md 接在正文末尾
//（空文档直接作为全文），只增量解析追加的片段并挂到块树末尾；解析失败回退全文重解析自愈。
// 返回追加的块数组；失败返回 null（调用方直接结束本次流程）
async function appendBlocksAtEnd(md) {
  const base = content.value ? content.value.replace(/\s*$/, '') : '';
  const anchor = base ? base.length + 2 : 0;
  content.value = base ? base + '\n\n' + md : md;
  const parsed = await parseAnchoredBlocks(md, anchor).catch(() => null);
  if (!parsed) {
    await reparse();
    return null;
  }
  blocks.value.push(...parsed);
  return parsed;
}

// 合并片段提交落源并增量重解析（Enter 拆分 / pre Ctrl+Enter / Markdown 粘贴共用）：
// 追加流（文末 __append__）没有可替换的旧块——锚点为文末、整段拼接；否则原位替换当前
// 编辑块的源码区间。增量重解析合并片段（一次调用；空行分隔的两段与分别解析等价），
// splice 替换/追加并平移后续偏移、重编号有序列表、发布块树；
// 解析失败回退全文重解析（块树按正文重建自愈）。
// 返回 { oldBlock, replacements }；旧块缺失或解析失败返回 null（调用方直接结束本次流程）
async function commitCombinedRegion(combined) {
  const isAppend = editingId.value === '__append__';
  let index;
  let oldBlock;
  let base = '';
  if (isAppend) {
    base = content.value ? content.value.replace(/\s*$/, '') : '';
    index = blocks.value.length;
    const anchor = base ? base.length + 2 : 0;
    oldBlock = { start: anchor, end: anchor };
  } else {
    // 列表组编辑：整组区间（组首起点 → 组末终点）与 splice 块数
    const g = listEdit.value;
    index = g ? g.i0 : blocks.value.findIndex((b) => b.id === editingId.value);
    oldBlock = g
      ? { start: blocks.value[g.i0]?.start, end: blocks.value[g.i1]?.end }
      : blocks.value[index];
  }
  editableEl = null;
  if (!oldBlock) {
    editingId.value = null;
    return null;
  }
  if (isAppend) {
    content.value = base ? base + '\n\n' + combined : combined;
  } else {
    content.value = content.value.slice(0, oldBlock.start) + combined + content.value.slice(oldBlock.end);
  }
  const replacements = await parseAnchoredBlocks(combined, oldBlock.start).catch(() => null);
  if (!replacements) {
    editingId.value = null;
    await reparse();
    return null;
  }
  const delta = combined.length - (oldBlock.end - oldBlock.start);
  blocks.value.splice(index, isAppend ? 0 : (listEdit.value ? listEdit.value.i1 - listEdit.value.i0 + 1 : 1), ...replacements);
  shiftBlockOffsets(index + replacements.length, delta);
  renumberOrderedMarkers(index);
  publishBlocks();
  return { oldBlock, replacements };
}

// 源码模式光标统计：缓存上次扫描锚点，光标前进（连续输入）时只扫描增量区间，
// 避免每次按键/点击都复制并正则扫描光标前的全部文本（大文档每键 O(n)）
let caretAnchor = { content: null, offset: 0, line: 1, lastNewline: -1 };
function onInput(e) {
  const textarea = e.target;
  const pos = textarea.selectionStart;
  const text = textarea.value;
  let anchor = caretAnchor;
  // 内容变化或光标回退（点击/移动）时锚点失效，从头重扫
  if (anchor.content !== text || anchor.offset > pos) {
    anchor = { content: text, offset: 0, line: 1, lastNewline: -1 };
  }
  const slice = text.slice(anchor.offset, pos);
  let { line, lastNewline } = anchor;
  let idx = -1;
  while ((idx = slice.indexOf('\n', idx + 1)) >= 0) {
    line += 1;
    lastNewline = anchor.offset + idx;
  }
  caretAnchor = { content: text, offset: pos, line, lastNewline };
  cursorLine.value = line;
  cursorColumn.value = pos - lastNewline;
}

// 源码模式 Tab：插入制表符而非默认移焦（否则光标跳到下一控件丢失）；
// 支持选区替换，光标落在插入内容之后
function onSourceKeydown(e) {
  if (routeSourceKeys(e)) return; // Ctrl+G 跳行 / Esc 关闭跳行框（F-G）
  if (e.key !== 'Tab' || e.ctrlKey || e.metaKey || e.altKey) return;
  e.preventDefault();
  const ta = sourceRoot.value;
  if (!ta) return;
  const start = ta.selectionStart;
  const end = ta.selectionEnd;
  content.value = content.value.slice(0, start) + '\t' + content.value.slice(end);
  nextTick(() => {
    if (ta.isConnected) ta.setSelectionRange(start + 1, start + 1);
  });
}

// 编辑容器挂载时：填充渲染好的 HTML；聚焦与光标放置推迟到 nextTick——
// ref 回调时元素可能尚未插入文档（Vue 键控列表换键场景），立即 focus 会静默失败。
// 注意：两个编辑容器（块容器与文末 __append__ 容器）共用本 ref，Vue 打补丁时
// 「挂载新容器 → 卸载旧容器」的顺序不定，卸载回调可能后于挂载执行——
// 因此卸载（el 为 null）时仅当当前容器已脱离文档才清空，避免误清新容器。
// 意外重挂载防护：非提交路径的卸载（如父级重渲染重建容器）会暂存未提交内容，
// 同一编辑目标重挂载时恢复——否则 setEditableEl 会用初始 HTML 覆盖用户/命令写入的内容。
let stashedEdit = null; // { id, html }：意外卸载时暂存的编辑内容与目标
function setEditableEl(el) {
  if (!el) {
    // 卸载回调（可能后于新容器挂载执行）：旧容器已脱离文档才暂存并清空
    if (editableEl && !editableEl.isConnected) {
      stashedEdit = stashedEdit ?? { id: editingId.value, html: editableEl.innerHTML };
      editableEl = null;
    }
    return;
  }
  // 同一容器的 ref 重复触发（Vue 对 v-if 分支补丁时会重设函数 ref）：
  // 直接跳过——否则无条件重填初始 HTML 会把未提交内容原地抹掉
  if (el === editableEl) return;
  // 挂载回调：「先挂新、后卸旧」的替换中，旧容器仍在文档且持未提交内容，先暂存
  if (editableEl && editableEl !== el && editableEl.isConnected) {
    stashedEdit = { id: editingId.value, html: editableEl.innerHTML };
  }
  editableEl = el;
  const block = blocks.value.find((b) => b.id === editingId.value);
  // 原子/保留类块按原始 Markdown 切片编辑，其余按渲染 HTML 就地编辑
  const rawSource = block && block.start != null ? content.value.slice(block.start, block.end) : '';
  if (stashedEdit && stashedEdit.id === editingId.value) {
    el.innerHTML = stashedEdit.html;
  } else {
    // 列表组编辑：整组（连续同族兄弟项）并入同一容器，组首行作宿主
    const g = listEdit.value;
    el.innerHTML = g
      ? blocks.value
          .slice(g.i0, g.i1 + 1)
          .map((b) => blockToHtml(b, undefined, 0, rootOrdinals.value.get(b.id) || 1))
          .join('')
      : editingId.value === '__append__' || !block
        ? emptyParagraphHtml()
        : blockToHtml(block, rawSource, 0, rootOrdinals.value.get(block.id) || 1);
  }
  stashedEdit = null;
  // 公式/图表块进入编辑：立即渲染一次编辑预览（后续输入经 onEditableInput 防抖更新）
  editPreviewSvg.value = '';
  editPreviewHidden.value = false;
  if (editPreviewKind(editingBlock.value)) renderEditPreview();
  // 代码块进入编辑：立即高亮一次（后续输入防抖重排）
  if (editingBlock.value?.type === 'codeBlock') applyCodeHighlight();
  nextTick(() => {
    // 换块 blur 抑制到此为止（无论是否成功聚焦）
    suppressBlurCommit = false;
    // 期间用户点击切换了编辑目标，或元素已被卸载，则放弃聚焦
    if (editableEl !== el || !el.isConnected) {
      pendingCaretOffset = null;
      pendingPreciseRawOffset = null;
      return;
    }
    el.focus({ preventScroll: true });
    if (pendingTableFocus) {
      // 斜杠表格即时渲染后：光标落进首个数据单元格
      pendingTableFocus = false;
      const cell = el.querySelector('tbody td') || el.querySelector('th');
      if (cell) placeCursorAtStart(cell);
    } else if (pendingPreciseRawOffset != null) {
      // 源码 → WYSIWYG 切换：块内源码偏移经「序列化对齐」精确换算为 DOM 位置
      const target = pendingPreciseRawOffset;
      pendingPreciseRawOffset = null;
      markdownOffsetToPlainOffset(el, target).then((plain) => {
        if (editableEl === el && el.isConnected) placeCaretAtTextOffset(el, plain);
      });
    } else if (pendingCaretOffset != null) {
      // 合并两块后：光标落在上一块原文末尾（接缝处）
      const offset = pendingCaretOffset;
      pendingCaretOffset = null;
      placeCaretAtTextOffset(el, offset);
    } else if (restoreCtxSelectionOnMount(el)) {
      // 右键菜单带入的渲染态框选：选区已按暂存的纯文本偏移重建，高亮保持连续
    } else if (placePendingGroupCaret(el)) {
      // 列表组编辑：光标落到被点击项的点击位置
    } else if (cursorAtStart) {
      cursorAtStart = false;
      placeCursorAtStart(el);
    } else {
      placeCursorAtEnd(el);
    }
    // 悬停工具栏排队的动作（对齐/插行列/删除/打开面板）：编辑容器就绪、光标落位后执行
    const queuedTableAction = takePendingTableAction();
    if (queuedTableAction) {
      queuedTableAction();
      // 收起自动放置的光标：后续工具栏操作按「无单元格上下文」处理
      // （对齐=整表、插/删行列=末尾）；用户点进单元格后恢复行/列上下文
      window.getSelection()?.removeAllRanges();
    }
    // 换块/新建块后：光标若被压到窗口底部，滚回可见下沿（帧末测量，等布局稳定）
    requestAnimationFrame(keepCaretAboveStatusBar);
    // 源码/WYSIWYG 切换后：光标已放置，把光标滚动到窗口中间。
    // 含图片时等待图片加载完成后再重算一次，避免布局变化导致光标错位。
    if (restoreCaretAdjustment) {
      restoreCaretAdjustment = null;
      nextTick(() => {
        requestAnimationFrame(() => adjustCaretToViewportCenter());
        waitForImages(scrollRoot.value).then(() => {
          requestAnimationFrame(() => adjustCaretToViewportCenter());
        });
      });
    }
  });
}

// 等待 root 内所有图片加载完成（包含已完成/加载失败）
function waitForImages(root) {
  if (!root) return Promise.resolve();
  const imgs = root.querySelectorAll('img');
  if (!imgs.length) return Promise.resolve();
  return Promise.all(
    [...imgs].map((img) =>
      img.complete
        ? Promise.resolve()
        : new Promise((resolve) => {
            img.addEventListener('load', resolve, { once: true });
            img.addEventListener('error', resolve, { once: true });
          }),
    ),
  );
}

// 把光标滚动到窗口中间
function adjustCaretToViewportCenter() {
  const sel = window.getSelection();
  const root = scrollRoot.value;
  if (!sel?.rangeCount || !root) return;
  const rect = sel.getRangeAt(0).getBoundingClientRect();
  const rootRect = root.getBoundingClientRect();
  const viewportHeight = root.clientHeight;
  root.scrollTop += rect.top - rootRect.top - viewportHeight / 2;
}

// setEditableEl 的下一次挂载把光标放在内容开头（Enter 拆分进入新块）
let cursorAtStart = false;
// 下一次挂载把光标放在指定纯文本偏移（块首退格合并两块）；优先于 cursorAtStart
let pendingCaretOffset = null;
// 下一次挂载把源码偏移经序列化对齐精确换算为 DOM 位置（源码 → WYSIWYG 切换）
let pendingPreciseRawOffset = null;

// 列表组编辑挂载：把点击点的「块内纯文本偏移」映射回组容器对应项
//（前缀 = 前序组内项顶层列表元素的纯文本长度和；项内偏移钳制到该项文本长度）
function placePendingGroupCaret(el) {
  const caret = pendingGroupCaret;
  pendingGroupCaret = null;
  const g = listEdit.value;
  if (!caret || !g) return false;
  const ids = blocks.value.slice(g.i0, g.i1 + 1).map((b) => b.id);
  const clickIdx = ids.indexOf(caret.id);
  if (clickIdx < 0) return false;
  const tops = [...el.children];
  let prefix = 0;
  for (let j = 0; j < clickIdx && j < tops.length; j++) prefix += tops[j].textContent.length;
  const maxIn = tops[clickIdx]?.textContent.length ?? 0;
  placeCaretAtTextOffset(el, prefix + Math.min(caret.offset, maxIn));
  return true;
}

function startEdit(block) {
  if (syncing.value) return;
  // 任何未塌陷选区（刚拖选完）：不进入编辑——保持选区供复制/右键格式化
  //（否则替换 DOM 会销毁选区，表现为「选中的文字立马消失、首次 Ctrl+C 复制不到」）；
  // 再点一次（选区已塌陷为光标）才进入编辑
  const sel = window.getSelection();
  if (sel && !sel.isCollapsed) return;
  // 列表组编辑：暂存点击点在渲染态行内的纯文本偏移（跳过 .md-marker 等非内容文本），
  // 组容器挂载后由 placePendingGroupCaret 映射回被点击项的同位置
  pendingGroupCaret = null;
  const gIdx = blocks.value.findIndex((b) => b.id === block.id);
  if (gIdx >= 0 && listGroupRangeAt(gIdx) && sel?.rangeCount) {
    const row = document.querySelector(`[data-block-id="${block.id}"]`);
    if (row && row.contains(sel.anchorNode)) {
      pendingGroupCaret = { id: block.id, offset: domTextOffset(row, sel.anchorNode, sel.anchorOffset) };
    }
  }
  if (editingId.value !== null) {
    // 编辑态残留（无活动容器）时自愈，避免点不出光标
    if (!editableEl) editingId.value = null;
    else {
      // 有未提交编辑（含延迟 blur 提交）：先冲掉延迟定时器并提交当前块，
      // 再进入新块（无需二次点击）；提交为空操作（容器丢失）时直接换块
      clearTimeout(blurCommitTimer);
      blurCommitTimer = null;
      commitEdit().finally(() => {
        editingId.value = block.id;
      });
      return;
    }
  }
  editingId.value = block.id;
}

function startAppend() {
  if (syncing.value) return;
  // 与 startEdit 同一守卫：未塌陷选区（刚拖选完）不进入编辑，保持选区可复制
  const sel = window.getSelection();
  if (sel && !sel.isCollapsed) return;
  if (editingId.value !== null) {
    if (!editableEl) editingId.value = null;
    else {
      clearTimeout(blurCommitTimer);
      blurCommitTimer = null;
      commitEdit().finally(() => {
        editingId.value = '__append__';
      });
      return;
    }
  }
  editingId.value = '__append__';
}

// 有序列表提交：把正在编辑块的显示序号写入首个有序项 DTO
//（序列化后源标记与渲染一致——源即所见；组内后续项由序列化器按组起点递增）
function applyListStartToDtos(dtos) {
  // 组编辑时首个有序项取组首块的显示序号（而非被点击项）
  const g = listEdit.value;
  const headId = g ? blocks.value[g.i0]?.id : editingId.value;
  const ordinal = rootOrdinals.value.get(headId);
  if (ordinal == null) return;
  const findFirstNumbered = (list) => {
    for (const d of list) {
      if (d.type === 'numberedListItem') return d;
      const hit = d.children?.length ? findFirstNumbered(d.children) : null;
      if (hit) return hit;
    }
    return null;
  };
  const first = findFirstNumbered(dtos);
  if (first) first.listStart = ordinal;
}

// preSerialized：调用方已序列化好的当前编辑内容（如 captureScrollPosition 已算过 fullMd），
// 传入则跳过重复序列化（Ctrl+/ 切换同一块不再序列化两次）；须经 applyListStartToDtos 处理
async function commitEdit(preSerialized = null) {
  if (editingId.value === null || !currentEditable()) return;
  syncing.value = true;
  try {
    // 前端只提取 DOM 结构，Markdown 序列化由 Rust 完成
    // 序列化失败（IPC 异常）：不动正文、保持编辑态（内容不丢），下次提交重试
    let md = preSerialized;
    if (md == null) {
      const dtos = domToBlockDtos(editableEl);
      applyListStartToDtos(dtos);
      md = await invoke('serialize_markdown', { blocks: dtos }).catch(() => null);
    }
    if (md == null) return;
    editableEl = null;
    suppressBlurCommit = false;
    if (editingId.value === '__append__') {
      const text = md.trim();
      if (text) {
        // 增量：只解析追加的片段并挂到块树末尾；解析失败回退全文重解析
        const parsed = await appendBlocksAtEnd(text);
        if (!parsed) return;
        renumberOrderedMarkers(blocks.value.length - parsed.length);
        publishBlocks();
      }
    } else {
      // 列表组编辑：替换整组源码区间（组首块起点 → 组末块终点），splice 整组
      const g = listEdit.value;
      const index = g ? g.i0 : blocks.value.findIndex((b) => b.id === editingId.value);
      const block = blocks.value[index];
      if (block) {
        const regionEnd = g ? blocks.value[g.i1].end : block.end;
        content.value = content.value.slice(0, block.start) + md + content.value.slice(regionEnd);
        // 增量：原位替换被编辑的块（组）
        await reparseRegion(index, { start: block.start, end: regionEnd }, md, g ? g.i1 - g.i0 + 1 : 1);
      }
    }
    editingId.value = null;
  } finally {
    syncing.value = false;
  }
}

// 空段检测：拆分/粘贴会在边界克隆出空的块级元素（如标题末尾的空 <h2>），
// 若参与序列化会生成孤立的标记文本（## 等），按空段处理（img/input/hr/table/pre 等非文本内容除外）
function isEmptyDiv(div) {
  return div.textContent.trim() === '' && !div.querySelector('img, input, hr, table, pre');
}

// Enter 提交：在光标处拆分当前块——光标前内容留在本块、光标后内容移入新块，
// 两段分别由 Rust 序列化与解析，光标落到新块开头。代码块/Mermaid 等 pre 编辑不经过此路径。
async function splitAndCommit() {
  const el = currentEditable();
  if (editingId.value === null || !el) return;
  syncing.value = true;
  try {
    const sel = window.getSelection();
    let beforeDiv;
    let afterDiv;
    if (sel.rangeCount && el.contains(sel.getRangeAt(0).startContainer)) {
      // 以光标为界克隆前后两段 DOM
      const range = sel.getRangeAt(0);
      const beforeRange = rangeBeforePoint(el, range.startContainer, range.startOffset);
      const afterRange = range.cloneRange();
      afterRange.selectNodeContents(el);
      afterRange.setStart(range.endContainer, range.endOffset);
      beforeDiv = document.createElement('div');
      beforeDiv.append(beforeRange.cloneContents());
      afterDiv = document.createElement('div');
      afterDiv.append(afterRange.cloneContents());
    } else {
      // 无有效光标（内容删空后浏览器丢失选区）：按块尾拆分，前段为整块内容
      beforeDiv = document.createElement('div');
      beforeDiv.innerHTML = el.innerHTML;
      afterDiv = document.createElement('div');
    }

    // 两次序列化并行（IPC 延迟是超长文档拆分卡顿的主要来源之一）；
    // 序列化失败：不动正文、保持编辑态（内容不丢），下次 Enter 重试
    const [beforeMdRaw, afterMdRaw] = await Promise.all([
      isEmptyDiv(beforeDiv)
        ? Promise.resolve('')
        : invoke('serialize_markdown', { blocks: domToBlockDtos(beforeDiv) }),
      isEmptyDiv(afterDiv)
        ? Promise.resolve('')
        : invoke('serialize_markdown', { blocks: domToBlockDtos(afterDiv) }),
    ]).catch(() => null);
    if (!beforeMdRaw) return;
    const beforeMd = beforeMdRaw.trim();
    const afterMd = afterMdRaw.trim();

    const combined = beforeMd + '\n\n' + afterMd;
    const committed = await commitCombinedRegion(combined);
    if (!committed) return;
    const { oldBlock, replacements } = committed;

    // 进入拆分出的新块编辑，光标在内容开头；抑制换块卸载触发的 blur 误提交
    //（目标 = 后段首块，即 start 越过 前段+空行 锚点的首个块；后段为空时取末块，与原行为一致）
    const afterAnchor = oldBlock.start + beforeMd.length + 2;
    const next =
      (afterMd ? replacements.find((b) => b.start != null && b.start >= afterAnchor) : null) ??
      replacements[replacements.length - 1];
    suppressBlurCommit = true;
    editingId.value = next ? next.id : null;
    cursorAtStart = true;
    if (!next) suppressBlurCommit = false;
  } finally {
    syncing.value = false;
  }
}

function cancelEdit() {
  editingId.value = null;
  editableEl = null;
  suppressBlurCommit = false;
}

// 块的纯文本（标题行 + 子块递归），用于块首退格合并后的光标接缝定位
function blockPlainText(b) {
  return plainText(b.title) + (b.children || []).map(blockPlainText).join('');
}

// 进入指定块编辑并把光标放在末尾（程序性换块走 setEditableEl 挂载链路）
function focusBlockAtEnd(id) {
  suppressBlurCommit = true;
  cursorAtStart = false;
  editingId.value = id;
}

// 非空块开头 + Backspace（Typora 式两步退格）：
// 标题/引用/列表项等样式块先降级为段落；段落并入上一文本块（块类型沿用上块）；
// 上一块是分割线则删除之；原子块/复杂结构只提交当前编辑并把光标移到上一块末尾。
async function backspaceAtBlockStart() {
  const el = currentEditable();
  if (editingId.value === null || !el || syncing.value) return;
  const TEXTLIKE = ['paragraph', 'heading', 'quote', 'bulletedListItem', 'numberedListItem', 'taskListItem'];
  const isAppend = editingId.value === '__append__';
  // 列表组编辑：光标在容器开头 = 组首块开头，并入/聚焦的「上一块」是组前的块
  const g = listEdit.value;
  const index = isAppend ? blocks.value.length : (g ? g.i0 : blocks.value.findIndex((b) => b.id === editingId.value));
  if (index < 0) return;
  const dtos = domToBlockDtos(el);
  const single = dtos.length === 1 ? dtos[0] : null;

  // 样式块（标题/引用/列表项）：第一次退格降级为段落，嵌套子块提升为顶层
  //（仅改编辑态 DOM，提交时落源）
  if (single && single.type !== 'paragraph' && TEXTLIKE.includes(single.type)) {
    demoteEditableToParagraph(el, single);
    return;
  }

  const prev = blocks.value[index - 1];
  if (!prev) return; // 首块段落：前面没有可并入的块

  // 上一块是分割线：删除分割线，当前块保持编辑（光标仍在开头）
  if (prev.type === 'separator') {
    removePrevSeparator(index - 1);
    return;
  }

  // 当前块为单段落且上一块为文本类：并入上一块，光标落在接缝处
  if (single && single.type === 'paragraph' && TEXTLIKE.includes(prev.type)) {
    await mergeIntoPrevBlock(single, index, isAppend);
    return;
  }

  // 其余（原子块/嵌套结构）：提交当前编辑，光标移到上一块末尾
  await commitEdit();
  focusBlockAtEnd(prev.id);
}

// 删除上一块分割线（含相邻换行）并平移后续偏移；当前编辑块不受影响
function removePrevSeparator(prevIndex) {
  const prev = blocks.value[prevIndex];
  if (!prev) return;
  let removeStart = prev.start;
  let removeEnd = prev.end;
  if (content.value[removeEnd] === '\n') removeEnd += 1;
  else if (removeStart > 0 && content.value[removeStart - 1] === '\n') removeStart -= 1;
  const removedLen = removeEnd - removeStart;
  content.value = content.value.slice(0, removeStart) + content.value.slice(removeEnd);
  blocks.value.splice(prevIndex, 1);
  shiftBlockOffsets(prevIndex, -removedLen);
  renumberOrderedMarkers(prevIndex);
  publishBlocks();
}

// 当前段落块并入上一文本块：合并源 = 上块源 + 当前行内 Markdown（上块类型保留），
// 重解析合并区间并平移后续偏移，光标落在上块原文末尾（接缝处）
async function mergeIntoPrevBlock(dto, index, isAppend) {
  syncing.value = true;
  try {
    const prev = blocks.value[index - 1];
    const curEnd = isAppend ? content.value.length : blocks.value[index].end;
    // 序列化/合并 IPC 失败：不动正文、保持编辑态（内容不丢）
    const curMd = await invoke('serialize_markdown', { blocks: [paragraphDtoFromTree(dto.title)] }).catch(() => null);
    if (curMd == null) return;
    const prevSource = content.value.slice(prev.start, prev.end);
    // 合并规则（接缝处理）由 Rust 统一维护，前端只做机械区间替换
    const combined = await invoke('merge_block_markdown', { prevSource, appendedMarkdown: curMd }).catch(() => null);
    if (combined == null) return;
    const junction = blockPlainText(prev).length;
    editableEl = null;
    content.value = content.value.slice(0, prev.start) + combined + content.value.slice(curEnd);
    const replacements = await parseAnchoredBlocks(combined, prev.start).catch(() => null);
    if (!replacements) {
      // 解析失败：全文重解析自愈
      editingId.value = null;
      await reparse();
      return;
    }
    const delta = combined.length - (curEnd - prev.start);
    blocks.value.splice(index - 1, isAppend ? 1 : 2, ...replacements);
    shiftBlockOffsets(index - 1 + replacements.length, delta);
    renumberOrderedMarkers(index - 1);
    publishBlocks();
    const merged = replacements[0];
    suppressBlurCommit = true;
    pendingCaretOffset = junction;
    editingId.value = merged ? merged.id : null;
    if (!merged) {
      suppressBlurCommit = false;
      pendingCaretOffset = null;
    }
  } finally {
    syncing.value = false;
  }
}

// 空块 + Backspace：删除当前块（含源码区间与相邻换行），光标移到上一块末尾；
// 首块为空则移到下一块开头；文档无块则落到追加区（Typora 式连续退格删除）。
async function deleteEmptyBlockAndFocusPrev() {
  if (editingId.value === null) return;
  syncing.value = true;
  try {
    let focusId = null;
    let focusAtStart = false;
    if (editingId.value === '__append__') {
      // 追加区为空：退格跳到末尾块
      const last = blocks.value[blocks.value.length - 1];
      if (last) {
        focusId = last.id;
      } else {
        // 文档无可跳块：追加区重置为空段落。editingId 不变时 Vue 不会重挂载容器，
        // 不重置则快捷转换出的空标题等元素会一直残留（表现为删空后仍是标题渲染）
        const el = currentEditable();
        if (el) {
          el.innerHTML = emptyParagraphHtml();
          placeCursorAtEnd(el);
        }
        return;
      }
    } else {
      // 列表组编辑：删空的是整组容器——删除整组源码区间（组首起点 → 组末终点）
      const g = listEdit.value;
      const index = g ? g.i0 : blocks.value.findIndex((b) => b.id === editingId.value);
      const oldBlock = blocks.value[index];
      editableEl = null;
      if (oldBlock) {
        // 删除块源码及其行尾换行（空段落块为零长源码，删除对应空行）
        let removeStart = oldBlock.start;
        let removeEnd = g ? blocks.value[g.i1].end : oldBlock.end;
        if (content.value[removeEnd] === '\n') removeEnd += 1;
        else if (removeStart > 0 && content.value[removeStart - 1] === '\n') removeStart -= 1;
        const removedLen = removeEnd - removeStart;
        content.value = content.value.slice(0, removeStart) + content.value.slice(removeEnd);
        blocks.value.splice(index, g ? g.i1 - g.i0 + 1 : 1);
        shiftBlockOffsets(index, -removedLen);
        renumberOrderedMarkers(index);
        publishBlocks();
        const prev = blocks.value[index - 1];
        if (prev) focusId = prev.id;
        else if (blocks.value.length) {
          focusId = blocks.value[0].id;
          focusAtStart = true;
        }
      }
    }
    editableEl = null;
    suppressBlurCommit = true;
    editingId.value = focusId ?? '__append__';
    cursorAtStart = focusAtStart;
    if (!focusId) suppressBlurCommit = false;
  } finally {
    syncing.value = false;
  }
}

// 代码块/Mermaid 等 pre 编辑的 Ctrl+Enter：整块提交渲染（不拆分），
// 并在其后新建空段落进入编辑。普通 Enter 在 pre 内插入换行（见 keydown 路由）。
async function commitCodeAndNewBlock() {
  if (editingId.value === null || !currentEditable()) return;
  syncing.value = true;
  try {
    // 序列化失败：不动正文、保持编辑态（内容不丢），下次 Ctrl+Enter 重试
    const mdRaw = await invoke('serialize_markdown', { blocks: domToBlockDtos(editableEl) }).catch(() => null);
    if (mdRaw == null) return;
    const md = mdRaw.trimEnd();

    // 整块 + 尾部空行（解析出一个空段落作为后续编辑块）；解析失败回退全文重解析
    const combined = md + '\n\n';
    const committed = await commitCombinedRegion(combined);
    if (!committed) return;
    const { replacements } = committed;

    // 最后一个替换块即尾部空段落，进入编辑；抑制换块 blur 误提交
    const next = replacements[replacements.length - 1];
    suppressBlurCommit = true;
    editingId.value = next ? next.id : null;
    cursorAtStart = true;
    if (!next) suppressBlurCommit = false;
  } finally {
    syncing.value = false;
  }
}

// 失焦提交：仅当失焦元素就是当前编辑容器时生效。
// Enter 拆分等程序性换块会先卸载聚焦的旧容器再挂载新容器，卸载触发的 blur
// 不是用户主动失焦（此刻 editableEl 还指向旧容器，单靠元素比对拦不住），
// 用 suppressBlurCommit 标记抑制。
function onEditableBlur(e) {
  if (suppressBlurCommit) return;
  if (e.target !== editableEl) return;
  // 焦点可能正转移到斜杠菜单内部（RGB 输入框）：Chromium 的 blur.relatedTarget
  // 不可靠（常为 null），延迟一帧用 document.activeElement 判定，在菜单内则不提交
  const el = editableEl;
  clearTimeout(blurCommitTimer);
  blurCommitTimer = setTimeout(() => {
    blurCommitTimer = null;
    if (suppressBlurCommit) return;
    if (document.activeElement?.closest?.('.md-slash-menu')) return;
    // 焦点在编辑容器内部（如代码块语言输入框）：不视为失焦
    if (document.activeElement && el.contains(document.activeElement)) return;
    // 期间已程序性换块（容器已换）：不提交旧容器
    if (editableEl !== el) return;
    commitEdit();
  }, 0);
}
// 失焦提交的延迟定时器（换块点击时先冲掉，避免重复提交）
let blurCommitTimer = null;
// 程序性换块进行中：抑制卸载 blur 触发的误提交
let suppressBlurCommit = false;

// 输入时应用 Markdown 快捷转换与 HTML 标签自动闭合（输入法组合期间跳过）
function onEditableInput(e) {
  if (e.isComposing || e.inputType === 'insertCompositionText') return;
  // 代码块语言输入框：同步到 pre 的 data-language（提交随块序列化）并触发语言补全菜单
  const langInput = e.target?.closest?.('[data-lang-input]');
  if (langInput) {
    langInput.closest('pre')?.setAttribute('data-language', langInput.value.trim());
    updateLangInputMenu(langInput);
    scheduleCodeHighlight(); // 语言变化后重排高亮
    return;
  }
  applyMarkdownShortcuts();
  applyHtmlAutoclose();
  updateSlashMenu();
  updateLangMenu();
  updatePathMenu();
  scheduleEditPreview();
  scheduleCodeHighlight();
  // 打字机模式（偏好 typewriter_mode）：输入后光标保持视口垂直居中
  if (getPref('typewriter_mode')) {
    nextTick(() => requestAnimationFrame(adjustCaretToViewportCenter));
  }
  // 查找条打开时：输入会重渲染行内容，防抖重扫高亮
  if (findVisible.value && hasFindKeyword()) scheduleFindHighlights();
  // 输入到底部时：光标若被压到窗口底部（状态栏上方不可见），滚到窗口首行
  nextTick(keepCaretAboveStatusBar);
}

// 编辑实时预览（公式/Mermaid/PlantUML）：编辑这些块时在编辑区下方浮动面板渲染预览。
// 输入防抖（公式 180ms，图表较重 400ms）；语法未完成渲染失败时保持上一帧不清空，
// 清空内容才清预览。面板可经右上角眼睛图标/点击面板本身隐藏（editPreviewHidden）
const editPreviewSvg = ref('');
const editPreviewHidden = ref(false);
let editPreviewTimer = null;
// 支持编辑预览的块类型：mathBlock / mermaidBlock / plantuml(puml) 代码块
function editPreviewKind(block) {
  if (!block) return null;
  if (block.type === 'mathBlock') return 'math';
  if (block.type === 'mermaidBlock') return 'mermaid';
  if (block.type === 'codeBlock' && ['plantuml', 'puml'].includes((block.language || '').toLowerCase())) {
    return 'plantuml';
  }
  return null;
}
function scheduleEditPreview() {
  const kind = editPreviewKind(editingBlock.value);
  if (!kind) return;
  clearTimeout(editPreviewTimer);
  editPreviewTimer = setTimeout(renderEditPreview, kind === 'math' ? 180 : 400);
}
async function renderEditPreview() {
  const el = currentEditable();
  const block = editingBlock.value;
  const kind = editPreviewKind(block);
  if (!el || !kind) {
    editPreviewSvg.value = '';
    return;
  }
  const raw = (el.querySelector('pre')?.textContent ?? el.textContent).trim();
  if (!raw) {
    editPreviewSvg.value = '';
    return;
  }
  let svg = null;
  if (kind === 'math') {
    const style = getComputedStyle(document.documentElement);
    const color = style.getPropertyValue('--t-text-default').trim() || '#000000';
    const baseFontSize = parseFloat(style.getPropertyValue('--t-text-size')) || 16;
    svg = await invoke('render_display_math', { raw, color, baseFontSize }).catch(() => null);
  } else if (kind === 'mermaid') {
    svg = await renderMermaid(raw).catch(() => null);
  } else {
    // plantuml：语言以编辑态 pre[data-language] 为准（编辑中可能改语言；改走则本次不更新）
    const lang = (el.querySelector('pre')?.getAttribute('data-language') || '').toLowerCase();
    if (!['plantuml', 'puml'].includes(lang)) return;
    svg = await renderPlantumlLocal(raw).catch(() => null);
  }
  if (svg && editingBlock.value === block) editPreviewSvg.value = svg;
}

// 代码块编辑态语法高亮：进入编辑即高亮一次，输入防抖 350ms 重排。
// span 包裹不影响提交（代码块按 textContent 提取）；重排后按纯文本偏移还原光标。
let codeHlTimer = null;
function scheduleCodeHighlight() {
  if (editingBlock.value?.type !== 'codeBlock') return;
  clearTimeout(codeHlTimer);
  codeHlTimer = setTimeout(applyCodeHighlight, 350);
}
// 光标在元素内的纯文本偏移（仅折叠光标；有选区时返回 null 由调用方跳过）
function caretOffsetWithin(el) {
  const sel = window.getSelection();
  if (!sel.rangeCount || !sel.isCollapsed) return null;
  const r = sel.getRangeAt(0);
  if (!el.contains(r.startContainer)) return null;
  return textBeforePoint(el, r.startContainer, r.startOffset).length;
}
async function applyCodeHighlight() {
  const el = currentEditable();
  if (!el || editingBlock.value?.type !== 'codeBlock') return;
  const pre = el.querySelector('pre[data-language]');
  const codeEl = pre?.querySelector('code');
  if (!codeEl) return;
  // 有非空选区时跳过（重排会拆选区），待选区收敛后的下次输入再触发
  const sel = window.getSelection();
  if (sel.rangeCount && !sel.isCollapsed && el.contains(sel.anchorNode)) return;
  const caretOffset = caretOffsetWithin(codeEl);
  const code = codeEl.textContent;
  const html = await highlightCodeHtml(code, pre.getAttribute('data-language') || '');
  // 等待期间已退出编辑/换块或内容又变了：放弃本次重排（下次输入再触发）
  if (currentEditable() !== el || !codeEl.isConnected || codeEl.textContent !== code) return;
  codeEl.innerHTML = html;
  if (caretOffset != null) placeCaretAtTextOffset(codeEl, caretOffset);
}

// 光标下缘贴近/越过可见下沿（容器底部内边距 + 半行缓冲）时：
// 按最小量向下滚动，让光标恰好回到可见下沿——输入时滚动逐行跟随、
// 屏幕位置保持相对静止（不闪变）；文档末尾的 .md-bottom-space 保证能上滚
function keepCaretAboveStatusBar() {
  const root = scrollRoot.value;
  const sel = window.getSelection();
  if (!root || !sel?.rangeCount) return;
  const range = sel.getRangeAt(0);
  if (!root.contains(range.startContainer)) return;
  const rect = range.getBoundingClientRect();
  const rootRect = root.getBoundingClientRect();
  const style = getComputedStyle(root);
  const lineH = parseFloat(style.lineHeight) || 28;
  const padBottom = parseFloat(style.paddingBottom) || 16;
  // 可见内容下沿 = 容器底 - 底部内边距；再留一行半缓冲，只滚动超出部分
  const limit = rootRect.bottom - padBottom - lineH * 1.5;
  if (rect.bottom > limit) {
    root.scrollTop += rect.bottom - limit;
  }
}

// 选区符号包裹（偏好 wrap_selection）：beforeinput 拦截符号输入，包裹后内部文字保持选中；
// 不包裹的情形（IME 组合中/塌陷光标/pre 内/跨容器/纯空白）走原生输入
function onEditableBeforeInput(e) {
  if (e.inputType !== 'insertText' || !e.data || e.data.length !== 1) return;
  if (!getPref('wrap_selection')) return;
  const el = currentEditable();
  if (!el || !shouldWrapSelection(el, e.data, e)) return;
  e.preventDefault();
  // 包裹内部已派发 input 事件，onEditableInput 的既有后处理自动接力
  wrapSelectionWithSymbol(el, e.data);
}

// 粘贴/拖放图片的落盘子目录：'assets' → assets；'assets-per-document' → assets/<文档主名>
//（未保存文档无文档名时回落 assets）；其余（'document'）保存到文档目录本身（null）
function imageSubDir(behavior) {
  if (behavior === 'assets') return 'assets';
  if (behavior === 'assets-per-document') return `assets/${currentFileStem.value || 'images'}`;
  return null;
}

// 粘贴图片：按偏好保存到文档目录/assets 子目录并插入相对路径引用（未保存文档跳过）
async function onEditablePaste(e) {
  // 代码块语言输入框内粘贴：原生文本粘贴（不走 Markdown 通道/图片落盘）
  if (e.target?.closest?.('[data-lang-input]')) return;
  const items = Array.from(e.clipboardData?.items || []);
  const imageItem = items.find((item) => item.type.startsWith('image/'));
  if (!imageItem) {
    // 文本粘贴：统一规范化换行符；代码/原文块（pre 编辑）与表格编辑走浏览器默认纯文本插入
    const html = e.clipboardData?.getData('text/html') || '';
    const text = (e.clipboardData?.getData('text/plain') || '').replace(/\r\n?/g, '\n');
    const el = currentEditable();
    const rawMode = !!el?.querySelector('pre') || editingBlock.value?.type === 'table';
    // 表格矩阵覆盖粘贴：光标在表格单元格且文本含制表符时，同步阻止默认插入，
    // 经 Rust 判定为 TSV 矩阵则从光标单元格起覆盖写入；否则回退手动纯文本插入
    //（必须同步 preventDefault：IPC 是异步的，await 回来浏览器已执行默认插入会双份）
    if (rawMode) {
      const cell = editingBlock.value?.type === 'table' ? caretTableCell() : null;
      if (cell && text.includes('\t')) {
        e.preventDefault();
        const matrix = await invoke('table_matrix_from_text', { text }).catch(() => null);
        if (matrix && pasteTableMatrix(cell, matrix)) return;
        document.execCommand('insertText', false, text);
      }
      return;
    }
    // 必须同步 preventDefault：Rust 嗅探判定是异步的，若先 await 再 preventDefault，
    // 浏览器已执行默认粘贴插入一份，Markdown 通道再插一份，内容重复（双份粘贴 bug）。
    // 改为统一手动插入：Markdown 走解析通道，纯文本走 insertText
    e.preventDefault();
    // 应用内复制的渲染 HTML（data-md-fragment 标记）：DOM→DTO→序列化，格式保真往返
    if (el && html.includes('data-md-fragment')) {
      const holder = document.createElement('div');
      holder.innerHTML = html;
      const fragRoot = holder.querySelector('[data-md-fragment]') || holder;
      const md = await invoke('serialize_markdown', { blocks: domToBlockDtos(fragRoot) }).catch(() => null);
      if (md && md.trim()) {
        await pasteMarkdownAtCaret(md);
        return;
      }
    }
    if (!text) return;
    // 表格粘贴转换（偏好 paste_table_convert）：Excel/TSV/HTML 表格 → Markdown 表格。
    // 同步预过滤（无 <table 且无制表符必非表格），避免每次粘贴一次 IPC；
    // 应用内 data-md-fragment 已在上方优先处理，此处只见外部剪贴板
    if (
      getPref('paste_table_convert') &&
      (html.toLowerCase().includes('<table') || text.includes('\t'))
    ) {
      const mdTable = await invoke('table_markdown_from_clipboard', {
        text: text || null,
        html: html || null,
      }).catch(() => null);
      if (mdTable) {
        await pasteMarkdownAtCaret(mdTable);
        return;
      }
    }
    if (await looksLikeMarkdown(text)) {
      await pasteMarkdownAtCaret(text);
    } else {
      document.execCommand('insertText', false, text);
    }
    return;
  }
  e.preventDefault();
  const behavior = getPref('image_paste_behavior');
  if (behavior === 'off') return;
  const baseDir = documentDir.value;
  if (!baseDir) return;
  const file = imageItem.getAsFile();
  if (!file) return;
  // 大小上限：字节经 IPC 以数字数组传输（JS 数组 + JSON 序列化放大数倍内存），
  // 超大剪贴板图片（如整张位图截图）会撑爆渲染进程内存
  const MAX_PASTE_IMAGE_BYTES = 32 * 1024 * 1024;
  if (file.size > MAX_PASTE_IMAGE_BYTES) {
    console.warn(`粘贴图片过大（${(file.size / 1024 / 1024).toFixed(1)}MB > 32MB），已忽略`);
    return;
  }
  const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
  const ext = imageItem.type.split('/')[1] || 'png';
  const subDir = imageSubDir(behavior);
  const relPath = await invoke('save_pasted_image', { bytes, baseDir, subDir, extension: ext }).catch(() => null);
  if (relPath) insertTextAtCursor(`![](${relPath})`);
}

// Markdown 语法嗅探：判定在 Rust（looks_like_markdown，与解析器同源，规则不漂移）；
// 调用前的轻量闸门（纯单行短文本且无任何标记字符直接判否，避免每贴一次一次 IPC）
async function looksLikeMarkdown(text) {
  if (!text.includes('\n') && text.length < 200 && !/[*_`#\[\]()>|~$=<%]/.test(text)) {
    return false;
  }
  return invoke('looks_like_markdown', { markdown: text }).catch(() => false);
}

// Markdown 粘贴：光标处拆分当前块，粘贴文本作为 Markdown 经 Rust 解析插入，
// 光标落在粘贴内容之后（after 块开头；无 after 则末块末尾）
async function pasteMarkdownAtCaret(text) {
  const el = currentEditable();
  if (editingId.value === null || !el) return false;
  syncing.value = true;
  try {
    const sel = window.getSelection();
    let beforeDiv;
    let afterDiv;
    if (sel.rangeCount && el.contains(sel.getRangeAt(0).startContainer)) {
      const range = sel.getRangeAt(0);
      const beforeRange = rangeBeforePoint(el, range.startContainer, range.startOffset);
      const afterRange = range.cloneRange();
      afterRange.selectNodeContents(el);
      afterRange.setStart(range.endContainer, range.endOffset);
      beforeDiv = document.createElement('div');
      beforeDiv.append(beforeRange.cloneContents());
      afterDiv = document.createElement('div');
      afterDiv.append(afterRange.cloneContents());
    } else {
      beforeDiv = document.createElement('div');
      beforeDiv.innerHTML = el.innerHTML;
      afterDiv = document.createElement('div');
    }
    // 前后段序列化并行（超长文档减少 IPC 等待）；
    // 序列化失败：不动正文、保持编辑态（视为本次粘贴未执行）
    const [beforeMdRaw, afterMdRaw] = await Promise.all([
      isEmptyDiv(beforeDiv)
        ? Promise.resolve('')
        : invoke('serialize_markdown', { blocks: domToBlockDtos(beforeDiv) }),
      isEmptyDiv(afterDiv)
        ? Promise.resolve('')
        : invoke('serialize_markdown', { blocks: domToBlockDtos(afterDiv) }),
    ]).catch(() => null);
    if (!beforeMdRaw) return true;
    const beforeMd = beforeMdRaw.trim();
    const afterMd = afterMdRaw.trim();

    const pasted = text.trim();
    const combined = [beforeMd, pasted, afterMd].filter((s) => s !== '').join('\n\n');

    const committed = await commitCombinedRegion(combined);
    if (!committed) return true;
    const { oldBlock, replacements } = committed;

    // 光标定位：粘贴内容之后（after 块开头；无 after 则末块末尾）。
    // after 段起点锚点 = 替换起点 +（合并文本长度 - after 长度），不再为计数二次解析
    let next = null;
    if (afterMd) {
      const anchor = oldBlock.start + (combined.length - afterMd.length);
      next = replacements.find((b) => b.start != null && b.start >= anchor);
    }
    next = next ?? replacements[replacements.length - 1];
    suppressBlurCommit = true;
    editingId.value = next ? next.id : null;
    cursorAtStart = !!afterMd;
    if (!next) suppressBlurCommit = false;
    return true;
  } finally {
    syncing.value = false;
  }
}

// 编辑容器双击：callout 变体标签双击 → 转原始 Markdown 编辑（可改 [!TYPE] 切换变体）
async function onEditableDblclick(e) {
  if (!e.target.closest?.('.md-callout-label')) return;
  const el = currentEditable();
  if (!el) return;
  e.preventDefault();
  suppressBlurCommit = true;
  try {
    await convertCalloutToRawEdit(el);
  } finally {
    suppressBlurCommit = false;
  }
  el.focus({ preventScroll: true });
}

// 编辑态代码块复制按钮：mousedown 阻止默认行为（光标/焦点不转移），click 复制代码文本
const CODE_COPY_SVG =
  '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"><rect x="5" y="5" width="8.5" height="9" rx="1.2" /><path d="M10.5 5V3.5A1.5 1.5 0 009 2H4.5A1.5 1.5 0 003 3.5V11a1.5 1.5 0 001.5 1.5H5" /></svg>';
const CODE_CHECK_SVG =
  '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M3 8.5l3.5 3.5L13 4.5" /></svg>';
function onEditingMousedown(e) {
  if (e.target.closest?.('.md-code-copy-edit')) e.preventDefault();
}
function onEditingClick(e) {
  const btn = e.target.closest?.('.md-code-copy-edit');
  if (!btn) return;
  e.preventDefault();
  const code = btn.closest('pre')?.querySelector('code')?.textContent ?? '';
  navigator.clipboard.writeText(code).catch(() => {});
  // 复制反馈：短暂切换为对勾图标
  btn.innerHTML = CODE_CHECK_SVG;
  setTimeout(() => {
    if (btn.isConnected) btn.innerHTML = CODE_COPY_SVG;
  }, 1000);
}

// 语言/路径补全菜单打开时的按键路由（两者交互一致）：↑/↓ 选择（循环回绕）、
// Enter/Tab 补全应用（回调各自不同）、Esc 关闭；已处理返回 true（调用方结束本次按键分发）
function routeCompletionKeys(e, open, list, index, apply) {
  if (!open.value) return false;
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault();
    if (list.length) {
      index.value =
        e.key === 'ArrowDown'
          ? (index.value + 1) % list.length
          : (index.value - 1 + list.length) % list.length;
    }
    return true;
  }
  if (e.key === 'Enter' || e.key === 'Tab') {
    e.preventDefault();
    if (list.length) apply(list[index.value]);
    return true;
  }
  if (e.key === 'Escape') {
    e.preventDefault();
    open.value = false;
    return true;
  }
  return false;
}

function onEditableKeydown(e) {
  if (e.isComposing) return;
  // Ctrl/Cmd+Z 撤销 / Ctrl+Shift+Z 或 Ctrl+Y 重做（文档历史快照；覆盖 contenteditable 原生撤销）
  if ((e.ctrlKey || e.metaKey) && !e.altKey) {
    const k = e.key.toLowerCase();
    if (k === 'z' && !e.shiftKey) {
      e.preventDefault();
      undoHistory();
      return;
    }
    if ((k === 'z' && e.shiftKey) || k === 'y') {
      e.preventDefault();
      redoHistory();
      return;
    }
  }
  // 斜杠菜单打开时优先路由：上下选择、Enter 应用、Esc 关闭
  if (routeSlashKeys(e)) return;
  // 围栏语言补全打开时：↑/↓ 选择（循环回绕）、Enter/Tab 补全、Esc 关闭
  if (routeCompletionKeys(e, langOpen, langItems.value, langIndex, applyLangItem)) return;
  // 图片路径补全打开时：↑/↓ 选择（循环回绕）、Enter/Tab 补全、Esc 关闭（与语言补全同交互）
  if (routeCompletionKeys(e, pathOpen, pathCandidates.value, pathIndex, applyPathItem)) return;
  // 渲染链接右缘按 →：展开为原始 md 文本编辑（光标落地址末尾；继续右移出 ) 即折叠渲染）。
  // 弥补 Chromium 边界归并：右移抵达链接文本末尾时光标常被归并到链接之后的父级边界位，
  // 不经 selectionchange 展开，永远无法键盘进入地址
  if (e.key === 'ArrowRight' && !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey) {
    const el = currentEditable();
    if (el) {
      const sel = window.getSelection();
      const anchor = sel.rangeCount ? sel.anchorNode : null;
      let prev = null;
      if (anchor && sel.isCollapsed && el.contains(anchor)) {
        if (anchor.nodeType === Node.TEXT_NODE) {
          if (sel.anchorOffset === 0) prev = anchor.previousSibling;
        } else {
          prev = anchor.childNodes[sel.anchorOffset - 1];
        }
      }
      const a = prev?.nodeType === Node.ELEMENT_NODE && prev.tagName === 'A' ? prev : null;
      if (a && el.contains(a) && !a.closest('pre')) {
        e.preventDefault();
        expandLinkAtCaretInUrl(a);
        return;
      }
    }
  }
  // 代码块语言输入框：Enter/Tab/Esc 回到代码区（语言补全打开时上面的菜单路由已拦截），
  // 其余按键原生（编辑语言；输入经 onEditableInput 触发补全菜单）
  if (e.target?.closest?.('[data-lang-input]')) {
    if (e.key === 'Enter' || e.key === 'Tab' || e.key === 'Escape') {
      e.preventDefault();
      dismissLangInput();
      const pre = e.target.closest('pre');
      const code = pre?.querySelector('code');
      if (code) placeCursorAtStart(code);
      pre?.closest('[contenteditable]')?.focus({ preventScroll: true });
    }
    return;
  }
  // Ctrl/Cmd+B/I/U：加粗/斜体/下划线开关——有选区包选区；光标在同种样式内再按即退出该样式
  //（toggle 逻辑在 insertInlineWrapper，与 /菜单、右键菜单同一入口）
  if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && ['b', 'i', 'u'].includes(e.key.toLowerCase())) {
    const el = currentEditable();
    if (!el) return;
    e.preventDefault();
    const id = e.key.toLowerCase() === 'b' ? 'bold' : e.key.toLowerCase() === 'i' ? 'italic' : 'underline';
    suppressBlurCommit = true;
    try {
      insertInlineWrapper(el, id);
    } finally {
      suppressBlurCommit = false;
    }
    if (document.activeElement !== el) el.focus({ preventScroll: true });
    return;
  }
  // Ctrl/Cmd+Shift+C/V/I/K/M 与 Ctrl+T（与右键菜单标注一致）：
  // 复制为 Markdown / 粘贴为纯文本 / 插入图像 / 代码块 / 公式块 / 表格
  if ((e.ctrlKey || e.metaKey) && e.shiftKey && !e.altKey) {
    const k = e.key.toLowerCase();
    if (k === 'c') {
      e.preventDefault();
      copySelection('markdown');
      return;
    }
    if (k === 'v') {
      e.preventDefault();
      pastePlainText();
      return;
    }
    if (k === 'i') {
      e.preventDefault();
      insertBlockFromMenu('image');
      return;
    }
    if (k === 'k') {
      e.preventDefault();
      insertBlockFromMenu('codeBlock');
      return;
    }
    if (k === 'm') {
      e.preventDefault();
      insertBlockFromMenu('mathBlock');
      return;
    }
  }
  if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 't') {
    e.preventDefault();
    insertBlockFromMenu('table');
    return;
  }
  // Ctrl+A 两段式：首次交给浏览器原生（选中当前块内容）；
  // 当前块已全选时再次按下，选择整个文档内容
  if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'a') {
    const el = currentEditable();
    const sel = window.getSelection();
    if (el && sel.rangeCount) {
      const range = sel.getRangeAt(0);
      if (
        el.contains(range.commonAncestorContainer) &&
        el.textContent.length > 0 &&
        range.toString().length === el.textContent.length
      ) {
        e.preventDefault();
        const all = document.createRange();
        all.selectNodeContents(scrollRoot.value);
        sel.removeAllRanges();
        sel.addRange(all);
      }
    }
    return;
  }
  // 右键菜单打开时：Esc 优先关闭（不打断编辑）
  if (e.key === 'Escape' && ctxMenu.value) {
    e.preventDefault();
    closeCtxMenu();
    return;
  }
  // Ctrl+0..6：段落/标题级别切换（Typora 式）
  if ((e.ctrlKey || e.metaKey) && /^Digit[0-6]$/.test(e.code) && currentEditable()) {
    e.preventDefault();
    convertBlockType(e.code === 'Digit0' ? 'paragraph' : `h${e.code[5]}`);
    return;
  }
  // 表格弹出层打开时：Esc 优先关闭面板（不打断表格编辑）
  if (e.key === 'Escape' && tablePanel.value) {
    e.preventDefault();
    tablePanel.value = null;
    return;
  }
  // 表格内 Alt+方向键：移动行/列（Typora 式，与「更多操作」菜单快捷键一致）
  if (e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey && editingBlock.value?.type === 'table') {
    const op = {
      ArrowUp: 'moveRowUp',
      ArrowDown: 'moveRowDown',
      ArrowLeft: 'moveColLeft',
      ArrowRight: 'moveColRight',
    }[e.key];
    if (op) {
      e.preventDefault();
      tableOp(op);
      return;
    }
  }
  if (e.key === 'Enter') {
    const sel = window.getSelection();
    const anchor = sel.rangeCount ? sel.anchorNode : null;
    const el = anchor ? (anchor.nodeType === Node.TEXT_NODE ? anchor.parentElement : anchor) : null;
    const inPre = !!(el && el.closest('pre'));
    // 表格单元格内 Enter（无修饰键）：光标移到下一行同列单元格（末行插入新行）
    if (!e.ctrlKey && !e.metaKey && !e.shiftKey && !inPre && editingBlock.value?.type === 'table') {
      const cell = caretTableCell();
      if (cell) {
        e.preventDefault();
        moveTableRowDown(cell);
        return;
      }
    }
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      // 表格内 Ctrl+Enter：下方插入行（Typora 式）；代码块/Mermaid 等 pre：整块提交并切到新块；
      // 其余块直接整块提交
      if (!inPre && editingBlock.value?.type === 'table' && caretTableCell()) {
        tableOp('insertRowBelow');
        return;
      }
      // `<div>|</div>` 光标在开闭标签之间：Ctrl+Enter 换行展开，光标留在标签内部
      if (!inPre) {
        const editable = currentEditable();
        if (editable) {
          expandHtmlTagAtCaret(editable).then((expanded) => {
            if (!expanded) commitEdit();
          });
          return;
        }
      }
      if (inPre) commitCodeAndNewBlock();
      else commitEdit();
      return;
    }
    // 代码块/Mermaid 等 pre 内 Enter 插入换行符（行为不变）；Shift+Enter 软换行；
    // 单行原文模板（图片/链接等 pre[data-raw]）按 Enter：整块提交渲染并切到新块；
    // 普通 Enter 在光标处拆分：当前块提交渲染，光标进入新块开头
    if (inPre) {
      e.preventDefault();
      const pre = el.closest('pre');
      if (pre?.hasAttribute('data-raw') && !pre.textContent.includes('\n')) {
        commitCodeAndNewBlock();
        return;
      }
      insertTextAtCursor('\n');
      return;
    }
    if (e.shiftKey) {
      e.preventDefault();
      insertLineBreakAtCursor();
      return;
    }
    e.preventDefault();
    const editable = currentEditable();
    // 列表项内围栏行（```lang）按 Enter：该列表项就地转换为代码块编辑（优先于列表拆分）
    const fenceLi = editable ? fenceCandidateLi(editable) : null;
    if (fenceLi) {
      convertFenceInListItem(fenceLi);
      return;
    }
    // 列表项内 Enter：非空项拆出同类型新列表项续写；空项退出列表转为段落（Typora 式）
    if (editable && handleListEnter(editable)) {
      return;
    }
    // data-list-resume 段落（嵌套空项 Backspace 移出列表产生）内 Enter：在其后继续原列表
    if (editable && resumeListEnter(editable)) {
      return;
    }
    // 围栏行（```lang）按 Enter：转换为代码块并进入块内编辑，不做拆分
    if (editable) {
      convertFenceToCodeBlock(editable).then((converted) => {
        if (converted) return;
        // `<section>` 行按 Enter：补全闭合标签，进入图文排版块原文编辑
        convertSectionToHtmlBlock(editable).then((section) => {
          if (section) return;
          // `<div>|</div>` 光标在开闭标签之间按 Enter：光标跳到闭合标签后（不展开）
          skipHtmlClosingTag(editable).then((skipped) => {
            if (!skipped) splitAndCommit();
          });
        });
      });
    } else {
      splitAndCommit();
    }
  } else if (e.key === 'Backspace') {
    if (e.ctrlKey || e.metaKey || e.shiftKey || !currentEditable()) return;
    // 块内容已删空：Backspace 删除该块并跳到上一块
    if (editableEl.textContent.replace(/\n/g, '') === '') {
      e.preventDefault();
      deleteEmptyBlockAndFocusPrev();
      return;
    }
    // 光标紧贴任务勾选框之后：删除勾选框，任务项降为普通列表项
    //（浏览器原生退格无法越过 contenteditable=false 的勾选框，不拦截则无反应）
    if (deleteTaskCheckboxBeforeCaret(editableEl)) {
      e.preventDefault();
      return;
    }
    // 嵌套空列表项 + Backspace：移出列表转为段落，光标留在原行
    //（浏览器原生会并入上一行；段落标记 data-list-resume，输入后 Enter 继续列表）
    if (outdentEmptyNestedListItemToParagraph(editableEl)) {
      e.preventDefault();
      return;
    }
    // 嵌套列表项行首（非空或带子列表）+ Backspace：降低一层缩进，光标保持在本项
    //（浏览器原生并入前一项导致光标跳走）
    if (outdentNestedListItemAtCaret(editableEl)) {
      e.preventDefault();
      return;
    }
    // 顶层空列表项行首 + Backspace：退出列表转为段落，光标留在原行
    //（原生并入上一项会把光标带到上一项末尾）
    if (exitEmptyTopLevelListItemAtCaret(editableEl)) {
      e.preventDefault();
      return;
    }
    // 光标在块开头：样式块降级为段落，段落并入上一块（Typora 式退格）；
    // 容器块（callout/quote）首行是空占位段且其后还有内容时，仅删除该空行（不做块级降级/合并）
    if (isCaretAtStart(editableEl)) {
      e.preventDefault();
      const firstBlock = editableEl.querySelector('p,h1,h2,h3,h4,h5,h6,li,pre,table,blockquote,ul,ol');
      if (firstBlock?.tagName === 'P' && firstBlock.textContent.trim() === '' && firstBlock.nextElementSibling) {
        const parent = firstBlock.parentElement;
        firstBlock.remove();
        const next = parent?.querySelector('p,h1,h2,h3,h4,h5,h6,li,pre,table,blockquote,ul,ol');
        if (next) placeCursorAtStart(next);
        return;
      }
      backspaceAtBlockStart();
    }
    // 未命中任何拦截时交给浏览器原生退格
  } else if (e.key === 'Tab') {
    // Tab 默认行为是移出焦点，必须拦截：代码块内插入制表符；
    // 列表项内 Tab 缩进、Shift+Tab 取消缩进（Typora 式）；表格内 Tab 跳格
    e.preventDefault();
    const editable = currentEditable();
    if (!editable) return;
    const sel = window.getSelection();
    const anchor = sel.rangeCount ? sel.anchorNode : null;
    const el = anchor ? (anchor.nodeType === Node.TEXT_NODE ? anchor.parentElement : anchor) : null;
    if (el?.closest('pre')) {
      insertTextAtCursor('\t');
      return;
    }
    // 表格单元格内 Tab/Shift+Tab：前后跳格
    if (editingBlock.value?.type === 'table') {
      const cell = caretTableCell();
      if (cell) {
        moveTableCaret(cell, e.shiftKey ? -1 : 1);
        return;
      }
    }
    handleListTab(editable, e.shiftKey);
  } else if (e.key === 'Escape') {
    e.preventDefault();
    const el = currentEditable();
    if (el) {
      // 图片/链接模板段落（p[data-raw]）：Esc 把光标移到模板之后继续编辑（不退出整块）；
      // 哨兵文本节点作光标宿主（提交提取时剥离），输入为普通文本
      const anchor = window.getSelection().anchorNode;
      const anchorEl = anchor?.nodeType === Node.TEXT_NODE ? anchor.parentElement : anchor;
      const rawP = anchorEl?.closest?.('p[data-raw]');
      if (rawP && el.contains(rawP)) {
        const zwsp = document.createTextNode('\uFEFF');
        rawP.append(zwsp);
        const r = document.createRange();
        r.setStart(zwsp, 1);
        r.collapse(true);
        const sel = window.getSelection();
        sel.removeAllRanges();
        sel.addRange(r);
        return;
      }
      // 光标在行内样式（加粗/斜体/下划线等）内：Esc 仅退出该样式输入状态；
      // 不在样式内：维持原行为（取消整块编辑）
      if (exitInlineWrapperAtCaret(el)) return;
    }
    cancelEdit();
  }
}


// 任务列表勾选：标记替换由 Rust 完成（替换根块源码中第 occurrence 个 [ ]/[x]），
// 再增量重解析该根块。嵌套任务项没有自身源码区间：按块树 DFS 前序定位其根块与序号。
// 图片拖拽调宽回写（BlockView resize-image 事件）：zoom<99% 转为 <img style="zoom:N%"> HTML 行
//（解析器按 htmlBlock+image 往返渲染）；拉回 ≈100% 还原为 markdown 图片语法
function onImageResize({ block, zoom }) {
  const idx = blocks.value.findIndex((b) => b.id === block.id);
  if (idx < 0) return;
  const root = blocks.value[idx];
  if (root.start == null) return;
  const pct = Math.round(zoom * 100);
  const md =
    pct >= 99
      ? `![${block.image.alt || ''}](${block.image.src})`
      : `<img src="${block.image.src}" alt="${block.image.alt || ''}" style="zoom: ${pct}%">`;
  content.value = content.value.slice(0, root.start) + md + content.value.slice(root.end);
  reparseRegion(idx, root, md);
}

async function toggleTask(block) {
  if (block.type !== 'taskListItem') return;
  let rootBlock = block;
  let occurrence = 0;
  if (block.start == null) {
    const hit = locateTaskInRoot(block.id);
    if (!hit) return;
    rootBlock = hit.root;
    occurrence = hit.occurrence;
  }
  const index = blocks.value.findIndex((b) => b.id === rootBlock.id);
  if (index < 0) return;
  const source = content.value.slice(rootBlock.start, rootBlock.end);
  const next = await invoke('toggle_task_markdown', { source, checked: !block.checked, occurrence });
  if (next === source) return;
  // 竞态守卫：await 期间文档可能已被编辑（输入/其他提交），偏移体系失效则放弃本次切换
  //（重新点击时按新偏移重试），不得按旧偏移写回——否则会替换错误区间
  if (content.value.slice(rootBlock.start, rootBlock.end) !== source) return;
  content.value = content.value.slice(0, rootBlock.start) + next + content.value.slice(rootBlock.end);
  await reparseRegion(index, rootBlock, next);
}

// 合并冲突块三向接受：把冲突块源码区间替换为所选侧内容（current/incoming/both=先后拼接），
// 走与 toggleTask 相同的「区间替换 + 增量重解析」链路（竞态守卫一致：await 期间内容变了就放弃）
async function resolveConflict({ block, choice }) {
  if (block.type !== 'conflictBlock' || block.start == null) return;
  const index = blocks.value.findIndex((b) => b.id === block.id);
  if (index < 0) return;
  const c = block.conflict;
  if (!c) return;
  const chosen =
    choice === 'current' ? c.current
    : choice === 'incoming' ? c.incoming
    : [c.current, c.incoming].filter(Boolean).join('\n');
  const source = content.value.slice(block.start, block.end);
  if (!source.includes('<<<<<<<')) return; // 已非冲突块（防御）
  content.value = content.value.slice(0, block.start) + chosen + content.value.slice(block.end);
  await reparseRegion(index, block, chosen);
}

// 在根块树中定位任务项：返回其根块与 DFS 前序序号（目标为第 occurrence 个任务标记）
function locateTaskInRoot(id) {
  for (const root of blocks.value) {
    let occurrence = 0;
    let found = false;
    const dfs = (node) => {
      if (found) return;
      if (node.type === 'taskListItem') {
        if (node.id === id) {
          found = true;
          return;
        }
        occurrence += 1;
      }
      for (const child of node.children || []) dfs(child);
    };
    dfs(root);
    if (found) return { root, occurrence };
  }
  return null;
}


// 拖放图片文件进编辑器：落点在编辑容器内则插到落点光标处，否则追加到文末；
// 落盘复用 save_pasted_image（与粘贴图片同策略：偏好 image_paste_behavior）
function onEditorDragOver(e) {
  if ([...(e.dataTransfer?.types || [])].includes('Files')) e.preventDefault();
}
async function onEditorDrop(e) {
  const files = [...(e.dataTransfer?.files || [])].filter((f) => f.type.startsWith('image/'));
  if (!files.length) return;
  e.preventDefault();
  const behavior = getPref('image_paste_behavior');
  if (behavior === 'off') return;
  const baseDir = documentDir.value;
  if (!baseDir) return;
  // 落点定位：编辑容器内把光标移到落点
  const el = currentEditable();
  if (el) {
    const caret = document.caretPositionFromPoint?.(e.clientX, e.clientY);
    if (caret && el.contains(caret.offsetNode)) {
      const r = document.createRange();
      r.setStart(caret.offsetNode, caret.offset);
      r.collapse(true);
      const sel = window.getSelection();
      sel.removeAllRanges();
      sel.addRange(r);
    }
  }
  for (const file of files) {
    const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
    const ext = file.name.split('.').pop() || file.type.split('/')[1] || 'png';
    const subDir = imageSubDir(behavior);
    const relPath = await invoke('save_pasted_image', { bytes, baseDir, subDir, extension: ext }).catch(() => null);
    if (!relPath) continue;
    if (currentEditable()) {
      insertTextAtCursor(`![](${relPath})`);
    } else {
      // 非编辑态：追加到文末
      if (await appendBlocksAtEnd(`![](${relPath})`)) publishBlocks();
    }
  }
}

// 捕获当前编辑器的滚动锚点（content 字符偏移 + 视口顶部偏移），
// 用于源码/WYSIWYG 切换后尽可能保持同一文档位置。
function getLineStartBefore(text, offset) {
  let i = Math.max(0, Math.min(offset, text.length));
  while (i > 0 && text[i - 1] !== '\n') i--;
  return i;
}

// 两个字符串的最长公共前缀长度（UTF-16 码元数，与 Rust 端偏移约定一致）
function lcpLength(a, b) {
  const n = Math.min(a.length, b.length);
  let i = 0;
  while (i < n && a[i] === b[i]) i++;
  return i;
}

// 编辑态光标 → 源码精确偏移的原理（序列化对齐）：光标前 DOM 片段经
// serialize_markdown 序列化后，与整块序列化求最长公共前缀——前缀终点即
// 光标在块源码中的精确位置（前后两部分序列化在光标前必然一致，光标后开始分叉）。
// 块内 Markdown 源码偏移 → 编辑态 DOM 纯文本偏移（精确）：本地收集每个文本节点
// 结束位置的「光标前片段」DTO，经 Rust lcp_offsets 一次批量求出各位置源码偏移
//（代替逐节点往返调用），命中目标后按字符细化（节点内文本与源码基本 1:1）。
async function markdownOffsetToPlainOffset(el, target) {
  const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
  const fullDtos = domToBlockDtos(el);
  const beforeParts = [];
  const nodes = [];
  // 一趟递增构建前缀片段：每步只抽取「上一节点末尾 → 本节点末尾」的小区间并累计
  //（原为每节点从容器头重新克隆整段 → 总 O(k²) 克隆开销）；
  // 区间克隆自平衡标签，累计前缀与整段克隆的文本序列一致（空包装元素提取时丢弃）
  const prefixDiv = document.createElement('div');
  let prevNode = null;
  let node;
  while ((node = walker.nextNode())) {
    const gap = document.createRange();
    if (prevNode) gap.setStart(prevNode, prevNode.textContent.length);
    else gap.setStart(el, 0);
    gap.setEnd(node, node.textContent.length);
    prefixDiv.append(gap.cloneContents());
    beforeParts.push(domToBlockDtos(prefixDiv));
    nodes.push(node);
    prevNode = node;
  }
  const mdOffs = await invoke('lcp_offsets', { fullBlocks: fullDtos, beforeParts }).catch(() => []);
  let plain = 0;
  let prevMdOff = 0;
  for (let i = 0; i < nodes.length; i++) {
    const mdOff = mdOffs[i] ?? 0;
    if (mdOff >= target) {
      const delta = target - prevMdOff;
      return plain + Math.min(Math.max(0, delta), nodes[i].textContent.length);
    }
    plain += nodes[i].textContent.length;
    prevMdOff = mdOff;
  }
  return plain;
}

// raw Markdown 源码偏移 → 编辑态 pre 中 escapeHtml 后的 DOM 文本偏移
function rawOffsetToDomOffset(rawSource, rawOffset) {
  let domOffset = 0;
  const limit = Math.max(0, Math.min(rawOffset, rawSource.length));
  for (let i = 0; i < limit; i++) {
    const c = rawSource[i];
    if (c === '&') domOffset += 5;
    else if (c === '<') domOffset += 4;
    else if (c === '>') domOffset += 4;
    else if (c === '"') domOffset += 6;
    else domOffset += 1;
  }
  return domOffset;
}

const RAW_BLOCK_TYPES = new Set([
  'codeBlock',
  'mathBlock',
  'mermaidBlock',
  'htmlBlock',
  'sectionBlock',
  'comment',
  'conflictBlock',
  'rawMarkdown',
  'footnoteDefinition',
]);

// 计算 textarea 内指定偏移光标的精确纵向位置（内容坐标）：
// 用同宽同字体的隐藏镜像 div 测量——长段落折行后「硬换行数 × 行高」会严重偏小，
// 镜像测量把折行也算进去（窗口越窄折行越多，估算误差越大）
function caretContentTopInTextarea(source, offset) {
  const style = window.getComputedStyle(source);
  const mirror = document.createElement('div');
  mirror.style.cssText =
    'position:absolute;visibility:hidden;white-space:pre-wrap;overflow-wrap:break-word;box-sizing:border-box;' +
    `width:${source.clientWidth}px;` +
    `font-family:${style.fontFamily};font-size:${style.fontSize};font-weight:${style.fontWeight};` +
    `line-height:${style.lineHeight};letter-spacing:${style.letterSpacing};` +
    `padding:${style.padding};border-width:${style.borderWidth};tab-size:${style.tabSize};`;
  mirror.textContent = source.value.slice(0, offset);
  const marker = document.createElement('span');
  marker.textContent = '​';
  mirror.append(marker);
  document.body.append(mirror);
  const top = marker.offsetTop;
  mirror.remove();
  return top;
}

async function captureScrollPosition() {
  const source = sourceRoot.value;
  const wysiwyg = scrollRoot.value;
  if (source) {
    // 源码模式：textarea 的 selectionStart 即精确 UTF-16 偏移
    savedAnchorOffset = source.selectionStart;
    const style = window.getComputedStyle(source);
    const lineHeight = parseFloat(style.lineHeight);
    const textBefore = source.value.slice(0, savedAnchorOffset);
    const topLine = lineHeight ? (textBefore.match(/\n/g) || []).length : 0;
    savedTopMargin = source.scrollTop - topLine * lineHeight;
  } else if (wysiwyg) {
    const root = wysiwyg;
    const el = currentEditable();
    // 编辑态：先按「序列化对齐」求光标的精确源码偏移，再提交当前编辑
    //（保证源码视图内容与编辑器一致；偏移在提交后的 content 中依然成立）
    if (el && editingId.value !== null) {
      const sel = window.getSelection();
      if (sel.rangeCount && el.contains(sel.anchorNode)) {
        const range = rangeBeforePoint(el, sel.anchorNode, sel.anchorOffset);
        const beforeDiv = document.createElement('div');
        beforeDiv.append(range.cloneContents());
        const beforeMd = await invoke('serialize_markdown', { blocks: domToBlockDtos(beforeDiv) }).catch(() => '');
        // 整块序列化（含 listStart 处理）供偏移换算与提交复用（同一块不再序列化两次）
        const fullDtos = domToBlockDtos(el);
        applyListStartToDtos(fullDtos);
        const fullMdRaw = await invoke('serialize_markdown', { blocks: fullDtos }).catch(() => '');
        if (editingId.value === '__append__') {
          const fullMd = fullMdRaw.trim();
          const base = content.value ? content.value.replace(/\s*$/, '') : '';
          const anchor = base ? base.length + 2 : 0;
          savedAnchorOffset = fullMd ? anchor + lcpLength(beforeMd, fullMd) : base.length;
        } else {
          // 组编辑：序列化对齐的基准为组首块起点（beforeMd/fullMdRaw 都是整组内容）
          const g = listEdit.value;
          const head = g ? blocks.value[g.i0] : blocks.value.find((b) => b.id === editingId.value);
          savedAnchorOffset = head && head.start != null ? head.start + lcpLength(beforeMd, fullMdRaw) : 0;
        }
        const rect = sel.getRangeAt(0).getBoundingClientRect();
        savedTopMargin = rect.top - root.getBoundingClientRect().top;
        await commitEdit(fullMdRaw);
        return;
      }
    }
    // 无编辑态时回退到视口顶部块
    const els = root.querySelectorAll('[data-block-id]');
    let anchorEl = null;
    for (const el2 of els) {
      if (el2.offsetTop >= root.scrollTop) {
        anchorEl = el2;
        break;
      }
    }
    if (!anchorEl && els.length) anchorEl = els[els.length - 1];
    const blockId = anchorEl?.getAttribute('data-block-id');
    const block = blocks.value.find((b) => b.id === blockId);
    savedAnchorOffset = block?.start != null ? getLineStartBefore(content.value, block.start) : 0;
    savedTopMargin = anchorEl ? anchorEl.offsetTop - root.scrollTop : 0;
  } else {
    savedAnchorOffset = 0;
    savedTopMargin = 0;
  }
}

function restoreScrollPosition() {
  nextTick(() => {
    requestAnimationFrame(() => {
      const source = sourceRoot.value;
      const wysiwyg = scrollRoot.value;
      if (source) {
        // 镜像测量光标真实纵坐标（含折行），再居中；先滚动后放选区避免原生滚动抢占
        const caretY = caretContentTopInTextarea(source, savedAnchorOffset);
        const targetTop = Math.max(0, caretY - source.clientHeight / 2);
        source.scrollTop = targetTop;
        source.focus({ preventScroll: true });
        source.setSelectionRange(savedAnchorOffset, savedAnchorOffset);
        setTimeout(() => {
          if (source.isConnected) source.scrollTop = targetTop;
        }, 0);
      } else if (wysiwyg) {
        const root = wysiwyg;
        const block =
          blocks.value.find((b) => b.start != null && b.start <= savedAnchorOffset && (b.end ?? Infinity) >= savedAnchorOffset) ??
          blocks.value.find((b) => b.start != null && b.start >= savedAnchorOffset);
        if (!block) return;
        const el = root.querySelector(`[data-block-id="${block.id}"]`);
        if (!el) return;
        // 初始把目标块大致放到视口中央，随后 setEditableEl 会把光标精确居中
        root.scrollTop = el.offsetTop - root.clientHeight / 2;
        // 切回 WYSIWYG 后自动进入锚点块编辑态并放置光标，
        // 后续在 setEditableEl 中再把光标精确滚动到窗口中间。
        suppressBlurCommit = true;
        editingId.value = block.id;
        cursorAtStart = false;
        restoreCaretAdjustment = true;
        // 组内块：编辑容器承载整组，原始偏移按组首块起点重定基
        const gIdx = blocks.value.findIndex((b) => b.id === block.id);
        const g = gIdx >= 0 ? listGroupRangeAt(gIdx) : null;
        const baseStart = (g ? blocks.value[g.i0]?.start : block.start) ?? block.start;
        const rawOffset = Math.max(0, savedAnchorOffset - baseStart);
        if (RAW_BLOCK_TYPES.has(block.type)) {
          // 原子/原文块：raw 源码与 DOM 文本 escape 换算后 1:1（精确）
          const rawSource = content.value.slice(block.start, block.end);
          pendingCaretOffset = rawOffsetToDomOffset(rawSource, rawOffset);
        } else {
          // 富文本块：交给 setEditableEl 经「序列化对齐」精确换算（见 pendingPreciseRawOffset）
          pendingPreciseRawOffset = rawOffset;
        }
      }
    });
  });
}

// 内容目录（[TOC] 块）：文档块数据源与标题定位（BlockView 注入使用）
provide('allBlocks', blocks);



// 加载新文档（打开/新建文件）：替换全文并整树重解析，退出当前编辑态
// 加载新文档（打开/新建文件）：替换全文并整树重解析，退出当前编辑态。
// pre 为打开命令预解析的首屏块（大文件渐进加载）：直接上屏，尾部由后台
// parse_blocks 补齐（首屏末块可能被截断，尾部带完整上下文重解析后原位替换自愈）；
// 尾部返回前若已发生编辑（content 引用变化），丢弃结果走全量重解析
async function loadDocument(text, pre = null) {
  ++parseSeq; // 作废旧文档可能在途的解析结果（渐进分支不经 reparse，需手动作废）
  setHistoryLoading(true); // 加载期间不写撤销历史
  resetHistory();
  editingId.value = null;
  editableEl = null;
  rowQuota.value = INITIAL_ROW_QUOTA;
  content.value = text;
  savedContent.value = text;
  try {
    if (!pre?.blocks) {
      await reparse();
      return;
    }
    blocks.value = rawBlocks(pre.blocks);
    publishBlocks();
    if (pre.tailFrom == null) return;
    const anchor = pre.tailFrom;
    const captured = text;
    try {
      const tail = await parseAnchoredBlocks(text.slice(anchor), anchor);
      if (content.value !== captured) {
        // 尾部解析期间发生了编辑：偏移体系已变，整树重解析
        await reparse();
        return;
      }
      // 接缝自愈：尾部首块替换首屏末块
      if (blocks.value.length === 0) blocks.value.push(...tail);
      else blocks.value.splice(blocks.value.length - 1, 1, ...tail);
      publishBlocks();
    } catch (e) {
      console.error('尾部解析失败，回退全量重解析:', e);
      if (content.value === captured) await reparse();
    }
  } finally {
    setHistoryLoading(false);
  }
}

// 保存/另存：返回当前全文；保存成功后同步快照（清除脏标记）
function getContent() {
  return content.value;
}
function markSaved() {
  savedContent.value = content.value;
}

// 复制/剪切事件：选区内容写入剪贴板为 Markdown——
// 原则：非编辑块一律按块区间截取源 Markdown（渲染态 DOM 对代码行号/高亮、
// mermaid、数学、图片块等是有损的，DOM 提取无法还原；源文本零丢失）；
// 仅编辑中的块用 DOM 提取（其源码滞后于未提交的实时内容）。
// 例外：单个文本类块（段落/标题）内的部分选区走 DOM 行内提取，保留「只复制选中文字」的精度。

// 文本类块（允许 DOM 行内提取部分选区；其余块类型部分选区按整块源文本复制）
const COPY_TEXTY_TYPES = new Set(['paragraph', 'heading']);

// 选区端点（容器+偏移，兼容 selectNodeContents 形式）→ 所属根块 { id, el }
// （VRow 占位行常驻 data-block-id，未挂载行也能定位；li 等子块向上爬到根块；
// 端点落在底部留白/追加区/脚注区等无块区域时，沿兄弟链向选区内侧找最近的根块）
function rootBlockAtPoint(node, offset, isStart, rootIds) {
  const toRootBlock = (el) => {
    let cur = el?.closest?.('[data-block-id]') || null;
    while (cur && !rootIds.has(cur.getAttribute('data-block-id'))) {
      cur = cur.parentElement?.closest?.('[data-block-id]') || null;
    }
    return cur;
  };
  // 从 el 的兄弟链向选区方向找最近的根块（含后代），逐级向上爬
  const inwardSearch = (startEl) => {
    let el = startEl;
    while (el) {
      let sib = isStart ? el.nextElementSibling : el.previousElementSibling;
      while (sib) {
        if (sib.hasAttribute?.('data-block-id') && rootIds.has(sib.getAttribute('data-block-id'))) {
          return sib;
        }
        const deep = [...(sib.querySelectorAll?.('[data-block-id]') || [])].find((x) =>
          rootIds.has(x.getAttribute('data-block-id')),
        );
        if (deep) return deep;
        sib = isStart ? sib.nextElementSibling : sib.previousElementSibling;
      }
      el = el.parentElement;
    }
    return null;
  };
  let n = node;
  if (n.nodeType === Node.ELEMENT_NODE) {
    n = (isStart ? n.childNodes[offset] : n.childNodes[offset - 1]) || n;
    while (n.nodeType === Node.ELEMENT_NODE && !n.hasAttribute?.('data-block-id')) {
      const next = isStart ? n.firstElementChild : n.lastElementChild;
      if (!next) break;
      n = next;
    }
  }
  const el = n.nodeType === Node.TEXT_NODE ? n.parentElement : n;
  const blockEl = toRootBlock(el) || inwardSearch(el);
  return blockEl ? { id: blockEl.getAttribute('data-block-id'), el: blockEl } : null;
}

// 选区是否完整覆盖块元素（用于「整块覆盖 → 源截取」与「部分选区 → DOM 提取」的分流）
function rangeCoversBlock(range, blockEl) {
  const full = document.createRange();
  full.selectNodeContents(blockEl);
  return (
    range.compareBoundaryPoints(Range.START_TO_START, full) <= 0 &&
    range.compareBoundaryPoints(Range.END_TO_END, full) >= 0
  );
}

// 把选区钳制到编辑容器内（跨块选区与编辑块的交集部分做 DOM 提取）
function clampRangeToElement(range, el) {
  const sub = document.createRange();
  sub.selectNodeContents(el);
  if (el.contains(range.startContainer)) sub.setStart(range.startContainer, range.startOffset);
  if (el.contains(range.endContainer)) sub.setEnd(range.endContainer, range.endOffset);
  return sub;
}

// DOM 提取通道：选区片段 → DTO → Rust 序列化（仅用于编辑块与文本块行内选区）
async function domRangeToMarkdown(range) {
  const div = document.createElement('div');
  div.append(range.cloneContents());
  const md = await invoke('serialize_markdown', { blocks: domToBlockDtos(div) }).catch(() => null);
  return md != null && md.trim() !== '' ? md : null;
}

// 选区 → Markdown 主入口：按覆盖的根块区间逐块拼装
// （编辑块 DOM 提取 + 其余块源文本截取，块间空行分隔）
async function selectionToMarkdown(range) {
  const rootIds = new Set(blocks.value.map((b) => b.id));
  let start = rootBlockAtPoint(range.startContainer, range.startOffset, true, rootIds);
  let end = rootBlockAtPoint(range.endContainer, range.endOffset, false, rootIds);
  // 两端都不在块区域（如追加区内的局部选择）：DOM 提取
  if (!start && !end) return domRangeToMarkdown(range);
  // 一端落到非块区域（选到文档外沿）：钳制到首/末根块
  if (!start) start = { id: blocks.value[0]?.id, el: null };
  if (!end) end = { id: blocks.value[blocks.value.length - 1]?.id, el: null };
  let i1 = blocks.value.findIndex((b) => b.id === start.id);
  let i2 = blocks.value.findIndex((b) => b.id === end.id);
  if (i1 === -1 || i2 === -1) return domRangeToMarkdown(range);
  if (i1 > i2) [i1, i2] = [i2, i1];
  const editId = editingId.value;

  // 单个文本块（非编辑态）的部分选区：DOM 行内提取，只复制选中的文字
  if (i1 === i2) {
    const block = blocks.value[i1];
    if (
      start.el &&
      block.id !== editId &&
      COPY_TEXTY_TYPES.has(block.type) &&
      !rangeCoversBlock(range, start.el)
    ) {
      return domRangeToMarkdown(range);
    }
  }

  // 无编辑块混入：整段连续截取源文——块间原始分隔（列表项单换行/段落空行）原样保留
  const first = blocks.value[i1];
  const last = blocks.value[i2];
  const editIdx = blocks.value.findIndex((b, i) => i >= i1 && i <= i2 && b.id === editId);
  if (editIdx === -1 && first.start != null && last.end != null) {
    const md = content.value.slice(first.start, last.end);
    return md.trim() !== '' ? md : null;
  }

  // 编辑块在选区内：逐块拼装，块间分隔取源文中的原始间隔（不自行造空行——
  // 列表项间是单换行，若统一加空行粘贴后会变成松散列表/断成多列）
  const parts = [];
  const editable = currentEditable();
  for (let i = i1; i <= i2; i++) {
    const b = blocks.value[i];
    if (b.id === editId && editable) {
      const part = await domRangeToMarkdown(clampRangeToElement(range, editable));
      parts.push(part ?? '');
    } else if (b.start != null && b.end != null) {
      parts.push(content.value.slice(b.start, b.end));
    } else {
      parts.push('');
    }
    if (i < i2) {
      const next = blocks.value[i + 1];
      parts.push(
        b.end != null && next.start != null && next.start >= b.end
          ? content.value.slice(b.end, next.start)
          : '\n\n',
      );
    }
  }
  const md = parts.join('').trim();
  return md !== '' ? md : null;
}

async function writeSelectionToClipboard(e, sel) {
  e.preventDefault();
  const range = sel.getRangeAt(0);
  // 多格式剪贴板：
  // text/plain = 纯净可见文本（粘贴到外部应用不带 Markdown 语法/转义符）；
  // text/html  = 渲染富文本（Word/浏览器富粘贴；应用内粘贴经此通道还原格式——
  //              data-md-fragment 标记包裹，onEditablePaste 识别后走 DOM→DTO→Markdown 通道）
  const div = document.createElement('div');
  div.append(range.cloneContents());
  const html = `<div data-md-fragment>${div.innerHTML}</div>`;
  const plain = sel.toString();
  const ok = await navigator.clipboard
    .write([
      new ClipboardItem({
        'text/plain': new Blob([plain], { type: 'text/plain' }),
        'text/html': new Blob([html], { type: 'text/html' }),
      }),
    ])
    .then(() => true)
    .catch(() => false);
  if (!ok) {
    // 回退（旧 WebView 等）：Markdown 源（应用内粘贴可保真），再退纯文本
    const md = await selectionToMarkdown(range).catch(() => null);
    await navigator.clipboard.writeText(md && md !== '' ? md : plain).catch(() => {});
  }
}

async function onDocumentCopy(e) {
  if (props.sourceMode) return;
  const sel = window.getSelection();
  if (!sel.rangeCount || sel.isCollapsed) return;
  const root = scrollRoot.value;
  if (!root || !root.contains(sel.anchorNode)) return;
  await writeSelectionToClipboard(e, sel);
}

// 剪切：与复制同通道写剪贴板；选区完整落在编辑容器内时原生删除，
// 跨块/整块选区走根块区间删除（与 Delete/Backspace 同通道）——否则剪切=复制
async function onDocumentCut(e) {
  if (props.sourceMode) return;
  const sel = window.getSelection();
  if (!sel.rangeCount || sel.isCollapsed) return;
  const root = scrollRoot.value;
  if (!root || !root.contains(sel.anchorNode)) return;
  await writeSelectionToClipboard(e, sel);
  const el = currentEditable();
  const range = sel.getRangeAt(0);
  if (el && el.contains(range.startContainer) && el.contains(range.endContainer)) {
    range.deleteContents();
    return;
  }
  await deleteBlockSelection(range);
}

// 未处于编辑态时的 Ctrl+A：只选择编辑器文档内容（不含侧栏/状态栏界面文字）；
// Delete/Backspace：删除跨块/整块选区（选区完整落在编辑容器内时交给原生编辑行为）
function onDocumentKeydown(e) {
  if (props.sourceMode) return;
  // Ctrl/Cmd+F 查找 / Ctrl+Cmd+H 替换（FindBar 输入框内按键由其自处理）
  if ((e.ctrlKey || e.metaKey) && !e.altKey && ['f', 'h'].includes(e.key.toLowerCase())) {
    if (e.target.closest?.('.find-bar')) return;
    e.preventDefault();
    findWithReplace.value = e.key.toLowerCase() === 'h';
    findVisible.value = true;
    return;
  }
  // 非编辑态的撤销/重做（输入框/文本域/编辑容器交原生与编辑态路由）
  if ((e.ctrlKey || e.metaKey) && !e.altKey && ['z', 'y'].includes(e.key.toLowerCase())) {
    if (e.target.closest?.('input, textarea, [contenteditable="true"]')) return;
    e.preventDefault();
    if (e.shiftKey || e.key.toLowerCase() === 'y') redoHistory();
    else undoHistory();
    return;
  }
  if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'a') {
    // 输入框/文本域/编辑容器内：原生全选（contenteditable 天然只选当前编辑块）
    if (e.target.closest?.('input, textarea, [contenteditable="true"]')) return;
    e.preventDefault();
    // 非编辑态 Ctrl+A：只选光标/选区所在的当前块（不含侧栏/状态栏，也不扩到全文）；
    // 无所在块（焦点不在任何块上）时不动作
    const sel = window.getSelection();
    const anchorEl = sel?.anchorNode
      ? sel.anchorNode.nodeType === Node.TEXT_NODE
        ? sel.anchorNode.parentElement
        : sel.anchorNode
      : null;
    const block = anchorEl?.closest?.('.md-block');
    if (!block) return;
    const all = document.createRange();
    all.selectNodeContents(block);
    sel.removeAllRanges();
    sel.addRange(all);
    return;
  }
  if ((e.key === 'Delete' || e.key === 'Backspace') && !e.ctrlKey && !e.metaKey && !e.altKey) {
    if (e.target.closest?.('input, textarea')) return;
    const sel = window.getSelection();
    if (!sel?.rangeCount || sel.isCollapsed) return;
    const root = scrollRoot.value;
    if (!root || !root.contains(sel.anchorNode)) return;
    const range = sel.getRangeAt(0);
    const editEl = currentEditable();
    // 选区完整落在编辑容器内：块内文本删除，交给 contenteditable 原生行为
    if (editEl && editEl.contains(range.startContainer) && editEl.contains(range.endContainer)) return;
    e.preventDefault();
    deleteBlockSelection(range);
  }
}

// 跨块/整块选区删除：移除选区覆盖的根块区间（源文接缝规范化为一个空行），
// 光标落在接缝处的块首（删到文档末尾则落末块块尾；删光则进入追加区）；
// 选区内含正在编辑的块时退出编辑态（未提交编辑随之放弃）
async function deleteBlockSelection(range) {
  const rootIds = new Set(blocks.value.map((b) => b.id));
  const start = rootBlockAtPoint(range.startContainer, range.startOffset, true, rootIds);
  const end = rootBlockAtPoint(range.endContainer, range.endOffset, false, rootIds);
  if (!start || !end) return;
  let i1 = blocks.value.findIndex((b) => b.id === start.id);
  let i2 = blocks.value.findIndex((b) => b.id === end.id);
  if (i1 === -1 || i2 === -1) return;
  if (i1 > i2) [i1, i2] = [i2, i1];
  const first = blocks.value[i1];
  const last = blocks.value[i2];
  if (first.start == null || last.end == null) return;
  const editCovered = blocks.value.slice(i1, i2 + 1).some((b) => b.id === editingId.value);
  syncing.value = true;
  try {
    if (editCovered) {
      editableEl = null;
      editingId.value = null;
    }
    const before = content.value.slice(0, first.start).replace(/\s+$/, '');
    const after = content.value.slice(last.end).replace(/^\s+/, '');
    content.value = [before, after].filter((s) => s !== '').join('\n\n');
    await reparse();
    window.getSelection()?.removeAllRanges();
    const next = blocks.value[Math.min(i1, blocks.value.length - 1)];
    suppressBlurCommit = true;
    if (next) {
      cursorAtStart = i1 < blocks.value.length;
      editingId.value = next.id;
    } else {
      suppressBlurCommit = false;
      editingId.value = '__append__';
    }
  } finally {
    syncing.value = false;
  }
}

// 表格编辑辅助监听：selectionchange 跟踪光标所在列对齐（工具栏高亮）；
// 捕获阶段 mousedown 实现「点击工具栏/面板外部关闭弹出层」
// 编辑中光标移动（方向键/鼠标点击）也保持光标在状态栏以上：
// 输入有 input 事件覆盖，而纯移动没有——rAF 合帧避免高频 selectionchange 抖动
let keepCaretRaf = 0;
function rafKeepCaret() {
  if (keepCaretRaf) return;
  keepCaretRaf = requestAnimationFrame(() => {
    keepCaretRaf = 0;
    keepCaretAboveStatusBar();
  });
}
function onDocumentSelectionChange() {
  if (editingBlock.value?.type === 'table') updateTableCaretAlign();
  if (editingId.value !== null) {
    rafKeepCaret();
    updateInlineSourceReveal();
  }
}

function onDocumentMouseDown(e) {
  if (ctxMenu.value && !e.target.closest?.('.md-ctx-menu')) {
    closeCtxMenu();
  }
  if (tablePanel.value && !e.target.closest?.('.md-table-panel, .md-table-toolbar')) {
    tablePanel.value = null;
  }
}
onMounted(() => {
  document.addEventListener('selectionchange', onDocumentSelectionChange);
  document.addEventListener('mousedown', onDocumentMouseDown, true);
  document.addEventListener('keydown', onDocumentKeydown);
  document.addEventListener('copy', onDocumentCopy);
  document.addEventListener('cut', onDocumentCut);
});
onBeforeUnmount(() => {
  document.removeEventListener('selectionchange', onDocumentSelectionChange);
  document.removeEventListener('mousedown', onDocumentMouseDown, true);
  document.removeEventListener('keydown', onDocumentKeydown);
  document.removeEventListener('copy', onDocumentCopy);
  document.removeEventListener('cut', onDocumentCut);
  // 全部定时器/空闲句柄一并清理（多标签/多窗口复用时防泄漏）
  clearTimeout(statsTimer);
  clearTimeout(blurCommitTimer);
  clearTimeout(editPreviewTimer);
  clearTimeout(codeHlTimer);
  clearTimeout(printRestoreTimer);
  cancelAnimationFrame(keepCaretRaf);
  keepCaretRaf = 0;
  if (statsIdleHandle != null) {
    cancelIdleCallback(statsIdleHandle);
    statsIdleHandle = null;
  }
});

// 打印模式：强制所有行挂载（虚拟滚动下未挂载行不会被打印），
// 配合 @media print 样式（隐藏界面元素）实现「打印当前文档」
const printMode = ref(false);
provide('printMode', printMode);

// 打印当前文档：提交当前编辑 → 全量挂载所有行 → 打印；结束（或超时兜底）后恢复虚拟滚动
async function printDocument() {
  if (editingId.value !== null) await commitEdit();
  rowQuota.value = renderedBlocks.value.length;
  printMode.value = true;
  await nextTick();
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  const restore = () => {
    window.removeEventListener('afterprint', restore);
    clearTimeout(printRestoreTimer);
    printMode.value = false;
    rowQuota.value = INITIAL_ROW_QUOTA;
  };
  window.addEventListener('afterprint', restore);
  clearTimeout(printRestoreTimer);
  // afterprint 不触发时的兜底恢复
  printRestoreTimer = setTimeout(restore, 60_000);
  window.print();
}
let printRestoreTimer = null;

defineExpose({ scrollToBlock, loadDocument, getContent, markSaved, isDirty: () => isDirty.value, captureScrollPosition, restoreScrollPosition, printDocument, undo: undoHistory, redo: redoHistory, openFind });
</script>

<template>
  <div class="relative flex h-full flex-col">
    <!-- 源码模式：flex 行容器（行号 gutter + textarea + Ctrl+G 跳行浮动框） -->
    <div v-if="sourceMode" class="md-src-wrap">
      <div v-if="lineNumbersOn" ref="gutterRoot" class="t-root font-mono md-src-gutter" aria-hidden="true"><div v-for="(line, i) in gutterLines" :key="i">{{ i + 1 }}</div></div>
      <textarea
        ref="sourceRoot"
        class="t-root flex-1 resize-none border-none font-mono outline-none"
        v-model="content"
        @input="onInput"
        @keydown="onSourceKeydown"
        @keyup="onInput"
        @click="onInput"
      ></textarea>
      <!-- Ctrl+G 跳行浮动框（Enter 跳行，Esc 关闭） -->
      <div v-if="jumpOpen" class="md-src-jump">
        <span class="md-src-jump-label">跳转到行</span>
        <input ref="jumpInput" v-model="jumpText" type="text" inputmode="numeric" autocomplete="off" @keydown="onJumpKeydown" />
      </div>
    </div>

    <div v-else ref="scrollRoot" tabindex="0" class="t-root flex-1 overflow-y-auto outline-none" :class="{ 'md-first-indent': firstLineIndent, 'sb-auto-hide': scrollbarAutoHide, 'sb-no-autohide': !scrollbarAutoHide }" @scroll.passive="onEditorScroll" @contextmenu="onContextMenu" @mousedown="onEditorMouseDown" @mousedown.capture="onCtrlLinkMousedown" @click.capture="onCtrlLinkClick" @mousemove.passive="onEditorMouseMove" @mouseleave="onEditorMouseLeave" @dragover="onEditorDragOver" @drop="onEditorDrop">
      <div class="t-measure">
      <!-- 虚拟滚动 + 渐进挂载：配额内每块包一层 VRow（占位保总高），仅视口余量内挂载内容；
           配额外行由下方估计高度占位条代替（大文档首屏组件规模恒定）；
           编辑块经 force 强制渲染（不可卸载，否则丢失光标与未提交内容） -->
      <VRow
        v-for="{ block, estimate } in visibleRows"
        :key="block.id"
        :block="block"
        :estimate="estimate"
        :force="editHostBlockId === block.id"
      >
        <!-- Typora 式就地编辑：渲染后的内容直接在 contenteditable 中编辑；
             列表组编辑：宿主在组首块行（含整组内容），组内其余行不渲染 -->
        <div
          class="md-block-host"
          @mouseover="block.type === 'table' && onTableBlockHover(block)"
          @mousemove="block.type === 'table' && onTableEdgeMove(block, $event)"
          @mouseleave="block.type === 'table' && onTableBlockLeave(block)"
        >
          <!-- 表格工具栏（Typora 式：左侧 尺寸/对齐，右侧 更多操作/删除；mousedown.prevent 保持单元格光标）。
               编辑中与鼠标滑过（未编辑）均以同一内联形式显示在表格上方；
               悬停态点击按钮先进入编辑、再执行动作 -->
          <div
            v-if="block.type === 'table' && (editingId === block.id || hoverTableId === block.id)"
            class="md-table-toolbar"
            @mousedown.prevent
            @click.stop
          >
            <div class="md-table-toolbar-group">
              <button
                type="button"
                class="md-table-btn tbl-resize"
                :class="{ active: tablePanel === 'grid' }"
                title="调整表格尺寸"
                @click="onToolbarPanel(block, 'grid', $event)"
              >
                <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3">
                  <rect x="1.5" y="1.5" width="5.6" height="5.6" rx="1" />
                  <rect x="8.9" y="1.5" width="5.6" height="5.6" rx="1" />
                  <rect x="1.5" y="8.9" width="5.6" height="5.6" rx="1" />
                  <rect x="8.9" y="8.9" width="5.6" height="5.6" rx="1" />
                </svg>
              </button>
              <button
                type="button"
                class="md-table-btn tbl-align"
                :class="{ active: tableCaretAlign === 'left' }"
                title="整列左对齐"
                @click="onToolbarOp(block, 'alignLeft')"
              >
                <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
                  <path d="M2 4h12M2 8h7M2 12h9.5" />
                </svg>
              </button>
              <button
                type="button"
                class="md-table-btn tbl-align"
                :class="{ active: tableCaretAlign === 'center' }"
                title="整列居中"
                @click="onToolbarOp(block, 'alignCenter')"
              >
                <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
                  <path d="M2 4h12M4.5 8h7M3.5 12h9" />
                </svg>
              </button>
              <button
                type="button"
                class="md-table-btn tbl-align"
                :class="{ active: tableCaretAlign === 'right' }"
                title="整列右对齐"
                @click="onToolbarOp(block, 'alignRight')"
              >
                <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
                  <path d="M2 4h12M7 8h7M4.5 12h9.5" />
                </svg>
              </button>
            </div>
            <div class="md-table-toolbar-group">
              <button
                type="button"
                class="md-table-btn tbl-more"
                :class="{ active: tablePanel === 'more' }"
                title="更多操作"
                @click="onToolbarPanel(block, 'more', $event)"
              >
                <span>更多操作</span>
                <svg viewBox="0 0 16 16" fill="currentColor">
                  <circle cx="8" cy="3.2" r="1.4" />
                  <circle cx="8" cy="8" r="1.4" />
                  <circle cx="8" cy="12.8" r="1.4" />
                </svg>
              </button>
              <button type="button" class="md-table-btn tbl-delete" title="删除表格" @click="onToolbarDelete(block)">
                <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M2.5 4h11M6.3 4V2.9c0-.5.4-.9.9-.9h1.6c.5 0 .9.4.9.9V4M4.2 4l.6 9.1c0 .6.5 1 1 1h4.4c.5 0 1-.4 1-1L11.8 4M6.8 7v4M9.2 7v4" />
                </svg>
              </button>
            </div>
          </div>
          <!-- 单元格四边 + 按钮：靠近上边→上方插行 / 下边→下方插行 / 左边→左侧插列 / 右边→右侧插列 -->
          <button
            v-if="tableEdgeBtn && tableEdgeBtn.blockId === block.id"
            type="button"
            class="md-table-edge-btn"
            :style="{ top: `${tableEdgeBtn.top}px`, left: `${tableEdgeBtn.left}px` }"
            :title="{ rowAbove: '上方插入行', rowBelow: '下方插入行', colLeft: '左侧插入列', colRight: '右侧插入列' }[tableEdgeBtn.kind]"
            @mousedown.prevent
            @click.stop="onTableEdgeAdd"
          >
            <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round">
              <path d="M6 1.5v9M1.5 6h9" />
            </svg>
          </button>
        <template v-if="editHostBlockId === block.id">
          <div
            :ref="setEditableEl"
            class="md-editing px-1 outline-none"
            contenteditable="true"
            :spellcheck="spellcheckOn"
            @input="onEditableInput"
            @beforeinput="onEditableBeforeInput"
            @paste="onEditablePaste"
            @keydown="onEditableKeydown"
            @dblclick="onEditableDblclick"
            @click="onEditingClick"
            @mousedown="onEditingMousedown"
            @blur="onEditableBlur"
          ></div>
          <!-- 公式/Mermaid/PlantUML 编辑实时预览（编辑区下方浮动面板；输入防抖更新，
               语法未完成时保持上一帧）。右上角眼睛图标开关预览；点击预览框本身也可关闭 -->
          <button
            v-if="editPreviewKind(block)"
            type="button"
            class="md-code-copy md-math-preview-toggle"
            :title="editPreviewHidden ? '显示预览' : '隐藏预览'"
            @mousedown.prevent
            @click.stop="editPreviewHidden = !editPreviewHidden"
          >
            <svg v-if="!editPreviewHidden" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">
              <path d="M1.8 8s2.4-4.3 6.2-4.3S14.2 8 14.2 8 11.8 12.3 8 12.3 1.8 8 1.8 8z" />
              <circle cx="8" cy="8" r="1.8" />
            </svg>
            <svg v-else viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">
              <path d="M3 3l10 10" />
              <path d="M6.2 4.2A6.7 6.7 0 018 3.7c3.8 0 6.2 4.3 6.2 4.3a11.6 11.6 0 01-2.1 2.6M9.8 11.8A6.7 6.7 0 018 12.3C4.2 12.3 1.8 8 1.8 8a11.6 11.6 0 012.1-2.6" />
            </svg>
          </button>
          <div
            v-if="editPreviewKind(block) && editPreviewSvg && !editPreviewHidden"
            class="md-math-preview"
            title="点击隐藏预览"
            @click="editPreviewHidden = true"
            v-html="editPreviewSvg"
          ></div>
        </template>
        <!-- 列表组编辑：组内非宿主行不渲染（内容在组首行的编辑容器内） -->
        <template v-else-if="editHiddenIds?.has(block.id)"></template>
        <div
          v-else
          class="md-block cursor-text px-1"
          :class="{ 'md-flash': flashId === block.id }"
          @click="startEdit(block)"
        >
          <BlockView :block="block" :ordinal="rootOrdinals.get(block.id) || 1" @toggle-task="toggleTask" @resize-image="onImageResize" @resolve-conflict="resolveConflict" />
        </div>
        </div>
      </VRow>
      <!-- 配额外行的占位条：估计高度和（渐进挂载期间保持文档总高近似正确） -->
      <div v-if="tailPadHeight > 0" :style="{ height: `${tailPadHeight}px` }" aria-hidden="true"></div>

      <div
        v-if="editingId === '__append__'"
        :ref="setEditableEl"
        class="md-editing px-1 outline-none"
        contenteditable="true"
        :spellcheck="spellcheckOn"
        @input="onEditableInput"
        @beforeinput="onEditableBeforeInput"
        @paste="onEditablePaste"
        @keydown="onEditableKeydown"
        @click="onEditingClick"
        @mousedown="onEditingMousedown"
        @blur="onEditableBlur"
      ></div>
      <div v-else class="min-h-24 cursor-text p-4" @click="startAppend"></div>
      <!-- 脚注定义集中显示在文档末尾，支持点击引用跳转与返回 -->
      <div v-if="footnotes.length" class="md-footnotes">
        <div class="md-footnotes-title">脚注</div>
        <div
          v-for="block in footnotes"
          :key="block.id"
          class="md-block cursor-text px-1"
          :class="{ 'md-flash': flashId === 'fn-def-' + plainText(block.title) }"
          :data-block-id="block.id"
          @click="startEdit(block)"
        >
          <BlockView :block="block" :ordinal="1" @toggle-task="toggleTask" @resize-image="onImageResize" @resolve-conflict="resolveConflict" />
        </div>
      </div>
      <!-- 文档末尾可滚空间：让最后一行能上移到标题栏下方（触底输入时平滑跟随不闪变） -->
      <div class="md-bottom-space" aria-hidden="true"></div>
      </div>
    </div>
    <!-- 覆盖层滚动条：彩色可拖动滑轨（自动隐藏模式下替代原生条；滚动/右缘悬停时显示） -->
    <div v-if="!sourceMode" class="md-scrollbar" :class="{ show: sbShow, dragging: sbDragging }" aria-hidden="true" @pointerdown="sbOnTrackPointerDown">
      <div class="md-scrollbar-thumb" :style="{ top: `${sbThumb.top}px`, height: `${sbThumb.height}px` }" @pointerdown.stop="sbOnThumbPointerDown"></div>
    </div>
    <!-- 改动审阅概览尺：右缘竖条，按变更在全文的比例位置标色（VSCode minimap 式比例视图） -->
    <div v-if="!sourceMode && reviewEnabled" class="md-chg-ruler" aria-hidden="true" :title="`+${changeCounts.added} ~${changeCounts.modified} -${changeCounts.deleted}`">
      <div
        v-for="(tick, i) in rulerTicks"
        :key="i"
        class="md-chg-ruler-tick"
        :class="{ 'md-chg-ruler-modified': tick.kind === 'modified', 'md-chg-ruler-deleted': tick.kind === 'deleted' }"
        :style="{ top: `${tick.topPct}%` }"
      ></div>
    </div>
    <!-- 返回顶部浮动按钮（仅 WYSIWYG，滚动超过 600px 显示） -->
    <button v-if="!sourceMode && backTopVisible" type="button" class="md-back-top" aria-label="返回顶部" title="返回顶部" @click="scrollToTop">
      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M8 12.5v-9M3.5 7 8 2.5 12.5 7" /></svg>
    </button>
    <!-- 改动审阅基线菜单：右上浮动 ± 计数按钮 + 下拉（比较方式：关闭/最近保存/Git HEAD/手动快照；显示设置） -->
    <div class="md-chg-menubox">
      <button type="button" class="md-chg-menubtn" :class="{ 'md-chg-off': !chgReviewOn }" title="改动审阅：比较基线与计数" @click="toggleChgMenu">
        <span class="md-chg-menubtn-icon">±</span>
        <template v-if="chgReviewOn && reviewEnabled">+{{ changeCounts.added }} ~{{ changeCounts.modified }} -{{ changeCounts.deleted }}</template>
        <template v-else>比较</template>
        <span class="md-chg-menubtn-caret">▾</span>
      </button>
      <div v-if="chgMenuOpen" class="md-chg-backdrop" @click="chgMenuOpen = false"></div>
      <div v-if="chgMenuOpen" class="md-chg-menu" @click.stop>
        <div v-if="chgReviewOn && reviewEnabled" class="md-chg-menu-head">+{{ changeCounts.added }} ~{{ changeCounts.modified }} -{{ changeCounts.deleted }} · 与{{ baselineTitle }}对比</div>
        <div class="md-chg-menu-sect">比较方式</div>
        <button type="button" class="md-chg-menu-item" :class="{ active: !chgReviewOn }" @click="closeReview">
          <span>关闭比较</span><span v-if="!chgReviewOn" class="md-chg-check">✓</span>
        </button>
        <button type="button" class="md-chg-menu-item" :class="{ active: chgReviewOn && baselineKind === 'saved' }" @click="setBaseline('saved'); chgMenuOpen = false">
          <span>与最近保存版本比较</span><span v-if="chgReviewOn && baselineKind === 'saved'" class="md-chg-check">✓</span>
        </button>
        <button type="button" class="md-chg-menu-item" :class="{ active: chgReviewOn && baselineKind === 'git' }" :disabled="gitDisabled" @click="setBaseline('git'); chgMenuOpen = false">
          <span>与 Git HEAD 比较<span class="t-dim">{{ gitSuffix }}</span></span><span v-if="chgReviewOn && baselineKind === 'git'" class="md-chg-check">✓</span>
        </button>
        <div class="md-chg-menu-item md-chg-manual-row" :class="{ active: chgReviewOn && baselineKind === 'manual' }">
          <button type="button" class="md-chg-manual-main" :title="manualSnapshotAvailable ? '' : '尚无快照，点击即拍'" @click="!manualSnapshotAvailable && captureManualSnapshot(); setBaseline('manual'); chgMenuOpen = false">
            与手动快照比较<span v-if="manualTimeLabel" class="t-dim">（{{ manualTimeLabel }}）</span>
          </button>
          <button type="button" class="md-chg-menu-mini" title="以当前内容重拍快照" @click.stop="captureManualSnapshot()">更新</button>
          <span v-if="chgReviewOn && baselineKind === 'manual'" class="md-chg-check">✓</span>
        </div>
        <div class="md-chg-menu-sect">显示设置</div>
        <label class="md-chg-menu-item md-chg-toggle">
          <span>显示修改前内容</span>
          <input type="checkbox" :checked="showOldContent" @change="setPref('change_review_show_old', $event.target.checked)" />
        </label>
      </div>
    </div>
  </div>
  <!-- 斜杠命令菜单 / 围栏语言补全菜单（同一面板，互斥触发）：Teleport 移出根布局（多根片段），其挂载/卸载不再引起
       布局子节点重建；.t-app 包装使主题 blocks 规则（.t-app 作用域）同样生效 -->
  <Teleport to="body">
    <div v-if="slashOpen || langOpen || pathOpen" class="t-app">
      <SlashMenu
        :items="langOpen ? langMenuItems : pathOpen ? pathMenuItems : slashItems"
        :index="langOpen ? langIndex : pathOpen ? pathIndex : slashIndex"
        :text-row="slashTextRow"
        :text-col="slashTextCol"
        :table-rows="slashTableRows"
        :table-cols="slashTableCols"
        :table-field="slashTableField"
        :callout-type="slashCalloutType"
        :font-row="slashFontRow"
        :font-color-index="slashFontColorIndex"
        :font-size-index="slashFontSizeIndex"
        :rgb-error="rgbError"
        :left="langOpen ? langPos.left : pathOpen ? pathPos.left : slashPos.left"
        :top="langOpen ? langPos.top : pathOpen ? pathPos.top : slashPos.top"
        @pick="(item, option) => (langOpen ? applyLangItem(item.id) : pathOpen ? applyPathItem(item.id) : applySlashItem(item, option))"
        @hover="(i) => (langOpen ? (langIndex = i) : pathOpen ? (pathIndex = i) : (slashIndex = i))"
        @text-cell="(c) => ((slashTextRow = c.row), (slashTextCol = c.col))"
        @table-field="(f) => (slashTableField = f)"
        @callout-type="(i) => (slashCalloutType = i)"
        @font-cell="(c) => ((slashFontRow = c.row), c.row === 1 ? (slashFontSizeIndex = c.col) : (slashFontColorIndex = c.col))"
        @rgb-apply="(text) => applyRgbInput(text)"
        @rgb-focus="onRgbFocus"
        @rgb-cancel="onRgbCancel"
        @rgb-nav="(d) => onRgbNav(d)"
      />
    </div>
  </Teleport>
  <!-- 表格工具栏弹出层：尺寸网格选择器 / 更多操作菜单（Teleport 移出根布局；
       @mousedown.prevent 保持单元格光标，外部点击经 document 捕获监听关闭） -->
  <Teleport to="body">
    <div v-if="tablePanel" class="t-app">
      <div
        v-if="tablePanel === 'grid'"
        class="md-table-panel md-table-grid-panel"
        :style="{ left: tablePanelPos.left + 'px', top: tablePanelPos.top + 'px' }"
        @mousedown.prevent
      >
        <div
          class="md-table-grid"
          :style="{ gridTemplateColumns: `repeat(${TABLE_GRID_COLS}, 16px)` }"
          @mouseleave="tableGridHover = { ...tableGridSize }"
        >
          <template v-for="r in TABLE_GRID_ROWS" :key="r">
            <span
              v-for="c in TABLE_GRID_COLS"
              :key="c"
              class="md-table-grid-cell"
              :class="{ on: r <= tableGridHover.rows && c <= tableGridHover.cols }"
              @mouseenter="tableGridHover = { rows: r, cols: c }"
              @click="applyTableGrid"
            ></span>
          </template>
        </div>
        <div class="md-table-grid-indicator">{{ tableGridHover.rows }} x {{ tableGridHover.cols }}</div>
      </div>
      <div
        v-else
        class="md-table-panel md-table-menu"
        :style="{ left: tablePanelPos.left + 'px', top: tablePanelPos.top + 'px' }"
        @mousedown.prevent
      >
        <template v-for="(item, i) in TABLE_MORE_ITEMS" :key="i">
          <div v-if="item === 'sep'" class="md-table-menu-sep"></div>
          <button v-else type="button" class="md-table-menu-item" @click="runTableMore(item)">
            <span>{{ item.label }}</span>
            <span v-if="item.shortcut" class="md-table-menu-shortcut">{{ formatShortcut(item.shortcut) }}</span>
          </button>
        </template>
      </div>
    </div>
  </Teleport>
  <!-- 编辑器右键菜单（剪贴板 / 复制粘贴为 / 格式 / 段落 / 插入） -->
  <ContextMenu
    v-if="ctxMenu"
    :x="ctxMenu.x"
    :y="ctxMenu.y"
    :current-type="ctxCurrentType"
    @action="onMenuAction"
  />
  <!-- 文档内查找/替换浮动条（Ctrl+F / Ctrl+H） -->
  <FindBar
    :visible="findVisible"
    :with-replace="findWithReplace"
    :match="findMatch"
    @close="closeFind"
    @search="onFindSearchInput"
    @nav="navFind"
    @replace="replaceInDocument"
    @replace-all="replaceAllInDocument"
  />
</template>
