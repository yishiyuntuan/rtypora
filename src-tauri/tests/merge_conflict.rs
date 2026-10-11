//! Git 合并冲突块（conflictBlock）解析/序列化集成测试。
//!
//! DTO 契约：根块 `type: "conflictBlock"`，顶层 `conflict` 字段
//! `{ currentLabel, current, incomingLabel, incoming }`（camelCase）；
//! `current`/`incoming` 为各侧原始行文本（不含标记行），保留内部换行、
//! 末尾换行剔除，空侧为空串；label 为标记行剩余文本（trim 后），无则 null。
//! 序列化精确还原：`<<<<<<<[ label]`、当前侧原文（空侧标记行相邻）、
//! `=======`、传入侧原文、`>>>>>>>[ label]`；块 start/end 覆盖全部标记行。

use tauri_app_lib::markdown;
use tauri_app_lib::markdown::model::{BlockDto, BlockKindDto};

fn parse(md: &str) -> Vec<BlockDto> {
    markdown::parse_markdown(md)
}

fn roundtrip(md: &str) -> String {
    markdown::serialize_markdown(parse(md))
}

/// 模拟前端 `String.slice(start, end)`（JS 字符串按 UTF-16 码元索引）。
fn utf16_slice(text: &str, start: usize, end: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().skip(start).take(end - start).collect();
    String::from_utf16(&units).unwrap()
}

fn is_conflict(block: &BlockDto) -> bool {
    matches!(block.kind, BlockKindDto::ConflictBlock)
}

#[test]
fn 基本解析_双标签() {
    let md = "<<<<<<< HEAD\n当前行\n=======\n传入行\n>>>>>>> feature";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    let block = &blocks[0];
    assert!(is_conflict(block));

    let conflict = block.conflict.as_ref().expect("conflictBlock 必带 conflict");
    assert_eq!(conflict.current_label.as_deref(), Some("HEAD"));
    assert_eq!(conflict.current, "当前行");
    assert_eq!(conflict.incoming_label.as_deref(), Some("feature"));
    assert_eq!(conflict.incoming, "传入行");

    // 块区间覆盖全部标记行（含终止行），与全文等长
    assert_eq!(block.start, Some(0));
    assert_eq!(block.end, Some(md.encode_utf16().count()));
    assert_eq!(utf16_slice(md, block.start.unwrap(), block.end.unwrap()), md);
}

#[test]
fn 基本解析_无标签() {
    let md = "<<<<<<<\n甲\n=======\n乙\n>>>>>>>";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    let conflict = blocks[0].conflict.as_ref().unwrap();
    assert_eq!(conflict.current_label, None);
    assert_eq!(conflict.incoming_label, None);
    assert_eq!(conflict.current, "甲");
    assert_eq!(conflict.incoming, "乙");
}

#[test]
fn 基本解析_空侧() {
    // 当前侧为空：标记行相邻
    let blocks = parse("<<<<<<< HEAD\n=======\n传入\n>>>>>>> f");
    assert_eq!(blocks.len(), 1);
    let conflict = blocks[0].conflict.as_ref().unwrap();
    assert_eq!(conflict.current, "");
    assert_eq!(conflict.incoming, "传入");

    // 传入侧为空
    let blocks = parse("<<<<<<< HEAD\n当前\n=======\n>>>>>>> f");
    let conflict = blocks[0].conflict.as_ref().unwrap();
    assert_eq!(conflict.current, "当前");
    assert_eq!(conflict.incoming, "");

    // 两侧皆空
    let blocks = parse("<<<<<<<\n=======\n>>>>>>>");
    let conflict = blocks[0].conflict.as_ref().unwrap();
    assert_eq!(conflict.current, "");
    assert_eq!(conflict.incoming, "");
}

#[test]
fn 侧文本保留内部换行() {
    let md = "<<<<<<< HEAD\n甲一\n甲二\n\n甲四\n=======\n乙一\n乙二\n>>>>>>> feature";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    let conflict = blocks[0].conflict.as_ref().unwrap();
    assert_eq!(conflict.current, "甲一\n甲二\n\n甲四");
    assert_eq!(conflict.incoming, "乙一\n乙二");
    assert_eq!(roundtrip(md), md);
}

#[test]
fn 往返不动点() {
    let cases = [
        "<<<<<<< HEAD\n当前行\n=======\n传入行\n>>>>>>> feature",
        "<<<<<<<\n甲\n=======\n乙\n>>>>>>>",
        "<<<<<<< HEAD\n=======\n传入\n>>>>>>> f",
        "<<<<<<< HEAD\n当前\n=======\n>>>>>>> f",
        "<<<<<<<\n=======\n>>>>>>>",
        "<<<<<<< my branch\n甲\n=======\n乙\n>>>>>>> their branch",
        // 含行内格式字符的侧文本按原文保留
        "<<<<<<< HEAD\n**粗** `码`\n=======\n[链](https://a.b)\n>>>>>>> f",
    ];
    for md in cases {
        assert_eq!(roundtrip(md), md, "往返必须逐字节一致: {md:?}");
    }
}

#[test]
fn 未闭合降级为段落() {
    // 无 ======= 也无 >>>>>>>：整体普通段落，往返一致
    let md = "<<<<<<< HEAD\n甲\n乙";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0].kind, BlockKindDto::Paragraph));
    assert!(blocks[0].conflict.is_none());
    assert_eq!(roundtrip(md), md);

    // 有 ======= 无 >>>>>>>：不得成为冲突块（setext 等常规 Markdown 语义照常生效）
    let md = "<<<<<<< HEAD\n甲\n=======\n乙";
    let blocks = parse(md);
    assert!(blocks.iter().all(|b| !is_conflict(b)));

    // ======= 带文字不算分界（顶格 7 个 = 且仅空白结尾才是分界）：未闭合
    let md = "<<<<<<< HEAD\n======= 附带\n乙\n>>>>>>> f";
    let blocks = parse(md);
    assert!(blocks.iter().all(|b| !is_conflict(b)));
}

#[test]
fn 基线标记整体降级() {
    // diff3 的 ||||||| 基线标记不支持：出现即不是冲突块
    let md = "<<<<<<< HEAD\n甲\n||||||| base\n基\n=======\n乙\n>>>>>>> f";
    let blocks = parse(md);
    assert!(blocks.iter().all(|b| !is_conflict(b)));
}

#[test]
fn 围栏代码块内不触发() {
    let md = "```text\n<<<<<<< HEAD\n=======\n>>>>>>>\n```";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0].kind, BlockKindDto::CodeBlock { .. }));
    assert_eq!(roundtrip(md), md);
}

#[test]
fn 缩进代码块内不触发() {
    // 缩进代码块序列化时按现有规则规范化为围栏形式（与冲突解析无关的既有行为），
    // 此处只断言不触发冲突块、且代码内容逐行保留。
    let md = "    <<<<<<< HEAD\n    =======\n    >>>>>>>";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0].kind, BlockKindDto::CodeBlock { .. }));
    let code: String = blocks[0]
        .title
        .fragments
        .iter()
        .map(|f| f.text.as_str())
        .collect();
    assert_eq!(code, "<<<<<<< HEAD\n=======\n>>>>>>>");
}

#[test]
fn 注释块内不触发() {
    let md = "<!--\n<<<<<<< HEAD\n=======\n>>>>>>>\n-->";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0].kind, BlockKindDto::Comment));
    assert_eq!(roundtrip(md), md);
}

#[test]
fn 非顶格标记不触发() {
    //  opener 前导空格：不构成冲突块（v1 仅顶格根级）
    let md = "  <<<<<<< HEAD\n甲\n=======\n乙\n>>>>>>> f";
    let blocks = parse(md);
    assert!(blocks.iter().all(|b| !is_conflict(b)));
}

#[test]
fn 引用块内不解析() {
    let md = "> <<<<<<< HEAD\n> 甲\n> =======\n> 乙\n> >>>>>>> f";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0].kind, BlockKindDto::Quote));
    fn any_conflict(block: &BlockDto) -> bool {
        is_conflict(block) || block.children.iter().any(any_conflict)
    }
    assert!(!blocks.iter().any(any_conflict));
}

#[test]
fn 前后段落偏移正确() {
    let conflict_md = "<<<<<<< HEAD\n甲\n=======\n乙\n>>>>>>> f";
    let md = format!("前文\n\n{conflict_md}\n\n后文");
    let blocks = parse(&md);
    assert_eq!(blocks.len(), 3);
    assert!(matches!(blocks[0].kind, BlockKindDto::Paragraph));
    assert!(is_conflict(&blocks[1]));
    assert!(matches!(blocks[2].kind, BlockKindDto::Paragraph));

    // 各块区间切片必须精确等于对应源码片段（UTF-16 码元）
    let slice = |block: &BlockDto| utf16_slice(&md, block.start.unwrap(), block.end.unwrap());
    assert_eq!(slice(&blocks[0]), "前文");
    assert_eq!(slice(&blocks[1]), conflict_md);
    assert_eq!(slice(&blocks[2]), "后文");
    assert_eq!(roundtrip(&md), md);
}

#[test]
fn 多冲突块共存() {
    let first = "<<<<<<< HEAD\n甲\n=======\n乙\n>>>>>>> f1";
    let second = "<<<<<<<\n丙\n=======\n丁\n>>>>>>> f2";
    let md = format!("{first}\n\n中段\n\n{second}");
    let blocks = parse(&md);
    assert_eq!(blocks.len(), 3);
    assert!(is_conflict(&blocks[0]));
    assert!(matches!(blocks[1].kind, BlockKindDto::Paragraph));
    assert!(is_conflict(&blocks[2]));

    let c0 = blocks[0].conflict.as_ref().unwrap();
    let c2 = blocks[2].conflict.as_ref().unwrap();
    assert_eq!((c0.current.as_str(), c0.incoming.as_str()), ("甲", "乙"));
    assert_eq!((c2.current.as_str(), c2.incoming.as_str()), ("丙", "丁"));
    assert_eq!(c2.current_label, None);
    assert_eq!(roundtrip(&md), md);
}

#[test]
fn dto_json_形状() {
    let blocks = parse("<<<<<<< HEAD\n当前行\n=======\n传入行\n>>>>>>> feature");
    let value = serde_json::to_value(&blocks[0]).unwrap();
    assert_eq!(value["type"], "conflictBlock");
    assert_eq!(
        value["conflict"],
        serde_json::json!({
            "currentLabel": "HEAD",
            "current": "当前行",
            "incomingLabel": "feature",
            "incoming": "传入行",
        })
    );

    // 无标签时 label 显式为 null（契约要求）
    let blocks = parse("<<<<<<<\n甲\n=======\n乙\n>>>>>>>");
    let value = serde_json::to_value(&blocks[0]).unwrap();
    assert_eq!(value["conflict"]["currentLabel"], serde_json::json!(null));
    assert_eq!(value["conflict"]["incomingLabel"], serde_json::json!(null));

    // 非冲突块不携带 conflict 键（skip_serializing_if）
    let blocks = parse("普通段落");
    let value = serde_json::to_value(&blocks[0]).unwrap();
    assert!(value.get("conflict").is_none());
}

#[test]
fn dto还原序列化_经into_node路径() {
    // serialize_markdown 走 BlockDto::into_node：conflict 载荷必须随 DTO 还原
    let md = "<<<<<<< HEAD\n当前行\n=======\n传入行\n>>>>>>> feature";
    let dtos = parse(md);
    let json = serde_json::to_string(&dtos).unwrap();
    let restored: Vec<BlockDto> = serde_json::from_str(&json).unwrap();
    assert_eq!(markdown::serialize_markdown(restored), md);
}
