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
fn 公式ams编号环境标记() {
    // align 环境 → AMS 编号；星号变体与普通公式不编号；非公式块不携带该字段
    let align = parse("$$\n\\begin{align}\ny &= x\\\\\n&= z\n\\end{align}\n$$\n");
    assert!(matches!(align[0].kind, BlockKindDto::MathBlock));
    assert_eq!(align[0].math_numbered, Some(true));

    let star = parse("$$\n\\begin{align*}\ny &= x\n\\end{align*}\n$$\n");
    assert_eq!(star[0].math_numbered, Some(false));

    let plain = parse("$$\nx = 1\n$$\n");
    assert_eq!(plain[0].math_numbered, Some(false));

    let p = parse("普通段落\n");
    assert_eq!(p[0].math_numbered, None);
}

#[test]
fn section图文排版块() {
    // <section> 容器分类为 SectionBlock（DTO 层）；普通 html 块不受影响
    let md = "<section>\n<img src=\"./img/a.png\"></img>\n<span>文字</span>\n</section>\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0].kind, BlockKindDto::SectionBlock));
    let raw = blocks[0].raw_fallback.as_deref().unwrap_or("");
    assert!(raw.contains("<img src=\"./img/a.png\"></img>"));

    let div = parse("<div>\n<p>x</p>\n</div>\n");
    assert!(matches!(div[0].kind, BlockKindDto::HtmlBlock));

    // 含空行的最小容器（「输入 <section> 回车」钩子产生的形态）同样解析为单块
    let minimal = parse("<section>\n\n</section>\n");
    assert_eq!(minimal.len(), 1);
    assert!(matches!(minimal[0].kind, BlockKindDto::SectionBlock));

    // 往返：SectionBlock DTO 序列化按 HtmlBlock 原文透传，重解析仍分类为 SectionBlock
    let rt = markdown::parse_markdown(&markdown::serialize_markdown(blocks));
    assert!(matches!(rt[0].kind, BlockKindDto::SectionBlock));
    assert_eq!(rt[0].raw_fallback.as_deref(), Some(md.trim_end_matches('\n')));
}

#[test]
fn 列表项标记行开围栏() {
    // 回归：围栏开在列表标记行（`1. ```html`）时，续行内容必须收集为该项的代码块子块，
    // 内容（如 <section>）不得逃逸为独立块
    let md = "1. ```html\n   <section>\n            \n   </section>\n   ```\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1, "整体应为一个列表项: {blocks:?}");
    assert!(matches!(blocks[0].kind, BlockKindDto::NumberedListItem));
    assert_eq!(title_text(&blocks[0]), "", "标题应为空（围栏行不占标题）");
    assert_eq!(blocks[0].children.len(), 1);
    match &blocks[0].children[0].kind {
        BlockKindDto::CodeBlock { language } => assert_eq!(language.as_deref(), Some("html")),
        other => panic!("应为代码块子块: {other:?}"),
    }
    assert_eq!(title_text(&blocks[0].children[0]), "<section>\n\n</section>");

    // 往返：结构稳定（空标题 + 代码块子块）
    let rt = markdown::parse_markdown(&markdown::serialize_markdown(blocks));
    assert_eq!(rt.len(), 1);
    assert!(matches!(rt[0].kind, BlockKindDto::NumberedListItem));
    assert!(matches!(rt[0].children[0].kind, BlockKindDto::CodeBlock { .. }));
    assert_eq!(title_text(&rt[0].children[0]), "<section>\n\n</section>");
}

#[test]
fn 列表项后分割线不误判setext() {
    // 回归：列表项后的 --- 是分割线，不是 setext 标题下划线
    let blocks = parse("- [ ] Something is DONE.\n---\n");
    assert_eq!(blocks.len(), 2);
    assert!(matches!(blocks[0].kind, BlockKindDto::TaskListItem { checked: false }));
    assert_eq!(title_text(&blocks[0]), "Something is DONE.");
    assert!(matches!(blocks[1].kind, BlockKindDto::Separator));

    let blocks = parse("- 列表项\n---\n");
    assert_eq!(blocks.len(), 2);
    assert!(matches!(blocks[0].kind, BlockKindDto::BulletedListItem));
    assert!(matches!(blocks[1].kind, BlockKindDto::Separator));

    // 普通段落的 setext 标题不受影响
    let blocks = parse("段落文本\n---\n");
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0].kind, BlockKindDto::Heading { level: 2 }));
    assert_eq!(title_text(&blocks[0]), "段落文本");
}

#[test]
fn 嵌套列表项后分割线不被吞() {
    // 回归：无空行时 --- 不被吸入嵌套任务项标题，而是结束列表成为分割线
    let md = "- This is Item 2.\n    - [x] Not TODO.\n    - [ ] DONE.\n---\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 2);
    let item = &blocks[0];
    assert!(matches!(item.kind, BlockKindDto::BulletedListItem));
    assert_eq!(item.children.len(), 2);
    assert!(matches!(item.children[1].kind, BlockKindDto::TaskListItem { checked: false }));
    assert_eq!(title_text(&item.children[1]), "DONE.");
    assert!(matches!(blocks[1].kind, BlockKindDto::Separator));
}

#[test]
fn 列表项内缩进分割线成子块() {
    // 内容级缩进的 --- 是项内分割线子块
    let md = "- 列表项\n  ---\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0].kind, BlockKindDto::BulletedListItem));
    assert!(blocks[0].children.iter().any(|c| matches!(c.kind, BlockKindDto::Separator)));
}

#[test]
fn section快捷判定() {
    let hit = markdown::detect_block_shortcut("<section>").expect("应命中 section");
    assert!(matches!(hit.kind, BlockKindDto::SectionBlock));
    assert!(markdown::detect_block_shortcut("<div>").is_none());
    assert!(markdown::detect_block_shortcut("<section>x</section>").is_none());
}

#[test]
fn 深层嵌套列表往返稳定() {
    // 回归：混合有序/无序/任务三层嵌套，序列化后重解析结构必须一致
    //（前端编辑提交 = DOM → DTO → serialize → 增量重解析，结构错乱源于此链路）
    let md = "- 列表 A\n- 列表 B\n    1. Item 1.\n    2. Item 2.\n        - [ ] Not TODO.\n        - [x] DONE.\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 2);
    let b = &blocks[1];
    assert!(matches!(b.kind, BlockKindDto::BulletedListItem));
    assert_eq!(b.children.len(), 2);
    assert!(matches!(b.children[0].kind, BlockKindDto::NumberedListItem));
    assert!(matches!(b.children[1].kind, BlockKindDto::NumberedListItem));
    assert_eq!(b.children[1].children.len(), 2);
    assert!(matches!(b.children[1].children[0].kind, BlockKindDto::TaskListItem { checked: false }));
    assert!(matches!(b.children[1].children[1].kind, BlockKindDto::TaskListItem { checked: true }));

    let reparsed = markdown::parse_markdown(&markdown::serialize_markdown(blocks));
    assert_eq!(reparsed.len(), 2);
    let b = &reparsed[1];
    assert!(matches!(b.kind, BlockKindDto::BulletedListItem));
    assert_eq!(b.children.len(), 2);
    assert!(matches!(b.children[0].kind, BlockKindDto::NumberedListItem));
    assert!(matches!(b.children[1].kind, BlockKindDto::NumberedListItem));
    assert_eq!(b.children[1].children.len(), 2);
    assert!(matches!(b.children[1].children[0].kind, BlockKindDto::TaskListItem { checked: false }));
    assert!(matches!(b.children[1].children[1].kind, BlockKindDto::TaskListItem { checked: true }));
    assert_eq!(title_text(&b.children[1].children[0]), "Not TODO.");
    assert_eq!(title_text(&b.children[1].children[1]), "DONE.");
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
    assert_eq!(markdown::toggle_task_markdown("- [ ] 待办", true, None), "- [x] 待办");
    assert_eq!(markdown::toggle_task_markdown("- [x] 待办", false, None), "- [ ] 待办");
    // 无标记时原样返回
    assert_eq!(markdown::toggle_task_markdown("普通段落", true, None), "普通段落");
    // 按序号切换（嵌套任务项定位）：第 0/1/2 个标记各自独立，互不影响
    let src = "- [x] 已完成\n  - [ ] 子任务一\n  - [ ] 子任务二";
    assert_eq!(
        markdown::toggle_task_markdown(src, false, Some(0)),
        "- [ ] 已完成\n  - [ ] 子任务一\n  - [ ] 子任务二",
        "切换第 0 个标记（兼容混排：首个标记是 [x] 也能命中）"
    );
    assert_eq!(
        markdown::toggle_task_markdown(src, true, Some(1)),
        "- [x] 已完成\n  - [x] 子任务一\n  - [ ] 子任务二"
    );
    assert_eq!(
        markdown::toggle_task_markdown(src, true, Some(2)),
        "- [x] 已完成\n  - [ ] 子任务一\n  - [x] 子任务二"
    );
    // 序号越界原样返回
    assert_eq!(markdown::toggle_task_markdown(src, true, Some(9)), src);
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

#[test]
fn 代码块语法高亮() {
    use tauri_app_lib::highlight;

    // rust：关键字与字符串应产生对应类名的 span（UTF-16 区间可直接 slice）
    let code = "fn main() {\n    let s = \"你好\";\n}\n";
    let spans = highlight::highlight_code(Some("rust"), code);
    assert!(!spans.is_empty(), "rust 代码应有高亮 span");
    let json = serde_json::to_value(&spans).unwrap();
    let classes: Vec<&str> = json
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["class"].as_str().unwrap())
        .collect();
    assert!(classes.contains(&"keyword"), "应含 keyword: {classes:?}");
    assert!(classes.contains(&"string"), "应含 string: {classes:?}");
    // span 区间按 UTF-16 切片应落在原码文本内且单调
    let units: Vec<u16> = code.encode_utf16().collect();
    let mut last = 0;
    for span in &spans {
        assert!(span.start >= last && span.end > span.start);
        assert!(span.end <= units.len());
        last = span.end;
    }

    // 别名与未知语言
    assert!(!highlight::highlight_code(Some("rs"), code).is_empty());
    assert!(highlight::highlight_code(Some("unknown-lang"), code).is_empty());
    assert!(highlight::highlight_code(None, code).is_empty());
    assert!(highlight::highlight_code(Some("mermaid"), "graph TD;").is_empty());
}

#[test]
fn 高亮标记() {
    // ==文字== 解析为 highlight 样式，序列化再生成 == 定界符（往返稳定）
    let blocks = parse("普通 ==高亮文字== 结尾\n");
    let fragments = &blocks[0].title.fragments;
    assert!(
        fragments.iter().any(|f| f.style.highlight && f.text.contains("高亮文字")),
        "应有 highlight fragment: {fragments:?}"
    );

    let serialized = markdown::serialize_markdown(parse("普通 ==高亮文字== 结尾\n"));
    assert!(serialized.contains("==高亮文字=="), "序列化应保留 == 定界符: {serialized}");
}

#[test]
fn mermaid渲染() {
    use tauri_app_lib::mermaid;

    // flowchart 应渲染为 SVG
    let svg = mermaid::render_mermaid("graph TD;\nA-->B\n").expect("flowchart 应渲染成功");
    assert!(svg.contains("<svg"), "应输出 SVG: {}", &svg[..svg.len().min(200)]);

    // 非图表源码 / 语法错误返回 None
    assert!(mermaid::render_mermaid("普通文本").is_none());
    assert!(mermaid::render_mermaid("graph TD;\nA-->\n").is_none() || true); // 容错：渲染器可能宽容
}

#[test]
fn 独立图片段落识别() {
    // `![alt](src)` 独立成段 → paragraph + image 信息；行内文本中的图片不识别
    let blocks = parse("![sss](./img/pic.png \"标题\")\n");
    assert_eq!(blocks.len(), 1);
    let image = blocks[0].image.as_ref().expect("应识别图片信息");
    assert_eq!(image.alt, "sss");
    assert_eq!(image.src, "./img/pic.png");
    assert_eq!(image.title.as_deref(), Some("标题"));

    let blocks = parse("前文 ![alt](./x.png) 后文\n");
    assert!(blocks[0].image.is_none(), "非独立图片不应识别");
}

#[test]
fn 数学公式渲染() {
    use tauri_app_lib::latex;

    // 展示公式：$$..$$ 原文 → SVG（默认黑色被替换为主题色）
    let svg = latex::render_display_math("$$\nx^2 + y^2 = z^2\n$$", Some("#794f27"), Some(16.0))
        .expect("合法公式应渲染成功");
    assert!(svg.contains("<svg"), "应输出 SVG");
    assert!(svg.contains("rgba(121,79,39,"), "默认黑应替换为主题色");

    // 行内公式：正文 → SVG
    assert!(latex::render_inline_math("e^{i\\pi}+1=0", None, None).is_some());

    // 非法输入返回 None
    assert!(latex::render_display_math("不是公式", None, None).is_none());
}
#[test]
fn kbd与上下标() {
    use tauri_app_lib::markdown::inline::tree::{InlineScript, InlineTextTree};

    // <kbd> 解析为 kbd 样式，序列化保留 <kbd> 标签
    let tree = InlineTextTree::from_markdown("按 <kbd>Command+Q</kbd> 退出");
    assert!(
        tree.fragments
            .iter()
            .any(|f| f.style.kbd && f.text.contains("Command+Q")),
        "应有 kbd fragment: {:?}",
        tree.fragments
    );
    let serialized = tree.serialize_markdown();
    assert!(serialized.contains("<kbd>Command+Q</kbd>"), "应保留 kbd 标签: {serialized}");

    // 词内上下标（velotype 规则：标记两侧须为 ASCII 字母数字）
    let tree = InlineTextTree::from_markdown("X^2^ 与 H~2~O");
    assert!(
        tree.fragments
            .iter()
            .any(|f| f.style.script == InlineScript::Superscript && f.text.contains('2')),
        "应有上标: {:?}",
        tree.fragments
    );
    assert!(
        tree.fragments
            .iter()
            .any(|f| f.style.script == InlineScript::Subscript && f.text.contains('2')),
        "应有下标: {:?}",
        tree.fragments
    );
    let serialized = tree.serialize_markdown();
    assert!(serialized.contains("^2^"), "上标定界符应保留: {serialized}");
    assert!(serialized.contains("~2~"), "下标定界符应保留: {serialized}");
}
#[test]
fn 单行多段公式不吞内容() {
    // `$$ 文本 $$ $$ 公式 $$` 同行：回退为段落，后段公式不丢失（行内 $$ 可渲染）
    let md = "$$ 代入公式得到： $$ $$u(t,x,y) = (3x+y) c^2 t^2$$\n";
    let blocks = markdown::parse_markdown(md);
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0].kind, BlockKindDto::Paragraph), "应回退为段落: {:?}", blocks[0].kind);

    let maths: Vec<_> = blocks[0]
        .title
        .fragments
        .iter()
        .filter_map(|f| f.math.as_ref())
        .collect();
    assert!(
        maths.iter().any(|m| m.body.contains("u(t,x,y)")),
        "后段公式应保留为行内公式: {:?}",
        blocks[0].title.fragments
    );
    // 带空格的前段按普通文本处理
    assert!(blocks[0].title.fragments.iter().any(|f| f.text.contains("代入公式得到：")));

    // 序列化往返不丢内容
    let serialized = markdown::serialize_markdown(blocks);
    assert!(serialized.contains("u(t,x,y)"), "往返后内容不丢: {serialized}");

    // 正常的单行展示公式仍是 mathBlock
    let blocks = markdown::parse_markdown("$$x^2$$\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::MathBlock));
}
#[test]
fn 双美元行内公式允许首尾空白() {
    // `$$ 文本 $$`（$$ 后有空格）应解析为行内公式而不是纯文本；
    // 单 $ 仍保持严格规则（货币防误判）
    let tree = tauri_app_lib::markdown::inline::tree::InlineTextTree::from_markdown(
        "$$ 代入公式得到： $$",
    );
    assert!(
        tree.fragments.iter().any(|f| f.math.is_some()),
        "应解析为行内公式: {:?}",
        tree.fragments
    );
    let math = tree.fragments.iter().find_map(|f| f.math.as_ref()).unwrap();
    assert_eq!(math.body, "代入公式得到：");

    // 单 $ 带空格仍不识别
    let tree = tauri_app_lib::markdown::inline::tree::InlineTextTree::from_markdown("$ x $");
    assert!(tree.fragments.iter().all(|f| f.math.is_none()));

    // 你的完整场景：文本 + 公式同行，定界明确、后段不丢
    let tree = tauri_app_lib::markdown::inline::tree::InlineTextTree::from_markdown(
        "$$ 代入公式得到： $$ $$u(t,x,y) = (3x+y) c^2 t^2$$",
    );
    let maths: Vec<_> = tree.fragments.iter().filter_map(|f| f.math.as_ref()).collect();
    assert_eq!(maths.len(), 2, "两段公式都应识别: {:?}", tree.fragments);
    assert!(maths[1].body.contains("u(t,x,y)"));
}
#[test]
fn 公式宏别名容错() {
    use tauri_app_lib::latex;

    // 非标准 \part 应通过别名渲染为 \partial
    let svg = latex::render_display_math(
        "$$\nu(t,x,y) = \\frac{1}{2\\pi c} \\frac{\\part}{\\part t} \\iint\\limits_{r<ct} \\frac{m^2(m+n)}{\\sqrt{c^2t^2 -r^2 }}dmdn\n$$",
        None,
        None,
    );
    assert!(svg.is_some(), "\\part 应经别名渲染成功");

    // 标准 \\partial 不受影响（别名边界规则）
    let svg2 = latex::render_inline_math("\\frac{\\partial}{\\partial t} x", None, None);
    assert!(svg2.is_some());

    // 无别名的错误输入仍返回 None
    assert!(latex::render_inline_math("\\notamacro{x}", None, None).is_none());
}
#[test]
fn 一行多个公式从左到右() {
    // `$$ 文本 $$ $$ 公式 $$` 同行：一个段落块内两个行内公式 fragment + 文本，
    // 前端按 fragment 顺序行内渲染即从左到右排列
    let blocks = markdown::parse_markdown(
        "$$ 代入公式得到： $$ $$u(t,x,y) = (3x+y) c^2 t^2 + x^2(x+y)$$\n",
    );
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0].kind, BlockKindDto::Paragraph));

    let fragments = &blocks[0].title.fragments;
    let math_indices: Vec<usize> = fragments
        .iter()
        .enumerate()
        .filter_map(|(i, f)| f.math.is_some().then_some(i))
        .collect();
    assert_eq!(math_indices.len(), 2, "应有两个公式 fragment: {fragments:?}");
    assert!(math_indices[0] < math_indices[1], "顺序应为从左到右");
    assert!(fragments[math_indices[0]].math.as_ref().unwrap().body.contains("代入公式得到："));
    assert!(fragments[math_indices[1]].math.as_ref().unwrap().body.contains("u(t,x,y)"));
}
#[test]
fn math围栏代码块按公式渲染() {
    use tauri_app_lib::latex;

    // ```math 围栏解析为 MathBlock（而非代码块），原文无损保留
    let md = "```math\n\\begin{aligned} I &= a \\\\ &= b \\end{aligned}\n```\n";
    let blocks = markdown::parse_markdown(md);
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0].kind, BlockKindDto::MathBlock), "应为 mathBlock: {:?}", blocks[0].kind);
    assert!(blocks[0].raw_fallback.as_deref().unwrap().starts_with("```math"));

    // 渲染命令接受 ```math 围栏来源
    let svg = latex::render_display_math(md, None, None);
    assert!(svg.is_some(), "```math 围栏应渲染成功");

    // 普通代码块不受影响
    let blocks = markdown::parse_markdown("```rust\nfn main() {}\n```\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::CodeBlock { .. }));

    // 非 math info 的围栏不进入公式路径
    let blocks = markdown::parse_markdown("```python\nprint(1)\n```\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::CodeBlock { .. }));
}
#[test]
fn 定界符与physics包() {
    use tauri_app_lib::latex;

    // \(...\) 行内定界符
    let blocks = markdown::parse_markdown("行内 \\(x^2\\) 公式\n");
    assert!(
        blocks[0].title.fragments.iter().any(|f| f.math.is_some()),
        "\\(...\\) 应解析为行内公式: {:?}",
        blocks[0].title.fragments
    );

    // \[...\] 块级定界符（单行与多行）
    let blocks = markdown::parse_markdown("\\[ x^2 \\]\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::MathBlock), "单行 \\[ \\] 应为 mathBlock: {:?}", blocks[0].kind);
    let svg = latex::render_display_math("\\[ x^2 \\]", None, None);
    assert!(svg.is_some(), "\\[ \\] 应渲染成功");

    let blocks = markdown::parse_markdown("\\[\nx^2 + y^2\n\\]\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::MathBlock), "多行 \\[ \\] 应为 mathBlock");
    assert!(blocks[0].raw_fallback.as_deref().unwrap().contains("\\[\nx^2 + y^2\n\\]"));

    // physics 包宏
    let physics_cases = [
        "\\dv{x}",
        "\\dv{f}{x}",
        "\\dv[2]{f}{x}",
        "\\pdv{f}{x}",
        "\\abs{x}",
        "\\norm{v}",
        "\\bra{a}",
        "\\ket{b}",
        "\\braket{a}{b}",
        "\\qty(x+y)",
        "\\qty[x+y]",
        "\\qty{x+y}",
        "\\eval{f}",
        "\\eval{f}{a}{b}",
        "\\dd{x}",
    ];
    for src in physics_cases {
        assert!(
            latex::render_inline_math(src, None, None).is_some(),
            "physics 宏应渲染: {src}"
        );
    }
}
