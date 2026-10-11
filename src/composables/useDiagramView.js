// Mermaid / 数学公式块的渲染视图状态：分段视图（源码|并排|预览，默认预览）+ 渲染区缩放
//（0.25–4 倍、步进 0.25）。模块级 Map 按块 id 会话保持——虚拟滚动重挂载后视图状态恢复；
// 不写源码、不入 localStorage。
// 缩放为纯视图层 transform（与渲染 SVG 无关）：transform: scale 不改变布局盒，
// 容器 overflow:auto 需要显式占位才有正确滚动范围——ResizeObserver 量取内容未缩放高度，
// 舞台元素按 zoom 比例占位（宽度用百分比跟随视口，天然无反馈环），
// 内容元素静态流内按 100/zoom 宽度居中后绕顶中缩放，可视区恰好铺满舞台。
import { computed, reactive, ref, onBeforeUnmount } from 'vue';

const viewStates = new Map(); // blockId -> reactive({ mode, zoom })

export const DIAGRAM_MODES = [
  { id: 'source', label: '源码' },
  { id: 'split', label: '并排' },
  { id: 'preview', label: '预览' },
];
export const ZOOM_MIN = 0.25;
export const ZOOM_MAX = 4;
export const ZOOM_STEP = 0.25;

export function useDiagramView(blockId) {
  let state = viewStates.get(blockId);
  if (!state) {
    state = reactive({ mode: 'preview', zoom: 1 });
    viewStates.set(blockId, state);
  }

  function setMode(mode) {
    if (DIAGRAM_MODES.some((m) => m.id === mode)) state.mode = mode;
  }
  function zoomIn() {
    // 步进 0.25 为二进制精确值，累加无浮点漂移
    state.zoom = Math.min(ZOOM_MAX, state.zoom + ZOOM_STEP);
  }
  function zoomOut() {
    state.zoom = Math.max(ZOOM_MIN, state.zoom - ZOOM_STEP);
  }
  function resetZoom() {
    state.zoom = 1;
  }
  const zoomLabel = computed(() => `${Math.round(state.zoom * 100)}%`);

  // 内容高度测量：zoom 变化时内容像素宽度恒等于视口宽（stage% × content% 相约），
  // 不会触发重排；仅视口变宽/窄（换行重排）或渲染结果变化时 RO 更新高度
  const contentEl = ref(null);
  const contentHeight = ref(0);
  let ro = null;
  function measure() {
    if (contentEl.value) contentHeight.value = contentEl.value.offsetHeight;
  }
  // 函数 ref：模式切换/组件卸载导致元素替换时重挂观察
  function contentRef(el) {
    if (contentEl.value && contentEl.value !== el) ro?.unobserve(contentEl.value);
    contentEl.value = el;
    if (!el) return;
    if (!ro) ro = new ResizeObserver(measure);
    measure();
    ro.observe(el);
  }
  onBeforeUnmount(() => {
    ro?.disconnect();
    ro = null;
  });

  // 占位舞台：宽 = 视口 × zoom（百分比），高 = 内容未缩放高 × zoom（测量后补 px；
  // 未测量时高度 auto 由静态内容撑开，无塌陷闪烁）；zoom<1 时 margin auto 居中
  const stageStyle = computed(() => ({
    width: `${state.zoom * 100}%`,
    height: contentHeight.value ? `${contentHeight.value * state.zoom}px` : undefined,
    margin: '0 auto',
  }));
  // 内容元素：宽 = 舞台 / zoom（即视口宽），margin auto 居中，绕顶中缩放后可视区铺满舞台；
  // flow-root 建 BFC 把子元素垂直 margin（如 .math-display 的 0.5em）收进测量盒
  const contentStyle = computed(() => ({
    display: 'flow-root',
    width: `${(100 / state.zoom).toFixed(4)}%`,
    margin: '0 auto',
    transform: `scale(${state.zoom})`,
    transformOrigin: 'top center',
  }));

  return {
    state,
    setMode,
    zoomIn,
    zoomOut,
    resetZoom,
    zoomLabel,
    contentRef,
    stageStyle,
    contentStyle,
  };
}
