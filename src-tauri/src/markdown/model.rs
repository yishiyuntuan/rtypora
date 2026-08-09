//! 对外 serde DTO（JSON 契约）：前端块树的序列化形状。
//!
//! 内部模型（`block::state::BlockRecord` + `block::document::BlockNode`）与 DTO 分离：
//! DTO 把 kind 拍平为 `type` 标签、children 直接嵌套、并携带 UTF-16 码元偏移
//! （仅根块有 `start`/`end`，嵌套子块为 null——编辑以根块为单位）。

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::block::document::BlockNode;
use super::block::state::{BlockKind, BlockRecord, CalloutVariant};
use super::inline::html;
use super::inline::image::parse_standalone_image;
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
    /// `<section>...</section>` 图文排版容器（Mdmdt 式 grid 布局；DTO 层从 HtmlBlock/RawMarkdown
    /// 分类——velotype 的 HTML 安全分级不含 section 标签，解析为 RawMarkdown，此处统一归类；
    /// 序列化按 RawMarkdown 原文透传）
    SectionBlock,
    MathBlock,
    MermaidBlock,
    RawMarkdown,
}

/// section 图文排版容器判定：HTML 块原文以 `<section` 开标签开头（后跟 `>`/空白）。
/// 按字节比较，避免多字节字符落在前缀边界时切片 panic。
fn is_section_html_block(raw: &str) -> bool {
    let trimmed = raw.trim_start();
    let bytes = trimmed.as_bytes();
    bytes.len() > "<section".len()
        && bytes[.."<section".len()].eq_ignore_ascii_case(b"<section")
        && matches!(bytes["<section".len()], b'>' | b' ' | b'\t')
}

/// 公式源码是否使用 AMS 编号环境：align/gather/equation/multline/flalign/alignat/
/// eqnarray 的非星号变体（星号变体按 AMS 规则不编号）。
fn uses_ams_numbered_environment(source: &str) -> bool {
    const AMS_ENVIRONMENTS: [&str; 7] = [
        "align", "gather", "equation", "multline", "flalign", "alignat", "eqnarray",
    ];
    let mut rest = source;
    while let Some(pos) = rest.find("\\begin{") {
        rest = &rest[pos + "\\begin{".len()..];
        let Some(end) = rest.find('}') else { break };
        let env = rest[..end].trim();
        match env.strip_suffix('*') {
            // 星号变体不编号，继续扫描后续环境
            Some(_) => rest = &rest[end..],
            None => {
                if AMS_ENVIRONMENTS.contains(&env) {
                    return true;
                }
                rest = &rest[end..];
            }
        }
    }
    false
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
            BlockKindDto::SectionBlock => Self::RawMarkdown,
            BlockKindDto::MathBlock => Self::MathBlock,
            BlockKindDto::MermaidBlock => Self::MermaidBlock,
            BlockKindDto::RawMarkdown => Self::RawMarkdown,
        }
    }
}

/// 独立图片段落（`![alt](src)`）的图片信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageDto {
    pub alt: String,
    pub src: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// 缩放倍率（独立 `<img style="zoom:50%">` HTML 图片行；`![]()` 图片无此值）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<f32>,
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
    /// 段落为独立图片语法时携带图片信息（前端渲染 <img>）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageDto>,
    /// Raw 保留类块（raw/comment/html/math/mermaid）的原文。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_fallback: Option<String>,
    /// 展示公式块：源码是否使用 AMS 编号环境（align/gather/equation/multline/flalign/
    /// alignat/eqnarray 的非星号变体）。前端按偏好（math_numbering）决定编号显示。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub math_numbered: Option<bool>,
    /// 有序列表项的源标记数字（组起始序号；仅 NumberedListItem 有值）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_start: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<BlockDto>,
}

impl BlockDto {
    /// 从解析节点构建 DTO；`range` 为根块的 UTF-16 区间，子块传 None。
    pub fn from_node(node: &BlockNode, range: Option<(usize, usize)>) -> Self {
        // 独立图片段落：段落正文整体是 ![alt](src) 语法时携带图片信息
        //（引用式 ![alt][label] 经当前解析作用域的图片引用定义解析目标）
        let image = if node.record.kind == BlockKind::Paragraph && node.children.is_empty() {
            parse_standalone_image(&node.record.title.visible_text()).and_then(|syntax| {
                crate::markdown::inline::with_current_image_refs(|refs| {
                    syntax.resolve_target(refs).map(|target| ImageDto {
                        alt: syntax.alt,
                        src: target.src,
                        title: target.title,
                        zoom: None,
                    })
                })
            })
        } else if matches!(node.record.kind, BlockKind::HtmlBlock | BlockKind::RawMarkdown) {
            // 独立 <img> HTML 行：携带图片信息（含 zoom），前端按图片渲染；
            // 原文保留在 raw_fallback（编辑/序列化不丢样式属性）
            node.record
                .raw_fallback
                .as_deref()
                .and_then(html::parse_html_image_block)
                .map(|img| ImageDto {
                    alt: img.alt.clone(),
                    src: img.src.clone(),
                    title: None,
                    zoom: ((img.zoom_factor() - 1.0).abs() > f32::EPSILON)
                        .then_some(img.zoom_factor()),
                })
        } else {
            None
        };
        // section 图文排版容器：原文以 <section 开头的 HtmlBlock/RawMarkdown 分类为 SectionBlock（DTO 层）
        let kind = if matches!(node.record.kind, BlockKind::HtmlBlock | BlockKind::RawMarkdown)
            && node
                .record
                .raw_fallback
                .as_deref()
                .is_some_and(is_section_html_block)
        {
            BlockKindDto::SectionBlock
        } else {
            node.record.kind.clone().into()
        };
        Self {
            id: node.record.id.to_string(),
            kind,
            start: range.map(|range| range.0),
            end: range.map(|range| range.1),
            title: node.record.title.clone(),
            table: node.record.table.clone(),
            image,
            raw_fallback: node.record.raw_fallback.clone(),
            math_numbered: (node.record.kind == BlockKind::MathBlock)
                .then(|| uses_ams_numbered_environment(&node.record.title.visible_text())),
            list_start: node.record.list_start,
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
                list_start: self.list_start,
            },
            children: self
                .children
                .into_iter()
                .map(BlockDto::into_node)
                .collect(),
        }
    }
}
