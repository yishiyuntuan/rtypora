<script setup vapor>
import { ref, computed, watch, nextTick, inject, provide, onMounted, onBeforeUnmount, markRaw } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import BlockView from './BlockView.vue';
import VRow from './VRow.vue';
import SlashMenu from './SlashMenu.vue';
import ContextMenu from './ContextMenu.vue';
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
  SLASH_ITEMS,
  SLASH_TEXT_GROUPS,
  CALLOUT_TYPES,
  FONT_COLORS,
  FONT_SIZES,
  LANG_BADGES,
  htmlStyleCss,
  applySlashCommand,
  placeCursorAtEnd,
  placeCursorAtStart,
  placeCaretAtTextOffset,
  insertTextAtCursor,
  insertLineBreakAtCursor,
  numberedOrdinals,
  isCaretAtStart,
  deleteTaskCheckboxBeforeCaret,
  handleListEnter,
  handleListTab,
  paragraphDtoFromTree,
  demoteEditableToParagraph,
  insertInlineWrapper,
  plainText,
} from '../utils/wysiwyg.js';
import { formatShortcut } from '../utils/platform.js';
import { getPref, prefsVersion, structureVersion } from '../utils/prefs.js';
import { themeVersion } from '../themes/index.js';

// 双模式编辑器：sourceMode 为 Markdown 原文编辑，否则为所见即所得（Typora 式就地编辑）。
// 全文 Markdown 字符串是唯一数据源；块树由 Rust 命令 parse_markdown 解析得到。
// 前端只负责渲染与 DOM 结构提取：Markdown 解析、序列化、勾选切换、统计、快捷判定
// 全部由 Rust 命令完成（parse_markdown / parse_blocks / serialize_markdown /
// toggle_task_markdown / text_stats / detect_block_shortcut）。
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
// 虚拟滚动：全部行都渲染 VRow 占位（总高正确），内容按视口余量挂载（见 VRow.vue）；
// 包装对象按块身份记忆化（WeakMap）：提交编辑时未变化的块不再每次全量分配
// {block, estimate} 包装数组（主题切换时随估计缓存一并失效）
let rowWrapperCache = new WeakMap();
const renderedBlocks = computed(() =>
  allBlocks.value.map((block) => {
    let w = rowWrapperCache.get(block);
    if (!w) {
      w = { block, estimate: estimateBlockHeight(block) };
      rowWrapperCache.set(block, w);
    }
    return w;
  }),
);
const footnotes = computed(() => blocks.value.filter((b) => b.type === 'footnoteDefinition'));
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

// ---------- 长文档优化：虚拟滚动 + 重活懒执行 ----------
// 虚拟滚动行观察器：行进入视口余量时通知 VRow 挂载/卸载内容。
// 余量按窗口高度的 150%（rootMargin 百分比随窗口缩放动态生效），
// 加大提前量，避免滚动加载时内容闪烁
const rowCallbacks = new WeakMap();
let rowObserver = null;
function getRowObserver() {
  if (!rowObserver) {
    rowObserver = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          rowCallbacks.get(entry.target)?.(entry.isIntersecting);
        }
      },
      { root: null, rootMargin: '450% 0px' },
    );
  }
  return rowObserver;
}
provide('observeRow', (el, cb) => {
  rowCallbacks.set(el, cb);
  getRowObserver().observe(el);
});
provide('unobserveRow', (el) => {
  rowCallbacks.delete(el);
  // el 可能为 null（键控重建时 ref 已卸载），unobserve 非 Element 会抛 TypeError
  if (el) rowObserver?.unobserve(el);
});

// 虚拟行高估计：按源码行数 × 主题实际行高（首次渲染后由实测高度取代）。
// 按块 id 记忆化：提交编辑时未变化的块 id 稳定，估算不必随 renderedBlocks
// 全量重算（超长文档每次提交的 O(全文字符) 扫描是卡顿主因之一）
const estimateCache = new Map();
function estimateBlockHeight(block) {
  const hit = estimateCache.get(block.id);
  if (hit != null) return hit;
  let est;
  if (block.start == null || block.end == null) {
    est = 40;
  } else {
    // 只数换行符（不 slice/split 分配子串与数组）
    let lines = 1;
    for (let i = block.start; i < block.end; i++) {
      if (content.value.charCodeAt(i) === 10) lines++;
    }
    est = Math.max(1, lines) * lineHeightPx() + 16;
  }
  if (estimateCache.size > 50000) estimateCache.clear(); // 防跨文档/长期编辑膨胀
  estimateCache.set(block.id, est);
  return est;
}
let cachedLineHeightPx = 0;
function lineHeightPx() {
  if (cachedLineHeightPx) return cachedLineHeightPx;
  const style = getComputedStyle(document.documentElement);
  const size = parseFloat(style.getPropertyValue('--t-text-size')) || 17;
  const ratio = parseFloat(style.getPropertyValue('--t-text-line-height')) || 1.6;
  cachedLineHeightPx = Math.round(size * ratio);
  return cachedLineHeightPx;
}
// 主题/排版变化使缓存行高失效（估计缓存与行包装缓存一并清空）
watch(themeVersion, () => {
  cachedLineHeightPx = 0;
  estimateCache.clear();
  rowWrapperCache = new WeakMap();
});

// ---------- 大文档渐进挂载：首屏只创建前 INITIAL_ROW_QUOTA 个行组件，
// 其余行以「估计高度占位条」代替，空闲帧按步长扩配额直至全覆盖。
// 首屏 DOM/组件规模恒定（与文档大小无关），是超大文件秒开的关键
const INITIAL_ROW_QUOTA = 160;
const ROW_QUOTA_STEP = 500;
const rowQuota = ref(INITIAL_ROW_QUOTA);
let quotaScheduled = false;
function growRowQuota() {
  if (quotaScheduled) return;
  quotaScheduled = true;
  const step = () => {
    quotaScheduled = false;
    const total = renderedBlocks.value.length;
    if (rowQuota.value >= total) return;
    rowQuota.value = Math.min(total, rowQuota.value + ROW_QUOTA_STEP);
    if (rowQuota.value < total) growRowQuota();
  };
  if ('requestIdleCallback' in window) requestIdleCallback(step, { timeout: 200 });
  else setTimeout(step, 0);
}
// 配额内可见行与配额外尾部占位高度（估计高度和，保证滚动条总高近似正确）
const visibleRows = computed(() => renderedBlocks.value.slice(0, rowQuota.value));
const tailPadHeight = computed(() => {
  let h = 0;
  const rows = renderedBlocks.value;
  for (let i = rowQuota.value; i < rows.length; i++) h += rows[i].estimate;
  return h;
});
watch(
  () => renderedBlocks.value.length,
  (len) => {
    if (len > rowQuota.value) growRowQuota();
  },
);
// 配额增长后新挂载的行需要补测标题位置（防抖，避免逐批 querySelector）
watch(
  () => visibleRows.value.length,
  () => {
    clearTimeout(headingRebuildTimer);
    headingRebuildTimer = setTimeout(rebuildHeadingPositions, 300);
  },
);

// 重活懒执行：共享 IntersectionObserver，块进入视口（含 60% 窗口高度余量）才回调
//（语法高亮/公式/Mermaid 渲染经此延后，长文档挂载不再一次性打满 IPC；
// 余量随窗口缩放动态生效）
const visibilityCallbacks = new WeakMap();
let blockObserver = null;
function getBlockObserver() {
  if (!blockObserver) {
    blockObserver = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue;
          const cb = visibilityCallbacks.get(entry.target);
          if (cb) {
            visibilityCallbacks.delete(entry.target);
            blockObserver.unobserve(entry.target);
            cb();
          }
        }
      },
      { root: null, rootMargin: '200% 0px' },
    );
  }
  return blockObserver;
}
provide('onBlockVisible', (el, cb) => {
  if (!el) return cb();
  visibilityCallbacks.set(el, cb);
  getBlockObserver().observe(el);
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

// ---------- 表格编辑（Typora 式：就地编辑 + 尺寸/对齐/更多操作工具栏） ----------

// 光标所在表格单元格（不在表格单元格内返回 null）
function caretTableCell() {
  const el = currentEditable();
  if (!el) return null;
  const sel = window.getSelection();
  if (!sel.rangeCount) return null;
  const node = sel.anchorNode;
  const anchor = node?.nodeType === Node.TEXT_NODE ? node.parentElement : node;
  const cell = anchor?.closest('td, th');
  return cell && el.contains(cell) ? cell : null;
}

// 光标所在列的对齐（'' 默认 / left / center / right），工具栏对齐按钮高亮用；
// 由 document selectionchange 跟踪（见文件末尾 onMounted）
const tableCaretAlign = ref('');
function updateTableCaretAlign() {
  const cell = caretTableCell();
  tableCaretAlign.value = cell?.style.textAlign || '';
}

// 生成与表头同列数、对齐一致的空数据行
function emptyBodyRow(table) {
  const heads = [...(table.querySelector('thead tr')?.children || [])];
  const tr = document.createElement('tr');
  for (const th of heads.length ? heads : [null]) {
    const td = document.createElement('td');
    if (th?.style.textAlign) td.style.textAlign = th.style.textAlign;
    td.innerHTML = '<br>';
    tr.append(td);
  }
  return tr;
}

// 表格行列操作（纯编辑态 DOM 操作；提交时经 tableToBlock 提取 + Rust 序列化落源）
function tableOp(op, arg) {
  const el = currentEditable();
  const table = el?.querySelector('table');
  if (!table) return;
  // 单元格上下文：显式指定（边框 + 按钮）优先，否则取光标所在单元格
  const cell = arg?.cell ?? caretTableCell();
  const row = cell?.closest('tr');
  const inBody = row?.parentElement?.tagName === 'TBODY';
  const colIndex = cell ? [...row.children].indexOf(cell) : -1;
  const tbody = table.querySelector('tbody');

  // 插入行：相对光标行（表头行则插到首个数据行位置）；无光标插到末尾
  if (op === 'addRow' || op === 'insertRowAbove' || op === 'insertRowBelow') {
    const above = op === 'insertRowAbove';
    let ref;
    let where;
    if (inBody && row) {
      ref = row;
      where = above ? 'before' : 'after';
    } else if (row) {
      ref = tbody?.querySelector('tr:first-child');
      where = 'before';
    } else {
      ref = tbody?.querySelector('tr:last-child');
      where = 'after';
    }
    if (!ref) return;
    const clone = ref.cloneNode(true);
    clone.querySelectorAll('td, th').forEach((c) => {
      c.innerHTML = '<br>';
    });
    ref[where](clone);
    const target = clone.children[Math.max(0, colIndex)] || clone.firstElementChild;
    if (target) placeCursorAtStart(target);
    return;
  }
  if (op === 'delRow') {
    const target = inBody && row ? row : tbody?.querySelector('tr:last-child');
    if (!tbody || !target) return;
    target.remove();
    // 至少保留一个数据行
    if (!tbody.querySelector('tr')) tbody.append(emptyBodyRow(table));
    const fallback = tbody.querySelector('tr:last-child td');
    if (fallback) placeCursorAtStart(fallback);
    return;
  }
  // 行移动：仅数据行参与（表头固定为首行）
  if (op === 'moveRowUp' || op === 'moveRowDown') {
    if (!inBody || !row) return;
    const sibling = op === 'moveRowUp' ? row.previousElementSibling : row.nextElementSibling;
    if (!sibling) return;
    if (op === 'moveRowUp') tbody.insertBefore(row, sibling);
    else tbody.insertBefore(sibling, row);
    const target = row.children[Math.max(0, colIndex)] || row.firstElementChild;
    if (target) placeCursorAtStart(target);
    return;
  }

  const rows = [...table.querySelectorAll('tr')];
  const index = colIndex >= 0 ? colIndex : (rows[0]?.children.length ?? 1) - 1;
  // 插入列：相对光标列（无光标则在末尾右侧插入）
  if (op === 'addCol' || op === 'insertColLeft' || op === 'insertColRight') {
    const right = op !== 'insertColLeft';
    rows.forEach((r) => {
      const ref = r.children[index] || null;
      const next = document.createElement(ref ? ref.tagName.toLowerCase() : 'td');
      if (ref?.style.textAlign) next.style.textAlign = ref.style.textAlign;
      next.innerHTML = '<br>';
      if (!ref) r.append(next);
      else if (right) ref.after(next);
      else ref.before(next);
    });
    const focusIndex = right ? index + 1 : index;
    const target = table.querySelector('tbody tr')?.children[focusIndex];
    if (target) placeCursorAtStart(target);
    return;
  }
  if (op === 'delCol') {
    if ((rows[0]?.children.length ?? 0) <= 1) return; // 保留至少一列
    rows.forEach((r) => r.children[index]?.remove());
    const target = table.querySelector('tbody tr')?.children[Math.max(0, index - 1)];
    if (target) placeCursorAtStart(target);
    return;
  }
  // 列移动：整列（表头 + 数据行）与相邻列交换
  if (op === 'moveColLeft' || op === 'moveColRight') {
    const to = index + (op === 'moveColLeft' ? -1 : 1);
    if (to < 0 || to >= (rows[0]?.children.length ?? 0)) return;
    rows.forEach((r) => {
      const a = r.children[index];
      const b = r.children[to];
      if (!a || !b) return;
      if (op === 'moveColLeft') r.insertBefore(a, b);
      else r.insertBefore(b, a);
    });
    const target = (inBody ? row : table.querySelector('tbody tr'))?.children[to];
    if (target) placeCursorAtStart(target);
    return;
  }
  // 对齐：整列设置 textAlign（提交时 tableToBlock 提取为对齐列）；
  // 悬停工具栏触发（arg.all）或无光标单元格时作用于整个表格
  const align = { alignLeft: 'left', alignCenter: 'center', alignRight: 'right' }[op];
  if (align) {
    const allCols = arg?.all || colIndex < 0;
    rows.forEach((r) => {
      if (allCols) {
        [...r.children].forEach((c) => {
          c.style.textAlign = align;
        });
      } else {
        const c = r.children[index];
        if (c) c.style.textAlign = align;
      }
    });
    updateTableCaretAlign();
    return;
  }
  // 调整尺寸：数据行数/列数尾部增删（网格选择器），内容与对齐尽量保留
  if (op === 'resize' && arg) {
    const targetCols = Math.max(1, arg.cols);
    let current = rows[0]?.children.length ?? 0;
    while (current < targetCols) {
      [...table.querySelectorAll('tr')].forEach((r) => {
        const tag = r.parentElement.tagName === 'THEAD' ? 'th' : 'td';
        const cell = document.createElement(tag);
        cell.innerHTML = '<br>';
        r.append(cell);
      });
      current += 1;
    }
    while (current > targetCols && current > 1) {
      [...table.querySelectorAll('tr')].forEach((r) => r.lastElementChild?.remove());
      current -= 1;
    }
    if (tbody) {
      while (tbody.children.length < arg.rows) tbody.append(emptyBodyRow(table));
      while (tbody.children.length > Math.max(1, arg.rows)) tbody.lastElementChild.remove();
    }
    // 光标尽量留在原单元格（缩小后钳制到新范围内）
    const bodyRows = [...(tbody?.children || [])];
    const target =
      bodyRows[Math.min(inBody ? [...tbody.children].indexOf(row) : 0, bodyRows.length - 1)]
        ?.children[Math.min(Math.max(0, colIndex), targetCols - 1)];
    if (target) placeCursorAtStart(target);
  }
}

// Tab/Shift+Tab 在单元格间移动（跨行时到下一行首格/上一行末格）
function moveTableCaret(cell, dir) {
  const row = cell.closest('tr');
  const cells = [...row.children];
  const next = cells[cells.indexOf(cell) + dir];
  if (next) {
    placeCursorAtStart(next);
    return;
  }
  const sibling = dir > 0 ? row.nextElementSibling : row.previousElementSibling;
  const target = sibling?.children[dir > 0 ? 0 : sibling.children.length - 1];
  if (target) placeCursorAtStart(target);
}

// 单元格内 Enter：光标移到下一行同列单元格（末行则插入新行并进入）
function moveTableRowDown(cell) {
  const row = cell.closest('tr');
  const index = [...row.children].indexOf(cell);
  const nextRow = row.nextElementSibling;
  if (nextRow) {
    const target = nextRow.children[index] || nextRow.firstElementChild;
    if (target) placeCursorAtStart(target);
    return;
  }
  tableOp('addRow');
}

// ---------- 表格工具栏弹出层（尺寸网格 / 更多操作菜单） ----------
const tablePanel = ref(null); // 'grid' | 'more' | null
const tablePanelPos = ref({ left: 0, top: 0 });
// 表格块悬停跟踪：鼠标滑过表格（未编辑）时以浮层显示顶部工具栏
const hoverTableId = ref(null);
// 悬停工具栏动作：先进入编辑（pendingTableFocus 使光标落首个数据单元格），容器挂载后执行
let pendingTableAction = null;

function onTableBlockHover(block) {
  hoverTableId.value = block.id;
}
function onTableBlockLeave(block) {
  if (hoverTableId.value === block.id) hoverTableId.value = null;
  if (tableEdgeBtn.value?.blockId === block.id) tableEdgeBtn.value = null;
}

// 工具栏点击统一入口：编辑中直接执行；未编辑则排队动作并进入该表编辑
// （另一块在编辑时先提交再换块，避免未提交内容被丢弃）。
// 光标默认落表格末尾单元格：插行/列、删行/列等相对操作即「在末尾增删」
function tableToolbarClick(block, run) {
  if (editingId.value === block.id) {
    run();
    return;
  }
  pendingTableAction = run;
  suppressBlurCommit = true;
  if (editingId.value !== null && editableEl) {
    clearTimeout(blurCommitTimer);
    blurCommitTimer = null;
    commitEdit().finally(() => {
      editingId.value = block.id;
    });
    return;
  }
  editingId.value = block.id;
}
// 对齐操作：悬停触发（未选单元格）时作用于整个表格
function onToolbarOp(block, op) {
  const fromHover = editingId.value !== block.id;
  tableToolbarClick(block, () => tableOp(op, fromHover ? { all: true } : undefined));
}
function onToolbarDelete(block) {
  tableToolbarClick(block, () => deleteCurrentTableBlock());
}
// 面板类动作：弹出位置依赖按钮矩形，点击时先行捕获（排队到挂载后执行时事件对象已失效）
function onToolbarPanel(block, kind, e) {
  const rect = e.currentTarget.getBoundingClientRect();
  tableToolbarClick(block, () =>
    openTablePanel(kind, { currentTarget: { getBoundingClientRect: () => rect } }),
  );
}

// ---------- 表格单元格四边 + 按钮（行/列快速添加） ----------
// 鼠标靠近任意单元格的某条边（12px 内，取最近边）即浮现 +：
// 上边→上方插入行、下边→下方插入行、左边→左侧插入列、右边→右侧插入列
const tableEdgeBtn = ref(null); // { blockId, kind: 'rowAbove'|'rowBelow'|'colLeft'|'colRight', rowIndex, colIndex, top, left }

function onTableEdgeMove(block, e) {
  // 悬停在 + 按钮自身上：保持显示（移开按钮即隐藏会抖动）
  if (e.target.closest?.('.md-table-edge-btn')) return;
  const cell = e.target.closest?.('td, th');
  if (!cell) {
    if (tableEdgeBtn.value?.blockId === block.id) tableEdgeBtn.value = null;
    return;
  }
  const table = cell.closest('table');
  const hostRect = e.currentTarget.getBoundingClientRect();
  const r = cell.getBoundingClientRect();
  // 最近边判定（阈值 12px）
  const EDGE = 12;
  const dists = [
    ['top', e.clientY - r.top],
    ['bottom', r.bottom - e.clientY],
    ['left', e.clientX - r.left],
    ['right', r.right - e.clientX],
  ];
  const [edge, dist] = dists.reduce((a, b) => (b[1] < a[1] ? b : a));
  if (dist > EDGE) {
    if (tableEdgeBtn.value?.blockId === block.id) tableEdgeBtn.value = null;
    return;
  }
  const rows = [...table.querySelectorAll('tr')];
  const row = cell.closest('tr');
  const BTN = 18;
  let top;
  let left;
  let kind;
  if (edge === 'top' || edge === 'bottom') {
    kind = edge === 'top' ? 'rowAbove' : 'rowBelow';
    top = (edge === 'top' ? r.top : r.bottom) - hostRect.top - BTN / 2;
    left = r.left - hostRect.left + r.width / 2 - BTN / 2;
  } else {
    kind = edge === 'left' ? 'colLeft' : 'colRight';
    top = r.top - hostRect.top + r.height / 2 - BTN / 2;
    left = (edge === 'left' ? r.left : r.right) - hostRect.left - BTN / 2;
  }
  // 防被滚动容器横向/顶部裁切（最左列左边、首行上边的按钮会贴到容器边缘）
  tableEdgeBtn.value = {
    blockId: block.id,
    kind,
    rowIndex: rows.indexOf(row),
    colIndex: [...row.children].indexOf(cell),
    top: Math.max(2, top),
    left: Math.max(2, left),
  };
}

// 点击 +：未编辑时先进入编辑（复用工具栏点击链路），再对目标单元格执行对应插入
function onTableEdgeAdd() {
  const btn = tableEdgeBtn.value;
  if (!btn) return;
  const block = blocks.value.find((b) => b.id === btn.blockId);
  tableEdgeBtn.value = null;
  if (!block) return;
  tableToolbarClick(block, () => {
    const table = currentEditable()?.querySelector('table');
    if (!table) return;
    const rows = [...table.querySelectorAll('tr')];
    const row = rows[Math.min(Math.max(0, btn.rowIndex), rows.length - 1)];
    const cell = row?.children[Math.max(0, btn.colIndex)] ?? row?.firstElementChild ?? null;
    const op = {
      rowAbove: 'insertRowAbove',
      rowBelow: 'insertRowBelow',
      colLeft: 'insertColLeft',
      colRight: 'insertColRight',
    }[btn.kind];
    if (op) tableOp(op, { cell });
  });
}
// 尺寸网格行列上限与当前悬停的 N×M（1 基）
const TABLE_GRID_COLS = 10;
const TABLE_GRID_ROWS = 8;
const tableGridHover = ref({ rows: 1, cols: 1 });
// 打开尺寸选择器时的当前表格尺寸快照（网格初始高亮）
const tableGridSize = ref({ rows: 1, cols: 1 });

// 「更多操作」菜单项（顺序与 Typora 一致；'sep' 为分隔线）
const TABLE_MORE_ITEMS = [
  { id: 'insertRowAbove', label: '上方插入行' },
  { id: 'insertRowBelow', label: '下方插入行', shortcut: 'Ctrl+Enter' },
  { id: 'insertColLeft', label: '左侧插入列' },
  { id: 'insertColRight', label: '右侧插入列' },
  'sep',
  { id: 'moveRowUp', label: '上移该行', shortcut: 'Alt+↑' },
  { id: 'moveRowDown', label: '下移该行', shortcut: 'Alt+↓' },
  { id: 'moveColLeft', label: '左移该列', shortcut: 'Alt+←' },
  { id: 'moveColRight', label: '右移该列', shortcut: 'Alt+→' },
  'sep',
  { id: 'delRow', label: '删除行' },
  { id: 'delCol', label: '删除列' },
  'sep',
  { id: 'copyTable', label: '复制表格' },
  { id: 'formatTable', label: '格式化表格源码' },
  'sep',
  { id: 'deleteTable', label: '删除表格' },
];

function openTablePanel(kind, e) {
  if (tablePanel.value === kind) {
    tablePanel.value = null;
    return;
  }
  const rect = e.currentTarget.getBoundingClientRect();
  // 底部空间不足时向上翻；左右防溢出（估算：网格 ~200x290，菜单 ~210x430）
  const estH = kind === 'grid' ? 300 : 440;
  const estW = 220;
  const top =
    rect.bottom + estH > window.innerHeight ? Math.max(8, rect.top - estH) : rect.bottom + 4;
  const left = Math.max(8, Math.min(rect.left, window.innerWidth - estW - 8));
  tablePanelPos.value = { left, top };
  if (kind === 'grid') {
    const table = currentEditable()?.querySelector('table');
    tableGridSize.value = {
      rows: Math.max(1, table?.querySelectorAll('tbody tr').length || 1),
      cols: Math.max(1, table?.querySelector('thead tr')?.children.length || 1),
    };
    tableGridHover.value = { ...tableGridSize.value };
  }
  tablePanel.value = kind;
}

// 点击尺寸格子：调整表格为悬停的 N×M（N 数据行 × M 列）
function applyTableGrid() {
  const { rows, cols } = tableGridHover.value;
  tablePanel.value = null;
  tableOp('resize', { rows, cols });
}

function runTableMore(item) {
  tablePanel.value = null;
  if (item.id === 'copyTable') return copyTableMarkdown();
  if (item.id === 'formatTable') return formatTableSource();
  if (item.id === 'deleteTable') return deleteCurrentTableBlock();
  tableOp(item.id);
}

// 复制表格：当前编辑内容经 Rust 序列化为 Markdown 写入剪贴板（不打断编辑）
async function copyTableMarkdown() {
  const el = currentEditable();
  if (!el) return;
  const md = await invoke('serialize_markdown', { blocks: domToBlockDtos(el) }).catch(() => '');
  const text = md.trim();
  if (text) await navigator.clipboard.writeText(text).catch(() => {});
}

// 格式化表格源码：提交当前编辑 → Rust 按列宽对齐管道 → 原位替换并保持编辑
async function formatTableSource() {
  const el = currentEditable();
  if (!el || editingId.value === null || editingId.value === '__append__') return;
  syncing.value = true;
  try {
    const md = await invoke('serialize_markdown', { blocks: domToBlockDtos(el) });
    const padded = await invoke('format_table_source', { markdown: md }).catch(() => null);
    if (!padded || padded === md) return;
    const index = blocks.value.findIndex((b) => b.id === editingId.value);
    const block = blocks.value[index];
    if (!block) return;
    content.value = content.value.slice(0, block.start) + padded + content.value.slice(block.end);
    const replacements = await parseAnchoredBlocks(padded, block.start);
    const delta = padded.length - (block.end - block.start);
    editableEl = null;
    suppressBlurCommit = true;
    blocks.value.splice(index, 1, ...replacements);
    shiftBlockOffsets(index + replacements.length, delta);
    publishBlocks();
    // 重解析后块 id 随内容变化：重建编辑容器，光标落到首个数据单元格
    pendingTableFocus = true;
    editingId.value = replacements[0]?.id ?? null;
    if (!replacements[0]) suppressBlurCommit = false;
  } finally {
    syncing.value = false;
  }
}

// 删除整个表格块（含源码区间与相邻换行），光标移到上一块末尾（与空块退格删除同规则）
async function deleteCurrentTableBlock() {
  if (editingId.value === null || editingId.value === '__append__') return;
  syncing.value = true;
  try {
    const index = blocks.value.findIndex((b) => b.id === editingId.value);
    const oldBlock = blocks.value[index];
    editableEl = null;
    if (!oldBlock) {
      editingId.value = null;
      return;
    }
    let removeStart = oldBlock.start;
    let removeEnd = oldBlock.end;
    if (content.value[removeEnd] === '\n') removeEnd += 1;
    else if (removeStart > 0 && content.value[removeStart - 1] === '\n') removeStart -= 1;
    const removedLen = removeEnd - removeStart;
    content.value = content.value.slice(0, removeStart) + content.value.slice(removeEnd);
    blocks.value.splice(index, 1);
    shiftBlockOffsets(index, -removedLen);
    publishBlocks();
    const prev = blocks.value[index - 1];
    suppressBlurCommit = true;
    editingId.value = prev ? prev.id : (blocks.value[0]?.id ?? '__append__');
    cursorAtStart = !prev && blocks.value.length > 0;
    if (!prev && !blocks.value.length) suppressBlurCommit = false;
  } finally {
    syncing.value = false;
  }
}

// ---------- 斜杠命令菜单（/ 召唤 Markdown 语法菜单） ----------
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

// 光标所在的字体样式 span（无则 null）
function caretFontSpan(el) {
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

function applyFontStyle(style) {
  const el = currentEditable();
  if (!el) return;
  suppressBlurCommit = true;
  try {
    if (fontMenuSession) {
      // 多选会话：直接在当前样式 span 上合并（不重复删除触发符/重建结构）
      const existing = caretFontSpan(el);
      if (existing) {
        mergeStyleIntoSpan(existing, style);
        slashOpen.value = true; // 菜单保持打开，可继续选择
        return;
      }
      // 光标不在样式 span 内（如整段套用后移出）：按新样式插入
    } else {
      // 首次：删除触发文本——光标在编辑器内时按光标定位删除；
      // 焦点在菜单输入框（RGB）时按块首的 /query 删除
      const sel = window.getSelection();
      if (sel.rangeCount && el.contains(sel.anchorNode)) {
        const caret = sel.getRangeAt(0);
        const trigger = document.createRange();
        trigger.selectNodeContents(el);
        trigger.setEnd(caret.startContainer, caret.startOffset);
        trigger.deleteContents();
      } else {
        const query = '/' + slashQuery.value;
        const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
        const firstText = walker.nextNode();
        if (firstText && query.length > 1 && firstText.textContent.startsWith(query)) {
          firstText.textContent = firstText.textContent.slice(query.length);
        } else if (firstText?.textContent.startsWith('/')) {
          firstText.textContent = firstText.textContent.slice(1);
        }
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
    } else if (target.textContent.trim()) {
      // 块内已有文字：整段套用
      while (target.firstChild) span.append(target.firstChild);
      target.append(span);
      placeCursorAtEnd(span);
    } else {
      // 空块：插入样式 span，光标入内，输入即带样式文字
      target.innerHTML = '';
      target.append(span);
      placeCursorAtStart(span);
    }
    fontMenuSession = true;
    slashOpen.value = true; // 菜单保持打开，颜色与字号可叠加选择
  } finally {
    suppressBlurCommit = false;
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
      currentEditable()?.focus({ preventScroll: true });
    } else {
      clearSlashTriggerAndFocus();
    }
    return;
  }
  applyFontColor(color);
}

// 无效 RGB 输入的回落：清掉 / 触发文本并聚焦编辑器（普通文本输入）
function clearSlashTriggerAndFocus() {
  const el = currentEditable();
  if (!el) return;
  suppressBlurCommit = true;
  try {
    const sel = window.getSelection();
    if (sel.rangeCount && el.contains(sel.anchorNode)) {
      const caret = sel.getRangeAt(0);
      const trigger = document.createRange();
      trigger.selectNodeContents(el);
      trigger.setEnd(caret.startContainer, caret.startOffset);
      trigger.deleteContents();
    } else {
      // 焦点在菜单输入框（RGB）：按块首 /query 删除
      const query = '/' + slashQuery.value;
      const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
      const firstText = walker.nextNode();
      if (firstText && query.length > 1 && firstText.textContent.startsWith(query)) {
        firstText.textContent = firstText.textContent.slice(query.length);
      } else if (firstText?.textContent.startsWith('/')) {
        firstText.textContent = firstText.textContent.slice(1);
      }
    }
  } finally {
    suppressBlurCommit = false;
  }
  el.focus({ preventScroll: true });
}

// 菜单内 RGB 输入框获得焦点：编辑容器 blur 的提交需抑制（/ 触发文本保持未提交态）
function onRgbFocus() {
  suppressBlurCommit = true;
}
// RGB 输入框 Esc：关闭菜单并恢复编辑焦点与提交通路
function onRgbCancel() {
  slashOpen.value = false;
  suppressBlurCommit = false;
  currentEditable()?.focus({ preventScroll: true });
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
  suppressBlurCommit = false;
  currentEditable()?.focus({ preventScroll: true });
}

// ---------- 右键菜单（Typora 式：剪贴板 / 复制粘贴为 / 格式 / 段落 / 插入） ----------
const ctxMenu = ref(null); // { x, y }

function onContextMenu(e) {
  if (props.sourceMode) return;
  e.preventDefault();
  // 已处于编辑态：保留当前选区；否则进入被点击块的编辑
  const el = currentEditable();
  if (!(el && el.contains(e.target))) {
    const blockEl = e.target.closest?.('[data-block-id]');
    const block = blocks.value.find((b) => b.id === blockEl?.getAttribute('data-block-id'));
    if (!block) return;
    startEdit(block);
  }
  ctxMenu.value = { x: e.clientX, y: e.clientY };
}

function closeCtxMenu() {
  ctxMenu.value = null;
}

// 段落子菜单勾选态（heading 块映射为 h1-h6）
const ctxCurrentType = computed(() => {
  const b = editingBlock.value;
  if (!b) return 'paragraph';
  return b.type === 'heading' ? `h${b.level}` : b.type;
});

async function onMenuAction(id, payload) {
  closeCtxMenu();
  const el = currentEditable();
  switch (id) {
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
        suppressBlurCommit = true;
        try {
          insertInlineWrapper(el, id);
        } finally {
          suppressBlurCommit = false;
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

// 字体样式 span 构建（slash 与右键菜单共用；htmlStyle JSON 落 data 属性无损往返）
function buildFontSpan(style) {
  const span = document.createElement('span');
  span.setAttribute('style', htmlStyleCss(style));
  span.setAttribute('data-html-style', JSON.stringify(style));
  return span;
}

// 右键菜单字体样式：有选区包选区，无选区插空 span 光标入内
function ctxApplyFontStyle(style) {
  const el = currentEditable();
  if (!el || !style) return;
  suppressBlurCommit = true;
  try {
    const span = buildFontSpan(style);
    const sel = window.getSelection();
    if (sel.rangeCount && !sel.isCollapsed && el.contains(sel.anchorNode)) {
      const range = sel.getRangeAt(0);
      span.append(range.extractContents());
      range.insertNode(span);
      placeCursorAtEnd(span);
    } else {
      const range = sel.rangeCount && el.contains(sel.anchorNode) ? sel.getRangeAt(0) : null;
      if (range) {
        range.insertNode(span);
      } else {
        // 无光标：放入块的首个块级子元素内（span 不能包块级元素）
        (el.firstElementChild || el).append(span);
      }
      placeCursorAtStart(span);
    }
  } finally {
    suppressBlurCommit = false;
  }
  if (document.activeElement !== el) el.focus({ preventScroll: true });
}

// 插入 YAML Front Matter：文档头插入（已有则不重复）；模板由 Rust block_template 生成
async function insertYamlFrontMatter() {
  if (content.value.startsWith('---\n')) return;
  const tpl = await invoke('block_template', { kind: 'yamlFrontMatter' }).catch(() => null);
  if (!tpl) return;
  if (editingId.value !== null) await commitEdit();
  syncing.value = true;
  try {
    content.value = tpl.markdown + '\n' + content.value;
    await reparse();
    const target = blocks.value[0];
    suppressBlurCommit = true;
    pendingCaretOffset = tpl.caretOffset; // 光标落 title: 之后
    editingId.value = target ? target.id : null;
    if (!target) {
      suppressBlurCommit = false;
      pendingCaretOffset = null;
    }
  } finally {
    syncing.value = false;
  }
}

// 粘贴为纯文本（剪贴板 → 光标处；浏览器 insertText 自动处理多行分段，换行符先统一为 LF）
async function pastePlainText() {
  const text = await navigator.clipboard.readText().catch(() => null);
  const el = currentEditable();
  if (text == null || !el) return;
  el.focus({ preventScroll: true });
  document.execCommand('insertText', false, text.replace(/\r\n?/g, '\n'));
}

// 复制选区（无选区取当前整块）：markdown=Rust 序列化 / html=DOM HTML /
// simplified=按块分段的纯文本 / plain=纯文本
async function copySelection(kind) {
  const el = currentEditable();
  if (!el) return;
  const sel = window.getSelection();
  const div = document.createElement('div');
  if (sel.rangeCount && !sel.isCollapsed && el.contains(sel.anchorNode)) {
    div.append(sel.getRangeAt(0).cloneContents());
  } else {
    div.innerHTML = el.innerHTML;
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
  const el = currentEditable();
  if (!el) return;
  // 先取出块内容：单块时取其内部行内容（避免块套块），多块时取全部子节点
  const sourceChildren =
    el.firstElementChild && el.childElementCount === 1
      ? [...el.firstElementChild.childNodes]
      : [...el.childNodes];
  const fragment = document.createDocumentFragment();
  for (const node of sourceChildren) fragment.append(node);
  const hasContent = fragment.textContent.trim() !== '';
  suppressBlurCommit = true;
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
    suppressBlurCommit = false;
  }
  if (document.activeElement !== el) el.focus({ preventScroll: true });
}

// 插入类块（图像/分割线/表格/代码块/公式块/链接引用）：
// 当前块有内容时先在末尾拆出新空块（保住原内容），再按斜杠模板插入
async function insertBlockFromMenu(id) {
  let el = currentEditable();
  if (!el) return;
  if (el.textContent.trim()) {
    placeCursorAtEnd(el);
    await splitAndCommit();
    el = currentEditable();
    if (!el) return;
  }
  if (id === 'table') {
    await applyTableTemplate(el, { rows: 2, cols: 2 });
    return;
  }
  suppressBlurCommit = true;
  try {
    await applySlashCommand(el, id);
  } finally {
    suppressBlurCommit = false;
  }
  if (document.activeElement !== el) el.focus({ preventScroll: true });
  // 代码块：围栏已插入（```），立即弹出语言补全，先编辑语言
  if (id === 'codeBlock') updateLangMenu();
}

// 插入脚注：光标处插入 [^n] 引用（编号 = 现有最大编号 +1），提交后在文末追加定义块并进入编辑
async function insertFootnote() {
  const el = currentEditable();
  if (!el) return;
  let max = 0;
  for (const b of footnotes.value) {
    const n = parseInt(plainText(b.title), 10);
    if (!Number.isNaN(n)) max = Math.max(max, n);
  }
  const id = max + 1;
  insertTextAtCursor(`[^${id}]`);
  await commitEdit();
  // 模板（含脚注 id）由 Rust 生成，前端不做 Markdown 文本拼接
  const tpl = await invoke('block_template', { kind: 'footnoteDef', footnoteId: String(id) }).catch(() => null);
  if (!tpl) return;
  const def = tpl.markdown;
  syncing.value = true;
  try {
    const base = content.value ? content.value.replace(/\s*$/, '') : '';
    const anchor = base ? base.length + 2 : 0;
    content.value = base ? base + '\n\n' + def : def;
    const parsed = await parseAnchoredBlocks(def, anchor);
    blocks.value.push(...parsed);
    publishBlocks();
    const target = parsed[parsed.length - 1];
    suppressBlurCommit = true;
    cursorAtStart = false; // 光标落块尾（定义文本后）
    editingId.value = target ? target.id : null;
    if (!target) suppressBlurCommit = false;
  } finally {
    syncing.value = false;
  }
}

// 段落（上方/下方）：上方 = 块首拆分后进入前一个空段落；下方 = 块尾拆分（默认进入新空段落）
async function insertParagraphSibling(above) {
  const el = currentEditable();
  if (!el || !editingBlock.value) return;
  if (above) {
    placeCursorAtStart(el);
    await splitAndCommit();
    const idx = blocks.value.findIndex((b) => b.id === editingId.value);
    const prev = blocks.value[idx - 1];
    if (prev) {
      suppressBlurCommit = true;
      cursorAtStart = true;
      editingId.value = prev.id;
    }
  } else {
    placeCursorAtEnd(el);
    await splitAndCommit();
  }
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

// 光标处的斜杠上下文：块内文本（光标前）以 / 开头时返回查询串与菜单位置；
// 代码/原文 pre 编辑内不触发
function slashContext() {
  const el = currentEditable();
  if (!el || editingId.value === null) return null;
  const sel = window.getSelection();
  if (!sel.rangeCount || !sel.isCollapsed) return null;
  const caret = sel.getRangeAt(0);
  if (!el.contains(caret.startContainer)) return null;
  const anchorEl = caret.startContainer.nodeType === Node.TEXT_NODE ? caret.startContainer.parentElement : caret.startContainer;
  if (anchorEl?.closest('pre')) return null;
  const before = document.createRange();
  before.selectNodeContents(el);
  before.setEnd(caret.startContainer, caret.startOffset);
  const text = before.toString();
  if (!text.startsWith('/')) return null;
  const rect = caret.getBoundingClientRect();
  // 视口底部空间不足时向上翻（菜单最大高度 264 + 间隙）；文本行徽章较宽，左侧防溢出
  const estimatedHeight = 270;
  const top = rect.bottom + estimatedHeight > window.innerHeight
    ? Math.max(8, rect.top - estimatedHeight)
    : rect.bottom + 6;
  const left = Math.max(8, Math.min(rect.left, window.innerWidth - 380));
  return { query: text.slice(1), pos: { left, top } };
}

// 输入后刷新菜单：触发文本被删改（不再是 / 开头、含空格/换行）则关闭
function updateSlashMenu() {
  const ctx = slashContext();
  if (!ctx || ctx.query.includes('\n') || ctx.query.includes(' ')) {
    slashOpen.value = false;
    return;
  }
  slashQuery.value = ctx.query;
  if (slashIndex.value >= slashItems.value.length) slashIndex.value = 0;
  slashTextRow.value = 0;
  slashTextCol.value = 0;
  slashTableField.value = 'item';
  slashCalloutType.value = 0;
  slashFontRow.value = 0;
  slashFontColorIndex.value = 0;
  slashFontSizeIndex.value = 0;
  rgbError.value = false;
  slashPos.value = ctx.pos;
  slashOpen.value = true;
}

async function applySlashItem(item, option) {
  const el = currentEditable();
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
    await applyTableTemplate(el, opts);
    return;
  }
  // 替换 DOM 期间抑制 blur 误提交：清空聚焦中的容器可能触发同步 blur，
  // 不抑制则 onEditableBlur 抢先提交空内容（块被删，新内容插到已卸载节点上）
  suppressBlurCommit = true;
  try {
    await applySlashCommand(el, id, opts);
  } finally {
    suppressBlurCommit = false;
  }
  // 替换导致失焦时恢复焦点（光标已由构建函数放置）
  if (document.activeElement !== el) el.focus({ preventScroll: true });
  // 代码块：围栏已插入（```），立即弹出语言补全，先编辑语言
  if (id === 'codeBlock') updateLangMenu();
}

// 斜杠表格：取 Rust block_template 模板后立即提交渲染为表格块，并进入首个数据单元格编辑
async function applyTableTemplate(el, opts) {
  const tpl = await invoke('block_template', { kind: 'table', rows: opts?.rows, cols: opts?.cols }).catch((e) => {
    console.error('block_template 调用失败:', e);
    return null;
  });
  if (!tpl) return;
  syncing.value = true;
  try {
    // 聚焦新表格块的首个数据单元格（reparse 后旧 id 已失效，取替换位首块）
    const isAppend = editingId.value === '__append__';
    let tableBlock = null;
    if (isAppend) {
      const base = content.value ? content.value.replace(/\s*$/, '') : '';
      const anchor = base ? base.length + 2 : 0;
      content.value = base ? base + '\n\n' + tpl.markdown : tpl.markdown;
      const anchored = await parseAnchoredBlocks(tpl.markdown, anchor);
      blocks.value.push(...anchored);
      publishBlocks();
      tableBlock = anchored[0] ?? null;
    } else {
      const index = blocks.value.findIndex((b) => b.id === editingId.value);
      const block = blocks.value[index];
      if (block) {
        content.value = content.value.slice(0, block.start) + tpl.markdown + content.value.slice(block.end);
        await reparseRegion(index, block, tpl.markdown);
        tableBlock = blocks.value[index] ?? null;
      }
    }
    editableEl = null;
    suppressBlurCommit = true;
    pendingTableFocus = true;
    editingId.value = tableBlock ? tableBlock.id : null;
    if (!tableBlock) suppressBlurCommit = false;
  } finally {
    syncing.value = false;
  }
}
// 下一次挂载聚焦表格首个数据单元格（斜杠表格即时渲染）
let pendingTableFocus = false;

// ---------- 围栏语言自动补全（``` 后弹出语言菜单，含 mermaid/math） ----------
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
  const el = currentEditable();
  if (!el || editingId.value === null) return null;
  const sel = window.getSelection();
  if (!sel.rangeCount || !sel.isCollapsed) return null;
  const caret = sel.getRangeAt(0);
  if (!el.contains(caret.startContainer)) return null;
  const anchorEl = caret.startContainer.nodeType === Node.TEXT_NODE ? caret.startContainer.parentElement : caret.startContainer;
  if (anchorEl?.closest('pre')) return null;
  const before = document.createRange();
  before.selectNodeContents(el);
  before.setEnd(caret.startContainer, caret.startOffset);
  const text = before.toString();
  const match = /^(`{3,}|~{3,})([a-zA-Z0-9#+_-]*)$/.exec(text);
  if (!match) return null;
  const rect = caret.getBoundingClientRect();
  const estimatedHeight = 270;
  const top = rect.bottom + estimatedHeight > window.innerHeight
    ? Math.max(8, rect.top - estimatedHeight)
    : rect.bottom + 6;
  return { fence: match[1], query: match[2], pos: { left: rect.left, top } };
}

async function updateLangMenu() {
  const ctx = fenceContext();
  if (!ctx) {
    langOpen.value = false;
    return;
  }
  if (!langList.value.length) {
    langList.value = await invoke('code_languages').catch(() => []);
  }
  langMenuInput = null;
  langQuery.value = ctx.query;
  if (langIndex.value >= langItems.value.length) langIndex.value = 0;
  langPos.value = ctx.pos;
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
  const estimatedHeight = 270;
  langPos.value = {
    // 右缘钳制（语言框在代码块右上角，菜单默认向左伸出会溢出视口）
    left: Math.max(8, Math.min(rect.left, window.innerWidth - 340)),
    top:
      rect.bottom + estimatedHeight > window.innerHeight
        ? Math.max(8, rect.top - estimatedHeight)
        : rect.bottom + 6,
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
    const code = pre?.querySelector('code');
    if (code) placeCursorAtStart(code);
    pre?.closest('[contenteditable]')?.focus({ preventScroll: true });
    return;
  }
  const el = currentEditable();
  const ctx = fenceContext();
  if (!el || !ctx || !lang) return;
  langOpen.value = false;
  const after = el.textContent.slice((ctx.fence + ctx.query).length);
  const p = el.querySelector('p') || el;
  p.textContent = ctx.fence + lang + after;
  placeCaretAtTextOffset(p, (ctx.fence + lang).length);
}
// 滚动定位闪烁的块 id / 当前滚动位置对应的标题 id（供大纲高亮）
const flashId = ref(null);
const activeHeadingId = ref(null);
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
// 正在编辑的块 id；'__append__' 表示在文末追加新块
const editingId = ref(null);
// 当前编辑块（表格编辑工具栏显隐用）
const editingBlock = computed(() => blocks.value.find((b) => b.id === editingId.value) || null);
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
// 内容高度变化（块树发布/初次挂载）后重算滑轨
watch(renderedBlocks, () => nextTick(updateSbThumb), { flush: 'post' });
onMounted(() => nextTick(updateSbThumb));
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
async function reparseRegion(index, oldBlock, md) {
  try {
    const replacements = await parseAnchoredBlocks(md, oldBlock.start);
    // JS 字符串 length 即 UTF-16 码元数，与 Rust 端偏移约定一致
    const delta = md.length - (oldBlock.end - oldBlock.start);
    blocks.value.splice(index, 1, ...replacements);
    shiftBlockOffsets(index + replacements.length, delta);
    publishBlocks();
  } catch (e) {
    console.error('parse_blocks 调用失败，回退全文重解析:', e);
    await reparse();
  }
}

function onInput(e) {
  const textarea = e.target;
  const text = textarea.value.substring(0, textarea.selectionStart);
  cursorLine.value = (text.match(/\n/g) || []).length + 1;
  cursorColumn.value = text.length - text.lastIndexOf('\n');
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
    el.innerHTML = editingId.value === '__append__' || !block ? emptyParagraphHtml() : blockToHtml(block, rawSource, 0, rootOrdinals.value.get(block.id) || 1);
  }
  stashedEdit = null;
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
    } else if (cursorAtStart) {
      cursorAtStart = false;
      placeCursorAtStart(el);
    } else {
      placeCursorAtEnd(el);
    }
    // 悬停工具栏排队的动作（对齐/插行列/删除/打开面板）：编辑容器就绪、光标落位后执行
    if (pendingTableAction) {
      const action = pendingTableAction;
      pendingTableAction = null;
      action();
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

function startEdit(block) {
  if (syncing.value) return;
  // 拖动形成的跨块选区非空时：不进入编辑（避免替换 DOM 销毁选区；再点一次即可编辑）
  const sel = window.getSelection();
  if (sel && !sel.isCollapsed) return;
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

async function commitEdit() {
  if (editingId.value === null || !currentEditable()) return;
  syncing.value = true;
  try {
    // 前端只提取 DOM 结构，Markdown 序列化由 Rust 完成
    const md = await invoke('serialize_markdown', { blocks: domToBlockDtos(editableEl) });
    editableEl = null;
    suppressBlurCommit = false;    if (editingId.value === '__append__') {
      const text = md.trim();
      if (text) {
        const base = content.value ? content.value.replace(/\s*$/, '') : '';
        const anchor = base ? base.length + 2 : 0;
        content.value = base ? base + '\n\n' + text : text;
        // 增量：只解析追加的片段并挂到块树末尾
        const parsed = await parseAnchoredBlocks(text, anchor);
        blocks.value.push(...parsed);
        publishBlocks();
      }
    } else {
      const index = blocks.value.findIndex((b) => b.id === editingId.value);
      const block = blocks.value[index];
      if (block) {
        content.value = content.value.slice(0, block.start) + md + content.value.slice(block.end);
        // 增量：原位替换被编辑的块
        await reparseRegion(index, block, md);
      }
    }
    editingId.value = null;
  } finally {
    syncing.value = false;
  }
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
      const beforeRange = range.cloneRange();
      beforeRange.selectNodeContents(el);
      beforeRange.setEnd(range.startContainer, range.startOffset);
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

    // 空段检测：拆分会在边界克隆出空的块级元素（如标题末尾的空 <h2>），
    // 若参与序列化会生成孤立的标记文本（## 等），此处按空段处理
    const isEmptyDiv = (div) =>
      div.textContent.trim() === '' && !div.querySelector('img, input, hr, table, pre');
    // 两次序列化并行（IPC 延迟是超长文档拆分卡顿的主要来源之一）
    const [beforeMdRaw, afterMdRaw] = await Promise.all([
      isEmptyDiv(beforeDiv)
        ? ''
        : invoke('serialize_markdown', { blocks: domToBlockDtos(beforeDiv) }),
      isEmptyDiv(afterDiv)
        ? ''
        : invoke('serialize_markdown', { blocks: domToBlockDtos(afterDiv) }),
    ]);
    const beforeMd = beforeMdRaw.trim();
    const afterMd = afterMdRaw.trim();

    // 追加流（文末 __append__）没有可替换的旧块：锚点为文末，拼接整段
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
      index = blocks.value.findIndex((b) => b.id === editingId.value);
      oldBlock = blocks.value[index];
    }
    editableEl = null;
    if (!oldBlock) {
      editingId.value = null;
      return;
    }

    const combined = beforeMd + '\n\n' + afterMd;
    if (isAppend) {
      content.value = base ? base + '\n\n' + combined : combined;
    } else {
      content.value = content.value.slice(0, oldBlock.start) + combined + content.value.slice(oldBlock.end);
    }

    // 增量重解析合并片段（一次调用；空行分隔的两段与分别解析等价），
    // 原位替换并平移后续偏移
    const replacements = await parseAnchoredBlocks(combined, oldBlock.start);
    const delta = combined.length - (oldBlock.end - oldBlock.start);
    blocks.value.splice(index, isAppend ? 0 : 1, ...replacements);
    shiftBlockOffsets(index + replacements.length, delta);
    publishBlocks();

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
  const index = isAppend ? blocks.value.length : blocks.value.findIndex((b) => b.id === editingId.value);
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
  publishBlocks();
}

// 当前段落块并入上一文本块：合并源 = 上块源 + 当前行内 Markdown（上块类型保留），
// 重解析合并区间并平移后续偏移，光标落在上块原文末尾（接缝处）
async function mergeIntoPrevBlock(dto, index, isAppend) {
  syncing.value = true;
  try {
    const prev = blocks.value[index - 1];
    const curEnd = isAppend ? content.value.length : blocks.value[index].end;
    const curMd = await invoke('serialize_markdown', { blocks: [paragraphDtoFromTree(dto.title)] });
    const prevSource = content.value.slice(prev.start, prev.end);
    // 合并规则（接缝处理）由 Rust 统一维护，前端只做机械区间替换
    const combined = await invoke('merge_block_markdown', { prevSource, appendedMarkdown: curMd });
    const junction = blockPlainText(prev).length;
    editableEl = null;
    content.value = content.value.slice(0, prev.start) + combined + content.value.slice(curEnd);
    const replacements = await parseAnchoredBlocks(combined, prev.start);
    const delta = combined.length - (curEnd - prev.start);
    blocks.value.splice(index - 1, isAppend ? 1 : 2, ...replacements);
    for (let i = index - 1 + replacements.length; i < blocks.value.length; i++) {
      const b = blocks.value[i];
      if (b.start != null) {
        b.start += delta;
        b.end += delta;
      }
    }
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
      const index = blocks.value.findIndex((b) => b.id === editingId.value);
      const oldBlock = blocks.value[index];
      editableEl = null;
      if (oldBlock) {
        // 删除块源码及其行尾换行（空段落块为零长源码，删除对应空行）
        let removeStart = oldBlock.start;
        let removeEnd = oldBlock.end;
        if (content.value[removeEnd] === '\n') removeEnd += 1;
        else if (removeStart > 0 && content.value[removeStart - 1] === '\n') removeStart -= 1;
        const removedLen = removeEnd - removeStart;
        content.value = content.value.slice(0, removeStart) + content.value.slice(removeEnd);
        blocks.value.splice(index, 1);
        shiftBlockOffsets(index, -removedLen);
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
    const md = (await invoke('serialize_markdown', { blocks: domToBlockDtos(editableEl) })).trimEnd();

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
      index = blocks.value.findIndex((b) => b.id === editingId.value);
      oldBlock = blocks.value[index];
    }
    editableEl = null;
    if (!oldBlock) {
      editingId.value = null;
      return;
    }

    // 整块 + 尾部空行（解析出一个空段落作为后续编辑块）
    const combined = md + '\n\n';
    if (isAppend) {
      content.value = base ? base + '\n\n' + combined : combined;
    } else {
      content.value = content.value.slice(0, oldBlock.start) + combined + content.value.slice(oldBlock.end);
    }
    const replacements = await parseAnchoredBlocks(combined, oldBlock.start);
    const delta = combined.length - (oldBlock.end - oldBlock.start);
    blocks.value.splice(index, isAppend ? 0 : 1, ...replacements);
    shiftBlockOffsets(index + replacements.length, delta);
    publishBlocks();

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
    return;
  }
  applyMarkdownShortcuts();
  applyHtmlAutoclose();
  updateSlashMenu();
  updateLangMenu();
  // 输入到底部时：光标若被压到窗口底部（状态栏上方不可见），滚到窗口首行
  nextTick(keepCaretAboveStatusBar);
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

// 粘贴图片：按偏好保存到文档目录/assets 子目录并插入相对路径引用（未保存文档跳过）
async function onEditablePaste(e) {
  // 代码块语言输入框内粘贴：原生文本粘贴（不走 Markdown 通道/图片落盘）
  if (e.target?.closest?.('[data-lang-input]')) return;
  const items = Array.from(e.clipboardData?.items || []);
  const imageItem = items.find((item) => item.type.startsWith('image/'));
  if (!imageItem) {
    // 文本粘贴：含 Markdown 语法时按 Markdown 解析插入（标题/列表/代码/表格等结构保留）；
    // 纯文本走浏览器默认插入。统一规范化换行符（外部剪贴板可能带 CRLF/CR）
    const text = (e.clipboardData?.getData('text/plain') || '').replace(/\r\n?/g, '\n');
    // 代码/原文块（pre 编辑）与表格编辑中：内容即纯文本，绝不做 Markdown 解析——
    // 否则贴入的 #、```、| 等会被拆成新块，破坏代码与表格内容
    const el = currentEditable();
    const rawMode = !!el?.querySelector('pre') || editingBlock.value?.type === 'table';
    if (!rawMode && text && (await looksLikeMarkdown(text))) {
      e.preventDefault();
      await pasteMarkdownAtCaret(text);
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
  const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
  const ext = imageItem.type.split('/')[1] || 'png';
  const subDir = behavior === 'assets' ? 'assets' : null;
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
      const beforeRange = range.cloneRange();
      beforeRange.selectNodeContents(el);
      beforeRange.setEnd(range.startContainer, range.startOffset);
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
    // 空段检测（与 splitAndCommit 同规则，避免孤立标记文本）
    const isEmptyDiv = (div) =>
      div.textContent.trim() === '' && !div.querySelector('img, input, hr, table, pre');
    // 前后段序列化并行（超长文档减少 IPC 等待）
    const [beforeMdRaw, afterMdRaw] = await Promise.all([
      isEmptyDiv(beforeDiv)
        ? ''
        : invoke('serialize_markdown', { blocks: domToBlockDtos(beforeDiv) }),
      isEmptyDiv(afterDiv)
        ? ''
        : invoke('serialize_markdown', { blocks: domToBlockDtos(afterDiv) }),
    ]);
    const beforeMd = beforeMdRaw.trim();
    const afterMd = afterMdRaw.trim();

    const pasted = text.trim();
    const combined = [beforeMd, pasted, afterMd].filter((s) => s !== '').join('\n\n');

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
      index = blocks.value.findIndex((b) => b.id === editingId.value);
      oldBlock = blocks.value[index];
    }
    editableEl = null;
    if (!oldBlock) {
      editingId.value = null;
      return true;
    }
    if (isAppend) {
      content.value = base ? base + '\n\n' + combined : combined;
    } else {
      content.value = content.value.slice(0, oldBlock.start) + combined + content.value.slice(oldBlock.end);
    }
    // 增量重解析合并区间并平移后续偏移
    const replacements = await parseAnchoredBlocks(combined, oldBlock.start);
    const delta = combined.length - (oldBlock.end - oldBlock.start);
    blocks.value.splice(index, isAppend ? 0 : 1, ...replacements);
    shiftBlockOffsets(index + replacements.length, delta);
    publishBlocks();

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

function onEditableKeydown(e) {
  if (e.isComposing) return;
  // 斜杠菜单打开时优先路由：上下选择、Enter 应用、Esc 关闭
  if (slashOpen.value) {
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
        return;
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
        return;
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
        return;
      }
      if (items.length) {
        // 循环导航（到底/到顶回绕），滚动跟随由菜单面板负责
        slashIndex.value =
          e.key === 'ArrowDown'
            ? (slashIndex.value + 1) % items.length
            : (slashIndex.value - 1 + items.length) % items.length;
      }
      return;
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
        return;
      }
      if (item?.id === 'table') {
        e.preventDefault();
        const fields = ['item', 'rows', 'cols'];
        const step = e.key === 'ArrowRight' ? 1 : -1;
        const next = (fields.indexOf(slashTableField.value) + step + fields.length) % fields.length;
        slashTableField.value = fields[next];
        return;
      }
      // 警告框行：←/→ 循环切换类型（Enter 应用当前类型）
      if (item?.id === 'callout') {
        e.preventDefault();
        const step = e.key === 'ArrowRight' ? 1 : -1;
        slashCalloutType.value =
          (slashCalloutType.value + step + CALLOUT_TYPES.length) % CALLOUT_TYPES.length;
        return;
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
        return;
      }
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      applySlashItem(items[slashIndex.value]);
      return;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      slashOpen.value = false;
      return;
    }
  }
  // 围栏语言补全打开时：↑/↓ 选择（循环回绕）、Enter/Tab 补全、Esc 关闭
  if (langOpen.value) {
    const list = langItems.value;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (list.length) {
        langIndex.value =
          e.key === 'ArrowDown'
            ? (langIndex.value + 1) % list.length
            : (langIndex.value - 1 + list.length) % list.length;
      }
      return;
    }
    if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault();
      if (list.length) applyLangItem(list[langIndex.value]);
      return;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      langOpen.value = false;
      return;
    }
  }
  // 代码块语言输入框：Enter/Tab/Esc 回到代码区（语言补全打开时上面的菜单路由已拦截），
  // 其余按键原生（编辑语言；输入经 onEditableInput 触发补全菜单）
  if (e.target?.closest?.('[data-lang-input]')) {
    if (e.key === 'Enter' || e.key === 'Tab' || e.key === 'Escape') {
      e.preventDefault();
      langMenuInput = null;
      const pre = e.target.closest('pre');
      const code = pre?.querySelector('code');
      if (code) placeCursorAtStart(code);
      pre?.closest('[contenteditable]')?.focus({ preventScroll: true });
    }
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
    // 列表项内 Enter：非空项拆出同类型新列表项续写；空项退出列表转为段落（Typora 式）
    if (editable && handleListEnter(editable)) return;
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
    // 光标在块开头：样式块降级为段落，段落并入上一块（Typora 式退格）；
    // 其余位置交给浏览器原生删除
    if (isCaretAtStart(editableEl)) {
      e.preventDefault();
      backspaceAtBlockStart();
    }
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
    cancelEdit();
  }
}


// 任务列表勾选：标记替换由 Rust 完成（替换根块源码中第 occurrence 个 [ ]/[x]），
// 再增量重解析该根块。嵌套任务项没有自身源码区间：按块树 DFS 前序定位其根块与序号。
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
  content.value = content.value.slice(0, rootBlock.start) + next + content.value.slice(rootBlock.end);
  await reparseRegion(index, rootBlock, next);
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

// ---------- 大纲联动 ----------

// 滚动到指定脚注定义（底部集中区域）并短暂闪烁高亮
function scrollToFootnote(id) {
  const root = scrollRoot.value;
  if (!root) return;
  const el = root.querySelector(`[data-footnote-def="${id}"]`);
  if (!el) return;
  el.scrollIntoView({ behavior: 'smooth', block: 'center' });
  flashId.value = `fn-def-${id}`;
  setTimeout(() => {
    if (flashId.value === `fn-def-${id}`) flashId.value = null;
  }, 1200);
}

// 从脚注定义返回对应引用位置
function scrollToFootnoteRef(refIndex) {
  const root = scrollRoot.value;
  if (!root) return;
  const el = root.querySelector(`[data-footnote-ref="${refIndex}"]`);
  if (!el) return;
  el.scrollIntoView({ behavior: 'smooth', block: 'center' });
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
  let node;
  while ((node = walker.nextNode())) {
    const range = document.createRange();
    range.selectNodeContents(el);
    range.setEnd(node, node.textContent.length);
    const div = document.createElement('div');
    div.append(range.cloneContents());
    beforeParts.push(domToBlockDtos(div));
    nodes.push(node);
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
        const range = document.createRange();
        range.selectNodeContents(el);
        range.setEnd(sel.anchorNode, sel.anchorOffset);
        const beforeDiv = document.createElement('div');
        beforeDiv.append(range.cloneContents());
        const beforeMd = await invoke('serialize_markdown', { blocks: domToBlockDtos(beforeDiv) }).catch(() => '');
        if (editingId.value === '__append__') {
          const fullMd = (await invoke('serialize_markdown', { blocks: domToBlockDtos(el) }).catch(() => '')).trim();
          const base = content.value ? content.value.replace(/\s*$/, '') : '';
          const anchor = base ? base.length + 2 : 0;
          savedAnchorOffset = fullMd ? anchor + lcpLength(beforeMd, fullMd) : base.length;
        } else {
          const block = blocks.value.find((b) => b.id === editingId.value);
          const fullMd = await invoke('serialize_markdown', { blocks: domToBlockDtos(el) }).catch(() => '');
          savedAnchorOffset = block && block.start != null ? block.start + lcpLength(beforeMd, fullMd) : 0;
        }
        const rect = sel.getRangeAt(0).getBoundingClientRect();
        savedTopMargin = rect.top - root.getBoundingClientRect().top;
        await commitEdit();
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
        const rawOffset = Math.max(0, savedAnchorOffset - block.start);
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

provide('scrollToFootnote', (id) => scrollToFootnote(id));
provide('scrollToFootnoteRef', (refIndex) => scrollToFootnoteRef(refIndex));
// 内容目录（[TOC] 块）：文档块数据源与标题定位（BlockView 注入使用）
provide('allBlocks', blocks);
provide('scrollToBlock', (id) => scrollToBlock(id));

// 滚动到指定块并短暂闪烁高亮（侧边栏目录/大纲点击定位）
function scrollToBlock(id) {

  const root = scrollRoot.value;
  if (!root) return;
  // 渐进挂载：目标行可能尚在配额外（只有占位条）。分批扩配额（每批 2000 行），
  // 避免跳转大文档末尾时一次同步创建数千组件的单帧卡顿
  const idx = allBlocks.value.findIndex((b) => b.id === id);
  if (idx >= 0 && idx >= rowQuota.value) {
    const target = idx + 1;
    const step = () => {
      if (rowQuota.value >= target) return;
      rowQuota.value = Math.min(target, rowQuota.value + 2000);
      if (rowQuota.value < target) setTimeout(step, 0);
    };
    step();
  }
  // 行创建/内容挂载是异步的（配额分批扩展）：找不到元素时持续重试
  const tryScroll = (attempts) => {
    const el = root.querySelector(`[data-block-id="${id}"]`);
    if (!el) {
      if (attempts > 0) setTimeout(() => tryScroll(attempts - 1), 60);
      return;
    }
    el.scrollIntoView({ behavior: 'smooth', block: 'start' });
    flashId.value = id;
    setTimeout(() => {
      if (flashId.value === id) flashId.value = null;
    }, 1200);
  };
  tryScroll(30);
}

// 标题位置缓存（块树发布后重建，滚动停止后再校准；滚动判断零 DOM 查询）
let headingPositions = [];
watch(
  renderedBlocks,
  async () => {
    await nextTick();
    rebuildHeadingPositions();
  },
  { flush: 'post' },
);

// 滚动时同步当前标题（取滚动位置上方最近的标题块），供大纲高亮；滚动即关闭右键菜单。
// 虚拟滚动下占位行高度随内容挂载被修正，滚动停止后重建位置缓存保持高亮准确
let scrollTicking = false;
let headingRebuildTimer = null;
let scrollEndTimer = null;
function onEditorScroll(e) {
  closeCtxMenu();
  // 滚动中显示覆盖层滑轨并更新位置
  sbOnScroll();
  clearTimeout(headingRebuildTimer);
  headingRebuildTimer = setTimeout(rebuildHeadingPositions, 250);
  if (scrollTicking) return;
  scrollTicking = true;
  requestAnimationFrame(() => {
    scrollTicking = false;
    const top = e.target.scrollTop + 8;
    let current = null;
    for (const h of headingPositions) {
      if (h.top <= top) current = h.id;
      else break;
    }
    if (current !== activeHeadingId.value) {
      activeHeadingId.value = current;
      emit('update:active-heading', current);
    }
  });
}

function rebuildHeadingPositions() {
  const root = scrollRoot.value;
  if (!root) {
    headingPositions = [];
    return;
  }
  // 单次遍历已挂载块元素建 id→元素 表，再查表取标题位置——
  // 逐标题 querySelector 在标题上千时是每次提交数千次全树扫描（编辑卡顿主因）
  const mountedEls = new Map();
  for (const el of root.querySelectorAll('[data-block-id]')) {
    mountedEls.set(el.getAttribute('data-block-id'), el);
  }
  headingPositions = blocks.value
    .filter((b) => b.type === 'heading')
    .map((b) => ({ id: b.id, top: mountedEls.get(b.id)?.offsetTop ?? null }))
    .filter((h) => h.top != null);
}

// 滚动条悬停揭示（自动隐藏模式）：指针进入容器右缘 16px 内显示，离开隐藏
function onEditorMouseMove(e) {
  sbOnMouseMove(e);
}
function onEditorMouseLeave(e) {
  sbOnMouseLeave(e);
}

// 加载新文档（打开/新建文件）：替换全文并整树重解析，退出当前编辑态
// 加载新文档（打开/新建文件）：替换全文并整树重解析，退出当前编辑态。
// pre 为打开命令预解析的首屏块（大文件渐进加载）：直接上屏，尾部由后台
// parse_blocks 补齐（首屏末块可能被截断，尾部带完整上下文重解析后原位替换自愈）；
// 尾部返回前若已发生编辑（content 引用变化），丢弃结果走全量重解析
async function loadDocument(text, pre = null) {
  ++parseSeq; // 作废旧文档可能在途的解析结果（渐进分支不经 reparse，需手动作废）
  editingId.value = null;
  editableEl = null;
  rowQuota.value = INITIAL_ROW_QUOTA;
  content.value = text;
  savedContent.value = text;
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
  const md = await selectionToMarkdown(sel.getRangeAt(0)).catch(() => null);
  if (md != null && md !== '') {
    await navigator.clipboard.writeText(md).catch(() => {});
  } else {
    // 序列化失败时回退纯文本
    await navigator.clipboard.writeText(sel.toString()).catch(() => {});
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

// 剪切：与复制同通道写 Markdown；选区完整落在编辑容器内时才删除选中内容
// （渲染态区域不是 contenteditable，无删除语义）
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
  }
}

// 未处于编辑态时的 Ctrl+A：只选择编辑器文档内容（不含侧栏/状态栏界面文字）；
// Delete/Backspace：删除跨块/整块选区（选区完整落在编辑容器内时交给原生编辑行为）
function onDocumentKeydown(e) {
  if (props.sourceMode) return;
  if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'a') {
    // 输入框/文本域/编辑容器内：交给各自的原生或两段式逻辑
    if (e.target.closest?.('input, textarea, [contenteditable="true"]')) return;
    e.preventDefault();
    const root = scrollRoot.value;
    if (!root) return;
    const sel = window.getSelection();
    const all = document.createRange();
    all.selectNodeContents(root);
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
  if (editingId.value !== null) rafKeepCaret();
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
  clearTimeout(headingRebuildTimer);
  clearTimeout(scrollEndTimer);
  cancelAnimationFrame(keepCaretRaf);
  keepCaretRaf = 0;
  // 断开虚拟滚动与重活懒执行的观察器
  rowObserver?.disconnect();
  rowObserver = null;
  blockObserver?.disconnect();
  blockObserver = null;
});

defineExpose({ scrollToBlock, loadDocument, getContent, markSaved, isDirty: () => isDirty.value, captureScrollPosition, restoreScrollPosition });
</script>

<template>
  <div class="relative flex h-full flex-col">
    <textarea
      v-if="sourceMode"
      ref="sourceRoot"
      class="t-root flex-1 resize-none border-none font-mono outline-none"
      v-model="content"
      @input="onInput"
      @keyup="onInput"
      @click="onInput"
    ></textarea>

    <div v-else ref="scrollRoot" class="t-root flex-1 overflow-y-auto" :class="{ 'md-first-indent': firstLineIndent, 'sb-auto-hide': scrollbarAutoHide, 'sb-no-autohide': !scrollbarAutoHide }" @scroll.passive="onEditorScroll" @contextmenu="onContextMenu" @mousemove.passive="onEditorMouseMove" @mouseleave="onEditorMouseLeave">
      <div class="t-measure">
      <!-- 虚拟滚动 + 渐进挂载：配额内每块包一层 VRow（占位保总高），仅视口余量内挂载内容；
           配额外行由下方估计高度占位条代替（大文档首屏组件规模恒定）；
           编辑块经 force 强制渲染（不可卸载，否则丢失光标与未提交内容） -->
      <VRow
        v-for="{ block, estimate } in visibleRows"
        :key="block.id"
        :block="block"
        :estimate="estimate"
        :force="editingId === block.id"
      >
        <!-- Typora 式就地编辑：渲染后的内容直接在 contenteditable 中编辑 -->
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
        <template v-if="editingId === block.id">
          <div
            :ref="setEditableEl"
            class="md-editing px-1 outline-none"
            contenteditable="true"
            spellcheck="false"
            @input="onEditableInput"
            @paste="onEditablePaste"
            @keydown="onEditableKeydown"
            @blur="onEditableBlur"
          ></div>
        </template>
        <div
          v-else
          class="md-block cursor-text px-1"
          :class="{ 'md-flash': flashId === block.id }"
          @click="startEdit(block)"
        >
          <BlockView :block="block" :ordinal="rootOrdinals.get(block.id) || 1" @toggle-task="toggleTask" />
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
        spellcheck="false"
        @input="onEditableInput"
          @paste="onEditablePaste"
        @keydown="onEditableKeydown"
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
          <BlockView :block="block" :ordinal="1" @toggle-task="toggleTask" />
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
  </div>
  <!-- 斜杠命令菜单 / 围栏语言补全菜单（同一面板，互斥触发）：Teleport 移出根布局（多根片段），其挂载/卸载不再引起
       布局子节点重建；.t-app 包装使主题 blocks 规则（.t-app 作用域）同样生效 -->
  <Teleport to="body">
    <div v-if="slashOpen || langOpen" class="t-app">
      <SlashMenu
        :items="langOpen ? langMenuItems : slashItems"
        :index="langOpen ? langIndex : slashIndex"
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
        :left="langOpen ? langPos.left : slashPos.left"
        :top="langOpen ? langPos.top : slashPos.top"
        @pick="(item, option) => (langOpen ? applyLangItem(item.id) : applySlashItem(item, option))"
        @hover="(i) => (langOpen ? (langIndex = i) : (slashIndex = i))"
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
</template>
