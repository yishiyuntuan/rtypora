//! Markdown 解析器集成测试：验证 velotype 移植版的块模型、UTF-16 区间与往返稳定性。
//! 模拟前端行为：全文持有，`String.slice(start, end)` 用 UTF-16 码元切片。

use tauri_app_lib::markdown;
use tauri_app_lib::markdown::model::{BlockDto, BlockKindDto};

/// 模拟前端 `String.slice(start, end)`（JS 字符串按 UTF-16 码元索引）。
fn utf16_slice(text: &str, start: usize, end: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().skip(start).take(end - start).collect();
    String::from_utf16(&units).unwrap()
}

fn parse(md: &str) -> Vec<BlockDto> {
    markdown::parse_markdown(md)
}

/// 块标题的纯文本（连接所有 fragment）。
fn title_text(block: &BlockDto) -> String {
    block
        .title
        .fragments
        .iter()
        .map(|f| f.text.as_str())
        .collect()
}

#[test]
fn 标题与行内样式() {
    // 注意：velotype 的上下标要求标记两侧为 ASCII 字母数字（词内，Pandoc 风格）。
    let blocks = parse("# 标题 **粗** *斜* `码` ~~删~~ <u>下</u> H^2^O x~i~y");
    assert_eq!(blocks.len(), 1);
    let block = &blocks[0];
    assert!(matches!(block.kind, BlockKindDto::Heading { level: 1 }));

    let fragments = &block.title.fragments;
    let has = |pred: fn(&markdown::inline::tree::InlineStyle) -> bool, text: &str| {
        fragments
            .iter()
            .any(|f| pred(&f.style) && f.text.contains(text))
    };
    assert!(has(|s| s.bold, "粗"), "应有粗体 fragment: {fragments:?}");
    assert!(has(|s| s.italic, "斜"), "应有斜体 fragment");
    assert!(has(|s| s.code, "码"), "应有行内代码 fragment");
    assert!(has(|s| s.strikethrough, "删"), "应有删除线 fragment");
    assert!(has(|s| s.underline, "下"), "应有下划线 fragment");
    assert!(has(|s| s.script == markdown::inline::tree::InlineScript::Superscript, "2"), "应有上标 fragment");
    assert!(has(|s| s.script == markdown::inline::tree::InlineScript::Subscript, "i"), "应有下标 fragment");
}

#[test]
fn 块区间切片且中文不错位() {
    let md = "第一段 中文\n\n第二段 English\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 2);
    for (block, expected) in blocks.iter().zip(["第一段 中文", "第二段 English"]) {
        let (start, end) = (block.start.unwrap(), block.end.unwrap());
        assert_eq!(utf16_slice(md, start, end), expected);
    }
    // 首块起始偏移为 0，次块偏移跳过第一段的换行
    assert_eq!(blocks[0].start, Some(0));
    assert_eq!(blocks[1].start, Some("第一段 中文\n\n".encode_utf16().count()));
}

#[test]
fn 代码块() {
    let md = "```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    let block = &blocks[0];
    match &block.kind {
        BlockKindDto::CodeBlock { language } => assert_eq!(language.as_deref(), Some("rust")),
        other => panic!("应为代码块: {other:?}"),
    }
    assert_eq!(title_text(block), "fn main() {\n    println!(\"hi\");\n}");
    let (start, end) = (block.start.unwrap(), block.end.unwrap());
    assert_eq!(utf16_slice(md, start, end), md.trim_end_matches('\n'));
}

#[test]
fn 任务列表() {
    let md = "- [ ] 待办\n- [x] 已办\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 2, "每个列表项是独立根块");
    assert!(matches!(blocks[0].kind, BlockKindDto::TaskListItem { checked: false }));
    assert!(matches!(blocks[1].kind, BlockKindDto::TaskListItem { checked: true }));
    assert_eq!(title_text(&blocks[0]), "待办");
    // 区间切片可定位 [ ] 标记供前端勾选替换
    let (start, end) = (blocks[0].start.unwrap(), blocks[0].end.unwrap());
    assert_eq!(utf16_slice(md, start, end), "- [ ] 待办");
}

#[test]
fn 列表项嵌套子块() {
    let md = "- 父项\n  - 子项\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0].kind, BlockKindDto::BulletedListItem));
    assert_eq!(blocks[0].children.len(), 1);
    assert!(matches!(blocks[0].children[0].kind, BlockKindDto::BulletedListItem));
    // 嵌套子块不带区间（编辑以根块为单位）
    assert_eq!(blocks[0].children[0].start, None);
}

#[test]
fn 表格() {
    // 注意：velotype 表格分隔行要求至少 3 个连字符（比 GFM 严格）
    let md = "| 名称 | 数量 |\n|:-----|-----:|\n| 苹果 | 3 |\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    let block = &blocks[0];
    assert!(matches!(block.kind, BlockKindDto::Table));
    let table = block.table.as_ref().expect("应有表格数据");
    assert_eq!(table.header.len(), 2);
    assert_eq!(table.rows.len(), 1);
    let (start, end) = (block.start.unwrap(), block.end.unwrap());
    assert_eq!(utf16_slice(md, start, end), md.trim_end_matches('\n'));
}

#[test]
fn 引用与callout() {
    let blocks = parse("> 普通引用\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::Quote));
    assert_eq!(title_text(&blocks[0]), "普通引用");

    let blocks = parse("> [!WARNING] 小心\n> 正文\n");
    match &blocks[0].kind {
        BlockKindDto::Callout { variant } => {
            assert_eq!(*variant, markdown::block::state::CalloutVariant::Warning)
        }
        other => panic!("应为 callout: {other:?}"),
    }
    assert_eq!(title_text(&blocks[0]), "小心");
    assert!(!blocks[0].children.is_empty(), "callout 正文应在 children");
}

#[test]
fn 分割线() {
    let blocks = parse("上文\n\n---\n\n下文\n");
    assert_eq!(blocks.len(), 3);
    assert!(matches!(blocks[1].kind, BlockKindDto::Separator));
}

#[test]
fn 脚注定义与引用() {
    let blocks = parse("正文[^a]\n\n[^a]: 脚注内容\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::Paragraph));
    assert!(
        blocks[0].title.fragments.iter().any(|f| f.footnote.is_some()),
        "应有脚注引用 fragment"
    );
    assert!(matches!(blocks[1].kind, BlockKindDto::FootnoteDefinition));
    assert_eq!(title_text(&blocks[1]), "a");
}

#[test]
fn 数学与mermaid块保留原文() {
    let md = "$$\nx^2 + y^2\n$$\n\n```mermaid\ngraph TD;\n```\n";
    let blocks = parse(md);
    assert!(matches!(blocks[0].kind, BlockKindDto::MathBlock));
    assert_eq!(blocks[0].raw_fallback.as_deref(), Some("$$\nx^2 + y^2\n$$"));
    assert!(matches!(blocks[1].kind, BlockKindDto::MermaidBlock));
    assert_eq!(blocks[1].raw_fallback.as_deref(), Some("```mermaid\ngraph TD;\n```"));
}

#[test]
fn 链接与图片() {
    let blocks = parse("[标签](https://example.com \"标题\") 与 <https://auto.link>\n");
    let fragments = &blocks[0].title.fragments;
    let link = fragments
        .iter()
        .find_map(|f| f.link.as_ref())
        .expect("应有链接 fragment");
    assert_eq!(link.open_target(), "https://example.com");
    assert!(
        fragments
            .iter()
            .any(|f| f.link.as_ref().is_some_and(|l| l.open_target() == "https://auto.link")),
        "应有 autolink fragment"
    );

    let blocks = parse("![替代文本](img/pic.png)\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::Paragraph));
}

#[test]
fn 序列化为带类型标签的json() {
    let blocks = parse("## 二级\n");
    let json = serde_json::to_value(&blocks[0]).unwrap();
    assert_eq!(json["type"], "heading");
    assert_eq!(json["level"], 2);
    assert!(json["id"].is_string());
    assert!(json["start"].is_number());
    assert!(json["end"].is_number());
    assert_eq!(json["title"]["fragments"][0]["text"], "二级");
}

#[test]
fn 序列化往返不动点() {
    let md = "# 标题 *斜体*\n\n- [ ] 任务一\n- [x] 任务二\n\n```rust\nfn main() {}\n```\n\n> [!NOTE] 提示\n> 内容\n\n| a | b |\n|:---:|---:|\n| 1 | 2 |\n\n$$\nx^2\n$$\n\n[^n]: 脚注\n";
    let serialized = markdown::serialize_markdown(parse(md));
    let reparsed = parse(&serialized);
    let serialized2 = markdown::serialize_markdown(reparsed);
    assert_eq!(serialized, serialized2, "序列化应达到不动点:\n{serialized}\n---\n{serialized2}");
}

#[test]
fn 片段解析相对偏移() {
    // parse_blocks 用于编辑提交后的增量重解析：偏移相对片段起点，
    // 结构与全文解析对应区域一致（前端自行加锚点并平移后续块偏移）。
    let fragment = "- [ ] 任务\n\n## 标题\n";
    let blocks = markdown::parse_blocks(fragment);
    assert_eq!(blocks.len(), 2);
    assert!(matches!(blocks[0].kind, BlockKindDto::TaskListItem { checked: false }));
    assert!(matches!(blocks[1].kind, BlockKindDto::Heading { level: 2 }));
    let (s0, e0) = (blocks[0].start.unwrap(), blocks[0].end.unwrap());
    let (s1, e1) = (blocks[1].start.unwrap(), blocks[1].end.unwrap());
    assert_eq!(utf16_slice(fragment, s0, e0), "- [ ] 任务");
    assert_eq!(utf16_slice(fragment, s1, e1), "## 标题");
}

#[test]
fn dto序列化为markdown() {
    // 模拟前端提交路径：DOM 提取出的 BlockDto JSON → Rust serialize_markdown。
    // 定界符、引用前缀、任务标记等全部由 Rust 生成。
    let json = serde_json::json!([
        { "id": "a", "type": "heading", "level": 2,
          "title": { "fragments": [
            { "text": "标题", "style": {} },
            { "text": "粗体", "style": { "bold": true } }
          ] } },
        { "id": "b", "type": "taskListItem", "checked": true,
          "title": { "fragments": [{ "text": "已完成", "style": {} }] } },
        { "id": "c", "type": "quote",
          "title": { "fragments": [{ "text": "首行", "style": {} }] },
          "children": [
            { "id": "c1", "type": "paragraph",
              "title": { "fragments": [{ "text": "嵌套", "style": {} }] } }
          ] },
        { "id": "d", "type": "codeBlock", "language": "rust",
          "title": { "fragments": [{ "text": "fn main() {}", "style": {} }] } }
    ]);
    let blocks: Vec<BlockDto> = serde_json::from_value(json).unwrap();
    let md = markdown::serialize_markdown(blocks);
    assert_eq!(
        md,
        "## 标题**粗体**\n\n- [x] 已完成\n\n> 首行\n> 嵌套\n\n```rust\nfn main() {}\n```",
        "DTO 序列化结果不符:\n{md}"
    );
}

#[test]
fn 任务勾选切换() {
    assert_eq!(markdown::toggle_task_markdown("- [ ] 待办", true), "- [x] 待办");
    assert_eq!(markdown::toggle_task_markdown("- [x] 待办", false), "- [ ] 待办");
    // 无标记时原样返回
    assert_eq!(markdown::toggle_task_markdown("普通段落", true), "普通段落");
}

#[test]
fn 文本统计cjk词数() {
    let stats = markdown::text_stats("hello world 你好\n第二行");
    assert_eq!(stats.words, 2 + 2 + 3, "2 个拉丁词 + 2 个 CJK 字 + 3 个 CJK 字");
    assert_eq!(stats.lines, 2);
    assert_eq!(stats.chars, "hello world 你好\n第二行".encode_utf16().count());
}

#[test]
fn 块快捷输入检测() {
    let hit = markdown::detect_block_shortcut("## 标题").unwrap();
    assert!(matches!(hit.kind, BlockKindDto::Heading { level: 2 }));
    assert_eq!(hit.prefix_len, 3);

    let hit = markdown::detect_block_shortcut("- [x] 任务").unwrap();
    assert!(matches!(hit.kind, BlockKindDto::TaskListItem { checked: true }));

    // fence 与分割线
    let hit = markdown::detect_block_shortcut("```rust").unwrap();
    assert!(matches!(
        hit.kind,
        BlockKindDto::CodeBlock { ref language } if language.as_deref() == Some("rust")
    ));
    assert!(matches!(markdown::detect_block_shortcut("---").unwrap().kind, BlockKindDto::Separator));
    assert!(markdown::detect_block_shortcut("普通文本").is_none());
}

#[test]
fn 行内快捷输入检测() {
    // 粗体：**text** 结尾
    let hit = markdown::inline_shortcut("前缀 **粗体**").unwrap();
    assert_eq!(hit.kind, "bold");
    assert_eq!(hit.text, "粗体");
    assert_eq!(hit.match_len, "**粗体**".encode_utf16().count());

    // 斜体：单 * 结尾，不匹配 ** 包围
    let hit = markdown::inline_shortcut("a *斜*").unwrap();
    assert_eq!(hit.kind, "italic");
    assert_eq!(hit.text, "斜");
    assert!(markdown::inline_shortcut("a **粗**").map(|h| h.kind != "italic").unwrap_or(true));

    // 删除线与行内代码
    assert_eq!(markdown::inline_shortcut("~~删~~").unwrap().kind, "strikethrough");
    assert_eq!(markdown::inline_shortcut("`码`").unwrap().kind, "code");

    // 链接与图片
    let hit = markdown::inline_shortcut("见 [标签](https://a.b)").unwrap();
    assert_eq!(hit.kind, "link");
    assert_eq!(hit.text, "标签");
    assert_eq!(hit.dest.as_deref(), Some("https://a.b"));
    let hit = markdown::inline_shortcut("![alt](img.png)").unwrap();
    assert_eq!(hit.kind, "image");

    // 未闭合、普通文本不命中
    assert!(markdown::inline_shortcut("普通文本").is_none());
    assert!(markdown::inline_shortcut("**未闭合").is_none());
}

#[test]
fn velotype压力集不动点() {
    // velotype 仓库 test.md（702 行语法压力集）：解析不 panic，
    // serialize -> parse -> serialize 逐字稳定（raw_fallback 保证不支持的语法无损）。
    let md = include_str!("fixtures/velotype_stress.md");
    let blocks = parse(md);
    assert!(!blocks.is_empty());

    // 根块区间单调递增且不越界
    let mut last_end = 0;
    for block in &blocks {
        let (start, end) = (block.start.unwrap(), block.end.unwrap());
        assert!(start >= last_end, "块区间应顺序排列: {start} < {last_end}");
        assert!(end > start || start == end, "块区间非法: {start}..{end}");
        last_end = end;
    }
    assert!(last_end <= md.encode_utf16().count());

    let serialized = markdown::serialize_markdown(blocks);
    let serialized2 = markdown::serialize_markdown(parse(&serialized));
    assert_eq!(serialized, serialized2, "压力集序列化未达不动点");
}
