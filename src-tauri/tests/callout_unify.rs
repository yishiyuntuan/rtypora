//! 警告框统一转换开关测试（独立测试文件避免全局开关与其他并行测试竞争）。

use tauri_app_lib::markdown;
use tauri_app_lib::markdown::model::BlockKindDto;

#[test]
fn 开关关闭时扩展语法按原文保留() {
    markdown::set_callout_unify(false);

    // 别名标记不识别：按普通引用保留原文
    let blocks = markdown::parse_markdown("> [!hint]\n> 内容\n");
    assert!(
        matches!(blocks[0].kind, BlockKindDto::Quote),
        "关闭开关后别名不应解析为 callout: {:?}",
        blocks[0].kind
    );
    let md = markdown::serialize_markdown(blocks);
    assert!(md.contains("[!hint]"), "原文标记应保留: {md}");

    // 容器语法不识别：不解析为 callout
    let blocks = markdown::parse_markdown(":::warning\n内容\n:::\n");
    assert!(
        !blocks.iter().any(|b| matches!(b.kind, BlockKindDto::Callout { .. })),
        "关闭开关后 ::: 容器不应解析为 callout"
    );

    // GitHub 标准标记不受影响（原生格式始终识别）
    let blocks = markdown::parse_markdown("> [!NOTE]\n> 内容\n");
    assert!(
        matches!(blocks[0].kind, BlockKindDto::Callout { .. }),
        "标准标记应始终识别: {:?}",
        blocks[0].kind
    );

    markdown::set_callout_unify(true);
}
