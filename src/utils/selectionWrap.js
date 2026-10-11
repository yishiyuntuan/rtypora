// 选区符号包裹（对齐 MEO Enhanced）：编辑态选中文字后输入符号，用符号对包裹选区，
// 包裹后内部文字保持选中（再输入同符号可叠层）。纯 DOM 手术：选区前后各插入一个
// 文本节点（不用 surroundContents——选区可能跨节点），再重选内部文字；
// 成功后派发一次 input 事件，走 Editor.vue 既有 onEditableInput 后处理链路。

// 输入字符 -> [左界符, 右界符]（反引号为默认单界符，包裹时按内容升级）
export const WRAP_PAIRS = {
  '(': ['(', ')'],
  '[': ['[', ']'],
  '{': ['{', '}'],
  '<': ['<', '>'],
  '"': ['"', '"'],
  "'": ["'", "'"],
  '`': ['`', '`'],
  '*': ['*', '*'],
  _: ['_', '_'],
  '~': ['~', '~'],
  $: ['$', '$'],
  // 中文对（含直角引号）
  '（': ['（', '）'],
  '【': ['【', '】'],
  '“': ['“', '”'],
  '‘': ['‘', '’'],
  '《': ['《', '》'],
  '「': ['「', '」'],
  '『': ['『', '』'],
};

// 选区是否反向（focus 在 anchor 之前）：
// 新建 Range 先 setStart(anchor) 再 setEnd(focus)，focus 更早时 Range 被规范化为塌陷
function selectionIsBackward(sel) {
  if (sel.isCollapsed) return false;
  const r = document.createRange();
  r.setStart(sel.anchorNode, sel.anchorOffset);
  r.setEnd(sel.focusNode, sel.focusOffset);
  return r.collapsed;
}

// 判定当前输入是否应触发包裹（不触发则走原生输入）。
// editableEl：当前编辑容器；ch：输入字符；event：可选 beforeinput 事件（读 isComposing/data）
export function shouldWrapSelection(editableEl, ch, event = null) {
  if (!editableEl) return false;
  // IME 预编辑中不干预（与 onEditableInput 的组合期跳过约定一致）
  if (event && (event.isComposing || event.inputType === 'insertCompositionText')) return false;
  const pair = WRAP_PAIRS[event && event.data != null ? event.data : ch];
  if (!pair) return false;
  const sel = window.getSelection();
  if (!sel || !sel.rangeCount || sel.isCollapsed) return false;
  // 选区两端必须都在本编辑容器内（跨多个块宿主的选区不包裹）
  if (!editableEl.contains(sel.anchorNode) || !editableEl.contains(sel.focusNode)) return false;
  // pre/code 内不包裹（代码块编辑区）
  for (const node of [sel.anchorNode, sel.focusNode]) {
    const el = node.nodeType === Node.TEXT_NODE ? node.parentElement : node;
    if (el && el.closest('pre, code')) return false;
  }
  // 纯空格/Tab/换行的选区不包裹（包了也是空壳）
  if (/^[ \t\r\n]*$/.test(sel.getRangeAt(0).toString())) return false;
  return true;
}

// 反引号界符升级：内容含 ` 时用比内容最长反引号串长 1 的界符；
// 内容首/尾为反引号时界符内侧补空格（`` `x `` 而非 ``` `x ```）
function backtickDelimiters(content) {
  let longest = 0;
  for (const m of content.matchAll(/`+/g)) longest = Math.max(longest, m[0].length);
  const fence = '`'.repeat(longest + 1);
  const pad = content.startsWith('`') || content.endsWith('`') ? ' ' : '';
  return [fence + pad, pad + fence];
}

// 执行包裹：做了返回 true，否则 false（调用方据此决定 preventDefault / 走原生输入）。
// 成功后派发 input 事件（bubbles），让 Vue 的 @input="onEditableInput" 走既有后处理
export function wrapSelectionWithSymbol(editableEl, ch) {
  const pair = WRAP_PAIRS[ch];
  if (!pair || !editableEl) return false;
  const sel = window.getSelection();
  if (!sel || !sel.rangeCount || sel.isCollapsed) return false;
  if (!editableEl.contains(sel.anchorNode) || !editableEl.contains(sel.focusNode)) return false;
  const backward = selectionIsBackward(sel);
  // 纯空格/Tab/换行选区不包（与 shouldWrapSelection 一致，双保险）
  if (/^[ \t\r\n]*$/.test(sel.getRangeAt(0).toString())) return false;
  let [left, right] = pair;
  const range = sel.getRangeAt(0);
  if (ch === '`') [left, right] = backtickDelimiters(range.toString());
  // 左界符插在选区起点前（insertNode 插入后选区起点移到新节点之后，内容保持被选）
  const leftNode = document.createTextNode(left);
  range.insertNode(leftNode);
  // 右界符插在选区终点后（克隆塌陷到终点再插，避免原 Range 端点被插入规范化吞掉）
  const rightNode = document.createTextNode(right);
  const endRange = range.cloneRange();
  endRange.collapse(false);
  endRange.insertNode(rightNode);
  // 重选内部文字（两界符之间），保留原选区方向
  if (typeof sel.setBaseAndExtent === 'function') {
    if (backward) {
      sel.setBaseAndExtent(
        rightNode.parentNode,
        indexOfNode(rightNode),
        leftNode.parentNode,
        indexOfNode(leftNode) + 1,
      );
    } else {
      sel.setBaseAndExtent(
        leftNode.parentNode,
        indexOfNode(leftNode) + 1,
        rightNode.parentNode,
        indexOfNode(rightNode),
      );
    }
  } else {
    const inner = document.createRange();
    inner.setStartAfter(leftNode);
    inner.setEndBefore(rightNode);
    sel.removeAllRanges();
    sel.addRange(inner);
  }
  // 触发提交链路能感知的变化：走既有 input 后处理（快捷转换/菜单/高亮调度等）
  editableEl.dispatchEvent(new window.InputEvent('input', { bubbles: true, inputType: 'insertText', data: ch }));
  return true;
}

// 节点在父节点子节点列表中的下标（setBaseAndExtent 需要父+偏移形式的边界点）
function indexOfNode(node) {
  return Array.prototype.indexOf.call(node.parentNode.childNodes, node);
}
