<script setup>
import { ref } from 'vue';
import { listThemes, currentThemeId, applyTheme, importThemeJson } from '../themes/index.js';

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
    class="t-statusbar flex h-7 items-center justify-between border-t border-(--t-table-border) text-[12px] select-none"
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
