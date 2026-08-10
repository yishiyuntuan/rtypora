<script setup vapor>
import { Window } from '@tauri-apps/api/window';
import { isMac } from '../utils/platform.js';
import { trafficLightsVisible } from '../utils/prefs.js';

// 自定义标题栏：拖拽区、菜单按钮（打开滑出式菜单）、文件名显示、窗口控制按钮
// macOS 使用原生标题栏（Overlay 样式，原生红绿灯覆盖在左上角），
// 此处只提供拖拽区 + 居中标题 + 菜单按钮；其余平台为无边框自绘窗口控制。
defineProps({
  fileName: { type: String, default: '' },
  // macOS 下侧边栏收起时标题栏顶到窗口左缘，需为红绿灯留白
  sidebarVisible: { type: Boolean, default: true },
});

const emit = defineEmits(['toggle-menu']);

const appWindow = new Window('main');
</script>

<template>
  <!-- macOS：原生红绿灯在左上，菜单按钮紧挨着最大化（绿）按钮；
       侧边栏展开时红绿灯落在侧边栏留白条上，无需额外留白 -->
  <div
    v-if="isMac"
    data-tauri-drag-region
    class="t-titlebar relative flex h-8 items-center bg-transparent select-none"
    :class="{ 'pl-[84px]': !sidebarVisible && trafficLightsVisible }"
  >
    <div
      id="titlebar-menu"
      class="inline-flex h-full w-[46px] shrink-0 items-center justify-center text-inherit transition-[background] duration-[0.08s] ease-in-out hover:bg-(--t-status-bar-button-hover)"
      @click="emit('toggle-menu')"
    >
      <span class="hb-icon" aria-hidden="true" style="color: #3e69d7"><span></span><span></span><span></span></span>
    </div>
    <div class="flex-1 text-center text-xs text-inherit opacity-90 pointer-events-none" data-tauri-drag-region>
      {{ fileName ? `tauri-editor - ${fileName}` : 'tauri-editor' }}
    </div>
  </div>

  <!-- Windows/Linux：无边框自绘标题栏 -->
  <div
    v-else
    data-tauri-drag-region
    class="t-titlebar relative flex h-8 items-center justify-between bg-transparent select-none"
  >
    <div
      id="titlebar-menu"
      class="inline-flex h-full w-[46px] items-center justify-center text-inherit transition-[background] duration-[0.08s] ease-in-out hover:bg-(--t-status-bar-button-hover)"
      @click="emit('toggle-menu')"
    >
      <span class="hb-icon" aria-hidden="true" style="color: #3e69d7"><span></span><span></span><span></span></span>
    </div>

    <div class="flex-1 text-left text-xs text-inherit opacity-90 pointer-events-none" data-tauri-drag-region>
      {{ fileName ? `tauri-editor - ${fileName}` : 'tauri-editor' }}
    </div>

    <div class="flex h-full">
      <div
        id="titlebar-minimize"
        class="inline-flex h-full w-[46px] items-center justify-center text-inherit transition-[background] duration-[0.08s] ease-in-out hover:bg-(--t-status-bar-button-hover)"
        @click="appWindow.minimize()"
      >
        <svg viewBox="0 0 10 10" aria-hidden="true" class="size-[10px]">
          <rect x="1" y="4.5" width="8" height="1" fill="currentColor" />
        </svg>
      </div>
      <div
        id="titlebar-maximize"
        class="inline-flex h-full w-[46px] items-center justify-center text-inherit transition-[background] duration-[0.08s] ease-in-out hover:bg-(--t-status-bar-button-hover)"
        @click="appWindow.toggleMaximize()"
      >
        <svg viewBox="0 0 10 10" aria-hidden="true" class="size-[10px]">
          <rect x="1.5" y="1.5" width="7" height="7" fill="none" stroke="currentColor" stroke-width="1" />
        </svg>
      </div>
      <div
        id="titlebar-close"
        class="inline-flex h-full w-[46px] items-center justify-center text-inherit transition-[background] duration-[0.08s] ease-in-out hover:bg-[#e81123] hover:text-white active:bg-[#bf0f1d] active:text-white"
        @click="appWindow.close()"
      >
        <svg viewBox="0 0 10 10" aria-hidden="true" class="size-[10px]">
          <line x1="2" y1="2" x2="8" y2="8" stroke="currentColor" stroke-width="1.2" />
          <line x1="8" y1="2" x2="2" y2="8" stroke="currentColor" stroke-width="1.2" />
        </svg>
      </div>
    </div>
  </div>
</template>

<style scoped>
</style>
