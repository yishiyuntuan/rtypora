// 渲染态表格跨单元格矩形选区（Alt + 鼠标拖拽）：
// mousedown（altKey）于 td/th 起，mouseover 经过单元格更新矩形（min/max 行列），
// 选中格加 .md-cell-sel（样式在 style.css，半透明主题 accent 底）；mouseup 结束拖拽。
// Ctrl+C 且矩形选区存在时剪贴板写 TSV（单元格 textContent，行 \n、列 \t）并阻止默认复制；
// Esc 或点击表格外清除。无 Alt 的普通文本选择不受影响。
export function attachTableCellSelect(table) {
  let anchor = null; // { r, c } 拖拽起点（行索引含表头行，列索引为行内位置）
  let focus = null; // { r, c } 拖拽当前点
  let dragging = false;

  function cellPos(cell) {
    const rows = [...table.querySelectorAll('tr')];
    const row = cell.closest('tr');
    const r = rows.indexOf(row);
    const c = row ? [...row.children].indexOf(cell) : -1;
    return r < 0 || c < 0 ? null : { r, c };
  }

  // 矩形范围（行列 min/max），无选区返回 null
  function rect() {
    if (!anchor || !focus) return null;
    return {
      r0: Math.min(anchor.r, focus.r),
      r1: Math.max(anchor.r, focus.r),
      c0: Math.min(anchor.c, focus.c),
      c1: Math.max(anchor.c, focus.c),
    };
  }

  function paint() {
    table.querySelectorAll('.md-cell-sel').forEach((c) => c.classList.remove('md-cell-sel'));
    const box = rect();
    if (!box) return;
    const rows = [...table.querySelectorAll('tr')];
    for (let r = box.r0; r <= box.r1; r += 1) {
      const cells = rows[r]?.children || [];
      for (let c = box.c0; c <= box.c1 && c < cells.length; c += 1) {
        cells[c].classList.add('md-cell-sel');
      }
    }
  }

  function clear() {
    anchor = null;
    focus = null;
    dragging = false;
    table.querySelectorAll('.md-cell-sel').forEach((c) => c.classList.remove('md-cell-sel'));
  }

  function onMouseDown(e) {
    if (!e.altKey) return; // 无 Alt：保留浏览器普通文本选择
    const cell = e.target.closest?.('td, th');
    if (!cell || !table.contains(cell)) return;
    e.preventDefault(); // 阻止拖出原生选择
    dragging = true;
    anchor = cellPos(cell);
    focus = anchor;
    paint();
  }

  function onMouseOver(e) {
    if (!dragging) return;
    const cell = e.target.closest?.('td, th');
    if (!cell || !table.contains(cell)) return;
    const pos = cellPos(cell);
    if (!pos) return;
    focus = pos;
    paint();
  }

  function onMouseUp() {
    dragging = false;
  }

  function onKeyDown(e) {
    if (e.key === 'Escape' && anchor) clear();
  }

  // 点击表格外清除选区（表格内普通点击不清除，保留 Ctrl+C 复制能力）
  function onDocMouseDown(e) {
    if (anchor && !table.contains(e.target)) clear();
  }

  function onCopy(e) {
    const box = rect();
    if (!box) return;
    // 存在原生文本选择时不劫持复制（用户想复制的是文本而非矩形选区）
    const sel = window.getSelection();
    if (sel && !sel.isCollapsed) return;
    const rows = [...table.querySelectorAll('tr')];
    const lines = [];
    for (let r = box.r0; r <= box.r1; r += 1) {
      const cells = rows[r]?.children || [];
      const parts = [];
      for (let c = box.c0; c <= box.c1 && c < cells.length; c += 1) {
        // 单元格内换行压平为空格，保证 TSV 行列结构
        parts.push((cells[c].textContent || '').replace(/\r\n?/g, ' ').replace(/\n/g, ' '));
      }
      lines.push(parts.join('\t'));
    }
    e.preventDefault();
    e.clipboardData?.setData('text/plain', lines.join('\n'));
  }

  table.addEventListener('mousedown', onMouseDown);
  table.addEventListener('mouseover', onMouseOver);
  document.addEventListener('mouseup', onMouseUp);
  document.addEventListener('keydown', onKeyDown);
  document.addEventListener('mousedown', onDocMouseDown, true);
  // copy 事件派发到焦点元素（可能是表格内或 body），document 层统一兜底
  document.addEventListener('copy', onCopy);

  return () => {
    table.removeEventListener('mousedown', onMouseDown);
    table.removeEventListener('mouseover', onMouseOver);
    document.removeEventListener('mouseup', onMouseUp);
    document.removeEventListener('keydown', onKeyDown);
    document.removeEventListener('mousedown', onDocMouseDown, true);
    document.removeEventListener('copy', onCopy);
    clear();
  };
}
