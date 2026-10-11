// 链接展开/折叠行为回归（jsdom）。
// 测的是真实代码而非重写：折叠提取走 src/utils/wysiwyg.js 的 domToBlockDtos；
// 展开/折叠函数按名称从 src/composables/useInlineSourceReveal.js 抽取函数文本执行。
// 运行：node scripts/test-link-fold.mjs
import { JSDOM } from 'jsdom';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const dom = new JSDOM('<!DOCTYPE html><html><body></body></html>');
globalThis.window = dom.window;
globalThis.document = dom.window.document;
globalThis.Node = dom.window.Node;
globalThis.NodeFilter = dom.window.NodeFilter;

// wysiwyg.js 以 @tauri-apps/api/core 的 invoke 打桩后加载
const wsrc = fs.readFileSync(path.join(root, 'src/utils/wysiwyg.js'), 'utf8');
const stubbed = path.join(root, 'node_modules/.cache', 'wysiwyg.stubbed.mjs');
fs.mkdirSync(path.dirname(stubbed), { recursive: true });
fs.writeFileSync(
  stubbed,
  wsrc.replace("import { invoke } from '@tauri-apps/api/core';", 'const invoke = async () => null;'),
);
const W = await import(pathToFileURL(stubbed).href);

// 从 composable 抽取函数文本（按函数名起、花括号配平止）
const esrc = fs.readFileSync(path.join(root, 'src/composables/useInlineSourceReveal.js'), 'utf8');
function extractFn(name) {
  const start = esrc.indexOf(`function ${name}(`);
  if (start < 0) throw new Error(`${name} not found`);
  let depth = 0;
  let i = esrc.indexOf('{', start);
  for (; i < esrc.length; i++) {
    if (esrc[i] === '{') depth++;
    else if (esrc[i] === '}') {
      depth--;
      if (depth === 0) return esrc.slice(start, i + 1);
    }
  }
  throw new Error(`${name} unclosed`);
}
// pi-lens-ignore: no-global-eval-js — 测试脚手架：按函数名抽取真实函数文本构建沙箱执行（非用户输入）
const F = new Function(
  'W',
  `const { LINK_CLASS, markSpanFactory, textBeforePoint } = W;
${extractFn('collapseLinkTemplateOutside')}
${extractFn('expandLinkTemplate')}
${extractFn('expandLinkAtCaret')}
${extractFn('expandLinkAtCaretInUrl')}
return { collapseLinkTemplateOutside, expandLinkAtCaret, expandLinkAtCaretInUrl };`,
)(W);

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
function setCaret(node, offset) {
  const sel = window.getSelection();
  const r = document.createRange();
  r.setStart(node, offset);
  r.collapse(true);
  sel.removeAllRanges();
  sel.addRange(r);
}
function linkFrags(el) {
  return W.domToBlockDtos(el).flatMap((b) => b.title?.fragments ?? []);
}
// 展开结构：可带地址区变形（extra 为 ( 之后、) 之前插入的额外 HTML）
function buildExpanded(urlExtra = '') {
  const el = document.createElement('div');
  // pi-lens-ignore: no-inner-html-js — jsdom 测试夹具，字面量 HTML
  el.innerHTML =
    `<p>前文<span class="md-link-mark">[</span><span class="md-link-text">sfasf</span>` +
    `<span class="md-link-mark">]</span><span class="md-link-mark">(</span>${urlExtra}` +
    `<span class="md-link-url">xxxx</span><span class="md-link-mark">)</span>后文</p>`;
  document.body.append(el);
  return el;
}

// —— 提交提取（domToBlockDtos 折叠分支）——
{
  // 装饰标记一律原子不可编辑（contenteditable=false）：光标不会停进标记文本、
  // 输入不会污染标记（打进标记内的文本会使 isMark 结构识别永久失效）
  const el = buildExpanded();
  const marks = [...el.querySelectorAll('.md-link-mark')];
  // buildExpanded 用 innerHTML 字符串（无属性），改用工厂重建验证真实创建路径
  const mark = W.markSpanFactory('md-link-mark');
  const m = mark('(');
  check('markSpanFactory 产生的标记为原子不可编辑', m.getAttribute('contenteditable') === 'false');
  el.remove();
}
{
  const frags = linkFrags(buildExpanded());
  const link = frags.find((f) => f.link);
  check(
    '纯净结构折叠为链接 fragment',
    link?.text === 'sfasf' &&
      link.link.destination === 'xxxx' &&
      frags.some((f) => f.text === '前文') &&
      frags.some((f) => f.text === '后文'),
    JSON.stringify(frags),
  );
}
{
  // 地址区变形：( 后多裸文本节点（点击/输入边界归并）
  const frags = linkFrags(buildExpanded('ab'));
  const link = frags.find((f) => f.link);
  check('地址区含裸文本节点仍折叠', link?.link?.destination === 'abxxxx', JSON.stringify(frags));
}
{
  // 地址区变形：地址 span 删空 + 兄弟文本（删除重打）
  const el = buildExpanded();
  el.querySelector('.md-link-url').textContent = '';
  el.querySelector('.md-link-url').after(document.createTextNode('xxxx'));
  const frags = linkFrags(el);
  const link = frags.find((f) => f.link);
  check('空地址 span + 兄弟文本仍折叠', link?.link?.destination === 'xxxx', JSON.stringify(frags));
  el.remove();
}
{
  // 结构真残（缺 ) 标记）：按字面量保留，不折叠不丢文本
  const el = document.createElement('div');
  // pi-lens-ignore: no-inner-html-js — jsdom 测试夹具，字面量 HTML
  el.innerHTML =
    '<p><span class="md-link-mark">[</span><span class="md-link-text">sfasf</span>' +
    '<span class="md-link-mark">]</span><span class="md-link-mark">(</span><span class="md-link-url">xxxx</span></p>';
  document.body.append(el);
  const text = linkFrags(el).map((f) => f.text).join('');
  check('缺 ) 结构按字面量保留', text === '[sfasf](xxxx', text);
  el.remove();
}

// —— 光标折叠（collapseLinkTemplateOutside）——
{
  const el = buildExpanded();
  const p = el.firstElementChild;
  const closeMark = p.querySelectorAll('.md-link-mark')[3];
  setCaret(p, [...p.childNodes].indexOf(closeMark) + 1); // ) 之后
  F.collapseLinkTemplateOutside(el);
  const a = p.querySelector('a');
  const sel = window.getSelection();
  check(
    '光标在 ) 之后→折叠为 <a>',
    !!a && a.textContent === 'sfasf' && a.getAttribute('href') === 'xxxx' && p.textContent === '前文sfasf后文',
    p.innerHTML,
  );
  check('折叠后光标钉在 <a> 之后', sel.anchorNode === p && p.childNodes[sel.anchorOffset - 1] === a);
  el.remove();
}
{
  const el = buildExpanded();
  setCaret(el.querySelector('.md-link-url').firstChild, 2); // 地址内
  F.collapseLinkTemplateOutside(el);
  check('光标在地址内→保持展开', !!el.querySelector('.md-link-url'));
  el.remove();
}
{
  const el = buildExpanded();
  const closeMark = el.querySelectorAll('.md-link-mark')[3];
  setCaret(closeMark.firstChild, 0); // ) 标记内（字符前，视觉在地址与 ) 之间）
  F.collapseLinkTemplateOutside(el);
  check('光标在 ) 标记内→保持展开', !!el.querySelector('.md-link-url'));
  el.remove();
}
{
  // Chromium 常把「移到 ) 后」的光标锚在 ) 标记文本末尾——视觉已在括号外，必须折叠
  const el = buildExpanded();
  const p = el.firstElementChild;
  const closeMark = p.querySelectorAll('.md-link-mark')[3];
  setCaret(closeMark.firstChild, 1); // ) 标记文本末尾（视觉在 ) 之后）
  F.collapseLinkTemplateOutside(el);
  const a = p.querySelector('a');
  const sel = window.getSelection();
  check(
    '光标在 ) 标记末尾→折叠为 <a>',
    !!a && a.textContent === 'sfasf' && a.getAttribute('href') === 'xxxx' && !p.querySelector('.md-link-url'),
    p.innerHTML,
  );
  check(') 末尾折叠后光标钉在 <a> 之后', sel.anchorNode === p && p.childNodes[sel.anchorOffset - 1] === a);
  el.remove();
}
{
  const el = buildExpanded();
  const p = el.firstElementChild;
  const startMark = p.querySelector('.md-link-mark');
  setCaret(startMark.firstChild, 0); // [ 标记文本开头（视觉在 [ 之前）
  F.collapseLinkTemplateOutside(el);
  const a = p.querySelector('a');
  const sel = window.getSelection();
  check('光标在 [ 标记开头→折叠', !!a && !p.querySelector('.md-link-url'), p.innerHTML);
  check('[ 开头折叠后光标钉在 <a> 之前', sel.anchorNode === p && p.childNodes[sel.anchorOffset] === a);
  el.remove();
}
{
  const el = buildExpanded();
  const p = el.firstElementChild;
  const closeMark = p.querySelectorAll('.md-link-mark')[3];
  setCaret(p, [...p.childNodes].indexOf(closeMark)); // 地址与 ) 之间
  F.collapseLinkTemplateOutside(el);
  check('光标在地址与 ) 之间→保持展开', !!el.querySelector('.md-link-url'));
  el.remove();
}
{
  const el = buildExpanded();
  const p = el.firstElementChild;
  const startMark = p.querySelector('.md-link-mark');
  setCaret(p, [...p.childNodes].indexOf(startMark)); // [ 之前
  F.collapseLinkTemplateOutside(el);
  check('光标在 [ 之前→折叠', !!p.querySelector('a') && !p.querySelector('.md-link-url'));
  el.remove();
}
{
  // 地址区变形 + 光标在 ) 后：仍折叠且地址完整
  const el = buildExpanded('ab');
  const p = el.firstElementChild;
  const marks = p.querySelectorAll('.md-link-mark');
  const closeMark = marks[marks.length - 1];
  setCaret(p, [...p.childNodes].indexOf(closeMark) + 1);
  F.collapseLinkTemplateOutside(el);
  const a = p.querySelector('a');
  check('地址变形 + 光标在 ) 后→折叠且地址完整', a?.getAttribute('href') === 'abxxxx', p.innerHTML);
  el.remove();
}

// —— 斜杠模板段（p[data-raw]）：光标移出 ) 折叠并摘除 data-raw ——
// 与 applySlashCommand('link') 同构：p[data-raw] + \uFEFF 哨兵文字/地址
function buildRawTemplate() {
  const el = document.createElement('div');
  // pi-lens-ignore: no-inner-html-js — jsdom 测试夹具，字面量 HTML
  el.innerHTML =
    `<p data-raw><span class="md-link-mark">[</span><span class="md-link-text">文字</span>` +
    `<span class="md-link-mark">]</span><span class="md-link-mark">(</span>` +
    `<span class="md-link-url">https://a.b</span><span class="md-link-mark">)</span></p>`;
  document.body.append(el);
  return el;
}
{
  const el = buildRawTemplate();
  const p = el.firstElementChild;
  const closeMark = p.querySelectorAll('.md-link-mark')[3];
  setCaret(p, [...p.childNodes].indexOf(closeMark) + 1); // ) 之后（父边界）
  F.collapseLinkTemplateOutside(el);
  const a = p.querySelector('a');
  check(
    '斜杠模板段：光标在 ) 后→折叠为 <a> 且摘除 data-raw',
    !!a && a.textContent === '文字' && a.getAttribute('href') === 'https://a.b' && !p.hasAttribute('data-raw'),
    p.outerHTML,
  );
  const link = linkFrags(el).find((f) => f.link);
  check('模板段折叠后提交提取为链接 fragment', link?.text === '文字' && link.link.destination === 'https://a.b');
  el.remove();
}
{
  const el = buildRawTemplate();
  const p = el.firstElementChild;
  const closeMark = p.querySelectorAll('.md-link-mark')[3];
  setCaret(closeMark.firstChild, 1); // ) 标记文本末尾（Chromium 归并形态）
  F.collapseLinkTemplateOutside(el);
  check(
    '斜杠模板段：光标在 ) 标记末尾→折叠且摘除 data-raw',
    !!p.querySelector('a') && !p.hasAttribute('data-raw'),
    p.outerHTML,
  );
  el.remove();
}
{
  // Chromium 也可能把「) 之后」锚在标记元素自身（元素锚点 (mark, 子节点数)）
  const el = buildRawTemplate();
  const p = el.firstElementChild;
  const closeMark = p.querySelectorAll('.md-link-mark')[3];
  setCaret(closeMark, closeMark.childNodes.length);
  F.collapseLinkTemplateOutside(el);
  check(
    '斜杠模板段：) 标记元素锚点末尾→折叠且摘除 data-raw',
    !!p.querySelector('a') && !p.hasAttribute('data-raw'),
    p.outerHTML,
  );
  el.remove();
}
{
  // 宿主（contenteditable）边界：模板段之后（spliceSlashFrags 放回后文的形态）
  const el = buildRawTemplate();
  const p = el.firstElementChild;
  setCaret(el, 1);
  F.collapseLinkTemplateOutside(el);
  check(
    '斜杠模板段：光标在宿主边界模板段之后→折叠且摘除 data-raw',
    !!p.querySelector('a') && !p.hasAttribute('data-raw'),
    p.outerHTML,
  );
  el.remove();
}
{
  const el = buildRawTemplate();
  const p = el.firstElementChild;
  setCaret(p.querySelector('.md-link-url').firstChild, 2); // 地址内
  F.collapseLinkTemplateOutside(el);
  check(
    '斜杠模板段：光标在地址内→保持展开且 data-raw 保留',
    !!p.querySelector('.md-link-url') && p.hasAttribute('data-raw'),
  );
  el.remove();
}
{
  // 图片模板段（! 标记紧邻 [ 前）：保持原文编辑形态，不参与折叠
  const el = document.createElement('div');
  // pi-lens-ignore: no-inner-html-js — jsdom 测试夹具，字面量 HTML
  el.innerHTML =
    `<p data-raw><span class="md-link-mark">!</span><span class="md-link-mark">[</span>` +
    `<span class="md-link-text">alt</span><span class="md-link-mark">]</span>` +
    `<span class="md-link-mark">(</span><span class="md-link-url">./a.png</span><span class="md-link-mark">)</span></p>`;
  document.body.append(el);
  const p = el.firstElementChild;
  const marks = p.querySelectorAll('.md-link-mark');
  setCaret(p, [...p.childNodes].indexOf(marks[marks.length - 1]) + 1); // ) 之后
  F.collapseLinkTemplateOutside(el);
  check(
    '图片模板段：光标在 ) 后仍保持展开（不参与折叠）',
    !p.querySelector('a') && !!p.querySelector('.md-link-url') && p.hasAttribute('data-raw'),
    p.outerHTML,
  );
  el.remove();
}

// —— 展开（expandLinkAtCaret）——
{
  const el = document.createElement('div');
  el.innerHTML = '<p>前文<a href="https://baidu.com" class="underline">你好</a>后文</p>';
  const a = el.querySelector('a');
  setCaret(a.firstChild, 1);
  F.expandLinkAtCaret(a);
  const url = el.querySelector('.md-link-url');
  check(
    '展开为标记结构（地址带 md-link-url）',
    !!url && url.textContent === 'https://baidu.com' && el.querySelector('.md-link-text')?.textContent === '你好',
    el.innerHTML,
  );
  el.remove();
}
{
  const el = document.createElement('div');
  el.innerHTML = '<p><a href="xxxx" class="underline">sfasf</a></p>';
  document.body.append(el);
  const a = el.querySelector('a');
  setCaret(el.firstElementChild, 1); // 链接之后边界位（渲染链接右缘按 → 的触发形态）
  F.expandLinkAtCaretInUrl(a);
  const sel = window.getSelection();
  const urlText = el.querySelector('.md-link-url')?.firstChild;
  check('caretInUrl 展开光标落地址末尾', sel.anchorNode === urlText && sel.anchorOffset === 4);
  el.remove();
}

console.log(`\n${pass} pass, ${fail} fail`);
process.exit(fail ? 1 : 0);
