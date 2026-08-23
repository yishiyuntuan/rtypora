//! 多行续行长列表项回归测试：续行正文解析的结构正确性。
//! 覆盖列表项正文循环的每条分支（惰性 dedent 物化重构后必须保持行为不变）：
//! 纯续行并入标题、空行提升段落子块、各类型子块消费、未闭合围栏字面保留、
//! 标记行开围栏、懒惰续行、独立图片归属、往返稳定性。

use tauri_app_lib::markdown;
use tauri_app_lib::markdown::model::{BlockDto, BlockKindDto};

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

/// 子块类型名序列（不含字段）。
fn child_kind_names(block: &BlockDto) -> Vec<&str> {
    block
        .children
        .iter()
        .map(|child| match &child.kind {
            BlockKindDto::Paragraph => "Paragraph",
            BlockKindDto::Separator => "Separator",
            BlockKindDto::Heading { .. } => "Heading",
            BlockKindDto::BulletedListItem => "BulletedListItem",
            BlockKindDto::TaskListItem { .. } => "TaskListItem",
            BlockKindDto::NumberedListItem => "NumberedListItem",
            BlockKindDto::Quote => "Quote",
            BlockKindDto::Callout { .. } => "Callout",
            BlockKindDto::FootnoteDefinition => "FootnoteDefinition",
            BlockKindDto::Table => "Table",
            BlockKindDto::CodeBlock { .. } => "CodeBlock",
            BlockKindDto::Comment => "Comment",
            BlockKindDto::HtmlBlock => "HtmlBlock",
            BlockKindDto::SectionBlock => "SectionBlock",
            BlockKindDto::MathBlock => "MathBlock",
            BlockKindDto::MermaidBlock => "MermaidBlock",
            BlockKindDto::RawMarkdown => "RawMarkdown",
        })
        .collect()
}

/// 结构签名：类型 + 标题文本 + 子树（用于往返稳定性比较）。
fn sig(blocks: &[BlockDto]) -> String {
    fn walk(bs: &[BlockDto], depth: usize, out: &mut String) {
        for b in bs {
            out.push_str(&"  ".repeat(depth));
            let kind = format!("{:?}", b.kind);
            out.push_str(kind.split([' ', '{']).next().unwrap());
            out.push_str(&format!("|{}|\n", title_text(b).replace('\n', "\\n")));
            walk(&b.children, depth + 1, out);
        }
    }
    let mut out = String::new();
    walk(blocks, 0, &mut out);
    out
}

#[test]
fn 多行续行并入标题且行序完整() {
    let mut md = String::from("- 第一行\n");
    for i in 1..300 {
        md.push_str(&format!("  续行 {i}\n"));
    }

    let blocks = parse(&md);
    assert_eq!(blocks.len(), 1, "整段应解析为单个列表项根块");
    assert!(matches!(blocks[0].kind, BlockKindDto::BulletedListItem));
    assert!(blocks[0].children.is_empty(), "纯续行不得产生子块");

    let mut expected = String::from("第一行");
    for i in 1..300 {
        expected.push_str(&format!("\n续行 {i}"));
    }
    assert_eq!(title_text(&blocks[0]), expected, "续行应按序并入标题");

    // 根块区间覆盖全文（不含尾随换行）
    assert_eq!(blocks[0].start, Some(0));
    assert_eq!(blocks[0].end, Some(md.encode_utf16().count() - 1));
}

#[test]
fn 空行后的续行提升为段落子块() {
    let blocks = parse("- a\n  x\n\n  y\n");
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0].kind, BlockKindDto::BulletedListItem));
    assert_eq!(title_text(&blocks[0]), "a\nx");
    assert_eq!(child_kind_names(&blocks[0]), ["Paragraph"]);
    assert_eq!(title_text(&blocks[0].children[0]), "y");
}

#[test]
fn 任务与有序列表项续行保留标记状态() {
    let blocks = parse("- [x] 任务\n  续行一\n  续行二\n");
    assert!(
        matches!(blocks[0].kind, BlockKindDto::TaskListItem { checked: true }),
        "勾选状态必须保留: {:?}",
        blocks[0].kind
    );
    assert_eq!(title_text(&blocks[0]), "任务\n续行一\n续行二");

    let blocks = parse("3. 有序\n   续行一\n   续行二\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::NumberedListItem));
    assert_eq!(blocks[0].list_start, Some(3), "源序号必须保留");
    assert_eq!(title_text(&blocks[0]), "有序\n续行一\n续行二");
}

#[test]
fn 嵌套列表的续行归属内层项() {
    let blocks = parse("- a\n  - b\n    续行 deep\n  - c\n  续行 outer\n");
    assert_eq!(blocks.len(), 1);
    assert_eq!(title_text(&blocks[0]), "a");
    assert_eq!(
        child_kind_names(&blocks[0]),
        ["BulletedListItem", "BulletedListItem"]
    );
    assert_eq!(title_text(&blocks[0].children[0]), "b\n续行 deep");
    assert_eq!(title_text(&blocks[0].children[1]), "c\n续行 outer");
}

#[test]
fn 混排续行各子块类型与顺序() {
    let md = "- item\n  - sub1\n  - sub2\n\n  > quote\n\n  ---\n\n  ```rust\n  fn main() {}\n  ```\n\n  | A | B |\n  | --- | --- |\n  | 1 | 2 |\n\n  ![img](a.png)\n\n      indented code\n\n  [r]: /x\n\n  <!-- comment -->\n\n  [^f]: note\n\n  $$\n  y\n  $$\n\n  续行文本\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    assert_eq!(title_text(&blocks[0]), "item");
    assert_eq!(
        child_kind_names(&blocks[0]),
        [
            "BulletedListItem",
            "BulletedListItem",
            "Quote",
            "Separator",
            "CodeBlock",
            "Table",
            "Paragraph",
            "CodeBlock",
            "RawMarkdown",
            "Comment",
            "RawMarkdown",
            "MathBlock",
            "Paragraph",
        ]
    );

    let children = &blocks[0].children;
    // 围栏代码块带语言标记
    assert!(
        matches!(&children[4].kind, BlockKindDto::CodeBlock { language } if language.as_deref() == Some("rust"))
    );
    assert_eq!(title_text(&children[4]), "fn main() {}");
    // 独立图片段落子块携带图片信息
    let image = children[6].image.as_ref().expect("图片信息");
    assert_eq!((image.alt.as_str(), image.src.as_str()), ("img", "a.png"));
    // 缩进代码块无语言标记，内容保留
    assert!(
        matches!(&children[7].kind, BlockKindDto::CodeBlock { language: None })
    );
    assert_eq!(title_text(&children[7]), "indented code\n");
    // 引用定义与脚注定义在列表项内按原文保留
    assert_eq!(children[8].raw_fallback.as_deref(), Some("[r]: /x"));
    assert_eq!(children[10].raw_fallback.as_deref(), Some("[^f]: note"));
    // 注释与公式块原文保留
    assert_eq!(children[9].raw_fallback.as_deref(), Some("<!-- comment -->"));
    assert_eq!(children[11].raw_fallback.as_deref(), Some("$$\ny\n$$"));
    // 末尾续行在已有子块后提升为段落
    assert_eq!(title_text(&children[12]), "续行文本");
}

#[test]
fn 未闭合围栏续行保持字面文本() {
    let blocks = parse("- 文本\n  ```rust\n  未闭合围栏\n  后续行\n");
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].children.is_empty());
    assert_eq!(title_text(&blocks[0]), "文本\n```rust\n未闭合围栏\n后续行");
}

#[test]
fn 标记行开围栏作为代码子块() {
    let blocks = parse("- ```js\n  let a = 1;\n  ```\n  围栏后文本\n");
    assert_eq!(blocks.len(), 1);
    assert_eq!(title_text(&blocks[0]), "", "围栏行入子块后标题应为空");
    assert_eq!(child_kind_names(&blocks[0]), ["CodeBlock", "Paragraph"]);
    assert!(
        matches!(&blocks[0].children[0].kind, BlockKindDto::CodeBlock { language } if language.as_deref() == Some("js"))
    );
    assert_eq!(title_text(&blocks[0].children[0]), "let a = 1;");
    assert_eq!(title_text(&blocks[0].children[1]), "围栏后文本");
}

#[test]
fn 零缩进懒惰续行并入标题() {
    let blocks = parse("- a\nlazy 续行\n");
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].children.is_empty());
    assert_eq!(title_text(&blocks[0]), "a\nlazy 续行");
}

#[test]
fn 连续图片续行的归属规则不变() {
    // 第一行图片因下一行仍是图片（非空行/块起始）而并入标题；
    // 第二行图片作为独立图片段落子块。
    let blocks = parse("- 文本\n  ![img](b.png)\n  ![img2](c.png)\n");
    assert_eq!(blocks.len(), 1);
    assert_eq!(title_text(&blocks[0]), "文本\n![img](b.png)");
    assert_eq!(child_kind_names(&blocks[0]), ["Paragraph"]);
    let image = blocks[0].children[0].image.as_ref().expect("图片信息");
    assert_eq!((image.alt.as_str(), image.src.as_str()), ("img2", "c.png"));
}

#[test]
fn 长续行列表项往返结构稳定() {
    let mut md = String::from("- 第一行\n");
    for i in 1..300 {
        md.push_str(&format!("  续行 {i}\n"));
    }
    md.push_str("\n  提升段落\n\n- 第二项\n  续行\n");

    let first = parse(&md);
    let first_sig = sig(&first);
    let second = parse(&markdown::serialize_markdown(first));
    assert_eq!(first_sig, sig(&second), "往返结构不稳定: {md:?}");
}
