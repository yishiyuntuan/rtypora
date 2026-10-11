import { ref, watch, nextTick, onMounted, onBeforeUnmount } from 'vue';
import { getPref } from '../utils/prefs.js';

// 阅读位置记忆（F-E）+ 返回顶部（F-F）。
//
// 每文件记录滚动位置：
//   WYSIWYG —— 视口顶部第一个可见块的源码 UTF-16 起点偏移 + 该块顶到视口顶的像素差
//              （块经 .md-vrow[data-block-id] 与 ctx.blocks 对照；块 id 每次解析重新生成
//              uuid，跨会话锚定必须用源码偏移，不能用块 id）；
//   源码模式 —— textarea scrollTop / scrollHeight 比例。
// 写入时机：滚动防抖 800ms、路径切换前（flush:'sync' 侦听，先于 App 的 loadDocument 执行，
// 此时 DOM/块树仍是旧文档）、beforeunload、组件卸载；localStorage 键
// tauri-editor.reading-positions（Map JSON 条目数组，FIFO 上限 200 条）。
// 恢复：路径变化后等新文档首次渲染就绪（content/blocks 引用变化 + 块渲染轮询），按偏移找
// 「start <= 记录偏移的最后一个根块」，经大纲滚动 scrollToBlock 定位 + delta 像素微调；
// 源码模式按比例还原。记录模式与当前模式不同时按 content 长度比例互相折算（尽力而为）。
// 偏好 reading_position_restore（默认 true）关闭时既不写也不恢复。
//
// 返回顶部：backTopVisible（scrollRoot.scrollTop > 600）+ scrollToTop()（平滑回 0），仅 WYSIWYG。
//
// 滚动监听方案：自挂元素级 passive 监听（watch 元素 ref 挂载/卸载自动挂拆），
// 不改 useOutlineScroll 的 onEditorScroll——在已绑定模板的方法里插一行调用是跨模块
// 腐化点（重构该方法时易被静默丢弃）；元素级监听自包含，与模板绑定的处理并行不悖。
//
// ctx：Editor.vue 共享上下文（currentFilePath/scrollRoot/sourceRoot/content/blocks/allBlocks
// 为 ref；isSourceMode() 运行时查询模式；scrollToBlock 运行时经 ctx 解析）。
const STORAGE_KEY = 'tauri-editor.reading-positions';
const MAX_ENTRIES = 200; // FIFO 上限
const SAVE_DEBOUNCE = 800; // 滚动记录防抖（ms）
const BACK_TOP_THRESHOLD = 600; // 返回顶部按钮显隐阈值（px）

export function useReadingPosition(ctx) {
  // ---------- 存储（Map JSON，FIFO 200） ----------
  function loadStore() {
    try {
      const arr = JSON.parse(localStorage.getItem(STORAGE_KEY) || '[]');
      return Array.isArray(arr) ? new Map(arr) : new Map();
    } catch {
      return new Map();
    }
  }
  function getRecord(path) {
    return loadStore().get(path) ?? null;
  }
  function putRecord(path, record) {
    const map = loadStore();
    if (map.has(path)) map.delete(path); // 先删后插：刷新到末尾（最近使用）
    map.set(path, record);
    while (map.size > MAX_ENTRIES) map.delete(map.keys().next().value); // 逐出最旧
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify([...map]));
    } catch {
      // 隐私模式等写入失败：静默降级（功能失效不报错）
    }
  }

  // ---------- 位置捕获 ----------
  // 当前模式下的位置记录；元素未挂载/无可锚块时返回 null（保留旧记录）
  function capturePosition() {
    if (ctx.isSourceMode()) {
      const ta = ctx.sourceRoot?.value;
      if (!ta || !ta.scrollHeight) return null;
      return { mode: 'source', ratio: ta.scrollTop / ta.scrollHeight, ts: Date.now() };
    }
    const root = ctx.scrollRoot?.value;
    if (!root) return null;
    const top = root.scrollTop;
    // 视口顶部第一个可见行（底边越过视口顶的第一行）；行按文档顺序排列
    for (const el of root.querySelectorAll('.md-vrow[data-block-id]')) {
      if (el.offsetTop + el.offsetHeight > top) {
        const block = ctx.blocks.value.find((b) => b.id === el.getAttribute('data-block-id'));
        if (!block || block.start == null) return null;
        // delta = 块顶到视口顶的像素差（块跨顶时为负；与 restoreScrollPosition 同 offsetTop 约定）
        return { mode: 'wysiwyg', offset: block.start, delta: el.offsetTop - top, ts: Date.now() };
      }
    }
    return null;
  }

  // ---------- 写入（防抖 / 切换前 / 卸载前） ----------
  function saveNow() {
    if (!getPref('reading_position_restore')) return;
    const path = ctx.currentFilePath?.value;
    if (!path) return;
    const rec = capturePosition();
    if (rec) putRecord(path, rec);
  }
  let saveTimer = null;
  function scheduleSave() {
    if (restoring) return; // 恢复期间的程序性滚动不记录（避免覆盖待恢复值/写入中间态）
    clearTimeout(saveTimer);
    saveTimer = setTimeout(saveNow, SAVE_DEBOUNCE);
  }

  // ---------- 滚动监听（自挂元素级 passive；F-E 记录与 F-F 显隐共用） ----------
  const backTopVisible = ref(false);
  function onWysiwygScroll() {
    const root = ctx.scrollRoot?.value;
    backTopVisible.value = !!root && root.scrollTop > BACK_TOP_THRESHOLD;
    scheduleSave();
  }
  function onSourceScroll() {
    scheduleSave();
  }
  let attachedWysiwyg = null;
  let attachedSource = null;
  watch(
    () => ctx.scrollRoot?.value,
    (el) => {
      if (attachedWysiwyg) {
        attachedWysiwyg.removeEventListener('scroll', onWysiwygScroll);
        attachedWysiwyg = null;
      }
      if (el) {
        el.addEventListener('scroll', onWysiwygScroll, { passive: true });
        attachedWysiwyg = el;
        backTopVisible.value = el.scrollTop > BACK_TOP_THRESHOLD;
      } else {
        backTopVisible.value = false; // 源码模式（scrollRoot 卸载）下隐藏按钮
      }
    },
  );
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
      }
    },
  );

  // 平滑滚回顶部（仅 WYSIWYG 模板使用）
  function scrollToTop() {
    ctx.scrollRoot?.value?.scrollTo({ top: 0, behavior: 'smooth' });
  }

  // ---------- 恢复 ----------
  let restoring = false;
  let restoreToken = 0;

  // 路径变化：先为旧路径存位置（flush:'sync' 保证在 loadDocument 替换内容前捕获旧文档），
  // 再为新路径调度恢复
  watch(
    () => ctx.currentFilePath?.value,
    (newPath, oldPath) => {
      if (oldPath && oldPath !== newPath && getPref('reading_position_restore')) {
        const rec = capturePosition();
        if (rec) putRecord(oldPath, rec);
      }
      scheduleRestore(newPath);
    },
    { flush: 'sync' },
  );

  function scheduleRestore(path) {
    const token = ++restoreToken;
    if (!path || !getPref('reading_position_restore')) return;
    const rec = getRecord(path);
    if (!rec) return;
    // 此刻 content/blocks 仍是旧文档（本函数先于 loadDocument 执行）：捕获引用，
    // 等新文档首次渲染就绪（引用变化 + 渲染轮询）再恢复
    const oldContent = ctx.content.value;
    const oldBlocks = ctx.blocks.value;
    restoring = true;
    nextTick(() => pollRestore(token, path, rec, oldContent, oldBlocks, 0));
  }

  function pollRestore(token, path, rec, oldContent, oldBlocks, tries) {
    if (token !== restoreToken || ctx.currentFilePath?.value !== path) return; // 被更新的切换作废
    const docChanged = ctx.content.value !== oldContent || ctx.blocks.value !== oldBlocks;
    const ready = ctx.isSourceMode()
      ? !!ctx.sourceRoot?.value
      : !!(ctx.scrollRoot?.value && ctx.blocks.value.length && ctx.scrollRoot.value.querySelector('.md-vrow[data-block-id]'));
    // 就绪且文档已替换（或 600ms 后文档仍原样——同内容重开/另存场景兜底）即恢复
    if (ready && (docChanged || tries >= 12)) {
      applyRestore(rec);
      return;
    }
    if (tries >= 80) {
      restoring = false; // ~4s 超时放弃（空文档/大文档渐进加载兜底）
      return;
    }
    setTimeout(() => pollRestore(token, path, rec, oldContent, oldBlocks, tries + 1), 50);
  }

  function applyRestore(rec) {
    if (ctx.isSourceMode()) {
      const ta = ctx.sourceRoot?.value;
      if (!ta) {
        restoring = false;
        return;
      }
      // 按比例还原；WYSIWYG 记录折算为偏移比例（尽力而为）
      const len = ctx.content.value.length;
      const ratio = rec.mode === 'source' ? (rec.ratio ?? 0) : len ? rec.offset / len : 0;
      const target = Math.max(0, Math.round(ratio * ta.scrollHeight));
      ta.scrollTop = target;
      setTimeout(() => {
        if (ta.isConnected) ta.scrollTop = target; // 复核（字体加载/布局后移兜底）
      }, 300);
      setTimeout(() => {
        restoring = false;
      }, 350);
      return;
    }
    // WYSIWYG：按偏移找「start <= 记录偏移的最后一个根块」；源码记录折算偏移
    const len = ctx.content.value.length;
    const offset = rec.mode === 'wysiwyg' ? (rec.offset ?? 0) : Math.round((rec.ratio ?? 0) * len);
    const delta = rec.mode === 'wysiwyg' ? (rec.delta ?? 0) : 0;
    const roots = ctx.allBlocks.value.filter((b) => b.start != null);
    if (!roots.length) {
      restoring = false;
      return;
    }
    let target = roots[0];
    for (const b of roots) {
      if (b.start <= offset) target = b;
      else break;
    }
    ctx.scrollToBlock(target.id); // 经大纲滚动定位（含配额扩展与重试）
    waitRowEl(target.id, delta, 0);
  }

  // 内容挂载是异步的（配额分批/虚拟滚动）：轮询等目标行出现后再做 delta 微调
  function waitRowEl(id, delta, tries) {
    const el = ctx.scrollRoot?.value?.querySelector(`[data-block-id="${id}"]`);
    if (el) {
      settleThenAdjust(el, delta);
      return;
    }
    if (tries >= 40) {
      restoring = false; // ~2.4s
      return;
    }
    setTimeout(() => waitRowEl(id, delta, tries + 1), 60);
  }

  // scrollToBlock 的平滑滚动结束后再补回「块顶到视口顶的像素差」（瞬时滚动，避免与动画抢；
  // 二次补写兜底迟到的布局修正）
  function settleThenAdjust(el, delta) {
    const root = ctx.scrollRoot?.value;
    if (!root) {
      restoring = false;
      return;
    }
    const adjust = () => {
      if (el.isConnected) root.scrollTop = Math.max(0, el.offsetTop - delta);
    };
    setTimeout(adjust, 400);
    setTimeout(() => {
      adjust();
      restoring = false;
    }, 900);
  }

  // Editor 挂载时若路径已存在（会话恢复/启动直开文件），同样调度恢复
  onMounted(() => {
    const path = ctx.currentFilePath?.value;
    if (path) scheduleRestore(path);
  });

  // 窗口关闭前落盘
  const onBeforeUnload = () => saveNow();
  window.addEventListener('beforeunload', onBeforeUnload);

  onBeforeUnmount(() => {
    window.removeEventListener('beforeunload', onBeforeUnload);
    if (attachedWysiwyg) attachedWysiwyg.removeEventListener('scroll', onWysiwygScroll);
    if (attachedSource) attachedSource.removeEventListener('scroll', onSourceScroll);
    clearTimeout(saveTimer);
    ++restoreToken; // 作废在途恢复轮询
    saveNow(); // 组件卸载兜底写入（beforeunload 之外的关闭路径）
  });

  // 阅读位置记忆全部经事件/侦听驱动，模板只需返回顶部两项
  return { backTopVisible, scrollToTop };
}
