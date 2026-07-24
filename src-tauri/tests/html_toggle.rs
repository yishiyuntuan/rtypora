//! HTML→Markdown 开关测试（独立测试文件避免全局开关与其他并行测试竞争）。

use tauri_app_lib::markdown;
use tauri_app_lib::markdown::model::BlockKindDto;

#[test]
fn 开关关闭时容器标签按原文保留() {
    markdown::set_html_to_md(false);
    let blocks = markdown::parse_markdown("<h2>标题</h2>\n");
    markdown::set_html_to_md(true);
    assert!(
        !matches!(blocks[0].kind, BlockKindDto::Paragraph),
        "关闭开关后 h2 容器不应转换为标题/段落: {:?}",
        blocks[0].kind
    );
}
