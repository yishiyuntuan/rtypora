//! 对外 serde DTO（JSON 契约）：前端块树的序列化形状。
//!
//! 内部模型（`block::state::BlockRecord` + `block::document::BlockNode`）与 DTO 分离：
//! DTO 把 kind 拍平为 `type` 标签、children 直接嵌套、并携带 UTF-16 码元偏移
//! （仅根块有 `start`/`end`，嵌套子块为 null——编辑以根块为单位）。

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::block::document::BlockNode;
use super::block::state::{BlockKind, BlockRecord, CalloutVariant};
use super::inline::tree::InlineTextTree;
use super::table::TableData;

/// 块类型标签（拍平进 `BlockDto`，`type` 字段 + 各类型自带字段）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum BlockKindDto {
    Paragraph,
    Separator,
    Heading { level: u8 },
    BulletedListItem,
    TaskListItem { checked: bool },
    NumberedListItem,
    Quote,
    Callout { variant: CalloutVariant },
    FootnoteDefinition,
    Table,
    CodeBlock { language: Option<String> },
    Comment,
    HtmlBlock,
    MathBlock,
    MermaidBlock,
    RawMarkdown,
}

impl From<BlockKind> for BlockKindDto {
    fn from(kind: BlockKind) -> Self {
        match kind {
            BlockKind::Paragraph => Self::Paragraph,
            BlockKind::Separator => Self::Separator,
            BlockKind::Heading { level } => Self::Heading { level },
            BlockKind::BulletedListItem => Self::BulletedListItem,
            BlockKind::TaskListItem { checked } => Self::TaskListItem { checked },
            BlockKind::NumberedListItem => Self::NumberedListItem,
            BlockKind::Quote => Self::Quote,
            BlockKind::Callout(variant) => Self::Callout { variant },
            BlockKind::FootnoteDefinition => Self::FootnoteDefinition,
            BlockKind::Table => Self::Table,
            BlockKind::CodeBlock { language } => Self::CodeBlock { language },
            BlockKind::Comment => Self::Comment,
            BlockKind::HtmlBlock => Self::HtmlBlock,
            BlockKind::MathBlock => Self::MathBlock,
            BlockKind::MermaidBlock => Self::MermaidBlock,
            BlockKind::RawMarkdown => Self::RawMarkdown,
        }
    }
}

impl From<BlockKindDto> for BlockKind {
    fn from(kind: BlockKindDto) -> Self {
        match kind {
            BlockKindDto::Paragraph => Self::Paragraph,
            BlockKindDto::Separator => Self::Separator,
            BlockKindDto::Heading { level } => Self::Heading { level },
            BlockKindDto::BulletedListItem => Self::BulletedListItem,
            BlockKindDto::TaskListItem { checked } => Self::TaskListItem { checked },
            BlockKindDto::NumberedListItem => Self::NumberedListItem,
            BlockKindDto::Quote => Self::Quote,
            BlockKindDto::Callout { variant } => Self::Callout(variant),
            BlockKindDto::FootnoteDefinition => Self::FootnoteDefinition,
            BlockKindDto::Table => Self::Table,
            BlockKindDto::CodeBlock { language } => Self::CodeBlock { language },
            BlockKindDto::Comment => Self::Comment,
            BlockKindDto::HtmlBlock => Self::HtmlBlock,
            BlockKindDto::MathBlock => Self::MathBlock,
            BlockKindDto::MermaidBlock => Self::MermaidBlock,
            BlockKindDto::RawMarkdown => Self::RawMarkdown,
        }
    }
}

/// 前端块树节点（JSON）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockDto {
    pub id: String,
    #[serde(flatten)]
    pub kind: BlockKindDto,
    /// 块在全文中的 UTF-16 码元区间（仅根块；不含尾随换行）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<usize>,
    /// 行内内容（代码块为代码文本，脚注定义为脚注 id）。
    pub title: InlineTextTree,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<TableData>,
    /// Raw 保留类块（raw/comment/html/math/mermaid）的原文。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_fallback: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<BlockDto>,
}

impl BlockDto {
    /// 从解析节点构建 DTO；`range` 为根块的 UTF-16 区间，子块传 None。
    pub fn from_node(node: &BlockNode, range: Option<(usize, usize)>) -> Self {
        Self {
            id: node.record.id.to_string(),
            kind: node.record.kind.clone().into(),
            start: range.map(|range| range.0),
            end: range.map(|range| range.1),
            title: node.record.title.clone(),
            table: node.record.table.clone(),
            raw_fallback: node.record.raw_fallback.clone(),
            children: node
                .children
                .iter()
                .map(|child| Self::from_node(child, None))
                .collect(),
        }
    }

    /// DTO 还原为 BlockNode（serialize_markdown 命令用；偏移丢弃，id 无效时重新生成）。
    pub fn into_node(self) -> BlockNode {
        let id = Uuid::parse_str(&self.id).unwrap_or_else(|_| Uuid::new_v4());
        BlockNode {
            record: BlockRecord {
                id,
                kind: self.kind.into(),
                title: self.title,
                table: self.table,
                html: None,
                raw_fallback: self.raw_fallback,
            },
            children: self
                .children
                .into_iter()
                .map(BlockDto::into_node)
                .collect(),
        }
    }
}
