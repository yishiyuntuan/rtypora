// 渲染态表格防横向滚动（视图层自适应，不写源码）：
// ResizeObserver 监听表格父容器宽度——
//   溢出（table.scrollWidth > 容器 clientWidth + 1）时：
//     1. 量取各列当前渲染宽度为需求权重（用户经 useTableColumnWidth 拖过的列以其
//        col style.width 为准，参与比例分配而非被忽略），切 table-layout: fixed
//        并按比例分配容器宽度（单列下限 40px）；
//     2. 仍溢出（内容不可断行顶出单元格）则加 .md-table-compress（降单元格 padding +
//        overflow-wrap:anywhere 断词，CSS 在 style.css）；
//   容器变宽后逐步还原：先撤 compress 还 padding，再撤 fixed 恢复自然布局。
// 与 useTableColumnWidth 共存：进入 fixed 前快照 col 宽度/fixed 类持有状态，
// 还原时按快照恢复；接管期间用户拖过的列（宽度偏离我们写入值）保留用户值。
const MIN_COL_WIDTH = 40; // 单列下限（px），与 useTableColumnWidth 拖拽下限一致
const FIXED_CLASS = 'md-table-fixed';
const COMPRESS_CLASS = 'md-table-compress';

export function attachTableFitWidth(table) {
  const container = table.parentElement;
  if (!container || typeof ResizeObserver === 'undefined') return () => {};
  const colEls = () => [...table.querySelectorAll('colgroup col')];
  const headCells = () => [...(table.querySelector('thead tr')?.children || [])];
  if (!colEls().length || colEls().length !== headCells().length) return () => {};

  // mode: 'natural'（自适应布局）| 'fixed'（按比例分配）| 'compress'（fixed + 压缩）
  let mode = 'natural';
  let weights = []; // 各列需求权重（px）
  let assigned = []; // 我们写入各 col 的宽度（用于识别用户中途拖拽）
  let snapshot = null; // 进入 fixed 前的用户状态快照

  const overflowing = () => table.scrollWidth > container.clientWidth + 1;

  // 按比例把容器宽度分配给各列：下限 40px 顶死的列退出分配，余额在剩余列间迭代再分；
  // 全部顶到下限仍放不下时返回的宽度总和会超过容器（交由 compress 兜底）
  function allocate(budget, ws) {
    const widths = new Array(ws.length).fill(0);
    let rest = budget;
    let active = [...ws.keys()];
    while (active.length) {
      const sum = active.reduce((s, i) => s + ws[i], 0) || 1;
      const keep = [];
      for (const i of active) {
        if ((ws[i] / sum) * rest < MIN_COL_WIDTH) {
          widths[i] = MIN_COL_WIDTH;
          rest -= MIN_COL_WIDTH;
        } else {
          keep.push(i);
        }
      }
      if (keep.length === active.length) {
        // 无触限列：按比例分完（取整损失由 table width:100% 吸收）
        for (const i of active) widths[i] = Math.floor((ws[i] / sum) * rest);
        break;
      }
      active = keep; // 触限列退出，余额在剩余列间再分
    }
    return widths;
  }

  // 各列需求权重：拖拽过的列取 col style.width；自然布局列临时解除宽度钳制量取真实内容宽度
  function measureWeights() {
    const cols = colEls();
    const fixedByDrag = table.classList.contains(FIXED_CLASS);
    const prevWidth = table.style.width;
    const prevMaxWidth = table.style.maxWidth;
    if (!fixedByDrag) {
      table.style.width = 'max-content';
      table.style.maxWidth = 'none';
    }
    const ws = headCells().map((th, i) => {
      const dragged = parseFloat(cols[i]?.style.width) || 0;
      return Math.max(MIN_COL_WIDTH, dragged || th.getBoundingClientRect().width);
    });
    table.style.width = prevWidth;
    table.style.maxWidth = prevMaxWidth;
    return ws;
  }

  // 接管布局：快照用户状态 → 量权重 → fixed + 比例分配
  function enterFixed() {
    snapshot = {
      widths: colEls().map((c) => c.style.width),
      hadFixed: table.classList.contains(FIXED_CLASS),
      tableWidth: table.style.width,
    };
    weights = measureWeights();
    distribute();
    mode = 'fixed';
  }

  // 按当前容器宽度重分配（fixed/compress 下容器尺寸变化时调用）；
  // 权重实时读 col 当前 px 宽度——接管期间用户拖拽的列宽被采纳为权重参与缩放
  function distribute() {
    const live = colEls().map((c) => parseFloat(c.style.width) || 0);
    if (mode !== 'natural' && live.every((w) => w > 0)) weights = live;
    const widths = allocate(container.clientWidth, weights);
    assigned = widths;
    colEls().forEach((c, i) => {
      if (widths[i] != null) c.style.width = `${widths[i]}px`;
    });
    table.classList.add(FIXED_CLASS);
    table.style.width = '100%';
  }

  // 还原自然布局：用户中途拖过的列保留其宽度，其余按进入时快照恢复
  function restoreNatural() {
    colEls().forEach((c, i) => {
      const cur = parseFloat(c.style.width) || 0;
      const userDragged = assigned[i] != null && Math.abs(cur - assigned[i]) > 0.5;
      if (!userDragged) c.style.width = snapshot?.widths[i] || '';
    });
    if (!snapshot?.hadFixed) table.classList.remove(FIXED_CLASS);
    table.style.width = snapshot?.tableWidth || '';
    table.classList.remove(COMPRESS_CLASS);
    snapshot = null;
    assigned = [];
    mode = 'natural';
  }

  function recheck() {
    if (container.clientWidth <= 0) return; // 隐藏/离屏时不判定
    if (mode === 'natural') {
      if (overflowing()) {
        enterFixed();
        if (overflowing()) {
          table.classList.add(COMPRESS_CLASS);
          mode = 'compress';
        }
      }
      return;
    }
    distribute();
    if (mode === 'compress') {
      // 先还 padding：撤掉 compress 后不再溢出则留在 fixed
      table.classList.remove(COMPRESS_CLASS);
      mode = 'fixed';
      if (overflowing()) {
        table.classList.add(COMPRESS_CLASS);
        mode = 'compress';
        return;
      }
    }
    // 容器可能已足够宽：尝试还原自然布局，仍溢出则以自然渲染宽度重新量权重回到 fixed
    if (!overflowing()) {
      restoreNatural();
      if (overflowing()) {
        enterFixed();
        if (overflowing()) {
          table.classList.add(COMPRESS_CLASS);
          mode = 'compress';
        }
      }
    }
  }

  const ro = new ResizeObserver(recheck);
  ro.observe(container);
  recheck(); // 挂载时可能已处于溢出状态（RO 首帧回调前先行判定）

  return () => {
    ro.disconnect();
    if (mode !== 'natural') restoreNatural();
  };
}
