// renumberOrderedMarkers 算法的独立模拟验证（与 Editor.vue 中实现逐字一致）
function makeEnv(src, listStarts) {
  // 按行切块：每行一个块；numberedListItem 的 listStart 取自 listStarts（null 表示段落）
  const lines = src.split('\n');
  const blocks = [];
  let off = 0;
  lines.forEach((line, i) => {
    const ls = listStarts[i];
    blocks.push({
      type: ls == null ? 'paragraph' : 'numberedListItem',
      listStart: ls ?? undefined,
      title: { fragments: [{ text: line.replace(/^\s*\d+[.)]\s*/, '') }] },
      image: null,
      start: off,
      end: off + line.length,
    });
    off += line.length + 1;
  });
  return { content: src, blocks };
}

function plainText(tree) {
  return (tree?.fragments || []).map((f) => f.text).join('');
}

function renumberOrderedMarkers(env, fromIndex) {
  const bs = env.blocks;
  if (!bs.length) return;
  const isNum = (b) => b?.type === 'numberedListItem';
  const isGap = (b) => b?.type === 'paragraph' && !plainText(b.title).trim() && !b.image;
  let head = -1;
  for (let k = Math.min(fromIndex, bs.length - 1); k >= 0; k--) {
    if (isNum(bs[k])) head = k;
    else if (!isGap(bs[k])) break;
  }
  if (head < 0) return;
  const sites = [];
  let n = 0;
  let gap = false;
  for (let k = head; k < bs.length; k++) {
    const b = bs[k];
    if (isNum(b)) {
      const expect = n === 0 ? (b.listStart ?? 1) : gap && b.listStart === 1 ? 1 : n + 1;
      if (k >= fromIndex && (b.listStart ?? 1) !== expect && b.start != null) {
        const m = /^[ \t]{0,3}(\d+)(?=[.)])/.exec(env.content.slice(b.start, b.end));
        if (!m) return;
        const newText = String(expect);
        sites.push({ index: k, pos: b.start + (m[0].length - m[1].length), oldLen: m[1].length, delta: newText.length - m[1].length, expect, newText });
      }
      n = expect;
      gap = false;
    } else if (isGap(b)) gap = true;
    else break;
  }
  if (!sites.length) return;
  for (let s = sites.length - 1; s >= 0; s--) {
    const { pos, oldLen, newText } = sites[s];
    env.content = env.content.slice(0, pos) + newText + env.content.slice(pos + oldLen);
  }
  let si = 0, acc = 0;
  for (const b of env.blocks) {
    if (b.start == null) continue;
    while (si < sites.length && sites[si].pos < b.start) { acc += sites[si].delta; si++; }
    b.start += acc;
    b.end += acc;
  }
  for (const s of sites) {
    const b = env.blocks[s.index];
    b.end += s.delta;
    b.listStart = s.expect;
  }
}

let pass = 0, fail = 0;
function check(name, src, listStarts, fromIndex, wantContent, wantListStarts) {
  const env = makeEnv(src, listStarts);
  renumberOrderedMarkers(env, fromIndex);
  const okContent = env.content === wantContent;
  const gotLs = env.blocks.map((b) => (b.type === 'numberedListItem' ? b.listStart : null));
  const okLs = JSON.stringify(gotLs) === JSON.stringify(wantListStarts);
  // 偏移一致性：每块区间切片应以标记/原文开头
  const okOffsets = env.blocks.every((b) => env.content.slice(b.start, b.end).length === b.end - b.start);
  if (okContent && okLs && okOffsets) { pass++; console.log(`PASS ${name}`); }
  else {
    fail++;
    console.log(`FAIL ${name}\n  content: ${JSON.stringify(env.content)} want ${JSON.stringify(wantContent)}\n  ls: ${JSON.stringify(gotLs)} want ${JSON.stringify(wantListStarts)}\n  offsetsOk=${okOffsets}`);
  }
}

// 1. 中间插入：1.a [插入 3.x] 2.b 3.c → 全部顺移
check('中间插入',
  '1. a\n3. x\n2. b\n3. c', [1, 3, 2, 3], 1,
  '1. a\n2. x\n3. b\n4. c', [1, 2, 3, 4]);

// 2. 中间删除：1.a 3.c 4.d → 2,3
check('中间删除',
  '1. a\n3. c\n4. d', [1, 3, 4], 1,
  '1. a\n2. c\n3. d', [1, 2, 3]);

// 3. 多位数与缩进、) 标记
check('多位数/缩进/括号',
  '  9) a\n10) x\n  2) b', [9, 10, 2], 1,
  '  9) a\n10) x\n  11) b', [9, 10, 11]);

// 4. 空行后遇源标记 1 重启：不动
check('空行重启不改写',
  '1. a\n\n1. b', [1, null, 1], 2,
  '1. a\n\n1. b', [1, null, 1]);

// 5. 跨空行连续编号（gap 不重置，标记非 1 继续递增）
check('空行连续编号',
  '1. a\n\n3. b\n4. c', [1, null, 3, 4], 2,
  '1. a\n\n2. b\n3. c', [1, null, 2, 3]);

// 6. 组首从 5 开始：插入后顺移保持 5 起点
check('组首保留',
  '5. a\n7. x\n6. b', [5, 7, 6], 1,
  '5. a\n6. x\n7. b', [5, 6, 7]);

// 7. 列表旁无列表：无操作
check('无列表无操作',
  '# t\npara', [null, null], 1,
  '# t\npara', [null, null]);

// 8. 组尾遇内容块停止（前组顺移到 2；内容块之后的 9. 是独立组首，保留）
check('遇内容块停止',
  '1. a\n3. b\ntext\n9. c', [1, 3, null, 9], 1,
  '1. a\n2. b\ntext\n9. c', [1, 2, null, 9]);

// 9. 删除末项：后续无项，无改写
check('删除末项',
  '1. a\n2. b\ntext', [1, 2, null], 2,
  '1. a\n2. b\ntext', [1, 2, null]);

console.log(`\n${pass} pass, ${fail} fail`);
process.exit(fail ? 1 : 0);
