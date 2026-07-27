import { ref, computed } from 'vue';

// 覆盖层滚动条：自绘滑轨的位置/显示/拖动。
// 本 WebView 对 ::-webkit-scrollbar / scrollbar-color 均不生效时的替代实现——
// 原生条用 scrollbar-width:none 隐藏，由本模块驱动一条彩色可拖动滑轨。
// getScrollEl：返回滚动容器元素（可为 null）；autoHide：自动隐藏开关（computed/ref）。
export function useOverlayScrollbar(getScrollEl, autoHide) {
  const scrolling = ref(false);
  const hover = ref(false);
  const dragging = ref(false);
  const thumb = ref({ top: 0, height: 0, offsetTop: 0 });

  const show = computed(
    () => autoHide.value && (scrolling.value || hover.value || dragging.value) && thumb.value.height > 0,
  );

  function metrics() {
    const root = getScrollEl();
    if (!root) return null;
    const { scrollHeight, clientHeight, scrollTop } = root;
    if (scrollHeight <= clientHeight) return null;
    return { root, scrollHeight, clientHeight, scrollTop };
  }

  // 按滚动比例重算滑轨位置/高度
  function update() {
    const m = metrics();
    if (!m) {
      thumb.value = { top: 0, height: 0, offsetTop: 0 };
      return;
    }
    const height = Math.max(24, (m.clientHeight / m.scrollHeight) * m.clientHeight);
    const top = (m.scrollTop / (m.scrollHeight - m.clientHeight)) * (m.clientHeight - height);
    thumb.value = { top, height, offsetTop: m.root.offsetTop ?? 0 };
  }

  // 滚动事件：显示滑轨并更新位置（停止 600ms 后隐藏）
  let scrollEndTimer = null;
  function onScroll() {
    scrolling.value = true;
    clearTimeout(scrollEndTimer);
    scrollEndTimer = setTimeout(() => {
      scrolling.value = false;
    }, 600);
    update();
  }

  // 指针悬停右缘揭示（16px 热区）
  function onMouseMove(e) {
    if (!autoHide.value) return;
    hover.value = e.clientX > e.currentTarget.getBoundingClientRect().right - 16;
  }
  function onMouseLeave() {
    hover.value = false;
  }

  // 拖动滑块：按 可视高度差/可滑高度 比例换算 scrollTop
  function onThumbPointerDown(e) {
    const m = metrics();
    if (!m) return;
    e.preventDefault();
    e.stopPropagation();
    dragging.value = true;
    const startY = e.clientY;
    const startScrollTop = m.scrollTop;
    const move = (ev) => {
      const ratio = (m.scrollHeight - m.clientHeight) / (m.clientHeight - thumb.value.height);
      m.root.scrollTop = startScrollTop + (ev.clientY - startY) * ratio;
    };
    const up = () => {
      dragging.value = false;
      document.removeEventListener('pointermove', move);
      document.removeEventListener('pointerup', up);
    };
    document.addEventListener('pointermove', move);
    document.addEventListener('pointerup', up);
  }

  // 点击轨道翻页（滑块上方/下方各一页）
  function onTrackPointerDown(e) {
    const m = metrics();
    if (!m) return;
    e.preventDefault();
    const trackTop = e.currentTarget.getBoundingClientRect().top;
    const dir = e.clientY < trackTop + thumb.value.top ? -1 : 1;
    m.root.scrollTop += dir * m.clientHeight * 0.9;
  }

  return {
    scrolling,
    hover,
    dragging,
    thumb,
    show,
    update,
    onScroll,
    onMouseMove,
    onMouseLeave,
    onThumbPointerDown,
    onTrackPointerDown,
  };
}
