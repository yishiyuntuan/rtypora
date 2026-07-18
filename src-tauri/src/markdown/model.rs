//! 文档块模型：Markdown 解析结果的 JSON 表示，供前端 WYSIWYG 渲染与逐块编辑使用。

use serde::Serialize;

/// 文档中的一个块。
/// `start`/`end` 为该块源码在全文中的区间，单位是 **UTF-16 码元数**，
/// 前端可直接用 `String.slice(start, end)` 取出该块的 Markdown 原文（中文不会错位）。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Block {
    pub id: String,
    pub start: usize,
    pub end: usize,
    #[serde(flatten)]
    pub kind: BlockKind,
}

/// 块类型，序列化为 `type` 标签 + 驼峰字段，例如 `{"type":"heading","level":2,...}`。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum BlockKind {
    Paragraph {
        inlines: Vec<Inline>,
    },
    Heading {
        level: u8,
        inlines: Vec<Inline>,
    },
    CodeBlock {
        language: Option<String>,
        code: String,
    },
    BlockQuote {
        children: Vec<Block>,
    },
    List {
        ordered: bool,
        /// 有序列表起始序号。序列化名避开顶层 Block 的 start（源码偏移）
        #[serde(rename = "startNumber")]
        start: Option<u64>,
        items: Vec<ListItem>,
    },
    Table {
        alignments: Vec<String>,
        head: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    ThematicBreak,
    Html {
        html: String,
    },
}

/// 列表项。`checked` 为 `Some` 表示任务列表项；
/// `marker_offset` 是 `[ ]`/`[x]` 中 `[` 的 UTF-16 偏移，供前端勾选时替换这 3 个字符。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ListItem {
    pub checked: Option<bool>,
    pub marker_offset: Option<usize>,
    pub children: Vec<Block>,
}

/// 行内节点，同样以 `type` 标签序列化。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Inline {
    Text {
        text: String,
    },
    Bold {
        children: Vec<Inline>,
    },
    Italic {
        children: Vec<Inline>,
    },
    Strikethrough {
        children: Vec<Inline>,
    },
    Code {
        code: String,
    },
    Link {
        dest: String,
        title: String,
        children: Vec<Inline>,
    },
    Image {
        src: String,
        title: String,
        alt: String,
    },
    SoftBreak,
    HardBreak,
}
