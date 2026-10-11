import { ref, computed, watch, provide, onBeforeUnmount } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { getPref, setPref, prefsVersion } from '../utils/prefs.js';

// 改动审阅：对 content 与「比较基线」调 Rust diff（diff_saved_content_async），
// 把 hunk 映射到根块（VRow 条纹/删除徽标/旧内容块）与概览尺（rulerTicks）。
// 基线三选一（prefs 键 change_review_baseline）：
//   saved  与最近保存版本比较（默认，savedContent 快照）
//   git    与 Git HEAD 比较（git_head_content 命令；非仓库/未跟踪则禁用）
//   manual 与手动快照比较（会话级 Map<路径,{time,content}>，菜单「更新」重拍）
// 守卫：总开关（change_review）关闭、内容与基线一致、或两文本总长超阈值时清空且不发 IPC；
// 500ms 防抖 + seq 丢弃过期响应；块 id 换新经 watch(blocks) 用既有 hunks 立即重映射防闪。
const MAX_TOTAL_LEN = 2_000_000; // 两文本合计 UTF-16 码元数上限

// 手动快照：会话级，按文件路径键控（未保存新文档共用 '__untitled'）；
// Map 非响应式，snapshotVersion 驱动依赖方重读
const manualSnapshots = new Map();

export function useChangeReview(ctx) {
  // 结果版本号：每次更新 +1（VRow 据此重读标记；diff 结果本身不进响应式，避免大对象深追踪）
  const changeVersion = ref(0);
  // 根块 id -> { kind?, deleted?, runStart?, runEnd?, oldBlocks?: string[] }
  let blockMarks = new Map();
  let hunks = [];
  let totals = { added: 0, modified: 0, deleted: 0 };

  function clearResult() {
    if (!hunks.length && !blockMarks.size) return;
    hunks = [];
    blockMarks = new Map();
    totals = { added: 0, modified: 0, deleted: 0 };
    marksSig = '';
    changeVersion.value += 1;
  }

  // ---------- 基线状态机 ----------
  // 'saved' | 'git' | 'manual'（持久化在 prefs；总开关仍是 change_review）
  const baselineKind = ref(['saved', 'git', 'manual'].includes(getPref('change_review_baseline')) ? getPref('change_review_baseline') : 'saved');
  const gitContent = ref(null);
  const gitState = ref('unknown'); // 'unknown' | 'ok' | 'unavailable'
  const menuOpen = ref(false);

  const pathKey = () => ctx.currentFilePath?.value ?? '__untitled';
  const snapshotVersion = ref(0);
  const manualSnapshot = computed(() => {
    snapshotVersion.value;
    return manualSnapshots.get(pathKey()) ?? null;
  });

  async function loadGitBaseline() {
    gitState.value = 'unknown';
    const path = ctx.currentFilePath?.value;
    if (!path) {
      gitState.value = 'unavailable';
      gitContent.value = null;
      return;
    }
    const text = await invoke('git_head_content', { path }).catch(() => null);
    gitContent.value = text;
    gitState.value = text === null ? 'unavailable' : 'ok';
  }

  // 当前基线文本（无可用基线时 null → 不比较）
  function activeBaseline() {
    if (baselineKind.value === 'git') return gitContent.value;
    if (baselineKind.value === 'manual') return manualSnapshot.value?.content ?? null;
    return ctx.savedContent.value;
  }

  function setBaseline(kind) {
    baselineKind.value = kind;
    setPref('change_review_baseline', kind);
    setPref('change_review', true); // 选了基线即开启比较
    if (kind === 'git' && gitState.value !== 'ok') loadGitBaseline().then(scheduleDiff);
    else scheduleDiff();
  }

  function closeReview() {
    setPref('change_review', false);
    menuOpen.value = false;
  }

  function captureManualSnapshot() {
    manualSnapshots.delete(pathKey()); // 重插到末尾（LRU 序）
    manualSnapshots.set(pathKey(), { time: Date.now(), content: ctx.content.value });
    while (manualSnapshots.size > 50) manualSnapshots.delete(manualSnapshots.keys().next().value);
    snapshotVersion.value += 1; // Map 非响应式，驱动标签/可用性重算
    if (baselineKind.value === 'manual') scheduleDiff();
  }

  // 切换文档：git 基线作废重取；结果清空（新文档自己的 watch 链路会重算）
  watch(
    () => ctx.currentFilePath?.value,
    () => {
      gitContent.value = null;
      gitState.value = 'unknown';
      if (baselineKind.value === 'git') loadGitBaseline().then(scheduleDiff);
      else scheduleDiff();
    },
  );

  // 菜单标签
  const BASELINE_NAMES = { saved: '最近保存版本', git: 'Git HEAD', manual: '手动快照' };
  const baselineTitle = computed(() => BASELINE_NAMES[baselineKind.value]);
  const manualTimeLabel = computed(() => {
    const s = manualSnapshot.value;
    if (!s) return '';
    const d = new Date(s.time);
    return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
  });

  // 「显示修改前内容」开关（WYSIWYG：删除/修改处的旧文本块）
  const showOld = computed(() => {
    prefsVersion.value;
    return !!getPref('change_review_show_old');
  });
  provide('changeShowOld', showOld);

  // hunk → 根块映射（块 start/end 为 UTF-16 偏移，与 hunk 同坐标系）；
  // 相邻同类条纹块合并为「条带」（runStart/runEnd 供 CSS 圆角）；
  // 映射结果未变时不 bump changeVersion（避免无谓重渲染闪烁）
  function mapToBlocks() {
    const marks = new Map();
    const blocks = ctx.blocks.value;
    for (const h of hunks) {
      const oldText = h.oldLines?.length ? [h.oldLines.join('\n')] : [];
      if (h.kind === 'deleted') {
        // 删除点：归到首个 start >= 点的根块（无则最后一个块），累计删除行数与旧文本
        const target = blocks.find((b) => b.start >= h.start) ?? blocks[blocks.length - 1];
        if (!target) continue;
        const cur = marks.get(target.id);
        marks.set(target.id, {
          ...cur,
          deleted: (cur?.deleted ?? 0) + (h.deletedLines || 0),
          oldBlocks: [...(cur?.oldBlocks ?? []), ...oldText],
        });
        continue;
      }
      for (const b of blocks) {
        if (b.end <= h.start) continue;
        if (b.start >= h.end) break;
        // 一块多 hunk：modified 优先于 added；旧文本按序累计
        const cur = marks.get(b.id);
        if (h.kind === 'modified' || !cur?.kind) {
          marks.set(b.id, { ...cur, kind: h.kind, oldBlocks: [...(cur?.oldBlocks ?? []), ...oldText] });
        } else if (oldText.length) {
          cur.oldBlocks = [...(cur.oldBlocks ?? []), ...oldText];
        }
      }
    }
    // 连续同类条纹合并条带：中间块 runStart=runEnd=false，仅条带首尾为 true；
    // 删除徽标段与非标记块断开条带
    let prevStripe = null;
    for (const b of blocks) {
      const m = marks.get(b.id) || null;
      const stripe = m && (m.kind === 'added' || m.kind === 'modified') ? m : null;
      if (!stripe) {
        prevStripe = null;
        continue;
      }
      const continues = prevStripe !== null && prevStripe.kind === stripe.kind;
      stripe.runStart = !continues;
      stripe.runEnd = true;
      if (continues) prevStripe.runEnd = false;
      prevStripe = stripe;
    }
    const sig = [...marks]
      .map(([id, m]) => `${id}:${m.kind ?? ''}${m.deleted ?? ''}${m.runStart ? 1 : 0}${m.runEnd ? 1 : 0}${(m.oldBlocks ?? []).length}`)
      .join('|');
    if (sig === marksSig) return;
    marksSig = sig;
    blockMarks = marks;
    changeVersion.value += 1;
  }
  let marksSig = '';

  let debounceTimer = null;
  let seq = 0;

  async function runDiff() {
    const mySeq = ++seq;
    const newText = ctx.content.value;
    const oldText = activeBaseline();
    if (oldText == null) return;
    const result = await invoke('diff_saved_content_async', { old: oldText, new: newText }).catch(() => null);
    // 过期响应丢弃（期间又发生了输入/保存/开关切换）
    if (mySeq !== seq || !result) return;
    hunks = result.hunks || [];
    totals = { added: result.added, modified: result.modified, deleted: result.deleted };
    mapToBlocks(); // 内部仅在标记实际变化时 bump changeVersion
  }

  function scheduleDiff() {
    clearTimeout(debounceTimer);
    debounceTimer = null;
    const newText = ctx.content.value;
    const oldText = activeBaseline();
    // 守卫：总开关关、无基线、与基线一致、超大文档——清空且不发 IPC
    if (!getPref('change_review') || oldText == null || newText === oldText || newText.length + oldText.length > MAX_TOTAL_LEN) {
      seq += 1; // 使在途响应过期
      clearResult();
      return;
    }
    debounceTimer = setTimeout(runDiff, 500);
  }

  watch([ctx.content, ctx.savedContent], scheduleDiff);
  // 偏好变化（prefsVersion 驱动重读）：总开关/显示旧内容 toggle 都会落这里
  watch(prefsVersion, scheduleDiff);
  // 块树变化（编辑提交的增量重解析会换掉被编辑块的 id）：用既有 hunks 立即重映射，
  // 不等 500ms 防抖后的下一次 diff——消除「回车即闪」（条纹消失→diff 回来再出现）。
  // 展开数组只为让 Vue 追踪迭代依赖（splice 原位修改不会触发 ref 替换型 watch）
  watch(
    () => [...ctx.blocks.value],
    () => {
      if (hunks.length) mapToBlocks();
    },
    { flush: 'post' },
  );
  scheduleDiff();

  onBeforeUnmount(() => {
    clearTimeout(debounceTimer);
    seq += 1;
  });

  // VRow 查询块标记（changeVersion 触发后重读）
  function markOfBlock(id) {
    return blockMarks.get(id) ?? null;
  }
  provide('changeMarkOf', markOfBlock);
  provide('changeVersion', changeVersion);

  // 概览尺刻度：每 hunk 一条，topPct 相对全文起点（content 为空时 0）
  const rulerTicks = computed(() => {
    changeVersion.value;
    const len = ctx.content.value.length;
    return hunks.map((h) => ({
      topPct: len > 0 ? (h.start / len) * 100 : 0,
      kind: h.kind,
    }));
  });

  const counts = computed(() => {
    changeVersion.value;
    return totals;
  });

  const reviewEnabled = computed(() => {
    prefsVersion.value;
    changeVersion.value;
    return !!getPref('change_review') && activeBaseline() != null && ctx.content.value !== activeBaseline() && hunks.length > 0;
  });

  return {
    rulerTicks,
    counts,
    reviewEnabled,
    // 基线菜单
    menuOpen,
    baselineKind,
    baselineTitle,
    gitState,
    manualTimeLabel,
    manualSnapshotAvailable: computed(() => manualSnapshot.value !== null),
    showOld,
    setBaseline,
    closeReview,
    captureManualSnapshot,
    probeGit: loadGitBaseline, // 打开菜单时探测可用性
  };
}
