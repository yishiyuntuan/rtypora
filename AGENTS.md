# AGENTS.md

本文件面向 AI 编码代理，介绍本项目的架构、构建方式与开发约定。阅读前无需任何项目背景知识。

## 项目概览

这是一个使用 **Tauri 2 + Vue 3** 构建的桌面 Markdown 编辑器（窗口标题为 `tauri-editor`，应用标识符 `com.apple.tauri-app`）。已实现基于 Block 模型的 Markdown 编辑与渲染：支持所见即所得（Typora 式就地编辑）与 Markdown 原文编辑双模式。**Markdown 解析/序列化核心移植自 velotype**（`C:/Users/apple/RustroverProjects/velotype`，GPUI 桌面编辑器）的手写逐行解析器，替代了原先的 pulldown-cmark 方案。

- 前端界面文案使用中文（如「开始写作...」「目录」「大纲」），Rust 代码注释也以中文为主，提交代码时请保持中文注释习惯。
- 包管理器为 **pnpm**（存在 `pnpm-lock.yaml`）。
- 窗口配置为无边框（`decorations: false`），由前端自定义标题栏接管窗口控制。

## 技术栈

### 前端（`src/`）
- **Vue 3**（`<script setup>` 单文件组件），纯 JavaScript（无 TypeScript、无 JSX）。
- **Vite 8** + `@vitejs/plugin-vue`，`@tailwindcss/vite` 插件接入 **Tailwind CSS 4** 与 **daisyUI 5**（在 `src/assets/style.css` 中通过 `@import "tailwindcss"; @plugin "daisyui";` 启用，无 tailwind.config 文件）。
- `@tauri-apps/api` 用于调用窗口等原生能力；`@tauri-apps/plugin-opener`、`@tauri-apps/plugin-autostart` 为已安装的插件。
- 主题系统（`src/themes/`）：参照 velotype 的主题接口。`index.js` 为注册表（内置主题 + localStorage 自定义主题、基主题继承深合并、JSONC 导入、把主题写入 `documentElement` 的 `--t-*` CSS 变量，并把 `theme.blocks` 按块样式生成 CSS 注入 `#t-block-styles`）；`velotype.js`/`velotype-light.js` 为预置暗色/亮色主题（id 保持 velotype*，配色与版式参考 [Mdmdt](https://github.com/cayxc/Mdmdt)：主色 #3e69d7、标题 2px 字距 + h1 底线、引用蓝底圆角、表格斑马纹，经 `blocks` 实现；两主题另定义 quote_bg/inline_code_bg/link_hover/strikethrough_line/table_zebra_bg 等扩展 token 供 blocks 引用）。`main.js` 在挂载前调用 `initTheme()` 避免闪烁。自定义主题 JSON 格式见 `assets/custom-theme.example.jsonc`。
- 无状态管理库（无 Pinia/Vuex），组件间通过 props / `defineEmits` 事件通信（如 `Editor` 通过 `update:stats`/`update:blocks`/`update:active-heading` 事件向 `App.vue` 上报字数与块树，`App.vue` 经 `defineExpose` 的 `scrollToBlock` 反向调用，`Sidebar` 使用 `v-model:visible`）。

### 后端（`src-tauri/`，Rust）
- `src/main.rs`：入口，仅调用 `tauri_app_lib::run()`。保留 `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`，勿删除。
- `src/lib.rs`：`run()` 中构建 Tauri 应用，已注册 `tauri-plugin-opener`、`tauri-plugin-dialog` 和 `tauri-plugin-autostart`（含 `MacosLauncher::LaunchAgent`，仅桌面平台依赖，见 `Cargo.toml` 的 `[target.'cfg(not(any(target_os = "android", target_os = "ios")))'...]` 段）。Tauri 命令经 `.invoke_handler(tauri::generate_handler![...])` 注册，自有命令无需 capabilities 配置。
- `src/files.rs`：文件操作命令（`open_markdown_file` 弹框选读、`read_markdown_file` 按路径读、`open_markdown_parsed`/`read_markdown_parsed` 读 + 首屏块渐进加载（大文件入口，见「大文件打开优化」；**换行符规范化**——读入统一 CRLF/CR→LF（解析器与 UTF-16 偏移只认 \n），`lineEnding` 字段记录原文档主导风格（CRLF 居多记 crlf），前端保存时按此还原（`App.vue serializeForSave`），粘贴与源码模式入口同样规范化为 LF）、`list_dir` 列目录（`sort`: `"asc"|"desc"` 名称升降序、`"created_asc|created_desc"` 创建时间、`"modified_asc|modified_desc"` 修改时间，文件夹恒在前，排序在 Rust 完成）、`read_image_data_url` 本地图片读为 data URL（导出 HTML 自包含用）、`resolve_image_path` 本地图片解析为存在的绝对路径（编辑器内 `<img>` 走 asset 协议：`tauri.conf.json` 启用 `assetProtocol`（scope `**`）+ Cargo `protocol-asset` feature，前端 `image.js` 经 `convertFileSrc` 出 URL 并缓存——浏览器原生缓存使虚拟滚动重挂载零成本；导出 HTML 时 `assetUrlToDataUrl` 换回 data URL）、`save_file`/`save_file_as`/`save_html_as` 保存与导出（`source_name` 推导建议文件名，.md/.markdown → .html）、`build_export_html`（导出文档模板与 title 转义）、`save_pasted_image` 粘贴图片落盘、`pick_folder` 选择文件夹、`create_markdown_file` 新建 untitled.md）；文件对话框仅经 Rust 端 `tauri-plugin-dialog` 的 `DialogExt` 使用，前端无插件权限需求。
- `src/highlight.rs`：代码块语法高亮，移植自 velotype `code_highlight.rs`（tree-sitter + 20 种官方语法 + yaml/toml）。命令 `highlight_code(language, code)` 返回扁平 span（UTF-16 区间 + 11 个规范类名，与主题 `code_syntax_*` token 后缀一致）；语言未知/纯文本返回空数组。注意 tree-sitter 语法为 C 构建，全量编译耗时明显增加。
- Mermaid 渲染：**前端官方 mermaid.js**（`src/utils/mermaid.js`，动态 import 按需加载、Vite 分包；`securityLevel: 'strict'`；主题随应用明暗（按 `--t-editor-background` 亮度）重新 initialize，BlockView 经 `themeVersion` 触发重渲染；**SVG 结果按 主题|源 缓存（FIFO 120 条）**，滚动重挂载经 `peekMermaid` 同步回填；语法错误返回 null 回退源码占位并清理 mermaid 注入 body 的错误节点）。Rust 端 velotype 移植渲染器（`src/mermaid.rs` + `mermaid-rs-renderer` 依赖）**已随 mermaid.js 切换整体删除**，解析层的 mermaid 围栏识别（`markdown/block/mermaid.rs`）保留。
- PlantUML 渲染：` ```plantuml `/` ```puml ` 围栏在解析层即普通 codeBlock（language 标记），**零解析层改动**；渲染层叠加（`src/utils/plantuml.js`），**两种模式**（偏好 `plantuml_renderer`，默认 local）：①**local 内置引擎**——官方 TeaVM 构建 `@plantuml/core`（`renderToString(lines, onSuccess, onError)`；**注意：Graphviz 布局引擎 viz-global.js 必须经 `?url` 取资源 URL 后以经典 `<script>` 注入加载**——引擎对 `Viz` 是裸引用，ES 模块导入会让打包器包上 CJS exports、UMD 全局失真导致静默失败；引擎 ~8MB 动态 import 分包、首个 PlantUML 块可见才加载，纯 JS 离线渲染无需服务器/Java；SVG 结果 FIFO 缓存，重挂载 `peekPlantuml` 同步回填）；②**server 服务器**——`<img>` 直连 `/svg/<encoded>`（webview 跨域 fetch 会被 CORS 拦截故不用 fetch；`plantuml_server` 可换自部署实例）。失败（离线/语法错误）回退代码显示；偏好开关 `render_plantuml`；语言补全含 plantuml（`code_languages`，徽章 `LANG_BADGES.plantuml`）。
- `src/latex.rs`：LaTeX 公式渲染，移植自 velotype 管线（ratex parser→layout→svg，自包含 SVG 内嵌字形）。命令 `render_display_math`（`$$`/`\[...\]`/````math` 围栏原文）/`render_inline_math`（LaTeX 正文，含 `\(...\)`）返回 SVG；`set_math_unicode_font` 设置公式中文回落字体（主题 `math_cjk_font` 驱动）。**physics 包宏在解析前展开**（`\dv`/`\pdv`（含 `[n]` 阶数）/`\abs`/`\norm`/`\bra`/`\ket`/`\braket`/`\qty`/`\eval`/`\dd`）；非标准宏别名容错（`\part`→`\partial`）；颜色/字号由前端按当前主题传入，缩放 1.25/1.12 与 velotype 一致。展示公式块的 DTO 带 `mathNumbered`（源码是否使用 AMS 编号环境，非星号 align/gather/equation/multline/flalign/alignat/eqnarray），前端按偏好 `math_numbering`（off/ams/all）生成编号序列并显示 `(n)` 徽标。
- `src/markdown/`：Markdown 核心，移植自 velotype（GPUI 依赖已全部剥离）。`mod.rs` 暴露命令：`parse_markdown`（全文 → 块树 JSON）、`parse_blocks`（片段 → 块树 JSON，偏移相对片段起点，用于增量更新）、`serialize_markdown`（块树 JSON → 规范 Markdown，前端提交编辑时调用）、`toggle_task_markdown`（任务勾选标记替换，支持 occurrence 序号）、`text_stats`（行/词/字符统计，CJK 感知词数）、`detect_block_shortcut`（块级快捷输入检测，含 fence、分割线与 `<section>` 行）、`inline_shortcut`（行内快捷输入检测）、`lcp_offsets`（批量序列化对齐公共前缀，Ctrl+/ 光标精确映射一次调用）、`block_template`（斜杠命令原子块模板，表格经 TableData 序列化）、`merge_block_markdown`（块首退格合并接缝规则）。`model.rs` 是对外 DTO（`BlockDto`/`BlockKindDto`，嵌套 children + UTF-16 偏移；**SectionBlock 为 DTO 层分类**——`<section>` 容器因 velotype HTML 安全分级不含该标签而解析为 RawMarkdown，`from_node` 统一归类为 SectionBlock，序列化按 RawMarkdown 原文透传）；`inline/` 为行内引擎（`InlineTextTree` 解析/序列化、链接/HTML/脚注/图片）；`table.rs` 为表格数据与解析；`block/` 为块模型（`state.rs` 的 `BlockRecord`/`BlockKind`）、逐行解析器（`document.rs`，输出 `RootBlock{node,start_line,end_line}`）与序列化器（`tree.rs`）。Rust 端无状态，文档全文由前端持有。
- `Cargo.toml`：`edition = "2024"`（velotype 源码使用 let-chain）；`cssparser`（行内 HTML 样式解析）、`serde`/`serde_json`、`uuid`（v4，生成块 id）已投入使用；`im`（持久化不可变树，用于规划的 undo/redo）尚未使用。
- lib 目标名 `tauri_app_lib`，crate-type 含 `staticlib`/`cdylib`（为移动端预留），`export = ["_*"]` 仅导出 Tauri 必需符号；`test = false`（cargo 的 `rustc-link-arg-tests` 不应用于 lib 单测目标，会缺 Windows 清单，本 crate 测试全部走 tests/ 集成测试）。
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
├── assets/style.css   # Tailwind 4 + daisyUI 入口 + 主题语义层（.t-root/.md-*/.callout-*/.t-statusbar 等规则，消费 --t-* 变量）
├── themes/            # 主题系统（参照 velotype）
│   ├── index.js       # 注册表、基主题继承、CSS 变量注入、localStorage 持久化、JSONC 主题导入
│   ├── velotype.js    # 内置暗色主题（id: velotype，显示名 Mdmdt Dark）
│   └── velotype-light.js # 内置亮色主题（id: velotype-light，显示名 Mdmdt Light，默认）
└── component/         # 注意：目录名是 component 而非 components
    ├── TitleBar.vue   # 自定义标题栏（拖拽区、菜单按钮（打开滑出菜单）、文件名显示、窗口控制）
    ├── MenuDrawer.vue # 一体化菜单（Typora 风格）：左侧深色菜单列 + 右侧内容面板（打开[含最近文件]/导出/主题/偏好设置/关于），动作项直接执行
    ├── PrefsDialog.vue # 偏好设置对话框：编辑器(字号/行高/列宽/内边距)、图像(粘贴行为)、Markdown(渲染开关)、外观
    ├── AboutDialog.vue # 关于对话框
    ├── ConfirmDialog.vue # 未保存更改确认（保存/不保存/取消）
    ├── Sidebar.vue    # 侧边栏：「目录」（文件面板：中部文件树/列表 + 底部文件夹名操作菜单/新建/视图切换，含最近目录）与「大纲」（标题嵌套树 + 树形缩进标记/层级徽标），v-model:visible 控制显隐
    ├── Editor.vue     # 双模式编辑区：持有全文 Markdown（唯一数据源）；源码模式为 textarea，WYSIWYG 模式渲染块树，点击块进入 contenteditable 就地编辑；emit update:blocks/update:active-heading，defineExpose 暴露 scrollToBlock
    ├── BlockView.vue  # 递归块渲染器（17 种块类型：段落/标题/分割线/三种列表项/引用/callout/脚注定义/表格/代码块/注释/HTML/section 图文排版/数学/Mermaid/Raw）
    ├── SlashMenu.vue  # 斜杠命令菜单面板（/ 召唤语法菜单；样式走 --t-* 变量，blocks.slashMenu/slashMenuItemActive 可定制）
    ├── SectionView.vue # section 图文排版块渲染（Mdmdt 式 grid 布局，图片经 Rust 解析为 data URL；render_html_block 偏好可关）
    ├── InlineView.vue # 行内渲染器：按 fragment 样式标志递归包裹（粗/斜/删/下划线/行内代码/上下标/链接/脚注引用/行内公式占位）
    └── StatusBar.vue  # 状态栏，显示 Ln/Col、行/词/字符数，主题选择与导入，emit 切换侧边栏与源码模式
└── utils/
    ├── wysiwyg.js     # 就地编辑转换层：Block→可编辑 HTML、DOM→BlockDto JSON、Markdown 快捷输入（判定全部经 Rust 命令，JS 只做 DOM 替换；类名与 BlockView 保持一致，需同步修改）
    ├── highlight.js   # 高亮客户端：调 Rust highlight_code 取 span 拼 HTML（结果缓存 200 条），ts-* 类名映射 code_syntax_* token
    ├── image.js       # 图片源解析：远程直出，本地经 read_image_data_url 转 data URL（缓存）
    └── prefs.js       # 偏好设置存取（localStorage）：编辑器排版覆盖（主题后应用）、图像粘贴行为、Markdown 渲染开关、prefsVersion 全局版本号、renderVersion（仅 render_* 键递增，公式/Mermaid/高亮/图文排版的 watcher 依赖它，避免非渲染偏好触发全文重渲染）
src-tauri/
├── src/main.rs        # 薄入口
├── src/lib.rs         # Tauri Builder、插件注册、invoke_handler 注册
├── src/markdown/
│   ├── mod.rs         # 模块入口：parse_markdown / serialize_markdown 命令 + 行区间→UTF-16 偏移换算
│   ├── model.rs       # 对外 DTO（BlockDto/BlockKindDto，serde，camelCase，kind 拍平为 type 标签）
│   ├── inline/        # 行内引擎（tree.rs=InlineTextTree、link/html/footnote/image）
│   ├── table.rs       # 表格数据 + 解析/序列化
│   └── block/
│       ├── state.rs   # BlockRecord/BlockKind/CalloutVariant/markdown_line/快捷输入检测
│       ├── document.rs# 逐行解析器：Markdown → BlockNode 树（根块带行区间 RootBlock）
│       ├── tree.rs    # BlockNode 树 → 规范 Markdown 序列化 + 安全代码围栏
│       ├── math.rs    # $$ 展示数学源码解析（纯解析，无渲染管线）
│       └── mermaid.rs # mermaid 围栏识别（纯解析）
├── tests/
│   ├── markdown_parse.rs      # 解析器集成测试（块模型、UTF-16 区间、往返不动点）
│   └── fixtures/velotype_stress.md # velotype 仓库 test.md 语法压力集夹具
├── build.rs           # tauri_build::build() + 测试目标清单资源补链
├── capabilities/
│   ├── default.json   # 主窗口权限：core 默认 + 窗口拖拽/关闭/最小化/最大化等
│   └── desktop.json   # 桌面平台权限：autostart:default
├── tauri.conf.json    # Tauri 配置（见下）
└── Cargo.toml         # Rust 依赖与构建配置
```

## 开发约定

- Vue 组件一律使用 `<script setup>`；样式优先使用 Tailwind 工具类，仅过渡动画等少量样式写在 `<style scoped>`。**颜色/字号不写死**：编辑区组件只用 `style.css` 里的语义类（`.t-root` 作用域元素选择器 + `.md-*`/`.callout-*`/`.t-dim` 等），颜色来自主题系统的 `--t-*` CSS 变量；新增 UI 元素时同步走语义类，不要再引入 `dark:` 或硬编码色值（窗口关闭按钮红色等约定色除外）。`StatusBar.vue` 的 `wysiwyg.js` 类名常量与 `BlockView.vue` 必须保持同步。
- 主题开发：token 名称与 velotype 主题 JSON 一致（snake_case，见 `assets/custom-theme.example.jsonc`）；新增主题 token 时在 `src/themes/index.js` 的变量映射、`style.css` 语义规则与示例文件三处同步。按块自定义样式（`theme.blocks`）作用于组件根元素的 `blk-*` 类（`.blk-paragraph`/`.blk-code-block` 等，见 `index.js` 的 `BLOCK_SELECTORS`），新增块类型时同步选择器映射、BlockView 类名与 wysiwyg.js。
- 前端无 lint / format 配置（无 ESLint、Prettier），保持与现有代码一致的风格即可（双引号/单引号在现有文件中都存在，跟随所在文件）。
- **前后端职责划分**：Vue 只负责渲染与 DOM 结构提取；Markdown 的解析、序列化、文本修改、统计、快捷判定、**语法嗅探**（`looks_like_markdown` 粘贴通道判定）、文件/目录/排序/导出等逻辑全部由 Rust 命令完成。前端不得在 JS 里生成/解析 Markdown 语法（DOM 只提取为 BlockDto JSON 交给 `serialize_markdown`；块模板经 `block_template` 含参数生成，不做字符串拼接）。**允许的例外**（均为视图层职责）：contenteditable 编辑态的 DOM 操作与光标管理（列表 Enter 续项/Tab 缩进/退格合并等，提交时经 Rust 序列化落源）、DOM→DTO 提取、调用 Rust 命令前的首字符预过滤（仅性能闸门，判定仍在 Rust）、展示格式化（路径拆分、列表序号、主题过滤等纯显示计算）。**编辑态交互控件必须带 `contenteditable="false"`**（任务勾选框、代码块语言输入框等，否则 Chromium 下键入由编辑宿主接管、不产生 input 事件）。
- Markdown 数据流：前端 `Editor.vue` 持有全文 Markdown 字符串（唯一数据源），调用 Rust 命令 `parse_markdown` 得到块树渲染；块编辑是对全文做区间替换。**提交后增量更新**（与 velotype 一致）：只把被编辑片段发给 `parse_blocks` 重解析，原位 splice 替换旧块并按长度差平移后续块偏移（`shiftBlockOffsets`），未变化的块 id 稳定、DOM 复用——不再整树重解析（全文重解析仅在切回 WYSIWYG 模式或增量失败回退时发生）。**块的 `start`/`end` 是 UTF-16 码元偏移**（行区间在 Rust 端换算），前端直接 `String.slice(start, end)` 即可，中文不会错位——新增区间相关字段时必须保持同一约定。**仅根块带偏移**（嵌套子块 `start`/`end` 为 null），编辑以根块为单位；任务列表勾选 = `toggle_task_markdown` 替换标记后同样走增量重解析（标记判定要求列表标记前缀，代码内同形文本不计入）。**引用式链接/图片**（[text][label] / ![alt][label]）：`parse_to_dtos` 先全文扫描 `[label]: url` 定义再在线程本地作用域内解析（`inline/mod.rs with_parse_reference_definitions`）；片段增量解析只见片段内定义（文档级定义在片段外时按字面量保留，全文重解析恢复）。**嵌套深度上限**（防栈溢出）：块级引用/列表/警告框 128 层（`document.rs MAX_NESTING_DEPTH`，超限降级 raw）、行内分隔符/HTML 容器 128 层（`tree.rs MAX_INLINE_NESTING_DEPTH`，超限按未闭合处理）。编辑/解析/统计走 `*_async` 命令（异步运行时，离开主线程）：`parse_blocks_async`/`parse_markdown_async`/`text_stats_async`。
- 大文件打开优化（四项联动）：①**渐进解析**——`open_markdown_parsed`/`read_markdown_parsed` 一次 IPC 返回全文 + 首屏块（>128KB 文档按 `safe_prefix_end` 在「非围栏空行行首」截断，保证截断点不产生空段落伪块；`tailFrom` = 首屏末块起点，前端后台 `parse_blocks` 重解析尾部并原位替换末块自愈接缝；尾部返回前若已编辑——`content` 字符串引用变化——丢弃尾部走全量重解析）；②**块对象 `markRaw`**（`Editor.vue rawBlocks`，所有解析入口注入点统一包裹，杜绝深度响应代理；块树只按数组变更驱动更新，不依赖字段级响应）；③**渐进挂载**——首屏只创建 160 个 VRow 组件，配额外行以估计高度占位条（`tailPadHeight`）保总高，空闲帧按 500 步长扩配额；`scrollToBlock` 先扩配额再重试滚动；**行卸载有 3 秒宽限（VRow 延迟卸载），快速来回滚动不重建内容**（图片/Mermaid 闪烁的主要对策）；④**统计延迟**——`text_stats` 大文档（>512KB）经 `requestIdleCallback` 空闲执行；⑤**编辑提交链路瘦身**——拆分/粘贴的两次 `serialize_markdown` 并行（`Promise.all`）、重解析合并为一次 `parse_blocks`（Enter 提交从 4 次串行 IPC 减到 3 次且 2 次并行）；**行高估计按块 id 记忆化**（`estimateCache`，编辑提交不再 O(全文字符) 全量重扫，主题切换时清空）。
- 编辑交互：**普通 Enter 在光标处拆分块**（光标前内容提交渲染，光标后进入新块编辑、光标在开头）；**围栏行（```lang）按 Enter 转换为代码块并进入块内编辑**（不拆分）；`Ctrl/Cmd+Enter` 整块提交（在代码块/Mermaid 等 pre 内为整块提交并在其后新建空段落进入编辑，其余块为提交并退出编辑）；`Shift+Enter` 软换行；`Escape` 取消；失焦提交。代码块/Mermaid 等 pre（含 data-raw 原子块）内 Enter 插入换行符。语言补全覆盖两处：围栏行输入（``` 后）与代码块右上角语言输入框（同一菜单组件，应用时写回输入框并同步 `pre[data-language]`）。列表项退出后的新块依赖解析器保留列表尾随空行（`collect_list_item_region` 末尾不吞 pending_blank_lines——吞掉会导致 `- a\n\n` 末尾空段落丢失、退出列表后 Enter 焦点跳回列表开头）。
- velotype 模型差异要点：列表项是独立块（嵌套经 `children`，无 List 容器）；块不存储定界符，序列化时统一再生；不支持的语法以 `rawFallback` 原文保留可无损往返；上下标 `^x^`/`~x~` 要求标记两侧为 ASCII 字母数字（词内）；表格分隔行要求至少 3 个连字符；下划线用 `<u>`；原子块（表格/公式/Mermaid/HTML/注释/raw）在前端按原始 Markdown 切片编辑。
- 编辑交互补充：**callout 为富文本编辑**（`blockToHtml` 生成 `data-variant` + `contenteditable=false` 变体标签 + 标题/子块，`elementToBlocks` 按 `.blk-callout` 识别并剔除标签还原 DTO；**双击变体标签**经 `convertCalloutToRawEdit` 序列化当前内容转原文编辑以修改 `[!TYPE]`）；**无焦点框选的右键格式化**：非编辑态选区按「块内纯文本偏移」暂存（跳过 `.md-marker`/`.md-code-lang-badge`/`.md-callout-label` 非内容文本），右键直入编辑、挂载即重建高亮（`stashCtxSelection`/`rangeFromTextOffsets`/`restoreCtxSelectionOnMount`）；**点击进入编辑的守卫只拦跨块选区**（`selectionSpansMultipleBlocks`，同块选区点击即塌陷并进入编辑）；**有序列表中间插入/删除项后源标记联动重编号**（`renumberOrderedMarkers`，编号规则与 `numberedOrdinals`/Rust 序列化器逐字一致，仅编辑流调用、加载路径不调）；**容器块首行空占位段可 Backspace 删除**（仅删空行不触发块级降级）。
- 窗口控制相关的 capability 权限必须与前端调用对应：自定义标题栏依赖 `core:window:allow-*` 权限，新增原生调用时同步更新 `src-tauri/capabilities/*.json`。
- `vite.config.js` 中 `clearScreen: false`、固定端口 1420、忽略监听 `src-tauri/` 均为 Tauri 要求，不要随意修改。
- `Cargo.toml` 的 release profile 配置了体积优化（`lto = true`、`codegen-units = 1`、`panic = "abort"`、`strip = true`），勿删除。

## 测试

- Rust：`cd src-tauri && cargo test`，现有解析器集成测试 `tests/markdown_parse.rs`（块模型结构、UTF-16 区间、任务列表、表格、callout、脚注、raw 保留、往返不动点 + velotype 压力集夹具）。改动 markdown 模块后必须跑通。
- 前端无测试框架、无 CI 配置，验证方式为手动运行 `pnpm tauri dev` 检查行为；改动前端后至少跑 `pnpm build`。

## 安全注意事项

- `tauri.conf.json` 中 `app.security.csp` 为 `null`（未启用 CSP），发布前应考虑配置。
- 遵循最小权限原则：`capabilities/default.json` 仅授予窗口基础操作权限，新增插件或 core 调用时按需添加，不要整体放开。
- `tauri-plugin-autostart` 当前在 `lib.rs` 的 `setup` 中被 enable 后又立即 disable（模板示例代码），修改时注意不要在用户不知情的情况下注册开机自启。
