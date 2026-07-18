//! markdown 解析器集成测试：验证块模型结构、UTF-16 源码区间与 JSON 序列化。
//! 运行：`cd src-tauri && cargo test`

use tauri_app_lib::markdown::model::{Block, BlockKind, Inline};
use tauri_app_lib::markdown::parser::parse;

/// 按 UTF-16 码元区间取子串（模拟前端 String.slice）。
fn utf16_slice(text: &str, start: usize, end: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16(&units[start..end]).unwrap()
}

/// 块的源码切片。
fn block_source(text: &str, block: &Block) -> String {
    utf16_slice(text, block.start, block.end)
}

#[test]
fn 标题与行内样式() {
    let md = "# 你好 **世界**\n\n正文 *斜体* 与 `代码`、~~删除~~。\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 2);

    match &blocks[0].kind {
        BlockKind::Heading { level, inlines } => {
            assert_eq!(*level, 1);
            assert_eq!(
                *inlines,
                vec![
                    Inline::Text {
                        text: "你好 ".into()
                    },
                    Inline::Bold {
                        children: vec![Inline::Text { text: "世界".into() }]
                    },
                ]
            );
        }
        other => panic!("期望标题，得到 {other:?}"),
    }

    match &blocks[1].kind {
        BlockKind::Paragraph { inlines } => {
            assert!(matches!(&inlines[1], Inline::Italic { .. }));
            assert!(matches!(&inlines[3], Inline::Code { code } if code == "代码"));
            assert!(matches!(&inlines[5], Inline::Strikethrough { .. }));
        }
        other => panic!("期望段落，得到 {other:?}"),
    }
}

#[test]
fn 块区间切片且中文不错位() {
    let md = "你好，世界。\n\n## 二级标题\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 2);
    assert_eq!(block_source(md, &blocks[0]).trim(), "你好，世界。");
    assert_eq!(block_source(md, &blocks[1]).trim(), "## 二级标题");
    // 第二块的起点 = "你好，世界。"（4 个汉字 + 逗号 + 句号 = 6 码元）+ 两个换行
    assert_eq!(blocks[1].start, 8);
}

#[test]
fn 代码块() {
    let md = "```rust\nfn main() {}\n```\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    match &blocks[0].kind {
        BlockKind::CodeBlock { language, code } => {
            assert_eq!(language.as_deref(), Some("rust"));
            assert_eq!(code, "fn main() {}\n");
        }
        other => panic!("期望代码块，得到 {other:?}"),
    }
    assert_eq!(block_source(md, &blocks[0]).trim_end(), md.trim_end());
}

#[test]
fn 任务列表与勾选标记偏移() {
    let md = "- [ ] 待办\n- [x] 完成\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    match &blocks[0].kind {
        BlockKind::List {
            ordered, items, ..
        } => {
            assert!(!ordered);
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].checked, Some(false));
            assert_eq!(items[1].checked, Some(true));
            // marker_offset 指向 `[`，往后 3 个字符即 `[ ]`/`[x]`，可直接替换
            let off0 = items[0].marker_offset.unwrap();
            assert_eq!(utf16_slice(md, off0, off0 + 3), "[ ]");
            let off1 = items[1].marker_offset.unwrap();
            assert_eq!(utf16_slice(md, off1, off1 + 3), "[x]");
        }
        other => panic!("期望列表，得到 {other:?}"),
    }
}

#[test]
fn 嵌套列表不串标记() {
    // 外层普通项内嵌任务列表，外层项不应被误标为任务项
    let md = "- 外层\n  - [x] 内层任务\n- 第二项\n";
    let blocks = parse(md);
    match &blocks[0].kind {
        BlockKind::List { items, .. } => {
            assert_eq!(items[0].checked, None);
            assert_eq!(items[1].checked, None);
            match &items[0].children[1].kind {
                BlockKind::List { items, .. } => {
                    assert_eq!(items[0].checked, Some(true));
                }
                other => panic!("期望嵌套列表，得到 {other:?}"),
            }
        }
        other => panic!("期望列表，得到 {other:?}"),
    }
}

#[test]
fn 表格() {
    let md = "| 名称 | 数量 |\n| :--- | ---: |\n| 苹果 | 3 |\n| 梨 | 5 |\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    match &blocks[0].kind {
        BlockKind::Table {
            alignments,
            head,
            rows,
        } => {
            assert_eq!(alignments, &vec!["left".to_string(), "right".to_string()]);
            assert_eq!(head.len(), 2);
            assert_eq!(rows.len(), 2);
            assert!(matches!(&head[0][0], Inline::Text { text } if text == "名称"));
        }
        other => panic!("期望表格，得到 {other:?}"),
    }
    assert_eq!(block_source(md, &blocks[0]).trim_end(), md.trim_end());
}

#[test]
fn 引用与分割线() {
    let md = "> 引用一段\n>\n> - 列表\n\n---\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 2);
    match &blocks[0].kind {
        BlockKind::BlockQuote { children } => {
            assert!(matches!(children[0].kind, BlockKind::Paragraph { .. }));
            assert!(matches!(children[1].kind, BlockKind::List { .. }));
        }
        other => panic!("期望引用块，得到 {other:?}"),
    }
    assert!(matches!(blocks[1].kind, BlockKind::ThematicBreak));
}

#[test]
fn 链接与图片() {
    let md = "[链接](https://example.com \"标题\") 与 ![图片 **alt**](a.png)\n";
    let blocks = parse(md);
    match &blocks[0].kind {
        BlockKind::Paragraph { inlines } => {
            match &inlines[0] {
                Inline::Link {
                    dest,
                    title,
                    children,
                } => {
                    assert_eq!(dest, "https://example.com");
                    assert_eq!(title, "标题");
                    assert_eq!(
                        *children,
                        vec![Inline::Text {
                            text: "链接".into()
                        }]
                    );
                }
                other => panic!("期望链接，得到 {other:?}"),
            }
            match &inlines[2] {
                Inline::Image { src, alt, .. } => {
                    assert_eq!(src, "a.png");
                    assert_eq!(alt, "图片 alt");
                }
                other => panic!("期望图片，得到 {other:?}"),
            }
        }
        other => panic!("期望段落，得到 {other:?}"),
    }
}

#[test]
fn 列表序列化无键冲突() {
    // 顶层 start 是源码偏移，列表起始序号序列化为 startNumber，二者不得互相覆盖
    let md = "3. 第三\n4. 第四\n";
    let blocks = parse(md);
    let value = serde_json::to_value(&blocks[0]).unwrap();
    assert_eq!(value["type"], "list");
    assert_eq!(value["start"], blocks[0].start as u64);
    assert_eq!(value["startNumber"], 3);
    assert_eq!(value["items"][0]["markerOffset"], serde_json::Value::Null);
    assert_eq!(value["items"][0]["checked"], serde_json::Value::Null);
}

#[test]
fn 序列化为带类型标签的json() {
    let md = "# 标题\n";
    let blocks = parse(md);
    let value = serde_json::to_value(&blocks[0]).unwrap();
    assert_eq!(value["type"], "heading");
    assert_eq!(value["level"], 1);
    assert!(value["id"].is_string());
    assert!(value["start"].is_number());
    assert!(value["end"].is_number());
    assert_eq!(value["inlines"][0]["type"], "text");
    assert_eq!(value["inlines"][0]["text"], "标题");
}
