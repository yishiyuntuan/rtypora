import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from '@tailwindcss/vite';
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [vue(),tailwindcss()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      //    另忽略原子写临时目录（编辑器/工具保存文件时先写 .NAME.*.tmpdir\ 再改名，
      //    chokidar 跟进该目录会 EBUSY 崩溃、直接杀死 dev server）
      ignored: ["**/src-tauri/**", "**/.*.tmpdir/**"],
    },
  },
}));
