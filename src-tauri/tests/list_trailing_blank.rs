//! 列表项区域尾随空行回归：尾随空行不得吞入列表项（末尾空段落保留，用于退出列表后新建块）
use tauri_app_lib::markdown::parse_blocks;

fn kinds(src: &str) -> Vec<String> {
    parse_blocks(src).iter().map(|b| format!("{:?}", b.kind)).collect()
}

#[test]
fn 列表项末尾空行产出空段落() {
    assert_eq!(kinds("- a\n\n"), ["BulletedListItem", "Paragraph"]);
    assert_eq!(kinds("1. a\n\n"), ["NumberedListItem", "Paragraph"]);
    assert_eq!(kinds("- a\n\n\n"), ["BulletedListItem", "Paragraph", "Paragraph"]);
    // 单换行结尾无空段落
    assert_eq!(kinds("- a\n"), ["BulletedListItem"]);
    // 列表中间的既有行为不变
    assert_eq!(kinds("- a\n\n- b"), ["BulletedListItem", "Paragraph", "BulletedListItem"]);
    assert_eq!(kinds("- a\n\ntext"), ["BulletedListItem", "Paragraph"]);
    assert_eq!(kinds("> q\n\n"), ["Quote", "Paragraph"]);
}
