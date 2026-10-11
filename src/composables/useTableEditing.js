import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { placeCursorAtStart, domToBlockDtos } from '../utils/wysiwyg.js';

// 表格编辑（Typora 式：就地编辑 + 悬停工具栏/四边 + 按钮/尺寸网格/更多操作）
//
// ctx：Editor.vue 共享上下文（currentEditable/editingId/blocks/content/syncing 为 ref，
// editableEl/suppressBlurCommit/pendingTableFocus/cursorAtStart/blurCommitTimer 经访问器共享；
// commitEdit/reparse/parseAnchoredBlocks/shiftBlockOffsets/publishBlocks/renumberOrderedMarkers
// 为编辑链路函数，运行时解析）。
export function useTableEditing(ctx) {
  // 光标所在表格单元格（不在表格单元格内返回 null）
  function caretTableCell() {
    const el = ctx.currentEditable();
    if (!el) return null;
    const sel = window.getSelection();
    if (!sel.rangeCount) return null;
    const node = sel.anchorNode;
    const anchor = node?.nodeType === Node.TEXT_NODE ? node.parentElement : node;
    const cell = anchor?.closest('td, th');
    return cell && el.contains(cell) ? cell : null;
  }

  // 光标所在列的对齐（'' 默认 / left / center / right），工具栏对齐按钮高亮用；
  // 由 document selectionchange 跟踪（见 Editor.vue 文件末尾 onMounted）
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
    const el = ctx.currentEditable();
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

  // 表内矩阵覆盖粘贴：从光标单元格起把 TSV 矩阵覆盖写入，行列不足按需扩展
  //（列扩展按每行自身标签 th/td 追加、行扩展复用 emptyBodyRow），
  // 提交走既有 domToBlockDtos→serialize 链路（commitEdit）
  function pasteTableMatrix(cell, matrix) {
    if (!cell || !Array.isArray(matrix) || !matrix.length) return false;
    const el = ctx.currentEditable();
    const table = el?.querySelector('table');
    if (!table || !el.contains(cell)) return false;
    const rows = matrix.filter((r) => Array.isArray(r));
    if (!rows.length) return false;
    const cols = Math.max(...rows.map((r) => r.length));
    if (!cols) return false;
    const anchorRow = cell.closest('tr');
    const allRows = [...table.querySelectorAll('tr')];
    const r0 = allRows.indexOf(anchorRow);
    const c0 = anchorRow ? [...anchorRow.children].indexOf(cell) : -1;
    if (r0 < 0 || c0 < 0) return false;
    // 列不足：整列补齐（与 tableOp 插列同规则：按各行单元格标签追加空单元格）
    const needCols = c0 + cols;
    while ((allRows[0]?.children.length ?? 0) < needCols) {
      allRows.forEach((r) => {
        const tag = r.parentElement.tagName === 'THEAD' ? 'th' : 'td';
        const next = document.createElement(tag);
        next.innerHTML = '<br>';
        r.append(next);
      });
    }
    // 行不足：表体尾部追加空数据行（列数随补齐后的表头）
    const tbody = table.querySelector('tbody');
    const needRows = r0 + rows.length;
    while (allRows.length < needRows && tbody) {
      const tr = emptyBodyRow(table);
      tbody.append(tr);
      allRows.push(tr);
    }
    // 覆盖写入（纯文本；空串单元格保留 <br> 占位，避免空 td 塌高）
    rows.forEach((row, i) => {
      const tr = allRows[r0 + i];
      row.forEach((text, j) => {
        const target = tr?.children[c0 + j];
        if (!target) return;
        if (text) target.textContent = text;
        else target.innerHTML = '<br>';
      });
    });
    ctx.commitEdit();
    return true;
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
    if (ctx.editingId.value === block.id) {
      run();
      return;
    }
    pendingTableAction = run;
    ctx.suppressBlurCommit = true;
    if (ctx.editingId.value !== null && ctx.editableEl) {
      clearTimeout(ctx.blurCommitTimer);
      ctx.blurCommitTimer = null;
      ctx.commitEdit().finally(() => {
        ctx.editingId.value = block.id;
      });
      return;
    }
    ctx.editingId.value = block.id;
  }
  // 对齐操作：悬停触发（未选单元格）时作用于整个表格
  function onToolbarOp(block, op) {
    const fromHover = ctx.editingId.value !== block.id;
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
    const block = ctx.blocks.value.find((b) => b.id === btn.blockId);
    tableEdgeBtn.value = null;
    if (!block) return;
    tableToolbarClick(block, () => {
      const table = ctx.currentEditable()?.querySelector('table');
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
    'sep',
    { id: 'copyTableMarkdown', label: '复制为 Markdown' },
    { id: 'copyTableCsv', label: '复制为 CSV' },
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
      const table = ctx.currentEditable()?.querySelector('table');
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
    if (item.id === 'copyTableMarkdown') return copyTableAsMarkdown();
    if (item.id === 'copyTableCsv') return copyTableAsCsv();
    if (item.id === 'formatTable') return formatTableSource();
    if (item.id === 'deleteTable') return deleteCurrentTableBlock();
    tableOp(item.id);
  }

  // 复制表格：当前编辑内容经 Rust 序列化为 Markdown 写入剪贴板（不打断编辑）
  async function copyTableMarkdown() {
    const el = ctx.currentEditable();
    if (!el) return;
    const md = await invoke('serialize_markdown', { blocks: domToBlockDtos(el) }).catch(() => '');
    const text = md.trim();
    if (text) await navigator.clipboard.writeText(text).catch(() => {});
  }

  // 「复制为 Markdown / CSV」定位的表格块：悬停块优先，否则取当前编辑块
  function currentTableBlock() {
    const id = hoverTableId.value ?? ctx.editingId.value;
    return ctx.blocks.value.find((b) => b.id === id && b.type === 'table') || null;
  }

  // 复制为 Markdown：整块源码区间直写剪贴板（编辑中未提交改动不在源码内，复制的是已提交版本）
  async function copyTableAsMarkdown() {
    const block = currentTableBlock();
    if (!block) return;
    const md = ctx.content.value.slice(block.start, block.end).trim();
    if (md) await navigator.clipboard.writeText(md).catch((err) => console.error('复制表格 Markdown 失败', err));
  }

  // 复制为 CSV：块 DTO 的 table 字段经 Rust RFC4180 序列化写剪贴板
  async function copyTableAsCsv() {
    const block = currentTableBlock();
    if (!block?.table) return;
    const csv = await invoke('table_to_csv', { table: block.table }).catch(() => null);
    if (csv) await navigator.clipboard.writeText(csv).catch((err) => console.error('复制表格 CSV 失败', err));
  }

  // 格式化表格源码：提交当前编辑 → Rust 按列宽对齐管道 → 原位替换并保持编辑
  async function formatTableSource() {
    const el = ctx.currentEditable();
    if (!el || ctx.editingId.value === null || ctx.editingId.value === '__append__') return;
    ctx.syncing.value = true;
    try {
      const md = await invoke('serialize_markdown', { blocks: domToBlockDtos(el) }).catch(() => null);
      if (md == null) return; // 序列化失败：不动正文、保持编辑态
      const padded = await invoke('format_table_source', { markdown: md }).catch(() => null);
      if (!padded || padded === md) return;
      const index = ctx.blocks.value.findIndex((b) => b.id === ctx.editingId.value);
      const block = ctx.blocks.value[index];
      if (!block) return;
      ctx.content.value = ctx.content.value.slice(0, block.start) + padded + ctx.content.value.slice(block.end);
      const replacements = await ctx.parseAnchoredBlocks(padded, block.start).catch(() => null);
      const delta = padded.length - (block.end - block.start);
      ctx.editableEl = null;
      ctx.suppressBlurCommit = true;
      if (!replacements) {
        // 解析失败：全文重解析自愈
        ctx.editingId.value = null;
        ctx.suppressBlurCommit = false;
        await ctx.reparse();
        return;
      }
      ctx.blocks.value.splice(index, 1, ...replacements);
      ctx.shiftBlockOffsets(index + replacements.length, delta);
      ctx.publishBlocks();
      // 重解析后块 id 随内容变化：重建编辑容器，光标落到首个数据单元格
      ctx.pendingTableFocus = true;
      ctx.editingId.value = replacements[0]?.id ?? null;
      if (!replacements[0]) ctx.suppressBlurCommit = false;
    } finally {
      ctx.syncing.value = false;
    }
  }

  // 删除整个表格块（含源码区间与相邻换行），光标移到上一块末尾（与空块退格删除同规则）
  async function deleteCurrentTableBlock() {
    if (ctx.editingId.value === null || ctx.editingId.value === '__append__') return;
    ctx.syncing.value = true;
    try {
      const index = ctx.blocks.value.findIndex((b) => b.id === ctx.editingId.value);
      const oldBlock = ctx.blocks.value[index];
      ctx.editableEl = null;
      if (!oldBlock) {
        ctx.editingId.value = null;
        return;
      }
      let removeStart = oldBlock.start;
      let removeEnd = oldBlock.end;
      if (ctx.content.value[removeEnd] === '\n') removeEnd += 1;
      else if (removeStart > 0 && ctx.content.value[removeStart - 1] === '\n') removeStart -= 1;
      const removedLen = removeEnd - removeStart;
      ctx.content.value = ctx.content.value.slice(0, removeStart) + ctx.content.value.slice(removeEnd);
      ctx.blocks.value.splice(index, 1);
      ctx.shiftBlockOffsets(index, -removedLen);
      ctx.renumberOrderedMarkers(index);
      ctx.publishBlocks();
      const prev = ctx.blocks.value[index - 1];
      ctx.suppressBlurCommit = true;
      ctx.editingId.value = prev ? prev.id : (ctx.blocks.value[0]?.id ?? '__append__');
      ctx.cursorAtStart = !prev && ctx.blocks.value.length > 0;
      if (!prev && !ctx.blocks.value.length) ctx.suppressBlurCommit = false;
    } finally {
      ctx.syncing.value = false;
    }
  }

  return {
    // 状态与常量（模板绑定）
    tableCaretAlign, tablePanel, tablePanelPos, hoverTableId, tableEdgeBtn,
    tableGridHover, tableGridSize, TABLE_GRID_COLS, TABLE_GRID_ROWS, TABLE_MORE_ITEMS,
    // 事件处理（模板绑定）
    onTableBlockHover, onTableBlockLeave, onTableEdgeMove, onTableEdgeAdd,
    onToolbarOp, onToolbarDelete, onToolbarPanel, applyTableGrid, runTableMore,
    // 编辑链路共用（Editor.vue 键盘路由 / selectionchange / setEditableEl）
    caretTableCell, moveTableCaret, moveTableRowDown, updateTableCaretAlign, tableOp,
    // 表内矩阵覆盖粘贴（onEditablePaste 接入，见集成说明）
    pasteTableMatrix,
    // 悬停工具栏排队动作：setEditableEl 挂载后取走执行（取走即清空）
    takePendingTableAction() {
      const action = pendingTableAction;
      pendingTableAction = null;
      return action;
    },
  };
}
