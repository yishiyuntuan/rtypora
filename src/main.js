import { createVaporApp } from "vue";
import { invoke } from "@tauri-apps/api/core";
import App from "./App.vue";
import "./assets/style.css";
import { initTheme } from "./themes/index.js";

// 挂载前注入主题 CSS 变量，避免首屏闪烁
initTheme();
// Vapor 模式挂载（全部组件均为 Vapor SFC，无 VDOM 依赖）
createVaporApp(App).mount("#app");
// 主题与首帧已就绪：通知 Rust 显示主窗口（窗口以 visible:false 创建，消除启动白闪）
invoke("show_main_window").catch(() => {});
