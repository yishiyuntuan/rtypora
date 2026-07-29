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
fn 序列化对齐公共前缀() {
    // 真实场景：光标前片段为完整结构（DTO 层），整块为其延伸——公共前缀终点即光标位置
    let full = parse("# 标题 **粗体** 后续");
    let before = parse("# 标题 **粗**");
    let whole = full.clone();
    let offsets = markdown::lcp_offsets(full, vec![before, whole]);
    assert_eq!(offsets.len(), 2);
    assert_eq!(offsets[0], "# 标题 **粗".encode_utf16().count());
    // 第二段为整块（前缀即全长）
    assert_eq!(offsets[1], "# 标题 **粗体** 后续".encode_utf16().count());
}

#[test]
fn 块模板生成() {
    let table = markdown::block_template("table", Some(2), Some(3), None, None);
    assert_eq!(
        table.markdown,
        "| 列1 | 列2 | 列3 |\n| --- | --- | --- |\n|  |  |  |\n|  |  |  |"
    );
    assert_eq!(table.caret_offset, 2);
    // 模板可被解析器还原为表格（无平行实现）
    let blocks = parse(&table.markdown);
    assert!(matches!(blocks[0].kind, BlockKindDto::Table));
    assert_eq!(blocks[0].table.as_ref().unwrap().header.len(), 3);
    assert_eq!(blocks[0].table.as_ref().unwrap().rows.len(), 2);

    let math = markdown::block_template("mathBlock", None, None, None, None);
    assert_eq!(math.markdown, "$$\n\n$$");
    assert_eq!(math.caret_offset, 3);
    let section = markdown::block_template("sectionBlock", None, None, None, None);
    assert_eq!(section.markdown, "<section>\n\n</section>");
    assert_eq!(section.caret_offset, "<section>\n".len());
    let link = markdown::block_template("link", None, None, None, None);
    assert_eq!(link.markdown, "[链接]()");
    assert_eq!(link.caret_offset, 4);
    let inline_math = markdown::block_template("inlineMath", None, None, None, None);
    assert_eq!(inline_math.markdown, "$$");
    assert_eq!(inline_math.caret_offset, 1);
    let footnote_def = markdown::block_template("footnoteDef", None, None, None, None);
    assert_eq!(footnote_def.markdown, "[^1]: ");
    let link_ref = markdown::block_template("linkRef", None, None, None, None);
    assert_eq!(link_ref.markdown, "[1]: url \"title\"");
    assert_eq!(link_ref.caret_offset, 5);
    // 警告框类型：默认 NOTE，指定类型生效，未知类型回落 NOTE
    let note = markdown::block_template("callout", None, None, None, None);
    assert_eq!(note.markdown, "> [!NOTE]\n> ");
    let caution = markdown::block_template("callout", None, None, Some("CAUTION".into()), None);
    assert_eq!(caution.markdown, "> [!CAUTION]\n> ");
    assert_eq!(caution.caret_offset, caution.markdown.len());
    let unknown = markdown::block_template("callout", None, None, Some("FOO".into()), None);
    assert_eq!(unknown.markdown, "> [!NOTE]\n> ");
}

#[test]
fn 合并块源码() {
    assert_eq!(
        markdown::merge_block_markdown("# foo", "bar\n"),
        "# foobar"
    );
    // 并入文本按行内合并规则去首尾空白（与前端既有 trim 语义一致）
    assert_eq!(markdown::merge_block_markdown("- [ ] a", " b "), "- [ ] ab");
}

#[test]
fn 格式化表格源码() {
    // 列宽不齐的源码按最宽单元格补齐，管道对齐
    let formatted = markdown::format_table_source("| A | Longer | C |\n| --- | :---: | ---: |\n| 1 | 22 | 333 |")
        .expect("表格应可格式化");
    assert_eq!(
        formatted,
        "| A   | Longer | C    |\n| --- | :----: | ---: |\n| 1   | 22     | 333  |"
    );

    // 往返：格式化后的源码解析出的表格数据与原表一致
    let original = parse("| A | Longer | C |\n| --- | :---: | ---: |\n| 1 | 22 | 333 |");
    let reparsed = parse(&formatted);
    assert_eq!(original[0].table, reparsed[0].table);

    // 非表格输入返回 None（前端保持原样）
    assert!(markdown::format_table_source("# 标题").is_none());
    assert!(markdown::format_table_source("普通段落").is_none());
}

#[test]
fn 格式化表格源码_短列对齐行仍合法() {
    // 极短列：分隔行需补足到合法宽度（居中至少 `:---:`）
    let formatted = markdown::format_table_source("| a | b |\n| :---: | --- |\n| 1 | 2 |")
        .expect("表格应可格式化");
    assert_eq!(formatted, "| a     | b   |\n| :---: | --- |\n| 1     | 2   |");
    let reparsed = parse(&formatted);
    assert!(matches!(reparsed[0].kind, BlockKindDto::Table));
}

#[test]
fn 行内html标签不触发块级html() {
    // 回归：整行行内 HTML（<u>/<kbd> 等行内标签）应按段落行内解析，
    // 不得误判为块级 HtmlBlock（块级标签 div 不受影响）
    let blocks = parse("<u>下划线</u>\n");
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0].kind, BlockKindDto::Paragraph));
    assert!(blocks[0].title.fragments.iter().any(|f| f.style.underline && f.text.contains("下划线")));

    let blocks = parse("<kbd>Ctrl+C</kbd>\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::Paragraph));
    assert!(blocks[0].title.fragments.iter().any(|f| f.style.kbd));

    let blocks = parse("<div>\n<p>x</p>\n</div>\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::HtmlBlock));

    let blocks = parse("<section>\n<span>x</span>\n</section>\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::SectionBlock));
}

#[test]
fn html容器标签转换() {
    // 默认开启：h1-h6/p/div/center 单一容器按原生块解析（内联样式标签同步映射）
    let blocks = parse("<h2>标题 **粗体**</h2>\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::Heading { level: 2 }));
    assert!(blocks[0].title.fragments.iter().any(|f| f.style.bold));

    let blocks = parse("<p>段落 <s>删除</s> 与 <mark>高亮</mark></p>\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::Paragraph));
    assert!(blocks[0].title.fragments.iter().any(|f| f.style.strikethrough));
    assert!(blocks[0].title.fragments.iter().any(|f| f.style.highlight));

    let blocks = parse("<div>\n内容 <code>x</code>\n</div>\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::Paragraph));
    assert!(blocks[0].title.fragments.iter().any(|f| f.style.code));

    let blocks = parse("<center>居中</center>\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::Paragraph));

    // 嵌套同名容器或含块级标签不转换（走原文路径）
    let blocks = parse("<div><div>x</div></div>\n");
    assert!(
        !matches!(blocks[0].kind, BlockKindDto::Paragraph),
        "嵌套容器不应转换为段落: {:?}",
        blocks[0].kind
    );
    let blocks = parse("<div>\n<p>x</p>\n</div>\n");
    assert!(
        !matches!(blocks[0].kind, BlockKindDto::Paragraph),
        "内层块级标签不应转换为段落: {:?}",
        blocks[0].kind
    );
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
fn callout别名与折叠后缀() {
    use markdown::block::state::CalloutVariant;
    // Obsidian 别名 → 标准变体（统一转换默认开启）
    let blocks = parse("> [!hint]\n> 提示内容\n");
    match &blocks[0].kind {
        BlockKindDto::Callout { variant } => assert_eq!(*variant, CalloutVariant::Tip),
        other => panic!("应为 callout: {other:?}"),
    }
    let blocks = parse("> [!danger]\n> 危险\n");
    match &blocks[0].kind {
        BlockKindDto::Callout { variant } => assert_eq!(*variant, CalloutVariant::Caution),
        other => panic!("应为 callout: {other:?}"),
    }
    // 折叠后缀剥离（不入标题）
    let blocks = parse("> [!warning]- 折叠标题\n> 内容\n");
    match &blocks[0].kind {
        BlockKindDto::Callout { variant } => assert_eq!(*variant, CalloutVariant::Warning),
        other => panic!("应为 callout: {other:?}"),
    }
    assert_eq!(title_text(&blocks[0]), "折叠标题");
    // quote/cite 别名不映射（按普通引用处理，语义一致）
    let blocks = parse("> [!quote] 引文\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::Quote));
    // 自定义标题保留
    let blocks = parse("> [!note] 自定义标题\n> 内容\n");
    match &blocks[0].kind {
        BlockKindDto::Callout { variant } => assert_eq!(*variant, CalloutVariant::Note),
        other => panic!("应为 callout: {other:?}"),
    }
    assert_eq!(title_text(&blocks[0]), "自定义标题");
    // 序列化统一为标准 [!TYPE]（别名落源为标准标记）
    let md = markdown::serialize_markdown(blocks);
    assert!(md.contains("[!NOTE]"), "别名应统一为标准标记: {md}");
    // 普通引用以别名头部开头时转义防误判（往返保持引用）
    let blocks = parse("> \\[!hint] 只是引用\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::Quote));
    let md = markdown::serialize_markdown(blocks.clone());
    assert!(md.contains("\\[!hint]"), "引用头部应保留转义: {md}");
    // 标准标记同理（\[! 不得误判为 LaTeX \[ 展示公式起始）
    let blocks = parse("> \\[!NOTE] 也是引用\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::Quote));
    let md = markdown::serialize_markdown(blocks);
    assert!(md.contains("\\[!NOTE]"), "标准标记引用头部应保留转义: {md}");
}

#[test]
fn callout容器语法() {
    use markdown::block::state::CalloutVariant;
    // Docusaurus :::type（带 [标题]）
    let blocks = parse(":::warning[数据丢失风险]\n删除操作不可恢复。\n:::\n");
    match &blocks[0].kind {
        BlockKindDto::Callout { variant } => assert_eq!(*variant, CalloutVariant::Warning),
        other => panic!("应为 callout: {other:?}"),
    }
    assert_eq!(title_text(&blocks[0]), "数据丢失风险");
    assert!(!blocks[0].children.is_empty(), "容器内容应在 children");
    // 序列化统一为标准引用格式
    let md = markdown::serialize_markdown(blocks);
    assert!(md.contains("> [!WARNING] 数据丢失风险"), "容器应统一为引用式: {md}");

    // MkDocs !!! type "标题"（内容缩进 4 空格）
    let blocks = parse("!!! danger \"严重后果\"\n    直接操作生产数据库。\n");
    match &blocks[0].kind {
        BlockKindDto::Callout { variant } => assert_eq!(*variant, CalloutVariant::Caution),
        other => panic!("应为 callout: {other:?}"),
    }
    assert_eq!(title_text(&blocks[0]), "严重后果");
    // 尾部空行不吞并后续块
    let blocks = parse("!!! warning\n    内容。\n\n后续段落\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::Callout { .. }));
    assert_eq!(blocks.len(), 2, "尾部空行后的段落不应被吞: {}", blocks.len());

    // 未闭合 ::: 不识别（按普通文本）
    let blocks = parse(":::warning\n没有闭合。\n");
    assert!(!matches!(blocks[0].kind, BlockKindDto::Callout { .. }));
    // 无缩进内容的 !!! 不识别
    let blocks = parse("!!! warning\n没有缩进内容。\n");
    assert!(!matches!(blocks[0].kind, BlockKindDto::Callout { .. }));
}

#[test]
fn 分割线() {
    let blocks = parse("上文\n\n---\n\n下文\n");
    assert_eq!(blocks.len(), 3);
    assert!(matches!(blocks[1].kind, BlockKindDto::Separator));
}

#[test]
fn yaml_front_matter() {
    // 文档头 --- 且有闭合：整体为注释类块（原文无损，后续内容正常解析）
    let blocks = parse("---\ntitle: 标题\ntags: [a, b]\n---\n\n正文\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::Comment));
    assert_eq!(markdown::serialize_markdown(blocks.clone()), "---\ntitle: 标题\ntags: [a, b]\n---\n\n正文");
    assert!(blocks.iter().any(|b| matches!(b.kind, BlockKindDto::Paragraph)));

    // `...` 也可闭合
    let blocks = parse("---\ntitle: x\n...\n正文\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::Comment));

    // 文档头 --- 但无闭合：维持分割线语义（不是 front matter）
    let blocks = parse("---\n\n正文\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::Separator));

    // 文档中部的 --- 区块不是 front matter
    let blocks = parse("正文\n\n---\ntitle: x\n---\n");
    assert!(matches!(blocks[1].kind, BlockKindDto::Separator));
}

#[test]
fn font标签行内解析() {
    // <font color> 行内映射为 HtmlInlineStyle；序列化保持 <font> 标签
    //（color 属性用 hex，不改为 span）
    let blocks = parse("这是 <font color=\"red\">红色</font> 文字\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::Paragraph));
    assert_eq!(blocks[0].title.fragments[1].text, "红色");
    let md = markdown::serialize_markdown(blocks);
    assert!(
        md.contains("<font color=\"#ff0000\">红色</font>"),
        "font color 应保持 font 标签: {md}"
    );

    // <font size="5"> → 24px（HTML 档位映射，序列化到 style 属性）
    let blocks = parse("<font size=\"5\">大字</font> 普通\n");
    assert!(
        matches!(blocks[0].kind, BlockKindDto::Paragraph),
        "font 行首应按段落解析: {:?}",
        blocks[0].kind
    );
    let md = markdown::serialize_markdown(blocks);
    assert!(
        md.contains("<font style=\"font-size: 24px;\">大字</font>"),
        "font size 应保持 font 标签: {md}"
    );

    // font 独占一行也不再误判为 HTML 块
    let blocks = parse("<font color=\"red\">红色</font>\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::Paragraph));
}

#[test]
fn span的background与var颜色() {
    // background 简写（非 background-color）与 var(--x) 主题变量引用
    let blocks = parse("<span style=\"background:var(--color-2-0-c)\">文字</span>\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::Paragraph));
    let md = markdown::serialize_markdown(blocks);
    assert!(
        md.contains("background-color: var(--color-2-0-c);"),
        "background 简写 + var() 应保留: {md}"
    );
    // 普通 background 颜色值（background 简写）
    let blocks = parse("<span style=\"background:#ffee00\">高亮</span>\n");
    let md = markdown::serialize_markdown(blocks);
    assert!(md.contains("background-color:"), "background 简写应映射背景色: {md}");
    // background: url(...) 复合值不映射（span 按原文保留）
    let blocks = parse("<span style=\"background:url(x.png)\">文字</span>\n");
    let md = markdown::serialize_markdown(blocks);
    assert_eq!(md, "<span style=\"background:url(x.png)\">文字</span>");
}

#[test]
fn 独立img行携带图片信息() {
    let blocks = parse("<img src=\"./img/a.png\" alt=\"示意\" style=\"zoom:50%\" />\n");
    let image = blocks[0].image.as_ref().expect("独立 img 行应携带 image");
    assert_eq!(image.src, "./img/a.png");
    assert_eq!(image.alt, "示意");
    assert_eq!(image.zoom, Some(0.5));
    // 原文保留（rawFallback），序列化不丢样式属性
    let md = markdown::serialize_markdown(blocks);
    assert!(md.contains("zoom:50%"), "原文应保留: {md}");

    // 无 zoom 属性时 zoom 为 None
    let blocks = parse("<img src=\"a.png\" />\n");
    assert_eq!(blocks[0].image.as_ref().unwrap().zoom, None);
}

#[test]
fn a标签转链接() {
    // 无 href：空 destination 的链接（Typora 式渲染为链接样式）
    let blocks = parse("<a>ssss</a>\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::Paragraph));
    let f = &blocks[0].title.fragments[0];
    assert_eq!(f.text, "ssss");
    assert!(f.link.is_some(), "<a> 应转为链接: {:?}", blocks[0].title.fragments);
    // 序列化为 markdown 链接语法
    assert_eq!(markdown::serialize_markdown(blocks), "[ssss]()");

    // 带 href 与混排文本
    let blocks = parse("这是 <a href=\"https://example.com\">链接</a> 文字\n");
    assert_eq!(title_text(&blocks[0]), "这是 链接 文字");
    let linked: Vec<_> = blocks[0]
        .title
        .fragments
        .iter()
        .filter(|f| f.link.is_some())
        .collect();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].text, "链接");
    let md = markdown::serialize_markdown(blocks);
    assert_eq!(md, "这是 [链接](https://example.com) 文字");
}

#[test]
fn html标签自动闭合() {
    let cases = [
        ("<div>", Some("</div>")),
        ("<font color=\"red\">", Some("</font>")),
        ("<span class=\"x\">", Some("</span>")),
        ("<h2>", Some("</h2>")),
        ("<section>", Some("</section>")),
        ("<ul>", Some("</ul>")),
        ("<table>", Some("</table>")),
        ("文字 <kbd>", Some("</kbd>")),
        // void / 自闭合 / 闭合标签 / 注释声明 / 未知标签 / 散文比较符：不触发
        ("<br>", None),
        ("<hr>", None),
        ("<img src=\"a.png\">", None),
        ("<div/>", None),
        ("</div>", None),
        ("<!-- 注释 -->", None),
        ("<unknown>", None),
        ("a < y >", None),
        ("x <y>", None),
        // 未输入 > 不触发
        ("<div", None),
        // 属性含 > 保守放弃
        ("<div class=\"a>b\">", None),
    ];
    for (input, expected) in cases {
        assert_eq!(
            markdown::inline_html_autoclose(input).as_deref(),
            expected,
            "输入 {input:?}"
        );
    }
}

#[test]
fn html容器标签展开判定() {
    let cases = [
        ("<div>", "</div>", Some("div")),
        ("<div class=\"a\">", "</div>", Some("div")),
        ("<section>", "</section>", Some("section")),
        ("<table>", "</table>", Some("table")),
        ("<ul>", "</ul>", Some("ul")),
        ("<h2>", "</h2>", Some("h2")),
        // 标签内有内容也可展开（光标在内容之后）
        ("<div>内容", "</div>", Some("div")),
        // 行内标签不展开
        ("<span>", "</span>", None),
        ("<font color=\"red\">", "</font>", None),
        ("<kbd>", "</kbd>", None),
        // 闭标签不配对 / 缺失
        ("<div>", "</span>", None),
        ("<div>", "", None),
        ("<div>", "文本", None),
        // void / 未知标签
        ("<br>", "</br>", None),
        ("<unknown>", "</unknown>", None),
    ];
    for (before, after, expected) in cases {
        assert_eq!(
            markdown::html_container_tag_between(before, after).as_deref(),
            expected,
            "before={before:?} after={after:?}"
        );
    }
}

#[test]
fn html闭标签跳过判定() {
    let cases = [
        ("<div>", "</div>", Some("</div>")),
        ("<div class=\"a\">", "</div>", Some("</div>")),
        ("<section>", "</section>", Some("</section>")),
        // 标签内有内容：光标在内容之后、闭标签之前
        ("<a>ssss", "</a>", Some("</a>")),
        ("<div>文字", "</div>", Some("</div>")),
        // 行内标签同样适用（跳过不限制块级容器）
        ("<span>", "</span>", Some("</span>")),
        ("<font color=\"red\">", "</font>", Some("</font>")),
        // 不配对 / 缺失 / void / 未知标签 / 已闭合
        ("<div>", "</span>", None),
        ("<div>", "", None),
        ("文本", "</div>", None),
        ("<div></div>", "</div>", None),
        ("<br>", "</br>", None),
        ("<unknown>", "</unknown>", None),
    ];
    for (before, after, expected) in cases {
        assert_eq!(
            markdown::html_closing_tag_at(before, after).as_deref(),
            expected,
            "before={before:?} after={after:?}"
        );
    }
}

#[test]
fn 字体颜色值解析() {
    // 各种 CSS 写法与裸 RGB 三元组（序列化为 htmlStyle.color 的 JSON 形状）
    let red = serde_json::json!({"rgba": {"red": 207, "green": 34, "blue": 46, "alpha": 1.0}});
    for input in ["#cf222e", "rgb(207,34,46)", "207,34,46", " 207 , 34 , 46 "] {
        let color = markdown::parse_html_color(input).expect(input);
        assert_eq!(serde_json::to_value(&color).unwrap(), red, "输入 {input:?}");
    }
    // 命名颜色与 currentColor
    assert!(markdown::parse_html_color("red").is_some());
    assert_eq!(
        serde_json::to_value(markdown::parse_html_color("currentColor").unwrap()).unwrap(),
        serde_json::json!("currentColor")
    );
    // 非法值
    assert!(markdown::parse_html_color("not-a-color").is_none());
    assert!(markdown::parse_html_color("1,2").is_none());
    assert!(markdown::parse_html_color("").is_none());
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
fn 文本统计视觉行数() {
    // 段落分隔空行不计：一次回车（新段落）只 +1 行
    assert_eq!(markdown::text_stats("aaa").lines, 1);
    assert_eq!(markdown::text_stats("aaa\n\n").lines, 2, "回车后的空段落占一行");
    assert_eq!(markdown::text_stats("aaa\n\nbbb").lines, 2, "段落间空行不计");
    assert_eq!(markdown::text_stats("aaa\n\nbbb\n\n").lines, 3);
    assert_eq!(markdown::text_stats("").lines, 1);
    // 代码块围栏内的空行照常计
    assert_eq!(markdown::text_stats("```\na\n\nb\n```").lines, 5);
    assert_eq!(markdown::text_stats("```\na\n\n").lines, 4, "未闭合围栏内空行各计一次、结尾不重复加");
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
