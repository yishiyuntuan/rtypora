//! Markdown 模块：块模型定义与基于 pulldown-cmark 的解析器。
//! Rust 端无状态，仅提供「全文 → 块树」的解析命令，文档内容由前端持有。

pub mod model;
pub mod parser;

/// 解析 Markdown 全文，返回块树（JSON）。
/// 每个块带 `start`/`end`（UTF-16 码元偏移），前端可据此切片做逐块编辑。
#[tauri::command]
pub fn parse_markdown(markdown: &str) -> Vec<model::Block> {
    parser::parse(markdown)
}
