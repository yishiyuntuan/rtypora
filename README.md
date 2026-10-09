# rtypora

使用 **Tauri 2 + Vue 3（Vapor 模式）** 构建的桌面 Markdown 编辑器，Typora 式所见即所得与 Markdown 原文编辑双模式。Markdown 解析/序列化核心移植自 velotype（GPUI 桌面编辑器）的手写逐行解析器——块不存定界符、序列化统一再生，编辑操作只面对文本区间。

## 功能特性

- **双模式编辑**：所见即所得（Typora 式就地编辑，点击块即改）与 Markdown 源码模式，`Ctrl/Cmd + /` 切换且光标位置精确映射
- **完整块模型**：段落/标题/分割线/三种列表/任务列表/引用/callout 警告框/脚注定义/表格/代码块/HTML 注释/HTML 块/section 图文排版/展示数学/Mermaid/原文保留块
- **表格就地编辑**：悬停工具栏、单元格四边「+」快捷插入、尺寸网格、行/列移动与对齐、复制为 Markdown、格式化表格源码
- **代码高亮**：tree-sitter 驱动的 20+ 语言语法高亮（Rust 端计算 span，主题 token 上色），围栏语言自动补全
- **公式渲染**：LaTeX 经 ratex 在 Rust 端渲染为自包含 SVG（内嵌字形，离线可用），支持 `$...$`/`$$...$$`/`\(...\)` 与 physics 包宏，AMS 编号可选
- **图表**：Mermaid（mermaid.js 12，经典 dagre 布局）与 PlantUML（内置 TeaVM 本地引擎离线渲染，或连接渲染服务器）
- **编辑体验**：撤销/重做快照栈、查找替换（CSS Custom Highlight 高亮）、斜杠命令菜单（`/` 任意位置触发）、Markdown 快捷输入、右键格式化菜单、拖放图片、图片右缘拖拽调宽、打字机模式、拼写检查开关
- **文件与导航**：侧边栏文件树（目录排序在 Rust 完成）+ 大纲视图、最近文件/目录、大文件渐进打开（首屏 128KB 截断 + 后台接缝自愈）、虚拟滚动（超大文档秒开）
- **主题系统**：内置 Mdmdt 暗色/亮色主题，支持 JSONC 自定义主题导入与基主题继承、跟随系统外观、编辑器排版覆盖（字号/行高/列宽/字体）
- **导出**：自包含 HTML 导出（主题样式内联、图片转 data URL）
- **自动保存**：默认关闭，可选焦点切换或输入停止 N 秒触发

## 技术栈

| 层 | 技术 |
| --- | --- |
| 前端 | Vue 3.6 RC（**Vapor 模式**，无 VDOM）、Vite 8、Tailwind CSS 4 + daisyUI 5（语义类消费 `--t-*` 主题变量） |
| 后端 | Rust（edition 2024）+ Tauri 2：Markdown 解析/序列化、文件/目录/导出、tree-sitter 高亮、ratex 公式渲染 |
| 图表/公式 | mermaid.js 12、@plantuml/core（TeaVM 引擎，vendored 快照）、ratex |
| 包管理 | pnpm |

架构要点（详见 [AGENTS.md](./AGENTS.md)）：

- **前后端职责划分**：Vue 只负责渲染与 DOM 结构提取；Markdown 解析、序列化、文本修改、统计、快捷判定、文件/导出等逻辑全部由 Rust 命令完成
- **数据流**：前端 `Editor.vue` 持有全文 Markdown（唯一数据源），块编辑 = 区间替换 + `parse_blocks` 增量重解析；块偏移统一为 **UTF-16 码元**（`String.slice` 直接可用，中文不错位）
- **健壮性**：块/行内/HTML 嵌套深度上限 128 层（防栈溢出）、行内前瞻扫描预算（防 O(n²) 病态输入）、HTML 安全分级与 DOM 净化、图片/公式输入大小上限
- **性能**：`markRaw` 块对象（杜绝深度响应代理）、渐进挂载 + 行卸载宽限、行高估计记忆化、大文档统计空闲执行、Mermaid/PlantUML/高亮结果 FIFO 缓存

## 开发

```bash
pnpm install        # 安装前端依赖

pnpm tauri dev      # 开发模式（Vite HMR + Rust 后端）
pnpm tauri build    # 打包发布（前端产物 dist/ + 平台安装包）

pnpm dev            # 仅前端 Vite 开发服务器（固定端口 1420）
pnpm build          # 仅构建前端到 dist/
```

### 测试

```bash
cargo test                  # Rust 集成测试（块模型/UTF-16 区间/往返不动点/深度防护等）
node scripts/test-renumber.mjs   # 有序列表重编号回归脚本
pnpm build                  # 前端构建验证（无前端测试框架）
```

> 提示：深度递归压力测试在 64 MiB 大栈线程上运行（`tests/review_fixes.rs` 的 `big_stack`），macOS 默认线程栈下也能稳定通过。

## 环境要求

- **开发**：Node.js + pnpm、Rust 工具链（1.90+）、Tauri 2 系统依赖（见 [Tauri 官方文档](https://tauri.app/start/prerequisites/)）
- **运行**：mermaid 12 要求 ES2024 运行时——Windows 为常青 WebView2；macOS 14.4+（Safari 17.4 内核）；Linux 为发行版较新的 WebKitGTK

## 目录结构速览

```
src/            # 前端（Vue 3 Vapor SFC，纯 JavaScript）
├── App.vue         # 根组件：四大区域组合、文件操作、自动保存、快捷键
├── component/      # 组件（Editor/BlockView/Sidebar/TitleBar/StatusBar/菜单与对话框等）
├── utils/          # wysiwyg 转换层、高亮/图片/Mermaid/PlantUML 客户端、偏好存取
├── themes/         # 主题系统（注册表 + 内置暗色/亮色主题）
└── assets/         # 全局样式（Tailwind 语义层）与 vendored PlantUML 引擎
src-tauri/      # Rust 后端
├── src/markdown/   # Markdown 核心（块模型/逐行解析器/序列化器/行内引擎，移植自 velotype）
├── src/files.rs    # 文件/目录/导出命令
├── src/highlight.rs# tree-sitter 语法高亮命令
└── src/latex.rs    # LaTeX 公式渲染命令（ratex 管线）
```

## 许可证与致谢

- Markdown 核心移植自 velotype（GPUI 桌面编辑器，Apache-2.0）
- 内置主题配色参考 Typora 主题 [Mdmdt](https://github.com/cayxc/Mdmdt)（Apache-2.0）
- 图表渲染基于 [mermaid.js](https://mermaid.js.org/) 与 [PlantUML](https://plantuml.com/)（@plantuml/core TeaVM 本地引擎）
- 代码高亮基于 [tree-sitter](https://tree-sitter.github.io/)，公式渲染基于 ratex
