# AGENTS.md

本文件面向 AI 编码代理，介绍本项目的架构、构建方式与开发约定。阅读前无需任何项目背景知识。

## 项目概览

这是一个使用 **Tauri 2 + Vue 3** 构建的桌面 Markdown 编辑器（窗口标题为 `tauri-editor`，应用标识符 `com.apple.tauri-app`）。已实现基于 Block 模型的 Markdown 编辑与渲染：支持所见即所得（Typora 式就地编辑）与 Markdown 原文编辑双模式，解析在 Rust 端用 pulldown-cmark 完成。

- 前端界面文案使用中文（如「开始写作...」「目录」「大纲」），Rust 代码注释也以中文为主，提交代码时请保持中文注释习惯。
- 包管理器为 **pnpm**（存在 `pnpm-lock.yaml`）。
- 窗口配置为无边框（`decorations: false`），由前端自定义标题栏接管窗口控制。

## 技术栈

### 前端（`src/`）
- **Vue 3**（`<script setup>` 单文件组件），纯 JavaScript（无 TypeScript、无 JSX）。
- **Vite 8** + `@vitejs/plugin-vue`，`@tailwindcss/vite` 插件接入 **Tailwind CSS 4** 与 **daisyUI 5**（在 `src/assets/style.css` 中通过 `@import "tailwindcss"; @plugin "daisyui";` 启用，无 tailwind.config 文件）。
- `@tauri-apps/api` 用于调用窗口等原生能力；`@tauri-apps/plugin-opener`、`@tauri-apps/plugin-autostart` 为已安装的插件。
- 无状态管理库（无 Pinia/Vuex），组件间通过 props / `defineEmits` 事件通信（如 `Editor` 通过 `update:stats` 事件向 `App.vue` 上报字数、光标位置，`Sidebar` 使用 `v-model:visible`）。

### 后端（`src-tauri/`，Rust）
- `src/main.rs`：入口，仅调用 `tauri_app_lib::run()`。保留 `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`，勿删除。
- `src/lib.rs`：`run()` 中构建 Tauri 应用，已注册 `tauri-plugin-opener` 和 `tauri-plugin-autostart`（含 `MacosLauncher::LaunchAgent`，仅桌面平台依赖，见 `Cargo.toml` 的 `[target.'cfg(not(any(target_os = "android", target_os = "ios")))'...]` 段）。Tauri 命令经 `.invoke_handler(tauri::generate_handler![...])` 注册，自有命令无需 capabilities 配置。
- `src/markdown/`：Markdown Block 模型与解析器。`mod.rs` 暴露 `parse_markdown` 命令（全文 → 块树 JSON）；`model.rs` 定义 serde 序列化的 `Block`/`BlockKind`/`Inline`/`ListItem`；`parser.rs` 用 pulldown-cmark（开启表格/删除线/任务列表扩展）把事件流转为块树。Rust 端无状态，文档全文由前端持有。
- `Cargo.toml` 中 `im`（持久化不可变树，用于规划的 undo/redo）尚未使用；`pulldown-cmark`、`serde`/`serde_json`、`uuid`（v4，生成块 id）已投入使用。
- lib 目标名 `tauri_app_lib`，crate-type 含 `staticlib`/`cdylib`（为移动端预留），`export = ["_*"]` 仅导出 Tauri 必需符号。
- `build.rs` 除 `tauri_build::build()` 外，还把 tauri-build 生成的 Windows 清单资源（`libresource.a`，含 comctl32 v6 清单）通过 `rustc-link-arg-tests` 补链到测试目标——否则 `cargo test` 的测试程序缺清单，启动时报 `STATUS_ENTRYPOINT_NOT_FOUND`（缺 `TaskDialogIndirect`）。

## 构建与运行命令

```bash
pnpm install        # 安装前端依赖

pnpm tauri dev      # 开发模式（自动执行 pnpm dev 启动 Vite，再启动 Rust 后端，支持 HMR）
pnpm tauri build    # 打包发布（自动执行 pnpm build，前端产物输出到 dist/，再编译 Rust 并生成安装包）

pnpm dev            # 仅启动前端 Vite 开发服务器（端口固定 1420，strictPort）
pnpm build          # 仅构建前端到 dist/
pnpm tauri <args>   # Tauri CLI 入口
```

- 前端产物目录 `dist/` 由 `tauri.conf.json` 的 `frontendDist: "../dist"` 引用。
- Rust 侧常用检查：`cd src-tauri && cargo check`。
- 构建环境为 Windows + Git Bash。

## 代码结构

```
src/
├── main.js            # Vue 应用入口，挂载 App 并引入全局样式
├── App.vue            # 根组件，组合四大区域组件，管理 sidebarVisible、sourceMode 与 editorStats
├── assets/style.css   # Tailwind 4 + daisyUI 入口（仅两行导入）
└── component/         # 注意：目录名是 component 而非 components
    ├── TitleBar.vue   # 自定义标题栏（data-tauri-drag-region 拖拽、最小化/最大化/关闭，经 @tauri-apps/api/window 的 Window('main') 控制）
    ├── Sidebar.vue    # 侧边栏，「目录/大纲」两个占位标签页，v-model:visible 控制显隐
    ├── Editor.vue     # 双模式编辑区：持有全文 Markdown（唯一数据源）；源码模式为 textarea，WYSIWYG 模式渲染块树，点击块进入 contenteditable 就地编辑
    ├── BlockView.vue  # 递归块渲染器（段落/标题/代码块/引用/列表/表格/分割线/HTML）
    ├── InlineView.vue # 递归行内渲染器（粗斜体/删除线/行内代码/链接/图片/换行）
    └── StatusBar.vue  # 状态栏，显示 Ln/Col、行/词/字符数，emit 切换侧边栏与源码模式
└── utils/
    └── wysiwyg.js     # 就地编辑转换层：Block→可编辑 HTML、DOM→Markdown 序列化、Markdown 快捷输入即时转换（类名与 BlockView 保持一致，需同步修改）
src-tauri/
├── src/main.rs        # 薄入口
├── src/lib.rs         # Tauri Builder、插件注册、invoke_handler 注册
├── src/markdown/
│   ├── mod.rs         # 模块入口，parse_markdown 命令
│   ├── model.rs       # Block/BlockKind/Inline/ListItem 模型（serde 序列化）
│   └── parser.rs      # pulldown-cmark 事件流 → 块树
├── tests/
│   └── markdown_parse.rs # 解析器集成测试
├── build.rs           # tauri_build::build() + 测试目标清单资源补链
├── capabilities/
│   ├── default.json   # 主窗口权限：core 默认 + 窗口拖拽/关闭/最小化/最大化等
│   └── desktop.json   # 桌面平台权限：autostart:default
├── tauri.conf.json    # Tauri 配置（见下）
└── Cargo.toml         # Rust 依赖与构建配置
```

## 开发约定

- Vue 组件一律使用 `<script setup>`；样式优先使用 Tailwind 工具类，深色模式用 `dark:` 前缀，仅过渡动画等少量样式写在 `<style scoped>`。
- 前端无 lint / format 配置（无 ESLint、Prettier），保持与现有代码一致的风格即可（双引号/单引号在现有文件中都存在，跟随所在文件）。
- Markdown 数据流：前端 `Editor.vue` 持有全文 Markdown 字符串（唯一数据源），调用 Rust 命令 `parse_markdown` 得到块树渲染；块编辑是对全文做区间替换后重新解析。**块的 `start`/`end` 是 UTF-16 码元偏移**（pulldown-cmark 的字节偏移已在 Rust 端换算），前端直接 `String.slice(start, end)` 即可，中文不会错位——新增区间相关字段时必须保持同一约定。任务列表的 `markerOffset` 同理，指向 `[ ]`/`[x]` 的 `[`，替换 3 个字符即完成勾选切换。
- 窗口控制相关的 capability 权限必须与前端调用对应：自定义标题栏依赖 `core:window:allow-*` 权限，新增原生调用时同步更新 `src-tauri/capabilities/*.json`。
- `vite.config.js` 中 `clearScreen: false`、固定端口 1420、忽略监听 `src-tauri/` 均为 Tauri 要求，不要随意修改。
- `Cargo.toml` 的 release profile 配置了体积优化（`lto = true`、`codegen-units = 1`、`panic = "abort"`、`strip = true`），勿删除。

## 测试

- Rust：`cd src-tauri && cargo test`，现有解析器集成测试 `tests/markdown_parse.rs`（块模型结构、UTF-16 区间、任务列表标记、表格等）。改动 markdown 模块后必须跑通。
- 前端无测试框架、无 CI 配置，验证方式为手动运行 `pnpm tauri dev` 检查行为；改动前端后至少跑 `pnpm build`。

## 安全注意事项

- `tauri.conf.json` 中 `app.security.csp` 为 `null`（未启用 CSP），发布前应考虑配置。
- 遵循最小权限原则：`capabilities/default.json` 仅授予窗口基础操作权限，新增插件或 core 调用时按需添加，不要整体放开。
- `tauri-plugin-autostart` 当前在 `lib.rs` 的 `setup` 中被 enable 后又立即 disable（模板示例代码），修改时注意不要在用户不知情的情况下注册开机自启。
