<script setup vapor>
import { computed, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listThemes, currentThemeId, applyTheme, importThemeJson, isSystemDark, themeVersion } from '../themes/index.js';
import { getPref, setPref, prefsVersion } from '../utils/prefs.js';

const emit = defineEmits(['toggle-sidebar', 'toggle-source']);

defineProps({
  wordCount: { type: Number, default: 0 },
  charCount: { type: Number, default: 0 },
  lineCount: { type: Number, default: 0 },
  cursorLine: { type: Number, default: 1 },
  cursorColumn: { type: Number, default: 1 },
  sourceMode: { type: Boolean, default: false },
  sidebarVisible: { type: Boolean, default: true },
});

// 代码行号显示开关（偏好 render_code_line_numbers；prefsVersion 驱动 active 态与渲染联动）
const lineNumbersOn = computed(() => {
  prefsVersion.value;
  return getPref('render_code_line_numbers');
});

// 自动保存开关（偏好 auto_save_enabled，默认关闭；触发方式在偏好设置中配置）
const autoSaveOn = computed(() => {
  prefsVersion.value;
  return getPref('auto_save_enabled');
});

// 主题选择：内置明暗两套 + 用户导入的自定义主题（参照 velotype 的 Theme 菜单）
// themeVersion 驱动：偏好设置页/跟随系统切换主题、任何入口导入主题后此处同步刷新
const themes = computed(() => {
  themeVersion.value;
  return listThemes();
});
// 主题选择（v-model 双向）：get 取当前应用主题（themeVersion 驱动刷新）；
// set 处理切换/导入。vModelSelect 在选项渲染后写值，避免 :value 与 v-for 的竞态
const selectedTheme = computed({
  get() {
    themeVersion.value;
    return currentThemeId();
  },
  set(value) {
    if (value === '__import__') {
      // 原生文件对话框选择主题包（WKWebView 不认隐藏 file input 的程序化点击）；
      // 强制重建下拉使显示还原为当前主题
      onThemeImport();
      selectKey.value += 1;
      return;
    }
    // 跟随系统模式：写入当前系统外观对应的主题槽位（prefsVersion watcher 自动应用）
    if (getPref('theme_follow_system')) {
      setPref(isSystemDark() ? 'theme_dark_id' : 'theme_light_id', value);
      return;
    }
    applyTheme(value);
  },
});
const selectKey = ref(0);

// 导入主题：Rust 端原生对话框选读主题包文件
async function onThemeImport() {
  const text = await invoke('pick_theme_file').catch(() => null);
  if (!text) return;
  try {
    importThemeJson(text);
  } catch (err) {
    console.error('主题导入失败:', err);
    alert(`主题导入失败：${err.message}`);
  }
}
</script>

<template>
  <div
    class="t-statusbar flex h-7 items-center justify-between text-[12px] select-none"
  >
    <div class="flex h-full items-center gap-0.5 px-1">
      <div
        class="t-btn inline-flex h-[22px] cursor-pointer items-center justify-center gap-1 rounded-md px-2.5"
        :class="{ active: sidebarVisible }"
        @click="emit('toggle-sidebar')"
      >
        <svg viewBox="0 0 16 16" aria-hidden="true" class="size-[14px]" style="color: #3e69d7">
          <rect x="1" y="2" width="5" height="12" rx="1" fill="currentColor" />
          <rect :x="sidebarVisible ? 9 : 7" y="2" :width="sidebarVisible ? 6 : 8" height="12" rx="1" fill="none" stroke="currentColor" stroke-width="1" />
        </svg>
      </div>

      <div
        class="t-btn inline-flex h-[22px] cursor-pointer items-center justify-center gap-1 rounded-md px-2.5"
        :class="{ active: sourceMode }"
        @click="emit('toggle-source')"
      >
        <span style="color: #03b736">&lt;/&gt;</span>
      </div>

      <div
        class="t-btn inline-flex h-[22px] cursor-pointer items-center justify-center gap-1 rounded-md px-2.5"
        :class="{ active: lineNumbersOn }"
        title="代码行号显示"
        @click="setPref('render_code_line_numbers', !lineNumbersOn)"
      >
        <svg viewBox="0 0 16 16" aria-hidden="true" class="size-[14px]" style="color: #8250df">
          <rect x="2" y="2.5" width="3.2" height="11" rx="0.8" fill="currentColor" opacity="0.45" />
          <rect x="7" y="4" width="7" height="1.6" rx="0.8" fill="currentColor" />
          <rect x="7" y="7.5" width="7" height="1.6" rx="0.8" fill="currentColor" />
          <rect x="7" y="11" width="7" height="1.6" rx="0.8" fill="currentColor" />
        </svg>
      </div>

      <div
        class="t-btn inline-flex h-[22px] cursor-pointer items-center justify-center gap-1 rounded-md px-2.5"
        :class="{ active: autoSaveOn }"
        title="自动保存（触发方式见 菜单 → 偏好设置 → 编辑器）"
        @click="setPref('auto_save_enabled', !autoSaveOn)"
      >
        <svg viewBox="0 0 16 16" aria-hidden="true" class="size-[14px]" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" style="color: #f59102">
          <path d="M3 2.5h7.5l2.5 2.5v8.5H3z" />
          <path d="M5.5 2.5v3h5v-3M5.5 13.5v-4.5h5v4.5" />
        </svg>
      </div>
    </div>

    <div class="flex h-full items-center gap-1 px-1.5">
      <div class="t-dim inline-flex h-full items-center px-1 tabular-nums">
        Ln {{ cursorLine }}, Col {{ cursorColumn }}
      </div>
      <span class="h-3 w-px bg-(--t-table-border)"></span>
      <div class="t-dim inline-flex h-full items-center px-1 tabular-nums">
        {{ lineCount }} 行
      </div>
      <span class="h-3 w-px bg-(--t-table-border)"></span>
      <div class="t-dim inline-flex h-full items-center px-1 tabular-nums">
        {{ wordCount }} 词
      </div>
      <span class="h-3 w-px bg-(--t-table-border)"></span>
      <div class="t-dim inline-flex h-full items-center px-1 tabular-nums">
        {{ charCount }} 字符
      </div>
      <select
        :key="`${themes.length}-${selectKey}`"
        v-model="selectedTheme"
        class="t-btn mx-0.5 h-[22px] cursor-pointer rounded-md bg-transparent px-1.5 text-[11px] outline-none"
        title="编辑器主题"
      >
        <option v-for="theme in themes" :key="theme.id" :value="theme.id">{{ theme.name }}</option>
        <option value="__import__">导入主题…</option>
      </select>
    </div>
  </div>
</template>
