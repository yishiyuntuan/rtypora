// 选区符号包裹回归（jsdom）。
// 测真实代码：src/utils/selectionWrap.js 无 invoke 依赖，直接加载。
// 运行：node scripts/test-selection-wrap.mjs
import { JSDOM } from 'jsdom';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const dom = new JSDOM('<!DOCTYPE html><html><body></body></html>');
globalThis.window = dom.window;
globalThis.document = dom.window.document;
globalThis.Node = dom.window.Node;
globalThis.NodeFilter = dom.window.NodeFilter;

const M = await import(pathToFileURL(path.join(root, 'src/utils/selectionWrap.js')).href);
const { WRAP_PAIRS, shouldWrapSelection, wrapSelectionWithSymbol } = M;

let pass = 0;
let fail = 0;
function check(name, cond, extra = '') {
  if (cond) {
    pass++;
    console.log(`PASS ${name}`);
  } else {
    fail++;
    console.log(`FAIL ${name}${extra ? ' — ' + extra : ''}`);
  }
}

// 构建一个 contenteditable 容器（每次重建，互不污染）
function buildEditable(html) {
  document.body.innerHTML = '';
  const el = document.createElement('div');
  el.setAttribute('contenteditable', 'true');
  el.innerHTML = html;
  document.body.append(el);
  return el;
}

// 选中容器内首个包含 substr 的文本节点中的该子串
function selectSubstring(el, substr) {
  const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
  let n;
  while ((n = walker.nextNode())) {
    const i = n.data.indexOf(substr);
    if (i >= 0) {
      window.getSelection().setBaseAndExtent(n, i, n, i + substr.length);
      return true;
    }
  }
  return false;
}

// 反向选中（focus 在 anchor 之前）
function selectSubstringBackward(el, substr) {
  const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
  let n;
  while ((n = walker.nextNode())) {
    const i = n.data.indexOf(substr);
    if (i >= 0) {
      window.getSelection().setBaseAndExtent(n, i + substr.length, n, i);
      return true;
    }
  }
  return false;
}

// 当前选区是否反向（与模块内同款判定：focus 早于 anchor 时规范化 Range 塌陷）
function currentSelectionIsBackward() {
  const sel = window.getSelection();
  if (sel.isCollapsed) return false;
  const r = document.createRange();
  r.setStart(sel.anchorNode, sel.anchorOffset);
  r.setEnd(sel.focusNode, sel.focusOffset);
  return r.collapsed;
}

// —— 映射表完整性 ——
{
  const need = {
    '(': ['(', ')'], '[': ['[', ']'], '{': ['{', '}'], '<': ['<', '>'],
    '"': ['"', '"'], "'": ["'", "'"], '`': ['`', '`'],
    '*': ['*', '*'], _: ['_', '_'], '~': ['~', '~'], $: ['$', '$'],
    '（': ['（', '）'], '【': ['【', '】'], '“': ['“', '”'], '‘': ['‘', '’'],
    '《': ['《', '》'], '「': ['「', '」'], '『': ['『', '』'],
  };
  let ok = true;
  for (const [k, v] of Object.entries(need)) {
    if (!WRAP_PAIRS[k] || WRAP_PAIRS[k][0] !== v[0] || WRAP_PAIRS[k][1] !== v[1]) ok = false;
  }
  check('WRAP_PAIRS 覆盖全部符号对（含中文直角引号）', ok);
}

// —— 基本包裹：* ——
{
  const el = buildEditable('<p>hello world</p>');
  selectSubstring(el, 'world');
  let inputs = 0;
  el.addEventListener('input', () => inputs++);
  check('包裹前提判定 shouldWrapSelection(*)', shouldWrapSelection(el, '*'));
  const done = wrapSelectionWithSymbol(el, '*');
  check('基本包裹 *：返回 true', done);
  check('基本包裹 *：文本为 hello *world*', el.textContent === 'hello *world*', el.textContent);
  check('基本包裹 *：派发了一次 input 事件', inputs === 1, `inputs=${inputs}`);
  check('基本包裹 *：内部文字保持选中', window.getSelection().toString() === 'world', window.getSelection().toString());
}

// —— 基本包裹：( 与 【 ——
{
  const el = buildEditable('<p>hello world</p>');
  selectSubstring(el, 'world');
  wrapSelectionWithSymbol(el, '(');
  check('基本包裹 (：文本为 hello (world)', el.textContent === 'hello (world)', el.textContent);
  check('基本包裹 (：内部保持选中', window.getSelection().toString() === 'world');
}
{
  const el = buildEditable('<p>你好世界</p>');
  selectSubstring(el, '世界');
  wrapSelectionWithSymbol(el, '【');
  check('基本包裹 【：文本为 你好【世界】', el.textContent === '你好【世界】', el.textContent);
  check('基本包裹 【：内部保持选中', window.getSelection().toString() === '世界');
}

// —— 叠层二次包裹：* 后再 * 得 **文字**，选区仍在内部文字上 ——
{
  const el = buildEditable('<p>hello world</p>');
  selectSubstring(el, 'world');
  wrapSelectionWithSymbol(el, '*');
  check('叠层：一次包裹后选区不变', window.getSelection().toString() === 'world');
  wrapSelectionWithSymbol(el, '*');
  check('叠层：二次包裹得 **world**', el.textContent === 'hello **world**', el.textContent);
  check('叠层：二次包裹后内部文字仍选中', window.getSelection().toString() === 'world', window.getSelection().toString());
}

// —— 反向选区方向保留 ——
{
  const el = buildEditable('<p>hello world</p>');
  selectSubstringBackward(el, 'world');
  check('反向选区建立成功', currentSelectionIsBackward());
  wrapSelectionWithSymbol(el, '*');
  check('反向包裹：文本为 *world*', el.textContent === 'hello *world*', el.textContent);
  check('反向包裹：内部文字保持选中', window.getSelection().toString() === 'world');
  check('反向包裹：选区方向仍反向', currentSelectionIsBackward());
}

// —— 跨节点选区（不用 surroundContents 的场景）——
{
  const el = buildEditable('<p>he<b>llo wo</b>rld</p>');
  const sel = window.getSelection();
  const first = el.querySelector('p').firstChild; // "he"
  const last = el.querySelector('p').lastChild; // "rld"
  sel.setBaseAndExtent(first, 1, last, 2);
  check('跨节点包裹前提判定', shouldWrapSelection(el, '*'));
  wrapSelectionWithSymbol(el, '*');
  check('跨节点包裹：文本正确', el.textContent === 'h*ello worl*d', el.textContent);
  check('跨节点包裹：内部保持选中', sel.toString() === 'ello worl', sel.toString());
}

// —— 反引号升级：内容含 ` 用更长界符 ——
{
  const el = buildEditable('<p>a`b</p>');
  selectSubstring(el, 'a`b');
  wrapSelectionWithSymbol(el, '`');
  check('反引号升级：文本为 ``a`b``', el.textContent === '``a`b``', el.textContent);
  check('反引号升级：内部保持选中', window.getSelection().toString() === 'a`b');
}

// —— 内容首/尾为反引号：界符内侧补空格 ——
{
  const el = buildEditable('<p>`x</p>');
  selectSubstring(el, '`x');
  wrapSelectionWithSymbol(el, '`');
  check('首反引号补空格：文本为 `` `x ``', el.textContent === '`` `x ``', el.textContent);
}
{
  const el = buildEditable('<p>x`</p>');
  selectSubstring(el, 'x`');
  wrapSelectionWithSymbol(el, '`');
  check('尾反引号补空格：文本为 `` x` ``', el.textContent === '`` x` ``', el.textContent);
}

// —— 例外：纯空格/Tab/换行选区不包裹 ——
{
  const el = buildEditable('<p>a   b</p>');
  selectSubstring(el, '   ');
  check('纯空格选区：shouldWrapSelection 为 false', !shouldWrapSelection(el, '*'));
  check('纯空格选区：不执行包裹', !wrapSelectionWithSymbol(el, '*') && el.textContent === 'a   b', el.textContent);
}
{
  const el = buildEditable('<p>a\tb</p>');
  selectSubstring(el, '\t');
  check('纯 Tab 选区：不包裹', !shouldWrapSelection(el, '*'));
}
{
  const el = buildEditable('<p>a\nb</p>');
  selectSubstring(el, '\n');
  check('纯换行选区：不包裹', !shouldWrapSelection(el, '*'));
}

// —— 例外：塌陷光标不包裹 ——
{
  const el = buildEditable('<p>hello</p>');
  const t = el.querySelector('p').firstChild;
  window.getSelection().setBaseAndExtent(t, 2, t, 2);
  check('塌陷光标：shouldWrapSelection 为 false', !shouldWrapSelection(el, '*'));
  check('塌陷光标：wrapSelectionWithSymbol 返回 false', !wrapSelectionWithSymbol(el, '*'));
  check('塌陷光标：文本未变', el.textContent === 'hello');
}

// —— 例外：pre/code 内不包裹 ——
{
  const el = buildEditable('<pre data-language="js"><code>let x = 1;</code></pre>');
  selectSubstring(el, 'x = 1');
  check('pre/code 内：shouldWrapSelection 为 false', !shouldWrapSelection(el, '*'));
}

// —— 例外：跨多个块宿主（focus 不在同一 contenteditable 容器）不包裹 ——
{
  document.body.innerHTML = '';
  const a = document.createElement('div');
  a.setAttribute('contenteditable', 'true');
  a.innerHTML = '<p>first block</p>';
  const b = document.createElement('div');
  b.setAttribute('contenteditable', 'true');
  b.innerHTML = '<p>second block</p>';
  document.body.append(a, b);
  const t1 = a.querySelector('p').firstChild;
  const t2 = b.querySelector('p').firstChild;
  window.getSelection().setBaseAndExtent(t1, 0, t2, 6);
  check('跨容器选区：对容器 a 不包裹', !shouldWrapSelection(a, '*'));
  check('跨容器选区：对容器 b 不包裹', !shouldWrapSelection(b, '*'));
}

// —— 例外：IME 预编辑中不包裹 ——
{
  const el = buildEditable('<p>hello world</p>');
  selectSubstring(el, 'world');
  check('isComposing 中不包裹', !shouldWrapSelection(el, '*', { isComposing: true, data: '*' }));
  check('insertCompositionText 不包裹', !shouldWrapSelection(el, '*', { isComposing: false, inputType: 'insertCompositionText', data: '*' }));
}

// —— 中文对『』 ——
{
  const el = buildEditable('<p>引用文字</p>');
  selectSubstring(el, '引用');
  check('中文对『：前提判定通过', shouldWrapSelection(el, '『'));
  wrapSelectionWithSymbol(el, '『');
  check('中文对『』：文本为 『引用』文字', el.textContent === '『引用』文字', el.textContent);
  check('中文对『』：内部保持选中', window.getSelection().toString() === '引用');
}

console.log(`\n${pass} pass, ${fail} fail`);
process.exit(fail ? 1 : 0);
