//! 块语义状态与块级 Markdown 解析辅助。
//!
//! 本模块定义与 Markdown 互序列化的持久化块记录（BlockRecord）。
//! 块级解析有意保持克制：只有运行时树能够重建的语法才被解析为结构化块，
//! 其余语法以原文（raw_fallback）保留、可无损往返。
//!
//! 移植自 velotype `components/block/state.rs`。

// 部分构造器与判定方法为后续编辑/文档阶段预留。
#![allow(dead_code)]

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::markdown::inline::html::{HtmlDocument, parse_html_document};
use crate::markdown::inline::image::parse_standalone_image;
use crate::markdown::inline::tree::InlineTextTree;
use crate::markdown::table::TableData;

/// 从 `[!TYPE]` 引用头解析出的警告框（callout）变体（GitHub 五变体）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CalloutVariant {
    /// 信息性备注警告框。
    Note,
    /// 有帮助的提示警告框。
    Tip,
    /// 高强调的重要警告框。
    Important,
    /// 针对有风险或意外内容的警告警告框。
    Warning,
    /// 针对潜在有害操作的当心警告框。
    Caution,
}

impl CalloutVariant {
    pub fn marker(self) -> &'static str {
        match self {
            Self::Note => "NOTE",
            Self::Tip => "TIP",
            Self::Important => "IMPORTANT",
            Self::Warning => "WARNING",
            Self::Caution => "CAUTION",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Note => "Note",
            Self::Tip => "Tip",
            Self::Important => "Important",
            Self::Warning => "Warning",
            Self::Caution => "Caution",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Note => "i",
            Self::Tip => "+",
            Self::Important => "*",
            Self::Warning => "!",
            Self::Caution => "x",
        }
    }

    pub fn parse_header_line(line: &str) -> Option<(Self, String)> {
        let trimmed = line.trim_start();
        let rest = trimmed.strip_prefix("[!")?;
        let marker_end = rest.find(']')?;
        let marker = &rest[..marker_end];
        let variant = match marker.to_ascii_uppercase().as_str() {
            "NOTE" => Self::Note,
            "TIP" => Self::Tip,
            "IMPORTANT" => Self::Important,
            "WARNING" => Self::Warning,
            "CAUTION" => Self::Caution,
            _ => return None,
        };
        let title = rest[marker_end + 1..].trim_start().to_string();
        Some((variant, title))
    }

    /// 扩展标记映射（Obsidian 别名 → GitHub 五变体），
    /// 仅在「警告框语法统一转换」开启时由扩展头部解析使用。
    /// NOTE 组：note/info/todo/abstract/summary/tldr/example；
    /// TIP 组：tip/hint/success/check/done；IMPORTANT：important；
    /// WARNING 组：warning/question/help/faq/attention；
    /// CAUTION 组：caution/failure/fail/missing/danger/error/bug。
    /// （quote/cite 不映射：按普通引用处理，语义一致）
    pub fn from_marker(marker: &str) -> Option<Self> {
        Some(match marker.to_ascii_uppercase().as_str() {
            "NOTE" | "INFO" | "TODO" | "ABSTRACT" | "SUMMARY" | "TLDR" | "EXAMPLE" => Self::Note,
            "TIP" | "HINT" | "SUCCESS" | "CHECK" | "DONE" => Self::Tip,
            "IMPORTANT" => Self::Important,
            "WARNING" | "QUESTION" | "HELP" | "FAQ" | "ATTENTION" => Self::Warning,
            "CAUTION" | "FAILURE" | "FAIL" | "MISSING" | "DANGER" | "ERROR" | "BUG" => {
                Self::Caution
            }
            _ => return None,
        })
    }

    /// 扩展头部解析：接受 Obsidian 别名标记与折叠后缀（`[!warning]-`/`[!warning]+`，
    /// 后缀剥离不入标题）。GitHub 五标准标记同样命中（from_marker 覆盖）。
    pub fn parse_header_line_extended(line: &str) -> Option<(Self, String)> {
        let trimmed = line.trim_start();
        let rest = trimmed.strip_prefix("[!")?;
        let marker_end = rest.find(']')?;
        let variant = Self::from_marker(&rest[..marker_end])?;
        let mut title = &rest[marker_end + 1..];
        // Obsidian 折叠后缀：紧随 ] 的 -（默认折叠）或 +（默认展开）
        if let Some(stripped) = title.strip_prefix(['-', '+']) {
            title = stripped;
        }
        Some((variant, title.trim_start().to_string()))
    }

    pub fn header_markdown(self, title_markdown: &str) -> String {
        if title_markdown.trim().is_empty() {
            format!("[!{}]", self.marker())
        } else {
            format!("[!{}] {}", self.marker(), title_markdown)
        }
    }

    pub fn escape_plain_quote_header(title_markdown: &str) -> String {
        let mut lines = title_markdown.splitn(2, '\n');
        let first = lines.next().unwrap_or_default();
        let rest = lines.next();
        // 统一转换开启时别名头部（[!hint] 等）同样会被识别为警告框，需一并转义
        let is_callout_header = if crate::markdown::callout_unify_enabled() {
            Self::parse_header_line_extended(first).is_some()
        } else {
            Self::parse_header_line(first).is_some()
        };
        let escaped_first = if is_callout_header {
            format!("\\{first}")
        } else {
            first.to_string()
        };
        match rest {
            Some(rest) => format!("{escaped_first}\n{rest}"),
            None => escaped_first,
        }
    }
}

/// 块的语义类型，同时决定其 Markdown 语法与视觉渲染。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlockKind {
    /// 带行内格式的普通段落。
    Paragraph,
    /// 水平分割线。
    Separator,
    /// ATX 或 Setext 标题（带 CommonMark 标题级别）。
    Heading { level: u8 },
    /// 无序列表项。
    BulletedListItem,
    /// 任务列表项（带勾选状态）。
    TaskListItem { checked: bool },
    /// 有序列表项；序列化使用规范的点标记。
    NumberedListItem,
    /// 块引用容器。
    Quote,
    /// GitHub 风格的警告/提示框（callout）容器。
    Callout(CalloutVariant),
    /// 脚注定义容器。
    FootnoteDefinition,
    /// 原生管道表格块。
    Table,
    /// 围栏代码块（带可选语言信息串）。
    CodeBlock { language: Option<String> },
    /// 可见 HTML 注释块（按原始注释文本保留）。
    Comment,
    /// 安全 raw HTML 块（经原生语义元素渲染）。
    HtmlBlock,
    /// 展示数学块（经 LaTeX 管线渲染）。
    MathBlock,
    /// Mermaid 围栏块（渲染为 SVG）。
    MermaidBlock,
    /// 原生运行时子集之外语法的原文 Markdown 回落。
    RawMarkdown,
}

/// 从围栏代码块解析出的起始围栏。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeFenceOpening {
    /// 围栏字符：反引号或波浪号。
    pub ch: char,
    /// 起始围栏的连续字符数。
    pub len: usize,
    /// 起始围栏之后的可选语言/信息串。
    pub language: Option<String>,
}

impl BlockKind {
    /// 该类型的块在当前运行时树中是否可以拥有子块。
    pub fn supports_children(&self) -> bool {
        self.is_list_item() || self.is_quote_container() || self.is_footnote_definition()
    }

    pub fn is_list_item(&self) -> bool {
        matches!(
            self,
            Self::BulletedListItem | Self::TaskListItem { .. } | Self::NumberedListItem
        )
    }

    pub fn is_numbered_list_item(&self) -> bool {
        matches!(self, Self::NumberedListItem)
    }

    pub fn is_task_list_item(&self) -> bool {
        matches!(self, Self::TaskListItem { .. })
    }

    pub fn is_code_block(&self) -> bool {
        matches!(self, Self::CodeBlock { .. })
    }

    /// 以该类型块为目标时，右键「插入表格」入口是否有意义。
    /// 原子/结构块（表格、代码、公式等）渲染为自包含控件，
    /// 在其中插入表格没有意义，故排除。
    pub fn allows_context_table_insert(&self) -> bool {
        !matches!(
            self,
            Self::Table
                | Self::CodeBlock { .. }
                | Self::MathBlock
                | Self::MermaidBlock
                | Self::HtmlBlock
                | Self::Comment
                | Self::RawMarkdown
        )
    }

    pub fn is_quote_container(&self) -> bool {
        matches!(self, Self::Quote | Self::Callout(_))
    }

    /// 渲染为自包含控件、其后没有光标位置的块。
    /// 位于渲染文档末尾时需要在后面补一个尾随段落，
    /// 让「先渲染」的用户能越过结构继续输入，而不必切到源码模式。
    pub fn is_atomic_structural(&self) -> bool {
        matches!(
            self,
            Self::Separator
                | Self::Table
                | Self::CodeBlock { .. }
                | Self::MathBlock
                | Self::MermaidBlock
                | Self::HtmlBlock
                | Self::Comment
                | Self::RawMarkdown
        )
    }

    pub fn is_callout(&self) -> bool {
        matches!(self, Self::Callout(_))
    }

    /// 以多行原文文本编辑、渲染为自包含控件的块
    ///（代码、公式、HTML、mermaid、注释、raw markdown）。
    /// 用 `Down` 或 `Ctrl/Cmd+Enter` 向下退出时需要下方有一行可落。
    pub fn is_multiline_text_block(&self) -> bool {
        self.is_code_block()
            || matches!(
                self,
                Self::MathBlock
                    | Self::HtmlBlock
                    | Self::MermaidBlock
                    | Self::Comment
                    | Self::RawMarkdown
            )
    }

    pub fn is_footnote_definition(&self) -> bool {
        matches!(self, Self::FootnoteDefinition)
    }

    pub fn callout_variant(&self) -> Option<CalloutVariant> {
        match self {
            Self::Callout(variant) => Some(*variant),
            _ => None,
        }
    }

    pub fn is_separator(&self) -> bool {
        matches!(self, Self::Separator)
    }

    pub fn can_nest_under(&self, parent: &Self) -> bool {
        if !parent.is_list_item() {
            return false;
        }

        self.is_list_item()
            || matches!(
                self,
                Self::Paragraph
                    | Self::Quote
                    | Self::Callout(_)
                    | Self::FootnoteDefinition
                    | Self::Table
                    | Self::CodeBlock { .. }
                    | Self::Comment
                    | Self::HtmlBlock
                    | Self::MathBlock
                    | Self::MermaidBlock
                    | Self::RawMarkdown
            )
    }

    pub fn newline_sibling_kind(&self) -> Self {
        if matches!(self, Self::TaskListItem { .. }) {
            Self::TaskListItem { checked: false }
        } else if self.is_list_item() {
            self.clone()
        } else if self.is_quote_container() {
            self.clone()
        } else if self.is_footnote_definition() {
            Self::Paragraph
        } else if self.is_code_block() || self.is_separator() {
            Self::Paragraph
        } else {
            Self::Paragraph
        }
    }

    /// 从用户输入实时检测 Markdown 前缀，返回对应的 [`BlockKind`]
    /// 以及应剥离的前缀字符数。
    pub fn detect_markdown_shortcut(value: &str) -> Option<(Self, usize)> {
        if value.starts_with("###### ") {
            Some((Self::Heading { level: 6 }, 7))
        } else if value.starts_with("##### ") {
            Some((Self::Heading { level: 5 }, 6))
        } else if value.starts_with("#### ") {
            Some((Self::Heading { level: 4 }, 5))
        } else if value.starts_with("### ") {
            Some((Self::Heading { level: 3 }, 4))
        } else if value.starts_with("## ") {
            Some((Self::Heading { level: 2 }, 3))
        } else if value.starts_with("# ") {
            Some((Self::Heading { level: 1 }, 2))
        } else if let Some((checked, prefix_len)) = Self::parse_task_list_shortcut(value) {
            Some((Self::TaskListItem { checked }, prefix_len))
        } else if value.starts_with("* ") || value.starts_with("+ ") {
            Some((Self::BulletedListItem, 2))
        } else if value.starts_with("- ") {
            Some((Self::BulletedListItem, 2))
        } else if let Some(prefix_len) = Self::numbered_list_shortcut_prefix_len(value) {
            Some((Self::NumberedListItem, prefix_len))
        } else if value.starts_with("> ") {
            Some((Self::Quote, 2))
        } else {
            None
        }
    }

    pub fn parse_atx_heading_line(line: &str) -> Option<(u8, String)> {
        let trimmed_end = line.trim_end();
        let leading_spaces = trimmed_end.bytes().take_while(|b| *b == b' ').count();
        if leading_spaces > 3 {
            return None;
        }

        let rest = &trimmed_end[leading_spaces..];
        let level = rest.bytes().take_while(|b| *b == b'#').count();
        if !(1..=6).contains(&level) {
            return None;
        }

        let content = rest[level..].strip_prefix(' ')?;
        let mut content = content.trim_end().to_string();
        if let Some(closing_hash_start) = content.rfind(' ') {
            if content[closing_hash_start + 1..]
                .chars()
                .all(|ch| ch == '#')
            {
                content.truncate(closing_hash_start);
                content = content.trim_end().to_string();
            }
        }

        Some((level as u8, content))
    }

    pub fn parse_setext_underline(line: &str) -> Option<u8> {
        let trimmed_end = line.trim_end();
        let leading_spaces = trimmed_end.bytes().take_while(|b| *b == b' ').count();
        if leading_spaces > 3 {
            return None;
        }

        let rest = &trimmed_end[leading_spaces..];
        if rest.len() < 3 {
            return None;
        }

        if rest.bytes().all(|b| b == b'=') {
            Some(1)
        } else if rest.bytes().all(|b| b == b'-') {
            Some(2)
        } else {
            None
        }
    }

    pub fn parse_code_fence_opening(value: &str) -> Option<CodeFenceOpening> {
        let trimmed = value.trim_end();
        let ch = trimmed.chars().next()?;
        if ch != '`' && ch != '~' {
            return None;
        }

        let len = trimmed.chars().take_while(|&c| c == ch).count();
        if len < 3 {
            return None;
        }

        let rest = &trimmed[ch.len_utf8() * len..];
        if ch == '`' && rest.contains('`') {
            return None;
        }

        let language = rest.trim();
        Some(CodeFenceOpening {
            ch,
            len,
            language: if language.is_empty() {
                None
            } else {
                Some(language.to_string())
            },
        })
    }

    pub fn parse_separator_line(value: &str) -> bool {
        let trimmed_end = value.trim_end();
        let leading_spaces = trimmed_end.bytes().take_while(|b| *b == b' ').count();
        if leading_spaces > 3 {
            return false;
        }

        let rest = &trimmed_end[leading_spaces..];
        let mut marker = None;
        let mut marker_count = 0usize;
        for ch in rest.chars() {
            if ch == ' ' {
                continue;
            }
            if !matches!(ch, '-' | '*' | '_') {
                return false;
            }
            if let Some(existing) = marker {
                if existing != ch {
                    return false;
                }
            } else {
                marker = Some(ch);
            }
            marker_count += 1;
        }

        marker_count >= 3
    }

    /// 解析列表项内容开头的任务列表标记。
    ///
    /// 接受 `[ ]`、`[x]`、`[X]`，其后可跟一个空格或 tab 再接项文本；空标题同样合法。
    pub fn parse_task_list_item_prefix(value: &str) -> Option<(bool, usize)> {
        let bytes = value.as_bytes();
        if bytes.len() < 3 || bytes[0] != b'[' || bytes[2] != b']' {
            return None;
        }

        let checked = match bytes[1] {
            b' ' => false,
            b'x' | b'X' => true,
            _ => return None,
        };

        if bytes.len() == 3 {
            return Some((checked, 3));
        }

        if matches!(bytes[3], b' ' | b'\t') {
            Some((checked, 4))
        } else {
            None
        }
    }

    fn parse_task_list_shortcut(value: &str) -> Option<(bool, usize)> {
        let rest = value.strip_prefix("- ")?;
        let (checked, prefix_len) = Self::parse_task_list_item_prefix(rest)?;
        Some((checked, 2 + prefix_len))
    }

    fn numbered_list_shortcut_prefix_len(value: &str) -> Option<usize> {
        let digit_len = value.bytes().take_while(|b| b.is_ascii_digit()).count();
        if !(1..=9).contains(&digit_len) {
            return None;
        }

        let marker = *value.as_bytes().get(digit_len)?;
        if !matches!(marker, b'.' | b')') {
            return None;
        }

        let separator = *value.as_bytes().get(digit_len + 1)?;
        matches!(separator, b' ' | b'\t').then_some(digit_len + 2)
    }
}

/// 块的持久化数据（与编辑器运行时无关）。
///
/// 持有块的标识、类型与带行内格式的标题。原文保留的 Markdown 块把
/// 原始源码存在 `raw_fallback` 中，因此可以无损往返。
/// through save/load losslessly. 本项目使用嵌套块树表达层级，不再通过
/// UUID 维护 parent/content 链接。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockRecord {
    pub id: Uuid,
    pub kind: BlockKind,
    pub title: InlineTextTree,
    pub table: Option<TableData>,
    pub html: Option<HtmlDocument>,
    pub raw_fallback: Option<String>,
    /// 有序列表项的源标记数字（仅 NumberedListItem）：作为连续组的起始序号，
    /// 序列化/渲染均以其为组起点递增——源序号与渲染序号保持一致（源即所见）
    #[serde(default)]
    pub list_start: Option<usize>,
}

impl BlockRecord {
    pub fn new(kind: BlockKind, title: InlineTextTree) -> Self {
        let mut record = Self {
            id: Uuid::new_v4(),
            kind,
            title,
            table: None,
            html: None,
            raw_fallback: None,
            list_start: None,
        };
        record.sync_raw_fallback();
        record
    }

    pub fn with_plain_text(kind: BlockKind, text: impl Into<String>) -> Self {
        Self::new(kind, InlineTextTree::plain(text.into()))
    }

    pub fn paragraph(text: impl Into<String>) -> Self {
        Self::with_plain_text(BlockKind::Paragraph, text)
    }

    pub fn raw_markdown(markdown: impl Into<String>) -> Self {
        let markdown = markdown.into();
        let mut record = Self::with_plain_text(BlockKind::RawMarkdown, markdown.clone());
        record.raw_fallback = Some(markdown);
        record
    }

    pub fn comment(markdown: impl Into<String>) -> Self {
        let markdown = markdown.into();
        let mut record = Self::with_plain_text(BlockKind::Comment, markdown.clone());
        record.raw_fallback = Some(markdown);
        record
    }

    pub fn html(markdown: impl Into<String>) -> Self {
        let markdown = markdown.into();
        let html = parse_html_document(&markdown);
        Self::html_with_document(markdown, html)
    }

    /// 以已解析的 HTML 文档构造（调用方已完成安全分级），避免同一份文本被
    /// HTML tokenizer 重复解析（原链路分类/构造/sync 三处各解析一遍）。
    pub fn html_with_document(markdown: impl Into<String>, document: HtmlDocument) -> Self {
        let markdown = markdown.into();
        Self {
            id: Uuid::new_v4(),
            kind: BlockKind::HtmlBlock,
            title: InlineTextTree::plain(markdown.clone()),
            table: None,
            html: Some(document),
            raw_fallback: Some(markdown),
            list_start: None,
        }
    }

    pub fn math(markdown: impl Into<String>) -> Self {
        let markdown = markdown.into();
        let mut record = Self::with_plain_text(BlockKind::MathBlock, markdown.clone());
        record.raw_fallback = Some(markdown);
        record
    }

    pub fn mermaid(markdown: impl Into<String>) -> Self {
        let markdown = markdown.into();
        let mut record = Self::with_plain_text(BlockKind::MermaidBlock, markdown.clone());
        record.raw_fallback = Some(markdown);
        record
    }

    pub fn table(table: TableData) -> Self {
        let mut record = Self::new(BlockKind::Table, InlineTextTree::plain(String::new()));
        record.table = Some(table);
        record
    }

    pub fn set_title(&mut self, title: InlineTextTree) {
        self.title = title;
        self.sync_raw_fallback();
    }

    /// 把块标题导出为 Markdown：片段样式标志经 [`InlineTextTree::serialize_markdown`]
    /// 序列化回定界标记。
    pub fn title_markdown(&self) -> String {
        self.title.serialize_markdown()
    }

    /// 该块类型是否因按不透明 Markdown 保留而把原始源码存在 `raw_fallback` 中。
    pub fn kind_uses_raw_fallback(&self) -> bool {
        matches!(
            self.kind,
            BlockKind::RawMarkdown
                | BlockKind::Comment
                | BlockKind::HtmlBlock
                | BlockKind::MathBlock
                | BlockKind::MermaidBlock
        )
    }

    /// 把该块序列化回单行 Markdown，含嵌套块的缩进与有序列表项的序号。
    /// 原文保留块在深度 0 时输出其回落文本。
    pub fn markdown_line(&self, depth: usize, list_ordinal: Option<usize>) -> String {
        let indentation = "  ".repeat(depth);
        let title_markdown = self.title_markdown_for_output();
        match self.kind {
            BlockKind::Paragraph => indent_multiline(&title_markdown, &indentation),
            BlockKind::Separator => "---".to_string(),
            BlockKind::Heading { level } => {
                format!(
                    "{indentation}{} {title_markdown}",
                    "#".repeat(level as usize)
                )
            }
            BlockKind::BulletedListItem => prefixed_multiline(
                &title_markdown,
                &format!("{indentation}- "),
                &format!("{indentation}  "),
            ),
            BlockKind::TaskListItem { checked } => prefixed_multiline(
                &title_markdown,
                &format!("{indentation}- [{}] ", if checked { "x" } else { " " }),
                &format!("{indentation}      "),
            ),
            BlockKind::NumberedListItem => {
                // 序号：调用方计算值 > 源标记数字（list_start）> 1
                let ordinal = list_ordinal.or(self.list_start).unwrap_or(1);
                prefixed_multiline(
                    &title_markdown,
                    &format!("{indentation}{ordinal}. "),
                    &format!("{indentation}   "),
                )
            }
            BlockKind::Quote => prefixed_multiline(
                &CalloutVariant::escape_plain_quote_header(&title_markdown),
                &format!("{indentation}> "),
                &format!("{indentation}> "),
            ),
            BlockKind::Callout(variant) => format!(
                "{indentation}> {}",
                variant.header_markdown(&title_markdown)
            ),
            BlockKind::FootnoteDefinition => {
                format!("{indentation}[^{}]: ", self.title.visible_text())
            }
            BlockKind::Table => String::new(),
            BlockKind::CodeBlock { .. } => title_markdown,
            BlockKind::RawMarkdown
            | BlockKind::Comment
            | BlockKind::HtmlBlock
            | BlockKind::MathBlock
            | BlockKind::MermaidBlock => {
                if depth == 0 {
                    self.raw_fallback.clone().unwrap_or(title_markdown)
                } else {
                    indent_multiline(
                        &self.raw_fallback.clone().unwrap_or(title_markdown),
                        &indentation,
                    )
                }
            }
        }
    }

    fn title_markdown_for_output(&self) -> String {
        let visible = self.title.visible_text();
        if self.can_present_title_as_standalone_image()
            && parse_standalone_image(&visible).is_some()
        {
            return visible;
        }

        self.title_markdown()
    }

    fn can_present_title_as_standalone_image(&self) -> bool {
        matches!(
            self.kind,
            BlockKind::Paragraph
                | BlockKind::BulletedListItem
                | BlockKind::NumberedListItem
                | BlockKind::TaskListItem { .. }
        )
    }

    fn sync_raw_fallback(&mut self) {
        if self.kind_uses_raw_fallback() {
            self.raw_fallback = Some(self.title.visible_text().to_string());
            if self.kind == BlockKind::HtmlBlock {
                self.html = self
                    .raw_fallback
                    .as_ref()
                    .map(|raw| parse_html_document(raw));
            }
        } else {
            self.raw_fallback = None;
            self.html = None;
        }
    }
}

fn indent_multiline(content: &str, indentation: &str) -> String {
    content
        .split('\n')
        .map(|line| format!("{indentation}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn prefixed_multiline(content: &str, first_prefix: &str, continuation_prefix: &str) -> String {
    let mut lines = content.split('\n');
    let mut rendered = String::new();
    if let Some(first) = lines.next() {
        rendered.push_str(first_prefix);
        rendered.push_str(first);
    }

    for line in lines {
        rendered.push('\n');
        rendered.push_str(continuation_prefix);
        rendered.push_str(line);
    }

    rendered
}

/// 从剪贴板抽象中提取的图片载荷。
///
/// File-manager copies are usually represented as local paths; the GPUI 剪贴板
/// 位图变体（`ClipboardImage`）不随纯数据层移植。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PastedImageSource {
    LocalPath(PathBuf),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_table_insert_excludes_atomic_blocks() {
        assert!(BlockKind::Paragraph.allows_context_table_insert());
        assert!(BlockKind::Heading { level: 1 }.allows_context_table_insert());
        assert!(BlockKind::Quote.allows_context_table_insert());
        assert!(!BlockKind::Table.allows_context_table_insert());
        assert!(!BlockKind::CodeBlock { language: None }.allows_context_table_insert());
        assert!(!BlockKind::MathBlock.allows_context_table_insert());
        assert!(!BlockKind::MermaidBlock.allows_context_table_insert());
    }

    #[test]
    fn detects_markdown_shortcuts() {
        assert_eq!(
            BlockKind::detect_markdown_shortcut("- item"),
            Some((BlockKind::BulletedListItem, 2))
        );
        assert_eq!(
            BlockKind::detect_markdown_shortcut("1. item"),
            Some((BlockKind::NumberedListItem, 3))
        );
        assert_eq!(
            BlockKind::detect_markdown_shortcut("12. item"),
            Some((BlockKind::NumberedListItem, 4))
        );
        assert_eq!(
            BlockKind::detect_markdown_shortcut("1) item"),
            Some((BlockKind::NumberedListItem, 3))
        );
        assert_eq!(
            BlockKind::detect_markdown_shortcut("12)\titem"),
            Some((BlockKind::NumberedListItem, 4))
        );
        assert_eq!(
            BlockKind::detect_markdown_shortcut("1234567890) item"),
            None
        );
        assert_eq!(
            BlockKind::detect_markdown_shortcut("# heading"),
            Some((BlockKind::Heading { level: 1 }, 2))
        );
        assert_eq!(
            BlockKind::detect_markdown_shortcut("## heading"),
            Some((BlockKind::Heading { level: 2 }, 3))
        );
        assert_eq!(
            BlockKind::detect_markdown_shortcut("### heading"),
            Some((BlockKind::Heading { level: 3 }, 4))
        );
        assert_eq!(
            BlockKind::detect_markdown_shortcut("#### heading"),
            Some((BlockKind::Heading { level: 4 }, 5))
        );
        assert_eq!(
            BlockKind::detect_markdown_shortcut("##### heading"),
            Some((BlockKind::Heading { level: 5 }, 6))
        );
        assert_eq!(
            BlockKind::detect_markdown_shortcut("###### heading"),
            Some((BlockKind::Heading { level: 6 }, 7))
        );
        assert_eq!(
            BlockKind::detect_markdown_shortcut("- [ ] task"),
            Some((BlockKind::TaskListItem { checked: false }, 6))
        );
        assert_eq!(
            BlockKind::detect_markdown_shortcut("- [x] task"),
            Some((BlockKind::TaskListItem { checked: true }, 6))
        );
        assert_eq!(
            BlockKind::detect_markdown_shortcut("* item"),
            Some((BlockKind::BulletedListItem, 2))
        );
        assert_eq!(
            BlockKind::detect_markdown_shortcut("+ item"),
            Some((BlockKind::BulletedListItem, 2))
        );
        assert_eq!(BlockKind::detect_markdown_shortcut("#no-space"), None);
        assert_eq!(BlockKind::detect_markdown_shortcut("```"), None);
        assert_eq!(
            BlockKind::detect_markdown_shortcut("> quote"),
            Some((BlockKind::Quote, 2))
        );
        assert_eq!(BlockKind::detect_markdown_shortcut(">no-space"), None);
    }

    #[test]
    fn parses_separator_lines() {
        assert!(BlockKind::parse_separator_line("---"));
        assert!(BlockKind::parse_separator_line("----"));
        assert!(BlockKind::parse_separator_line("***"));
        assert!(BlockKind::parse_separator_line("_ _ _"));
        assert!(BlockKind::parse_separator_line(" - - - "));
        assert!(!BlockKind::parse_separator_line("--"));
        assert!(BlockKind::parse_separator_line(" ---"));
        assert!(!BlockKind::parse_separator_line("---x"));
    }

    #[test]
    fn parses_code_fence_openings() {
        assert_eq!(
            BlockKind::parse_code_fence_opening("```rust"),
            Some(CodeFenceOpening {
                ch: '`',
                len: 3,
                language: Some("rust".into()),
            })
        );
        assert_eq!(
            BlockKind::parse_code_fence_opening("~~~ts"),
            Some(CodeFenceOpening {
                ch: '~',
                len: 3,
                language: Some("ts".into()),
            })
        );
        assert_eq!(
            BlockKind::parse_code_fence_opening("```"),
            Some(CodeFenceOpening {
                ch: '`',
                len: 3,
                language: None,
            })
        );
        assert_eq!(BlockKind::parse_code_fence_opening("``"), None);
        assert_eq!(BlockKind::parse_code_fence_opening("```ru`st"), None);
    }

    #[test]
    fn parses_task_list_item_prefixes() {
        assert_eq!(
            BlockKind::parse_task_list_item_prefix("[ ] a"),
            Some((false, 4))
        );
        assert_eq!(
            BlockKind::parse_task_list_item_prefix("[x] a"),
            Some((true, 4))
        );
        assert_eq!(
            BlockKind::parse_task_list_item_prefix("[X] a"),
            Some((true, 4))
        );
        assert_eq!(BlockKind::parse_task_list_item_prefix("[a] a"), None);
    }

    #[test]
    fn serializes_supported_block_kinds() {
        let list = BlockRecord::new(
            BlockKind::BulletedListItem,
            InlineTextTree::from_markdown("*item*"),
        );
        let numbered = BlockRecord::new(
            BlockKind::NumberedListItem,
            InlineTextTree::from_markdown("step"),
        );
        let task = BlockRecord::new(
            BlockKind::TaskListItem { checked: true },
            InlineTextTree::from_markdown("done"),
        );
        let heading = BlockRecord::new(
            BlockKind::Heading { level: 2 },
            InlineTextTree::from_markdown("**title**"),
        );
        let quote = BlockRecord::new(BlockKind::Quote, InlineTextTree::plain("quoted text"));
        let paragraph = BlockRecord::paragraph("plain");
        let comment = BlockRecord::comment("<!--\ncomment\n-->");

        assert_eq!(list.markdown_line(0, None), "- *item*");
        assert_eq!(list.markdown_line(2, None), "    - *item*");
        assert_eq!(task.markdown_line(0, None), "- [x] done");
        assert_eq!(task.markdown_line(2, None), "    - [x] done");
        assert_eq!(numbered.markdown_line(0, Some(3)), "3. step");
        assert_eq!(numbered.markdown_line(2, Some(12)), "    12. step");
        assert_eq!(heading.markdown_line(0, None), "## **title**");
        assert_eq!(quote.markdown_line(0, None), "> quoted text");
        assert_eq!(quote.markdown_line(2, None), "    > quoted text");
        assert_eq!(paragraph.markdown_line(1, None), "  plain");
        assert_eq!(comment.markdown_line(0, None), "<!--\ncomment\n-->");
        assert_eq!(comment.markdown_line(1, None), "  <!--\n  comment\n  -->");
    }

    #[test]
    fn standalone_image_markdown_line_preserves_underscores() {
        let markdown = "![1.1_进制转换例子](./NetworkEngineerSummer.assets/1.1_进制转换例子.jpg)";
        let paragraph = BlockRecord::paragraph(markdown);

        assert_eq!(paragraph.markdown_line(0, None), markdown);
    }

    #[test]
    fn quote_serializes_back_to_markdown() {
        let record = BlockRecord::new(BlockKind::Quote, InlineTextTree::plain("text"));
        let line = record.markdown_line(0, None);
        assert_eq!(line, "> text");
    }

    #[test]
    fn parses_h2_and_h3_lines_with_correct_levels() {
        let h2 = BlockKind::parse_atx_heading_line("## hello");
        assert_eq!(h2, Some((2, "hello".to_string())));

        let h3 = BlockKind::parse_atx_heading_line("### hello");
        assert_eq!(h3, Some((3, "hello".to_string())));
    }

    #[test]
    fn parses_atx_headings_with_closing_hashes_and_setext_underlines() {
        let atx = BlockKind::parse_atx_heading_line("  ### title ######");
        assert_eq!(atx, Some((3, "title".to_string())));

        assert_eq!(BlockKind::parse_setext_underline("==="), Some(1));
        assert_eq!(BlockKind::parse_setext_underline("---"), Some(2));
        assert_eq!(BlockKind::parse_setext_underline("- - -"), None);
    }

    #[test]
    fn code_block_kind_stores_language() {
        let kind = BlockKind::CodeBlock {
            language: Some("rust".to_string()),
        };
        assert!(kind.is_code_block());
        assert!(!kind.is_list_item());

        let no_lang = BlockKind::CodeBlock { language: None };
        assert!(no_lang.is_code_block());
    }

    #[test]
    fn code_block_markdown_line_returns_plain_content() {
        let record = BlockRecord::new(
            BlockKind::CodeBlock {
                language: Some("rust".into()),
            },
            InlineTextTree::plain("let x = 1;\nprintln!(\"hi\");"),
        );
        // markdown_line 返回裸内容；围栏由序列化层（tree.rs）添加。
        let line = record.markdown_line(0, None);
        assert_eq!(line, "let x = 1;\nprintln!(\"hi\");");
    }

    #[test]
    fn separator_markdown_line_round_trips() {
        let record = BlockRecord::new(BlockKind::Separator, InlineTextTree::plain(String::new()));
        assert_eq!(record.markdown_line(0, None), "---");
        assert!(BlockKind::parse_separator_line("---"));
    }

    #[test]
    fn task_list_serializes_canonical_markdown() {
        let record = BlockRecord::new(
            BlockKind::TaskListItem { checked: false },
            InlineTextTree::plain("todo"),
        );
        assert_eq!(record.markdown_line(0, None), "- [ ] todo");
    }
}
