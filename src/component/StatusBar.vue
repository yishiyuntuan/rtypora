<script setup>
import { computed, ref } from 'vue';
import { listThemes, currentThemeId, applyTheme, importThemeJson } from '../themes/index.js';
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
const themeId = ref(currentThemeId());
const themes = ref(listThemes());
const fileInput = ref(null);

function onThemeChange(e) {
  const value = e.target.value;
  if (value === '__import__') {
    // 还原选中项，改为触发文件选择
    e.target.value = themeId.value;
    fileInput.value?.click();
    return;
  }
  themeId.value = applyTheme(value);
}

async function onThemeFile(e) {
  const file = e.target.files?.[0];
  e.target.value = '';
  if (!file) return;
  try {
    const text = await file.text();
    themeId.value = importThemeJson(text);
    themes.value = listThemes();
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
    <div class="flex h-full items-center">
      <div
        class="t-btn inline-flex h-full cursor-pointer items-center gap-1 px-3"
        :class="{ active: sidebarVisible }"
        @click="emit('toggle-sidebar')"
      >
        <svg viewBox="0 0 16 16" aria-hidden="true" class="size-[14px]">
          <rect x="1" y="2" width="5" height="12" rx="1" fill="currentColor" />
          <rect :x="sidebarVisible ? 9 : 7" y="2" :width="sidebarVisible ? 6 : 8" height="12" rx="1" fill="none" stroke="currentColor" stroke-width="1" />
        </svg>
      </div>

      <div
        class="t-btn inline-flex h-full cursor-pointer items-center gap-1 px-3"
        :class="{ active: sourceMode }"
        @click="emit('toggle-source')"
      >
        <span>&lt;/&gt;</span>
      </div>

      <div
        class="t-btn inline-flex h-full cursor-pointer items-center gap-1 px-3"
        :class="{ active: lineNumbersOn }"
        title="代码行号显示"
        @click="setPref('render_code_line_numbers', !lineNumbersOn)"
      >
        <svg viewBox="0 0 16 16" aria-hidden="true" class="size-[14px]">
          <rect x="2" y="2.5" width="3.2" height="11" rx="0.8" fill="currentColor" opacity="0.45" />
          <rect x="7" y="4" width="7" height="1.6" rx="0.8" fill="currentColor" />
          <rect x="7" y="7.5" width="7" height="1.6" rx="0.8" fill="currentColor" />
          <rect x="7" y="11" width="7" height="1.6" rx="0.8" fill="currentColor" />
        </svg>
      </div>

      <div
        class="t-btn inline-flex h-full cursor-pointer items-center gap-1 px-3"
        :class="{ active: autoSaveOn }"
        title="自动保存（触发方式见 菜单 → 偏好设置 → 编辑器）"
        @click="setPref('auto_save_enabled', !autoSaveOn)"
      >
        <svg viewBox="0 0 16 16" aria-hidden="true" class="size-[14px]" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round">
          <path d="M3 2.5h7.5l2.5 2.5v8.5H3z" />
          <path d="M5.5 2.5v3h5v-3M5.5 13.5v-4.5h5v4.5" />
        </svg>
      </div>
    </div>

    <div class="flex h-full items-center">
      <div class="inline-flex h-full items-center px-2">
        Ln {{ cursorLine }}, Col {{ cursorColumn }}
      </div>
      <div class="inline-flex h-full items-center px-2">
        {{ lineCount }} 行
      </div>
      <div class="inline-flex h-full items-center px-2">
        {{ wordCount }} 词
      </div>
      <div class="inline-flex h-full items-center px-2">
        {{ charCount }} 字符
      </div>
      <select
        class="t-btn mx-1 h-5 cursor-pointer rounded bg-transparent px-1 text-[11px] outline-none"
        :value="themeId"
        @change="onThemeChange"
        title="编辑器主题"
      >
        <option v-for="theme in themes" :key="theme.id" :value="theme.id">{{ theme.name }}</option>
        <option value="__import__">导入主题…</option>
      </select>
      <input ref="fileInput" type="file" accept=".json,.jsonc" class="hidden" @change="onThemeFile" />
    </div>
  </div>
</template>
