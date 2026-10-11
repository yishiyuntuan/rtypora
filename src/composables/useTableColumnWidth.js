// 渲染态表格列宽拖拽 + 会话保持：
// 表头每个 th 右缘注入 6px 拖拽手柄（.md-col-resize，仅 hover 表格时可见，样式在 style.css）；
// pointerdown 记录起始 x 与该列当前宽度，pointermove 更新对应 <col> 的 px 宽度，pointerup 结束；
// 拖拽中给表格加 .md-table-fixed（table-layout: fixed）。
// 宽度按块 id 存模块级 Map（同一文档会话内有效，不写源码、不入 localStorage），
// 表格组件重挂载（块 id 不变）时重套 col 宽度与 fixed 类。
const colWidths = new Map(); // blockId -> number[]（px，与表头列一一对应）

export function attachTableColumnWidth(table, blockId) {
  const headCells = [...(table.querySelector('thead tr')?.children || [])];
  const cols = [...table.querySelectorAll('colgroup col')];
  if (!headCells.length || cols.length !== headCells.length) return () => {};

  // 会话恢复：该块有已存宽度则重套 col 宽度并切 fixed 布局
  const saved = colWidths.get(blockId);
  if (saved) {
    saved.forEach((w, i) => {
      if (cols[i] && w > 0) cols[i].style.width = `${w}px`;
    });
    table.classList.add('md-table-fixed');
  }

  const disposers = [];
  headCells.forEach((th, i) => {
    const handle = document.createElement('span');
    handle.className = 'md-col-resize';
    th.append(handle);

    let startX = 0;
    let startW = 0;
    function onMove(e) {
      // 最小列宽 40px，避免拖成不可见
      const w = Math.max(40, startW + e.clientX - startX);
      cols[i].style.width = `${w}px`;
    }
    function onUp() {
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
      // 记录整表各列宽（未拖过的列取当前渲染宽度，fixed 布局下重套后稳定）
      colWidths.set(blockId, headCells.map((c) => Math.round(c.getBoundingClientRect().width)));
    }
    function onDown(e) {
      if (e.altKey) return; // Alt+拖拽是矩形选区，手柄让位
      e.preventDefault();
      e.stopPropagation();
      startX = e.clientX;
      startW = th.getBoundingClientRect().width;
      table.classList.add('md-table-fixed');
      window.addEventListener('pointermove', onMove);
      window.addEventListener('pointerup', onUp);
    }
    handle.addEventListener('pointerdown', onDown);
    disposers.push(() => {
      handle.removeEventListener('pointerdown', onDown);
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
      handle.remove();
    });
  });

  return () => disposers.forEach((d) => d());
}
