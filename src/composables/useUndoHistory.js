import { watch } from 'vue';

// 撤销/重做历史（正文快照；1s 内的连续变化合并为一站）
// 快照在 watch(content) 里自动记录（覆盖全部编辑路径：提交/拆分/粘贴/删除/勾选等），
// 历史回放与文档加载期间不写历史。Ctrl+Z 撤销，Ctrl+Shift+Z / Ctrl+Y 重做。
//
// ctx：Editor.vue 共享上下文。content/blocks/editingId 为 ref（按引用共享）；
// editableEl/suppressBlurCommit/pendingCaretOffset 经 get/set 访问器共享；
// currentEditable/textBeforeCaretIn/reparse/commitEdit 为编辑链路函数（运行时解析）。
export function useUndoHistory(ctx) {
  const undoStack = [];
  const redoStack = [];
  const HISTORY_LIMIT = 200;
  let applyingHistory = false; // 历史回放期间不写历史
  let loadingDoc = false;      // loadDocument 期间不写历史
  let lastPushAt = 0;

  // 当前光标的文档近似偏移（块源起点 + 块内纯文本前缀；格式化文本为近似值，仅用于回放落点）
  function captureCaretOffset() {
    const el = ctx.currentEditable();
    if (!el || ctx.editingId.value === null) return null;
    const block = ctx.blocks.value.find((b) => b.id === ctx.editingId.value);
    if (!block || block.start == null) return null;
    const probed = ctx.textBeforeCaretIn(el);
    return block.start + (probed ? probed.text.length : 0);
  }

  watch(ctx.content, (next, prev) => {
    if (applyingHistory || loadingDoc || prev === next) return;
    const now = Date.now();
    // 1s 内的连续变化合并为一站（上一站快照已覆盖爆发前状态）
    if (now - lastPushAt < 1000) {
      lastPushAt = now;
      return;
    }
    undoStack.push({ content: prev, caret: captureCaretOffset() });
    if (undoStack.length > HISTORY_LIMIT) undoStack.shift();
    redoStack.length = 0;
    lastPushAt = now;
  });

  // 历史回放：替换正文并全文重解析，光标落到快照记录的块（近似偏移）
  async function applyHistorySnapshot(snap) {
    applyingHistory = true;
    ctx.editableEl = null;
    ctx.editingId.value = null;
    ctx.content.value = snap.content;
    try {
      await ctx.reparse();
    } finally {
      applyingHistory = false;
    }
    if (snap.caret != null) {
      const block =
        ctx.blocks.value.find((b) => b.start != null && b.end != null && snap.caret >= b.start && snap.caret <= b.end) ??
        ctx.blocks.value[ctx.blocks.value.length - 1];
      if (block) {
        ctx.suppressBlurCommit = true;
        ctx.pendingCaretOffset = Math.max(0, snap.caret - block.start);
        ctx.editingId.value = block.id;
      }
    }
  }

  // 撤销：编辑中先提交当前编辑（进入历史）再回退；覆盖 contenteditable 原生撤销
  async function undoHistory() {
    if (ctx.editingId.value !== null && ctx.currentEditable()) await ctx.commitEdit();
    const snap = undoStack.pop();
    if (!snap) return;
    redoStack.push({ content: ctx.content.value, caret: captureCaretOffset() });
    await applyHistorySnapshot(snap);
  }

  // 重做：与撤销对称
  async function redoHistory() {
    if (ctx.editingId.value !== null && ctx.currentEditable()) await ctx.commitEdit();
    const snap = redoStack.pop();
    if (!snap) return;
    undoStack.push({ content: ctx.content.value, caret: captureCaretOffset() });
    await applyHistorySnapshot(snap);
  }

  // 加载新文档时清空历史（上一文档的撤销栈不带入新文档）
  function resetHistory() {
    undoStack.length = 0;
    redoStack.length = 0;
    lastPushAt = 0;
  }

  return {
    undoHistory,
    redoHistory,
    resetHistory,
    // 文档加载期间不写历史（loadDocument 链路经此开关）
    setHistoryLoading: (v) => {
      loadingDoc = v;
    },
  };
}
