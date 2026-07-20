import { createApp } from "vue";
import App from "./App.vue";
import "./assets/style.css";
import { initTheme } from "./themes/index.js";

// 挂载前注入主题 CSS 变量，避免首屏闪烁
initTheme();
createApp(App).mount("#app");
