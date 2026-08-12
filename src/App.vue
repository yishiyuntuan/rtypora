<script setup vapor>
import { computed, onMounted, onUnmounted, provide, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { openPath } from "@tauri-apps/plugin-opener";
import { Window } from "@tauri-apps/api/window";
import TitleBar from "./component/TitleBar.vue";
import StatusBar from "./component/StatusBar.vue";
import Sidebar from "./component/Sidebar.vue";
import Editor from "./component/Editor.vue";
import MenuDrawer from "./component/MenuDrawer.vue";
import PrefsDialog from "./component/PrefsDialog.vue";
import AboutDialog from "./component/AboutDialog.vue";
import ConfirmDialog from "./component/ConfirmDialog.vue";
import { applyEditorOverrides, getPref, syncRustPrefs } from "./utils/prefs.js";
import { assetUrlToDataUrl } from "./utils/image.js";
import { pathKey, dedupPaths } from "./utils/platform.js";

const appWindow = new Window("main");

const sidebarVisible = ref(true);
const sourceMode = ref(false);
const editorStats = ref({ wordCount: 0, charCount: 0, lineCount: 1, cursorLine: 1, cursorColumn: 1 });
// 文档块树与当前标题（Editor 上报，供侧边栏目录/大纲使用）
const docBlocks = ref([]);
const activeHeadingId = ref(null);
const editorRef = ref(null);

// Editor 高频发布块树（每次提交）：静默期首次立即更新（打开/切文档无延迟感），
// 连续编辑期 150ms 去抖——数千标题时侧栏不再每次提交都全量重建/重绘
let docBlocksTimer = null;
let lastBlocksApply = 0;
function onDocBlocks(blocks) {
  const now = Date.now();
  clearTimeout(docBlocksTimer);
  if (now - lastBlocksApply > 500) {
    lastBlocksApply = now;
    docBlocks.value = blocks;
  } else {
    docBlocksTimer = setTimeout(() => {
      lastBlocksApply = Date.now();
      docBlocks.value = blocks;
    }, 150);
  }
}

// 当前文件路径（新建为 null）；文件名用于标题栏显示
const currentFilePath = ref(null);
const currentFileName = computed(() => {
  if (!currentFilePath.value) return "";
  return currentFilePath.value.split(/[\\/]/).pop() || "";
});
// 文档所在目录（图片相对路径的解析基准，注入给块渲染层）
// 拼接统一用正斜杠：macOS/Linux 原生分隔符，Windows 侧 Rust 路径 API 同样接受
const documentDir = computed(() => {
  if (!currentFilePath.value) return null;
  const parts = currentFilePath.value.split(/[\\/]/);
  parts.pop();
  return parts.join("/") || null;
});
provide("documentDir", documentDir);
// 侧边栏工作目录（打开文件时跟随其目录，也可经「打开文件夹」独立设置）
const workspaceDir = ref(null);
watch(currentFilePath, (path) => {
  if (path) {
    const parts = path.split(/[\\/]/);
    parts.pop();
    workspaceDir.value = parts.join("/") || path;
  }
});

// 菜单与各对话框状态
const menuVisible = ref(false);
const prefsVisible = ref(false);
const prefsPage = ref("editor");
const aboutVisible = ref(false);
const confirmVisible = ref(false);
// 未保存确认后的待执行动作
let pendingAction = null;

// 最近使用的文件（localStorage，最多 10 条，最新在前；按规范化路径键去重——
// 对话框反斜杠形式与拼接正斜杠形式、Windows 大小写差异不会重复入列）
const RECENT_KEY = "tauri-editor.recent-files";
const recentFiles = ref(dedupPaths(JSON.parse(localStorage.getItem(RECENT_KEY) || "[]")));
function recordRecent(path) {
  const list = [path, ...recentFiles.value.filter((p) => pathKey(p) !== pathKey(path))].slice(0, 10);
  recentFiles.value = list;
  localStorage.setItem(RECENT_KEY, JSON.stringify(list));
}

onMounted(async () => {
  applyEditorOverrides();
  // 把影响 Rust 解析的偏好（html_to_md）同步到 Rust 端
  syncRustPrefs();
  // 焦点切换触发：窗口失焦时按偏好自动保存
  unlistenWindowBlur = await appWindow.listen("tauri://blur", () => {
    if (getPref("auto_save_trigger") === "blur") autoSave();
  });
});
onUnmounted(() => {
  clearTimeout(delaySaveTimer);
  unlistenWindowBlur?.();
});

// ---------- 自动保存（默认关闭；触发方式：焦点切换 / 输入停止 N 秒） ----------

let delaySaveTimer = null;
let unlistenWindowBlur = null;

// 自动保存条件：开关开启、已有文件路径（新文档不弹另存对话框）、内容有改动
function autoSaveEligible() {
  return getPref("auto_save_enabled") && currentFilePath.value && editorRef.value?.isDirty?.();
}

// 当前文档的换行风格（打开文件时由 Rust 探测；新文档默认 LF）。
// 编辑模型统一 LF，保存时按此还原，保持磁盘文件原有换行约定
const lineEnding = ref("lf");
function serializeForSave(text) {
  return lineEnding.value === "crlf" ? text.replace(/\n/g, "\r\n") : text;
}

async function autoSave() {
  if (!autoSaveEligible()) return;
  try {
    await invoke("save_file", { path: currentFilePath.value, content: serializeForSave(editorRef.value.getContent()) });
    editorRef.value.markSaved();
  } catch (e) {
    console.error("自动保存失败:", e);
  }
}

// 延时模式：内容/光标活动后 N 秒无操作则保存（统计更新即活动信号）
function scheduleDelaySave() {
  if (getPref("auto_save_trigger") !== "delay") return;
  clearTimeout(delaySaveTimer);
  const seconds = Number(getPref("auto_save_delay_seconds")) || 3;
  delaySaveTimer = setTimeout(autoSave, Math.max(1, seconds) * 1000);
}

watch(editorStats, scheduleDelaySave);

// ---------- 文件操作 ----------

function doNew() {
  currentFilePath.value = null;
  lineEnding.value = "lf";
  editorRef.value?.loadDocument("");
}

async function doOpen() {
  const opened = await invoke("open_markdown_parsed");
  if (!opened) return;
  currentFilePath.value = opened.path;
  lineEnding.value = opened.lineEnding || "lf";
  recordRecent(opened.path);
  editorRef.value?.loadDocument(opened.content, { blocks: opened.blocks, tailFrom: opened.tailFrom });
}

// 侧边栏文件树点击：按路径打开
async function onSidebarOpenFile(path) {
  const opened = await invoke("read_markdown_parsed", { path });
  if (!opened) return;
  currentFilePath.value = opened.path;
  lineEnding.value = opened.lineEnding || "lf";
  recordRecent(opened.path);
  editorRef.value?.loadDocument(opened.content, { blocks: opened.blocks, tailFrom: opened.tailFrom });
}

// 侧边栏：打开文件夹（弹框或直接传入路径）
async function onOpenFolder(path) {
  const dir = path ?? (await invoke("pick_folder"));
  if (dir) workspaceDir.value = dir;
}

// 侧边栏：在当前文件夹新建文件并打开
async function onCreateFile() {
  if (!workspaceDir.value) return;
  const path = await invoke("create_markdown_file", { dir: workspaceDir.value }).catch(() => null);
  if (path) await onSidebarOpenFile(path);
}

// 侧边栏：在系统资源管理器中显示目录
async function onShowInExplorer(dir) {
  if (dir) await openPath(dir).catch((e) => console.error("打开资源管理器失败:", e));
}

async function doSave() {
  if (!currentFilePath.value) {
    await doSaveAs();
    return;
  }
  const content = serializeForSave(editorRef.value?.getContent() ?? "");
  try {
    await invoke("save_file", { path: currentFilePath.value, content });
    editorRef.value?.markSaved();
  } catch (e) {
    alert(`保存失败：${e}`);
  }
}

async function doSaveAs() {
  const content = serializeForSave(editorRef.value?.getContent() ?? "");
  const result = await invoke("save_file_as", { content });
  if (!result) return;
  if (result.Err) {
    alert(`保存失败：${result.Err}`);
    return;
  }
  currentFilePath.value = result.Ok;
  recordRecent(result.Ok);
  editorRef.value?.markSaved();
}

// ---------- 导出 HTML ----------

// 渲染态内容 HTML + 主题样式文本（视图层提取）→ Rust build_export_html 装配
//（文档模板与 title 转义在 Rust 完成）；含已渲染的公式/图表/图片
async function doExport() {
  const root = document.querySelector(".t-root .t-measure");
  if (!root) return;
  const clone = root.cloneNode(true);
  clone.querySelectorAll("[contenteditable]").forEach((el) => el.removeAttribute("contenteditable"));
  // 编辑器内图片走 asset 协议（仅 webview 内可访问）：导出时换回 data URL 保证自包含
  await Promise.all(
    [...clone.querySelectorAll("img")].map(async (img) => {
      const src = img.getAttribute("src") || "";
      if (!src) return;
      const dataUrl = await assetUrlToDataUrl(src);
      if (dataUrl) img.setAttribute("src", dataUrl);
    }),
  );
  const rootStyle = document.documentElement.style;
  const vars = Array.from(rootStyle)
    .filter((name) => name.startsWith("--t-"))
    .map((name) => `  ${name}: ${rootStyle.getPropertyValue(name)};`)
    .join("\n");
  const rules = [];
  for (const sheet of Array.from(document.styleSheets)) {
    let cssRules;
    try {
      cssRules = sheet.cssRules;
    } catch {
      continue;
    }
    for (const rule of Array.from(cssRules)) {
      if (rule.selectorText?.startsWith(".t-")) rules.push(rule.cssText);
    }
  }
  const title = currentFileName.value || "document";
  const cssText = `:root {\n${vars}\n}\nbody { margin: 0; background: var(--t-editor-background); color: var(--t-text-default); }\n${rules.join("\n")}`;
  const html = await invoke("build_export_html", { contentHtml: clone.innerHTML, cssText, title });
  // 建议文件名由 Rust 端按源文件名推导（.md/.markdown → .html）
  const result = await invoke("save_html_as", { content: html, sourceName: title });
  if (result?.Err) alert(`导出失败：${result.Err}`);
}

// ---------- 未保存更改守卫 ----------

function guardThen(run) {
  if (editorRef.value?.isDirty?.()) {
    pendingAction = run;
    confirmVisible.value = true;
  } else {
    run();
  }
}

async function onConfirmSave() {
  confirmVisible.value = false;
  await doSave();
  // 保存成功（或用户取消另存对话框时仍脏）才继续后续动作
  if (!editorRef.value?.isDirty?.()) {
    pendingAction?.();
  }
  pendingAction = null;
}

function onConfirmDiscard() {
  confirmVisible.value = false;
  pendingAction?.();
  pendingAction = null;
}

function onConfirmCancel() {
  confirmVisible.value = false;
  pendingAction = null;
}

// ---------- 菜单动作 ----------

function onMenuAction(action) {
  menuVisible.value = false;
  if (action && typeof action === "object") {
    if (action.type === "prefs") {
      prefsPage.value = action.page;
      prefsVisible.value = true;
    } else if (action.type === "open-recent") {
      guardThen(() => onSidebarOpenFile(action.path));
    }
    return;
  }
  switch (action) {
    case "new":
      guardThen(doNew);
      break;
    case "open":
      guardThen(doOpen);
      break;
    case "save":
      doSave();
      break;
    case "save-as":
      doSaveAs();
      break;
    case "export":
      doExport();
      break;
    case "print":
      // 打印当前打开的文档（Editor 内部先全量挂载行再走打印样式）
      editorRef.value?.printDocument();
      break;
    case "prefs":
      prefsPage.value = "editor";
      prefsVisible.value = true;
      break;
    case "about":
      aboutVisible.value = true;
      break;
    case "close":
      guardThen(() => appWindow.close());
      break;
  }
}

// ---------- 全局快捷键 ----------

function onGlobalKeydown(e) {
  if (!(e.ctrlKey || e.metaKey) || e.isComposing) return;
  const key = e.key.toLowerCase();
  if (key === "n") {
    e.preventDefault();
    guardThen(doNew);
  } else if (key === "o") {
    e.preventDefault();
    guardThen(doOpen);
  } else if (key === "s" && e.shiftKey) {
    e.preventDefault();
    doSaveAs();
  } else if (key === "s") {
    e.preventDefault();
    doSave();
  } else if (key === "w") {
    e.preventDefault();
    guardThen(() => appWindow.close());
  } else if (key === "/") {
    e.preventDefault();
    onToggleSource();
  }
}
// Ctrl+/ 或状态栏 </>：先精确捕获光标（编辑态则先提交），再切换源码/WYSIWYG，
// 恢复滚动位置由 Editor.vue 的 sourceMode watch 在重新解析后执行
async function onToggleSource() {
  await editorRef.value?.captureScrollPosition();
  sourceMode.value = !sourceMode.value;
}
onMounted(() => window.addEventListener("keydown", onGlobalKeydown));
onUnmounted(() => window.removeEventListener("keydown", onGlobalKeydown));
</script>

<template>

  <div class="t-app relative flex h-screen flex-col">
    <div class="flex flex-1 overflow-hidden">
      <Sidebar
        v-model:visible="sidebarVisible"
        :blocks="docBlocks"
        :active-heading-id="activeHeadingId"
        :current-file-path="currentFilePath"
        :workspace-dir="workspaceDir"
        @select-block="editorRef?.scrollToBlock($event)"
        @open-file="onSidebarOpenFile"
        @open-folder="onOpenFolder"
        @create-file="onCreateFile"
        @show-in-explorer="onShowInExplorer"
      />
      <div class="flex flex-1 flex-col overflow-hidden">
        <TitleBar :file-name="currentFileName" :sidebar-visible="sidebarVisible" @toggle-menu="menuVisible = !menuVisible" />
        <main class="mt-1 flex-1 overflow-hidden">
          <Editor
            ref="editorRef"
            :source-mode="sourceMode"
            @update:stats="editorStats = $event"
            @update:blocks="onDocBlocks"
            @update:active-heading="activeHeadingId = $event"
          />
        </main>
        <StatusBar
          :sidebar-visible="sidebarVisible"
          :source-mode="sourceMode"
          :word-count="editorStats.wordCount"
          :char-count="editorStats.charCount"
          :line-count="editorStats.lineCount"
          :cursor-line="editorStats.cursorLine"
          :cursor-column="editorStats.cursorColumn"
          @toggle-sidebar="sidebarVisible = !sidebarVisible"
          @toggle-source="onToggleSource"
        />
      </div>
    </div>

    <!-- 偏好设置：覆盖整个窗口的整页层（含侧边栏/标题栏/状态栏），同一窗口打开非对话框 -->
    <PrefsDialog v-if="prefsVisible" :visible="prefsVisible" :page="prefsPage" @close="prefsVisible = false" />

    <MenuDrawer :visible="menuVisible" :recent-files="recentFiles" @close="menuVisible = false" @action="onMenuAction" />
    <AboutDialog :visible="aboutVisible" @close="aboutVisible = false" />
    <ConfirmDialog
      :visible="confirmVisible"
      :file-name="currentFileName"
      @save="onConfirmSave"
      @discard="onConfirmDiscard"
      @cancel="onConfirmCancel"
    />
  </div>
</template>
