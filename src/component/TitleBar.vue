<script setup>
import { Window } from '@tauri-apps/api/window';

const appWindow = new Window('main');
</script>

<template>
  <div
    data-tauri-drag-region
    class="flex h-8 items-center justify-between bg-transparent select-none"
  >
    <div
      id="titlebar-menu"
      class="inline-flex h-full w-[46px] items-center justify-center text-inherit transition-[background] duration-[0.08s] ease-in-out hover:bg-black/15 active:bg-black/25 dark:hover:bg-white/10 dark:active:bg-white/20"
    >
      <svg viewBox="0 0 16 16" aria-hidden="true" class="size-[14px]">
        <line x1="2" y1="4" x2="14" y2="4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
        <line x1="2" y1="8" x2="14" y2="8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
        <line x1="2" y1="12" x2="14" y2="12" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
      </svg>
    </div>
    <div class="flex-1 text-left text-xs text-inherit opacity-90 pointer-events-none">
      Tauri App
    </div>
    <div class="flex h-full">
      <div
        id="titlebar-minimize"
        class="inline-flex h-full w-[46px] items-center justify-center text-inherit transition-[background] duration-[0.08s] ease-in-out hover:bg-black/15 active:bg-black/25 dark:hover:bg-white/10 dark:active:bg-white/20"
        @click="appWindow.minimize()"
      >
        <svg viewBox="0 0 10 10" aria-hidden="true" class="size-[10px]">
          <rect x="1" y="4.5" width="8" height="1" fill="currentColor" />
        </svg>
      </div>
      <div
        id="titlebar-maximize"
        class="inline-flex h-full w-[46px] items-center justify-center text-inherit transition-[background] duration-[0.08s] ease-in-out hover:bg-black/15 active:bg-black/25 dark:hover:bg-white/10 dark:active:bg-white/20"
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
