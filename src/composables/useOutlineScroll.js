import { ref, watch, nextTick, provide, onBeforeUnmount } from 'vue';

// 大纲联动与滚动定位：滚动到块/脚注（含渐进挂载配额扩展与重试）、当前标题跟踪（大纲高亮）、
// 标题位置缓存重建（防抖）、覆盖层滚动条悬停揭示。
//
// ctx：Editor.vue 共享上下文（scrollRoot/allBlocks/blocks/footnoteRefList/rowQuota/flashId 为 ref，
// emit 为组件 emit，sbOnScroll/sbOnMouseMove/sbOnMouseLeave 为覆盖层滚动条处理函数；
// closeCtxMenu/findVisible/hasKeyword/scheduleFindHighlights/renderedBlocks 运行时经 ctx 解析）。
export function useOutlineScroll(ctx) {
  // 当前滚动位置对应的标题 id（供大纲高亮）
  const activeHeadingId = ref(null);

  // 目标行可能尚在配额外（只有占位条）：分批扩配额（每批 2000 行），
  // 避免跳转大文档末尾时一次同步创建数千组件的单帧卡顿
  function expandRowQuotaTo(index) {
    if (index < 0 || index < ctx.rowQuota.value) return;
    const target = index + 1;
    const step = () => {
      if (ctx.rowQuota.value >= target) return;
      ctx.rowQuota.value = Math.min(target, ctx.rowQuota.value + 2000);
      if (ctx.rowQuota.value < target) setTimeout(step, 0);
    };
    step();
  }
  // 滚动到指定脚注定义（底部集中区域）并短暂闪烁高亮
  function scrollToFootnote(id) {
    const root = ctx.scrollRoot.value;
    if (!root) return;
    const el = root.querySelector(`[data-footnote-def="${id}"]`);
    if (!el) return;
    el.scrollIntoView({ behavior: 'smooth', block: 'center' });
    ctx.flashId.value = `fn-def-${id}`;
    setTimeout(() => {
      if (ctx.flashId.value === `fn-def-${id}`) ctx.flashId.value = null;
    }, 1200);
  }

  // 从脚注定义返回对应引用位置
  function scrollToFootnoteRef(refIndex) {
    const root = ctx.scrollRoot.value;
    if (!root) return;
    // 引用所在块可能被虚拟滚动卸载/尚在配额外：经 footnoteRefList 反查块 id，
    // 先扩配额（与 scrollToBlock 同批），再把行滚入视口触发内容挂载，持续重试到找到引用
    const blockId = ctx.footnoteRefList.value[refIndex - 1]?.blockId;
    expandRowQuotaTo(blockId ? ctx.allBlocks.value.findIndex((b) => b.id === blockId) : -1);
    const tryScroll = (attempts) => {
      const el = root.querySelector(`[data-footnote-ref="${refIndex}"]`);
      if (el) {
        el.scrollIntoView({ behavior: 'smooth', block: 'center' });
        return;
      }
      if (blockId) root.querySelector(`[data-block-id="${blockId}"]`)?.scrollIntoView({ block: 'center' });
      if (attempts > 0) setTimeout(() => tryScroll(attempts - 1), 80);
    };
    tryScroll(25);
  }

  // 滚动到指定块并短暂闪烁高亮（侧边栏目录/大纲点击定位）
  function scrollToBlock(id) {
    const root = ctx.scrollRoot.value;
    if (!root) return;
    expandRowQuotaTo(ctx.allBlocks.value.findIndex((b) => b.id === id));
    // 行创建/内容挂载是异步的（配额分批扩展）：找不到元素时持续重试
    const tryScroll = (attempts) => {
      const el = root.querySelector(`[data-block-id="${id}"]`);
      if (!el) {
        if (attempts > 0) setTimeout(() => tryScroll(attempts - 1), 60);
        return;
      }
      el.scrollIntoView({ behavior: 'smooth', block: 'start' });
      ctx.flashId.value = id;
      setTimeout(() => {
        if (ctx.flashId.value === id) ctx.flashId.value = null;
      }, 1200);
    };
    tryScroll(30);
  }

  provide('scrollToFootnote', (id) => scrollToFootnote(id));
  provide('scrollToFootnoteRef', (refIndex) => scrollToFootnoteRef(refIndex));
  provide('scrollToBlock', (id) => scrollToBlock(id));

  // 标题位置缓存（块树发布后重建，滚动停止后再校准；滚动判断零 DOM 查询）
  let headingPositions = [];
  watch(
    () => ctx.renderedBlocks.value,
    async () => {
      await nextTick();
      rebuildHeadingPositions();
    },
    { flush: 'post' },
  );

  // 滚动时同步当前标题（取滚动位置上方最近的标题块），供大纲高亮；滚动即关闭右键菜单。
  // 虚拟滚动下占位行高度随内容挂载被修正，滚动停止后重建位置缓存保持高亮准确
  let scrollTicking = false;
  let headingRebuildTimer = null;
  // 标题位置重建防抖调度（虚拟滚动扩配额与滚动共用同一计时器，后到者优先）
  function scheduleRebuildHeadingPositions(delay = 250) {
    clearTimeout(headingRebuildTimer);
    headingRebuildTimer = setTimeout(rebuildHeadingPositions, delay);
  }
  function onEditorScroll(e) {
    ctx.closeCtxMenu();
    // 滚动中显示覆盖层滑轨并更新位置
    ctx.sbOnScroll();
    // 查找高亮：滚动会挂载新行（虚拟滚动），防抖重扫
    if (ctx.findVisible.value && ctx.hasKeyword()) ctx.scheduleFindHighlights();
    scheduleRebuildHeadingPositions();
    if (scrollTicking) return;
    scrollTicking = true;
    requestAnimationFrame(() => {
      scrollTicking = false;
      const top = e.target.scrollTop + 8;
      let current = null;
      for (const h of headingPositions) {
        if (h.top <= top) current = h.id;
        else break;
      }
      if (current !== activeHeadingId.value) {
        activeHeadingId.value = current;
        ctx.emit('update:active-heading', current);
      }
    });
  }

  function rebuildHeadingPositions() {
    const root = ctx.scrollRoot.value;
    if (!root) {
      headingPositions = [];
      return;
    }
    // 单次遍历已挂载块元素建 id→元素 表，再查表取标题位置——
    // 逐标题 querySelector 在标题上千时是每次提交数千次全树扫描（编辑卡顿主因）
    const mountedEls = new Map();
    for (const el of root.querySelectorAll('[data-block-id]')) {
      mountedEls.set(el.getAttribute('data-block-id'), el);
    }
    headingPositions = ctx.blocks.value
      .filter((b) => b.type === 'heading')
      .map((b) => ({ id: b.id, top: mountedEls.get(b.id)?.offsetTop ?? null }))
      .filter((h) => h.top != null);
  }

  // 组件卸载时清理标题重建定时器（多标签/多窗口复用防泄漏）
  onBeforeUnmount(() => clearTimeout(headingRebuildTimer));

  // 滚动条悬停揭示（自动隐藏模式）：指针进入容器右缘 16px 内显示，离开隐藏
  function onEditorMouseMove(e) {
    ctx.sbOnMouseMove(e);
  }
  function onEditorMouseLeave(e) {
    ctx.sbOnMouseLeave(e);
  }

  // activeHeadingId 仅作内部变更守卫（注入 emit 去重），scrollToFootnote* 经上方 provide
  // 供块渲染层 inject，rebuildHeadingPositions 仅内部与防抖包装使用——均不进 ctx
  //（scrollToBlock 双通道：Editor 经 ctx 解构、BlockView 经 provide/inject）
  return {
    scrollToBlock,
    onEditorScroll,
    scheduleRebuildHeadingPositions,
    onEditorMouseMove,
    onEditorMouseLeave,
  };
}
