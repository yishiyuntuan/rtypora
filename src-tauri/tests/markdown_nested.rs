//! 嵌套结构渲染测试：验证各类块级/行内元素在嵌套组合下的解析形状，
//! 以及经「解析 → 序列化 → 重解析」链路的结构稳定性（前端编辑提交走同一链路，
//! 结构签名不一致即编辑后渲染错乱）。

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

/// 结构签名：块类型 + 每个 fragment 的文本/完整样式/链接/脚注 + 子树缩进。
/// 用于往返稳定性比较（序列化允许规范化空白与定界符，但结构与样式不得漂移）。
fn sig(blocks: &[BlockDto]) -> String {
    fn walk(bs: &[BlockDto], depth: usize, out: &mut String) {
        for b in bs {
            out.push_str(&"  ".repeat(depth));
            let kind = format!("{:?}", b.kind);
            out.push_str(kind.split([' ', '{']).next().unwrap());
            out.push('[');
            for f in &b.title.fragments {
                out.push_str(&f.text.replace('\n', "\\n"));
                out.push_str(&format!("{:?}", f.style));
                if let Some(link) = &f.link {
                    out.push_str(&format!("@{}", link.open_target()));
                }
                if f.footnote.is_some() {
                    out.push_str("^fn");
                }
                out.push('|');
            }
            out.push_str("]\n");
            walk(&b.children, depth + 1, out);
        }
    }
    let mut out = String::new();
    walk(blocks, 0, &mut out);
    out
}

/// 断言往返稳定：parse → serialize → reparse 后结构签名一致。
fn assert_roundtrip(md: &str) {
    let a = parse(md);
    let sa = sig(&a);
    let b = markdown::parse_markdown(&markdown::serialize_markdown(a));
    let sb = sig(&b);
    assert_eq!(sa, sb, "往返结构不稳定: {md:?}");
}

/// 递归断言：嵌套子块均不带区间（仅根块有 start/end，编辑以根块为单位）。
fn assert_nested_offsets_null(blocks: &[BlockDto]) {
    for b in blocks {
        for child in &b.children {
            assert_eq!(child.start, None, "嵌套子块不应带 start");
            assert_eq!(child.end, None, "嵌套子块不应带 end");
        }
        assert_nested_offsets_null(&b.children);
    }
}

// ---------- 引用容器内的块级元素 ----------

#[test]
fn 引用内各类块级元素() {
    // 引用内容作为 children：段落/列表/围栏/表格/数学/mermaid 各就其位
    let md = "> 正文段落\n> - 列表项\n> ```js\n> code\n> ```\n> | a |\n> |---|\n> | 1 |\n> $$\n> x=1\n> $$\n> ```mermaid\n> graph TD\n> A-->B\n> ```\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0].kind, BlockKindDto::Quote));
    // 引用首段文字作为引用块标题，其余元素为 children
    assert_eq!(title_text(&blocks[0]), "正文段落");
    let kinds: Vec<_> = blocks[0].children.iter().map(|c| &c.kind).collect();
    assert!(
        matches!(kinds[0], BlockKindDto::BulletedListItem)
            && matches!(kinds[1], BlockKindDto::CodeBlock { .. })
            && matches!(kinds[2], BlockKindDto::Table)
            && matches!(kinds[3], BlockKindDto::MathBlock)
            && matches!(kinds[4], BlockKindDto::MermaidBlock),
        "引用子块序列不正确: {kinds:?}"
    );
    assert_nested_offsets_null(&blocks);
    assert_roundtrip(md);
}

#[test]
fn 引用内标题按原文保留() {
    // velotype 引用子块不含标题类型：按 RawMarkdown 原文保留，可无损往返
    let md = "> # 标题\n> 正文\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0].kind, BlockKindDto::Quote));
    assert!(matches!(blocks[0].children[0].kind, BlockKindDto::RawMarkdown));
    assert_eq!(title_text(&blocks[0].children[0]), "# 标题");
    assert_roundtrip(md);
}

#[test]
fn 多层引用嵌套() {
    let md = "> 一层\n> > 二层\n> > > 三层\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    let l1 = &blocks[0];
    assert!(matches!(l1.kind, BlockKindDto::Quote));
    assert_eq!(title_text(l1), "一层");
    let l2 = &l1.children[0];
    assert!(matches!(l2.kind, BlockKindDto::Quote));
    assert_eq!(title_text(l2), "二层");
    let l3 = &l2.children[0];
    assert!(matches!(l3.kind, BlockKindDto::Quote));
    assert_eq!(title_text(l3), "三层");
    assert_nested_offsets_null(&blocks);
    assert_roundtrip(md);
}

#[test]
fn 引用内callout与callout内引用() {
    // 引用包 callout
    let blocks = parse("> > [!warning] 警告\n");
    assert!(matches!(blocks[0].kind, BlockKindDto::Quote));
    assert!(matches!(
        blocks[0].children[0].kind,
        BlockKindDto::Callout {
            variant: markdown::block::state::CalloutVariant::Warning
        }
    ));

    // callout 包引用（callout 正文里的引用行）
    let md = "> [!note]\n> > 内层引用\n";
    let blocks = parse(md);
    assert!(matches!(blocks[0].kind, BlockKindDto::Callout { .. }));
    let has_quote = blocks[0]
        .children
        .iter()
        .any(|c| matches!(c.kind, BlockKindDto::Quote));
    assert!(has_quote, "callout 内应含引用子块: {:?}", blocks[0].children.iter().map(|c| &c.kind).collect::<Vec<_>>());
    assert_roundtrip("> [!note]\n> > 内层引用\n");
}

// ---------- 列表项容器内的块级元素 ----------

#[test]
fn 列表项内各类块级元素() {
    // 缩进到项内容列的 围栏/引用/表格/数学 均为项的子块
    let md = "- item\n  > 引用\n  ```js\n  code\n  ```\n  | a |\n  |---|\n  | 1 |\n  $$\n  x=1\n  $$\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0].kind, BlockKindDto::BulletedListItem));
    let kinds: Vec<_> = blocks[0].children.iter().map(|c| &c.kind).collect();
    assert!(
        kinds.iter().any(|k| matches!(k, BlockKindDto::Quote)),
        "缺引用子块: {kinds:?}"
    );
    assert!(
        kinds.iter().any(|k| matches!(k, BlockKindDto::CodeBlock { .. })),
        "缺围栏子块: {kinds:?}"
    );
    assert!(kinds.iter().any(|k| matches!(k, BlockKindDto::Table)), "缺表格子块: {kinds:?}");
    assert!(
        kinds.iter().any(|k| matches!(k, BlockKindDto::MathBlock)),
        "缺数学子块: {kinds:?}"
    );
    assert_nested_offsets_null(&blocks);
    assert_roundtrip(md);
}

#[test]
fn 有序与任务列表项的子块() {
    // 有序列表项内容列宽随标记（"1. " = 3 列）
    let md = "1. item\n   ```js\n   code\n   ```\n";
    let blocks = parse(md);
    assert!(matches!(blocks[0].kind, BlockKindDto::NumberedListItem));
    assert!(matches!(blocks[0].children[0].kind, BlockKindDto::CodeBlock { .. }));
    assert_eq!(title_text(&blocks[0].children[0]), "code");
    assert_roundtrip(md);

    // 任务列表项嵌套子列表与围栏
    let md = "- [x] done\n  - [ ] todo\n  ```js\n  x\n  ```\n";
    let blocks = parse(md);
    assert!(matches!(blocks[0].kind, BlockKindDto::TaskListItem { checked: true }));
    let kinds: Vec<_> = blocks[0].children.iter().map(|c| &c.kind).collect();
    assert!(kinds.iter().any(|k| matches!(k, BlockKindDto::TaskListItem { checked: false })));
    assert!(kinds.iter().any(|k| matches!(k, BlockKindDto::CodeBlock { .. })));
    assert_nested_offsets_null(&blocks);
    assert_roundtrip(md);
}

#[test]
fn 三种列表混合深层嵌套() {
    // 无序 → 有序 → 任务 三层；每层文本与勾选状态精确
    let md = "- L1\n  1. L2\n     - [ ] L3a\n     - [x] L3b\n  2. L2b\n- L1b\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 2);
    let l1 = &blocks[0];
    assert_eq!(title_text(l1), "L1");
    assert_eq!(l1.children.len(), 2);
    assert!(matches!(l1.children[0].kind, BlockKindDto::NumberedListItem));
    assert_eq!(l1.children[0].children.len(), 2);
    assert!(matches!(
        l1.children[0].children[0].kind,
        BlockKindDto::TaskListItem { checked: false }
    ));
    assert!(matches!(
        l1.children[0].children[1].kind,
        BlockKindDto::TaskListItem { checked: true }
    ));
    assert_eq!(title_text(&l1.children[0].children[1]), "L3b");
    assert!(matches!(l1.children[1].kind, BlockKindDto::NumberedListItem));
    assert_eq!(title_text(&blocks[1]), "L1b");
    assert_roundtrip(md);
}

#[test]
fn 列表项内嵌套mermaid() {
    let md = "- item\n  ```mermaid\n  graph TD\n  A-->B\n  ```\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0].kind, BlockKindDto::BulletedListItem));
    assert!(matches!(blocks[0].children[0].kind, BlockKindDto::MermaidBlock));
    let raw = blocks[0].children[0].raw_fallback.as_deref().unwrap_or("");
    assert!(raw.contains("graph TD"), "mermaid 原文应保留: {raw:?}");
    assert_roundtrip(md);
}

// ---------- callout 容器内的块级元素 ----------

#[test]
fn callout内各类块级元素() {
    let md = "> [!note] 提示\n> - 列表 a\n> - 列表 b\n> ```js\n> code\n> ```\n> 正文段落\n";
    let blocks = parse(md);
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0].kind, BlockKindDto::Callout { .. }));
    assert_eq!(title_text(&blocks[0]), "提示");
    let kinds: Vec<_> = blocks[0].children.iter().map(|c| &c.kind).collect();
    assert!(kinds.iter().filter(|k| matches!(k, BlockKindDto::BulletedListItem)).count() == 2);
    assert!(kinds.iter().any(|k| matches!(k, BlockKindDto::CodeBlock { .. })));
    assert!(kinds.iter().any(|k| matches!(k, BlockKindDto::Paragraph)));
    assert_nested_offsets_null(&blocks);
    assert_roundtrip(md);
}

// ---------- 行内元素嵌套 ----------

/// 断言存在满足条件的 fragment。
fn has_frag(block: &BlockDto, pred: impl Fn(&markdown::inline::tree::InlineFragment) -> bool) -> bool {
    block.title.fragments.iter().any(pred)
}

#[test]
fn 粗体内嵌斜体() {
    let blocks = parse("**粗体与*斜体*混排**");
    let b = &blocks[0];
    assert!(has_frag(b, |f| f.text == "粗体与" && f.style.bold && !f.style.italic));
    assert!(has_frag(b, |f| f.text == "斜体" && f.style.bold && f.style.italic));
    assert!(has_frag(b, |f| f.text == "混排" && f.style.bold && !f.style.italic));
    assert_roundtrip("**粗体与*斜体*混排**");
}

#[test]
fn 粗斜一体与删除线内粗体() {
    let blocks = parse("***粗斜***");
    assert!(has_frag(&blocks[0], |f| f.text == "粗斜" && f.style.bold && f.style.italic));

    let blocks = parse("~~**删除里的粗体**~~");
    assert!(has_frag(&blocks[0], |f| f.text == "删除里的粗体" && f.style.bold && f.style.strikethrough));
    assert_roundtrip("***粗斜*** 与 ~~**删除里的粗体**~~");
}

#[test]
fn 链接与样式互嵌() {
    // 样式在链接文本内 / 链接在样式内，两种写法效果一致
    for md in ["[**粗体链接**](https://a.b)", "**[粗体链接](https://a.b)**"] {
        let blocks = parse(md);
        assert!(
            has_frag(&blocks[0], |f| f.text == "粗体链接"
                && f.style.bold
                && f.link.as_ref().is_some_and(|l| l.open_target() == "https://a.b")),
            "{md} 应为 粗体+链接"
        );
    }
    assert_roundtrip("[**粗**](https://a.b) 与 *[斜链](https://c.d)*");
}

#[test]
fn 行内代码不解析内部标记() {
    let blocks = parse("`代码里 **不解析** 标记`");
    let frags = &blocks[0].title.fragments;
    assert_eq!(frags.len(), 1);
    assert!(frags[0].style.code);
    assert!(!frags[0].style.bold, "代码内标记不得生效");
    assert_eq!(frags[0].text, "代码里 **不解析** 标记");

    // 代码片段嵌在粗体内：继承粗体样式但内容不解析
    let blocks = parse("**粗体里的`代码`片段**");
    assert!(has_frag(&blocks[0], |f| f.text == "代码" && f.style.code && f.style.bold));
    assert_roundtrip("**粗体里的`代码`片段**");
}

#[test]
fn 下划线与高亮内嵌样式() {
    let blocks = parse("<u>下划线*斜体*</u>");
    assert!(has_frag(&blocks[0], |f| f.text == "下划线" && f.style.underline && !f.style.italic));
    assert!(has_frag(&blocks[0], |f| f.text == "斜体" && f.style.underline && f.style.italic));

    let blocks = parse("==高亮*斜体*==");
    assert!(has_frag(&blocks[0], |f| f.text == "高亮" && f.style.highlight && !f.style.italic));
    assert!(has_frag(&blocks[0], |f| f.text == "斜体" && f.style.highlight && f.style.italic));
    assert_roundtrip("<u>下划线*斜体*</u> 与 ==高亮*斜体*==");
}

#[test]
fn 样式内的脚注引用() {
    let md = "**文本[^1]**\n\n[^1]: 注释\n";
    let blocks = parse(md);
    assert!(has_frag(&blocks[0], |f| f.footnote.is_some() && f.style.bold));
    assert!(matches!(blocks[1].kind, BlockKindDto::FootnoteDefinition));
    assert_roundtrip(md);
}

#[test]
fn 表格单元格行内元素() {
    let md = "| **粗** | [链](https://a.b) |\n|---|---|\n| `码` | *斜* |\n";
    let blocks = parse(md);
    let table = blocks[0].table.as_ref().expect("应有表格数据");
    let h0 = &table.header[0].fragments;
    assert!(h0.iter().any(|f| f.text == "粗" && f.style.bold));
    let h1 = &table.header[1].fragments;
    assert!(h1.iter().any(|f| f.link.as_ref().is_some_and(|l| l.open_target() == "https://a.b")));
    let r0 = &table.rows[0][0].fragments;
    assert!(r0.iter().any(|f| f.text == "码" && f.style.code));
    let r1 = &table.rows[0][1].fragments;
    assert!(r1.iter().any(|f| f.text == "斜" && f.style.italic));
    assert_roundtrip(md);
}

// ---------- 综合嵌套文档 ----------

#[test]
fn 综合嵌套文档往返稳定() {
    // 覆盖：标题 / 引用(含列表+围栏) / 混合嵌套列表 / callout(含围栏) / 表格 / 数学 / mermaid
    let md = "# 标题\n\n> 引用段落\n> - 引用内列表\n> ```py\n> code\n> ```\n\n- 列表\n  1. 有序\n     - [x] 任务\n\n> [!tip] 提示\n> ```js\n> x\n> ```\n\n| 表 |\n|---|\n| 格 |\n\n$$\nE=mc^2\n$$\n\n```mermaid\ngraph TD\nA-->B\n```\n";
    let blocks = parse(md);
    let kinds: Vec<_> = blocks.iter().map(|b| &b.kind).collect();
    assert!(matches!(kinds[0], BlockKindDto::Heading { level: 1 }));
    assert!(kinds.iter().any(|k| matches!(k, BlockKindDto::Quote)));
    assert!(kinds.iter().any(|k| matches!(k, BlockKindDto::BulletedListItem)));
    assert!(kinds.iter().any(|k| matches!(k, BlockKindDto::Callout { .. })));
    assert!(kinds.iter().any(|k| matches!(k, BlockKindDto::Table)));
    assert!(kinds.iter().any(|k| matches!(k, BlockKindDto::MathBlock)));
    assert!(kinds.iter().any(|k| matches!(k, BlockKindDto::MermaidBlock)));
    assert_nested_offsets_null(&blocks);
    assert_roundtrip(md);
}
