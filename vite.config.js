import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from '@tailwindcss/vite';
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [vue(), tailwindcss()],
  // mermaid 12 要求 ES2024（Safari 17.4+ / 现代 WebView2/Chromium），
  // 构建目标对齐，避免 esbuild 把 mermaid 产物降级成它不认识的语法
  build: {
    target: "es2024",
    rollupOptions: {
      output: {
        manualChunks(id) {
          // mermaid 12 按图类型自动分包产生数十个碎 chunk：与其独占依赖归组为单文件，
          // 减少安装包碎文件数量（本体仍是动态 import，首个图表可见时才加载）
          if (/node_modules[\\/](mermaid|@mermaid-js|dagre|katex|cytoscape|d3|khroma|ts-dedent|stylis|uuid|marked|dompurify)/.test(id)) {
            return 'mermaid';
          }
        },
      },
    },
  },

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
