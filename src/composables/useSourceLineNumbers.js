import { ref, computed, watch, nextTick, onBeforeUnmount } from 'vue';
import { getPref, prefsVersion } from '../utils/prefs.js';

// 源码模式行号栏 + Ctrl+G 跳行（F-G）。
//
// 行号 gutter：computed 行数数组（content.split('\n')，按硬换行计行——与跳行行号同坐标系，
// textarea 软折行不占号）；gutter div 与 textarea 同步滚动（textarea scroll → gutter.scrollTop，
// 元素级 passive 监听自挂自拆）；样式等宽右对齐淡色（gutter 元素带 t-root font-mono 类，
// 与 textarea 同字号/行高/字体，逐行对齐）。
// Ctrl+G 跳行：routeSourceKeys 由 Editor 的 onSourceKeydown 首行调用（已处理返回 true 早退）。
// 打开浮动输入框（jumpOpen + jumpText），Enter 跳行：第 N 行 UTF-16 起点 = 前 N-1 行切分累计
// （每行 +1 换行符），textarea.setSelectionRange(off, off)，并把该行滚到视口上 1/3 处
// （scrollTop = 行高×(N-1) − clientHeight/3；行高 getComputedStyle 实测，'normal' 按字号 1.5 倍兜底）；
// Esc 关闭（输入框内由 onJumpKeydown 处理，焦点在 textarea 时由 routeSourceKeys 处理）。
// 偏好 source_line_numbers（默认 true）关闭时不渲染 gutter（Ctrl+G 跳行保留）。
//
// ctx：Editor.vue 共享上下文（sourceRoot 为 textarea 元素 ref、content 为 ref、
// isSourceMode() 运行时查询模式）。
export function useSourceLineNumbers(ctx) {
  // gutter 元素（模板 ref 绑定；scrollTop 同步目标）
  const gutterRoot = ref(null);
  // 跳行输入框元素（打开时聚焦）
  const jumpInput = ref(null);
  // 跳行浮动框显隐与输入值
  const jumpOpen = ref(false);
  const jumpText = ref('');

  // 行号开关（prefsVersion 驱动响应；关闭时模板不渲染 gutter，跳行不受影响）
  const lineNumbersOn = computed(() => {
    prefsVersion.value;
    return !!getPref('source_line_numbers');
  });

  // 行数数组（模板仅按序号渲染 1..N）
  const gutterLines = computed(() => ctx.content.value.split('\n'));

  // ---------- gutter 滚动同步 ----------
  function onSourceScroll() {
    const ta = ctx.sourceRoot?.value;
    const g = gutterRoot.value;
    if (ta && g) g.scrollTop = ta.scrollTop;
  }
  let attachedSource = null;
  watch(
    () => ctx.sourceRoot?.value,
    (el) => {
      if (attachedSource) {
        attachedSource.removeEventListener('scroll', onSourceScroll);
        attachedSource = null;
      }
      if (el) {
        el.addEventListener('scroll', onSourceScroll, { passive: true });
        attachedSource = el;
        nextTick(onSourceScroll);
      }
    },
  );
  // gutter 重挂载（开关切换/模式切换）后回填 scrollTop，保持与 textarea 对齐
  watch(gutterRoot, (el) => {
    if (el) nextTick(onSourceScroll);
  });

  // ---------- Ctrl+G 跳行 ----------
  // 源码模式 keydown 路由（Editor 的 onSourceKeydown 首行调用）；已处理返回 true
  function routeSourceKeys(e) {
    if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && (e.key === 'g' || e.key === 'G')) {
      e.preventDefault();
      openJump();
      return true;
    }
    if (e.key === 'Escape' && jumpOpen.value) {
      closeJump();
      return true;
    }
    return false;
  }

  function openJump() {
    jumpText.value = '';
    jumpOpen.value = true;
    nextTick(() => {
      jumpInput.value?.focus();
      jumpInput.value?.select();
    });
  }
  function closeJump() {
    jumpOpen.value = false;
    ctx.sourceRoot?.value?.focus({ preventScroll: true });
  }

  // 跳行输入框键盘：Enter 跳行并关闭，Esc 关闭
  function onJumpKeydown(e) {
    if (e.key === 'Enter') {
      e.preventDefault();
      doJump();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      closeJump();
    }
  }

  // 实测行高（line-height: normal 时 parseFloat 得 NaN，按字号 1.5 倍兜底）
  function lineHeightOf(ta) {
    const style = window.getComputedStyle(ta);
    const lh = parseFloat(style.lineHeight);
    if (Number.isFinite(lh)) return lh;
    const fs = parseFloat(style.fontSize);
    return Number.isFinite(fs) ? fs * 1.5 : 20;
  }

  function doJump() {
    const ta = ctx.sourceRoot?.value;
    if (!ta) return;
    const lines = gutterLines.value;
    const n = parseInt(jumpText.value, 10);
    if (!Number.isFinite(n)) return; // 非数字：保持输入框，等待修正
    const target = Math.min(Math.max(n, 1), lines.length); // 钳到 [1, 行数]
    // 第 N 行的 UTF-16 起点：前 N-1 行长度 + 每行 1 个换行符累计
    let off = 0;
    for (let i = 0; i < target - 1; i++) off += lines[i].length + 1;
    // 先滚到「视口上 1/3 处」再放选区，避免原生滚动抢占；setTimeout 复核兜底
    const top = Math.max(0, lineHeightOf(ta) * (target - 1) - ta.clientHeight / 3);
    ta.scrollTop = top;
    ta.focus({ preventScroll: true });
    ta.setSelectionRange(off, off);
    setTimeout(() => {
      if (ta.isConnected) ta.scrollTop = top;
    }, 0);
    jumpOpen.value = false;
  }

  // 切出源码模式时关掉跳行框（gutter 随 v-if 卸载，无需处理）
  watch(
    () => ctx.isSourceMode(),
    (v) => {
      if (!v) jumpOpen.value = false;
    },
  );

  onBeforeUnmount(() => {
    if (attachedSource) attachedSource.removeEventListener('scroll', onSourceScroll);
  });

  return {
    gutterRoot,
    gutterLines,
    lineNumbersOn,
    jumpInput,
    jumpOpen,
    jumpText,
    routeSourceKeys,
    onJumpKeydown,
  };
}
