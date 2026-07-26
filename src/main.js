import { createVaporApp } from "vue";
import App from "./App.vue";
import "./assets/style.css";
import { initTheme } from "./themes/index.js";

// 挂载前注入主题 CSS 变量，避免首屏闪烁
initTheme();
// Vapor 模式挂载（全部组件均为 Vapor SFC，无 VDOM 依赖）
createVaporApp(App).mount("#app");
