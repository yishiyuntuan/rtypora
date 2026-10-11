// 菜单弹出位置的视口适配：底部空间不足时向上翻，否则贴在锚点下方（间隙 6px）
//（围栏语言/图片路径补全与斜杠菜单共用同一翻折规则）
export function flipMenuTop(rect, estimatedHeight) {
  return rect.bottom + estimatedHeight > window.innerHeight
    ? Math.max(8, rect.top - estimatedHeight)
    : rect.bottom + 6;
}

// 光标锚点元素（文本节点取父元素）；在 pre（代码块/原文编辑）内返回 null——
// 围栏补全与斜杠菜单都不在 pre 内触发
export function caretAnchorOutsidePre(caret) {
  const el = caret.startContainer.nodeType === Node.TEXT_NODE ? caret.startContainer.parentElement : caret.startContainer;
  return el?.closest('pre') ? null : el;
}
