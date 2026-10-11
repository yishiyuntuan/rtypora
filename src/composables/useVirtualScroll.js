import { ref, computed, watch, provide, onBeforeUnmount } from 'vue';
import { themeVersion } from '../themes/index.js';

// 大文档渐进挂载：首屏只创建前 INITIAL_ROW_QUOTA 个行组件，
// 其余行以「估计高度占位条」代替，空闲帧按步长扩配额直至全覆盖。
// 首屏 DOM/组件规模恒定（与文档大小无关），是超大文件秒开的关键
export const INITIAL_ROW_QUOTA = 160;
const ROW_QUOTA_STEP = 500;

// 长文档优化：虚拟滚动 + 渐进挂载 + 重活懒执行
//
// ctx：Editor.vue 共享上下文（content/allBlocks 为 ref；
// scheduleRebuildHeadingPositions 为标题位置重建防抖调度，运行时解析）。
export function useVirtualScroll(ctx) {
  // 虚拟滚动行观察器：行进入视口余量时通知 VRow 挂载/卸载内容。
  // 余量按窗口高度的 150%（rootMargin 百分比随窗口缩放动态生效），
  // 加大提前量，避免滚动加载时内容闪烁
  const rowCallbacks = new WeakMap();
  let rowObserver = null;
  function getRowObserver() {
    if (!rowObserver) {
      rowObserver = new IntersectionObserver(
        (entries) => {
          for (const entry of entries) {
            rowCallbacks.get(entry.target)?.(entry.isIntersecting);
          }
        },
        { root: null, rootMargin: '450% 0px' },
      );
    }
    return rowObserver;
  }
  provide('observeRow', (el, cb) => {
    rowCallbacks.set(el, cb);
    getRowObserver().observe(el);
  });
  provide('unobserveRow', (el) => {
    rowCallbacks.delete(el);
    // el 可能为 null（键控重建时 ref 已卸载），unobserve 非 Element 会抛 TypeError
    if (el) rowObserver?.unobserve(el);
  });

  // 虚拟滚动：全部行都渲染 VRow 占位（总高正确），内容按视口余量挂载（见 VRow.vue）；
  // 包装对象按块身份记忆化（WeakMap）：提交编辑时未变化的块不再每次全量分配
  // {block, estimate} 包装数组（主题切换时随估计缓存一并失效——themeVersion
  // 是显式依赖：失效后 computed 才会重算并读入清空的缓存）
  let rowWrapperCache = new WeakMap();
  const renderedBlocks = computed(() => {
    themeVersion.value;
    return ctx.allBlocks.value.map((block) => {
      let w = rowWrapperCache.get(block);
      if (!w) {
        w = { block, estimate: estimateBlockHeight(block) };
        rowWrapperCache.set(block, w);
      }
      return w;
    });
  });

  // 虚拟行高估计：按源码行数 × 主题实际行高（首次渲染后由实测高度取代）。
  // 按块 id 记忆化：提交编辑时未变化的块 id 稳定，估算不必随 renderedBlocks
  // 全量重算（超长文档每次提交的 O(全文字符) 扫描是卡顿主因之一）
  const estimateCache = new Map();
  function estimateBlockHeight(block) {
    const hit = estimateCache.get(block.id);
    if (hit != null) return hit;
    let est;
    if (block.start == null || block.end == null) {
      est = 40;
    } else {
      // 只数换行符（不 slice/split 分配子串与数组）
      let lines = 1;
      for (let i = block.start; i < block.end; i++) {
        if (ctx.content.value.charCodeAt(i) === 10) lines++;
      }
      est = Math.max(1, lines) * lineHeightPx() + 16;
    }
    if (estimateCache.size > 50000) estimateCache.clear(); // 防跨文档/长期编辑膨胀
    estimateCache.set(block.id, est);
    return est;
  }
  let cachedLineHeightPx = 0;
  function lineHeightPx() {
    if (cachedLineHeightPx) return cachedLineHeightPx;
    const style = getComputedStyle(document.documentElement);
    const size = parseFloat(style.getPropertyValue('--t-text-size')) || 17;
    const ratio = parseFloat(style.getPropertyValue('--t-text-line-height')) || 1.6;
    cachedLineHeightPx = Math.round(size * ratio);
    return cachedLineHeightPx;
  }
  // 主题/排版变化使缓存行高失效（估计缓存与行包装缓存一并清空）
  watch(themeVersion, () => {
    cachedLineHeightPx = 0;
    estimateCache.clear();
    rowWrapperCache = new WeakMap();
  });

  const rowQuota = ref(INITIAL_ROW_QUOTA);
  let quotaScheduled = false;
  function growRowQuota() {
    if (quotaScheduled) return;
    quotaScheduled = true;
    const step = () => {
      quotaScheduled = false;
      const total = renderedBlocks.value.length;
      if (rowQuota.value >= total) return;
      rowQuota.value = Math.min(total, rowQuota.value + ROW_QUOTA_STEP);
      if (rowQuota.value < total) growRowQuota();
    };
    if ('requestIdleCallback' in window) requestIdleCallback(step, { timeout: 200 });
    else setTimeout(step, 0);
  }
  // 配额内可见行与配额外尾部占位高度（估计高度和，保证滚动条总高近似正确）
  const visibleRows = computed(() => renderedBlocks.value.slice(0, rowQuota.value));
  const tailPadHeight = computed(() => {
    let h = 0;
    const rows = renderedBlocks.value;
    for (let i = rowQuota.value; i < rows.length; i++) h += rows[i].estimate;
    return h;
  });
  watch(
    () => renderedBlocks.value.length,
    (len) => {
      if (len > rowQuota.value) growRowQuota();
    },
  );
  // 配额增长后新挂载的行需要补测标题位置（防抖，避免逐批 querySelector）
  watch(
    () => visibleRows.value.length,
    () => ctx.scheduleRebuildHeadingPositions(300),
  );

  // 重活懒执行：共享 IntersectionObserver，块进入视口（含 60% 窗口高度余量）才回调
  //（语法高亮/公式/Mermaid 渲染经此延后，长文档挂载不再一次性打满 IPC；
  // 余量随窗口缩放动态生效）
  const visibilityCallbacks = new WeakMap();
  let blockObserver = null;
  function getBlockObserver() {
    if (!blockObserver) {
      blockObserver = new IntersectionObserver(
        (entries) => {
          for (const entry of entries) {
            if (!entry.isIntersecting) continue;
            const cb = visibilityCallbacks.get(entry.target);
            if (cb) {
              visibilityCallbacks.delete(entry.target);
              blockObserver.unobserve(entry.target);
              cb();
            }
          }
        },
        { root: null, rootMargin: '200% 0px' },
      );
    }
    return blockObserver;
  }
  provide('onBlockVisible', (el, cb) => {
    if (!el) return cb();
    visibilityCallbacks.set(el, cb);
    getBlockObserver().observe(el);
  });

  // 组件卸载时断开观察器（多标签/多窗口复用防泄漏）
  onBeforeUnmount(() => {
    rowObserver?.disconnect();
    rowObserver = null;
    blockObserver?.disconnect();
    blockObserver = null;
  });

  // growRowQuota 仅模块内部调度（配额监听/递归步进），不进 ctx
  return { renderedBlocks, visibleRows, tailPadHeight, rowQuota };
}
