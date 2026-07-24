//! Markdown 文档解析器：移植自 velotype `editor/document.rs`。
//!
//! 原始 Markdown 被解析为可安全编辑的原生块结构（BlockNode 树）。
//! 超出当前运行时模型能力的语法以 RawMarkdown 块原样保留，可无损往返。

use super::math::{is_math_info_string, parse_display_math_source};
use super::mermaid::is_mermaid_info_string;
use super::state::{BlockKind, BlockRecord, CalloutVariant, CodeFenceOpening};
use crate::markdown::inline::footnote::parse_footnote_definition_head;
use crate::markdown::inline::html::{HtmlSafetyClass, is_inline_tag, parse_html_document};
use crate::markdown::inline::image::parse_standalone_image;
use crate::markdown::inline::tree::InlineTextTree;
use crate::markdown::table::{
    collect_pipeless_table_region, collect_root_table_candidate_region,
    collect_table_candidate_region, is_root_table_candidate_line, is_table_candidate_line,
    parse_root_table_region, parse_table_region,
};

/// 文档块节点：BlockRecord + 嵌套子块（替代 velotype 的 Entity<Block> 与 parent/content 链接）。
#[derive(Clone, Debug)]
pub struct BlockNode {
    pub record: BlockRecord,
    pub children: Vec<BlockNode>,
}

impl BlockNode {
    /// 无子块的叶子节点。
    pub fn leaf(record: BlockRecord) -> Self {
        Self {
            record,
            children: Vec::new(),
        }
    }

    pub fn kind(&self) -> &BlockKind {
        &self.record.kind
    }
}

/// Parsed opening code-fence metadata.
///
/// The opening fence records both the marker character and its run length so
/// only a matching closing fence can terminate the block.
type FenceInfo = CodeFenceOpening;

/// HTML block form recognized by the Markdown importer.
enum HtmlBlockStart {
    /// HTML comment region beginning with `<!--`.
    Comment,
    /// HTML tag block whose closing behavior depends on the tag shape.
    Tag {
        name: String,
        self_closing: bool,
        closes_same_line: bool,
    },
}

/// Ordered-list or unordered-list marker parsed from one source line.
#[derive(Clone)]
struct ListMarker {
    kind: BlockKind,
    indent_columns: usize,
    content_indent_columns: usize,
    text: String,
}

fn strip_fence_indent(line: &str) -> Option<&str> {
    let indent = line.bytes().take_while(|b| *b == b' ').count();
    (indent <= 3).then_some(&line[indent..])
}

fn collect_until_blank_line(lines: &[String], start: usize) -> usize {
    let mut index = start + 1;
    while index < lines.len() && !lines[index].trim().is_empty() {
        index += 1;
    }
    index
}

fn collect_html_fallback_region(lines: &[String], start: usize) -> usize {
    let mut index = start + 1;
    while index < lines.len() {
        if lines[index].trim().is_empty()
            || looks_like_root_block_start(lines, index)
            || parse_standalone_image(&lines[index]).is_some()
        {
            break;
        }
        index += 1;
    }
    index
}

fn pending_inline_code_run_len(markdown: &str) -> Option<usize> {
    let mut open_run_len = None;
    let mut chars = markdown.char_indices().peekable();

    while let Some((_, ch)) = chars.next() {
        if open_run_len.is_none() && ch == '\\' {
            let _ = chars.next();
            continue;
        }

        if ch != '`' {
            continue;
        }

        let mut run_len = 1usize;
        while chars.peek().is_some_and(|(_, ch)| *ch == '`') {
            let _ = chars.next();
            run_len += 1;
        }

        if open_run_len == Some(run_len) {
            open_run_len = None;
        } else if open_run_len.is_none() {
            open_run_len = Some(run_len);
        }
    }

    open_run_len
}

fn line_contains_matching_backtick_run(line: &str, run_len: usize) -> bool {
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '`' {
            continue;
        }

        let mut current_run_len = 1usize;
        while chars.peek().is_some_and(|ch| *ch == '`') {
            let _ = chars.next();
            current_run_len += 1;
        }

        if current_run_len == run_len {
            return true;
        }
    }

    false
}

fn paragraph_can_continue_through_boundary(
    paragraph_lines: &[String],
    lines: &[String],
    boundary_index: usize,
) -> bool {
    let Some(run_len) = pending_inline_code_run_len(&paragraph_lines.join("\n")) else {
        return false;
    };

    lines[boundary_index..]
        .iter()
        .any(|line| line_contains_matching_backtick_run(line, run_len))
}

pub(crate) fn parse_opening_fence(line: &str) -> Option<FenceInfo> {
    BlockKind::parse_code_fence_opening(strip_fence_indent(line)?.trim_end())
}

fn is_closing_fence(line: &str, opener: &FenceInfo) -> bool {
    let Some(trimmed) = strip_fence_indent(line).map(str::trim_end) else {
        return false;
    };
    if !trimmed.starts_with(opener.ch) {
        return false;
    }
    // CommonMark：闭合围栏只需不短于开围栏（`!=` 会误拒合法的更长闭合围栏）
    let run_len = trimmed.chars().take_while(|&c| c == opener.ch).count();
    if run_len < opener.len {
        return false;
    }
    trimmed[opener.ch.len_utf8() * run_len..].trim().is_empty()
}

fn find_matching_closing_fence(
    lines: &[String],
    start_index: usize,
    opener: &FenceInfo,
) -> Option<usize> {
    for index in (start_index + 1)..lines.len() {
        let line = &lines[index];
        // A fenced block closes at its first matching fence, as in CommonMark.
        // Scanning for a later fence (the previous behavior) let any opener
        // swallow the following blocks whose closing fences are bare, merging
        // them and corrupting them on round-trip (issue #58). A bare closing
        // fence is indistinguishable from an empty opener, so first-match is
        // the only unambiguous rule.
        if is_closing_fence(line, opener) {
            return Some(index);
        }

        // An info-tagged opener can never be a closing fence, so reaching one
        // first means this block was never closed and stays unmatched.
        if parse_opening_fence(line)
            .as_ref()
            .and_then(|fence| fence.language.as_ref())
            .is_some()
        {
            break;
        }
    }

    None
}

fn leading_indent_columns_and_bytes(line: &str) -> (usize, usize) {
    let mut columns = 0usize;
    let mut bytes = 0usize;
    for ch in line.chars() {
        match ch {
            ' ' => {
                columns += 1;
                bytes += 1;
            }
            '\t' => {
                columns += 4 - (columns % 4);
                bytes += 1;
            }
            _ => break,
        }
    }
    (columns, bytes)
}

fn strip_indented_code_prefix(line: &str) -> Option<&str> {
    if let Some(rest) = line.strip_prefix('\t') {
        Some(rest)
    } else {
        line.strip_prefix("    ")
    }
}

fn display_columns(value: &str) -> usize {
    let mut columns = 0usize;
    for ch in value.chars() {
        match ch {
            '\t' => columns += 4 - (columns % 4),
            _ => columns += 1,
        }
    }
    columns
}

fn strip_leading_columns(line: &str, columns: usize) -> Option<&str> {
    if columns == 0 {
        return Some(line);
    }
    if line.trim().is_empty() {
        return Some("");
    }

    let mut consumed_columns = 0usize;
    for (idx, ch) in line.char_indices() {
        let bytes_after_char = idx + ch.len_utf8();
        match ch {
            ' ' => {
                consumed_columns += 1;
            }
            '\t' => {
                consumed_columns += 4 - (consumed_columns % 4);
            }
            _ => break,
        }

        if consumed_columns >= columns {
            return Some(&line[bytes_after_char..]);
        }
    }

    None
}

fn dedent_lines(lines: &[String], columns: usize) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            strip_leading_columns(line, columns)
                .unwrap_or(line.as_str())
                .to_string()
        })
        .collect()
}

fn parse_list_marker(line: &str) -> Option<ListMarker> {
    let (indent_columns, indent_bytes) = leading_indent_columns_and_bytes(line);
    let rest = &line[indent_bytes..];

    if let Some(marker) = rest.chars().next()
        && matches!(marker, '-' | '*' | '+')
    {
        let after_marker = &rest[marker.len_utf8()..];
        let separator_len = after_marker
            .chars()
            .next()
            .filter(|ch| matches!(ch, ' ' | '\t'))
            .map(char::len_utf8)?;
        let text = after_marker
            .strip_prefix(' ')
            .or_else(|| after_marker.strip_prefix('\t'))?;
        let (kind, text) =
            if let Some((checked, prefix_len)) = BlockKind::parse_task_list_item_prefix(text) {
                (
                    BlockKind::TaskListItem { checked },
                    text[prefix_len..].to_string(),
                )
            } else {
                (BlockKind::BulletedListItem, text.to_string())
            };
        return Some(ListMarker {
            kind,
            indent_columns,
            content_indent_columns: display_columns(
                &line[..indent_bytes + marker.len_utf8() + separator_len],
            ),
            text,
        });
    }

    let (digit_len, marker_len, text) = parse_ordered_list_marker(rest)?;
    Some(ListMarker {
        kind: BlockKind::NumberedListItem,
        indent_columns,
        content_indent_columns: display_columns(&line[..indent_bytes + digit_len + marker_len]),
        text: text.to_string(),
    })
}

fn parse_ordered_list_marker(rest: &str) -> Option<(usize, usize, &str)> {
    let digit_len = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
    if !(1..=9).contains(&digit_len) {
        return None;
    }

    let marker = *rest.as_bytes().get(digit_len)?;
    if !matches!(marker, b'.' | b')') {
        return None;
    }

    let separator = *rest.as_bytes().get(digit_len + 1)?;
    if !matches!(separator, b' ' | b'\t') {
        return None;
    }

    Some((digit_len, 2, &rest[digit_len + 2..]))
}

fn strip_one_quote_level(line: &str) -> Option<String> {
    let leading_spaces = line.bytes().take_while(|b| *b == b' ').count();
    if leading_spaces > 3 {
        return None;
    }

    let rest = &line[leading_spaces..];
    if !rest.starts_with('>') {
        return None;
    }

    Some(
        rest[1..]
            .strip_prefix(' ')
            .unwrap_or(&rest[1..])
            .to_string(),
    )
}

fn is_quote_start(line: &str) -> bool {
    let trimmed_end = line.trim_end();
    let leading_spaces = trimmed_end.bytes().take_while(|b| *b == b' ').count();
    leading_spaces <= 3 && trimmed_end[leading_spaces..].starts_with('>')
}

fn is_reference_definition_start(line: &str) -> bool {
    let trimmed_end = line.trim_end();
    let leading_spaces = trimmed_end.bytes().take_while(|b| *b == b' ').count();
    if leading_spaces > 3 {
        return false;
    }

    let rest = &trimmed_end[leading_spaces..];
    let Some(label_end) = rest.find("]:") else {
        return false;
    };
    rest.starts_with('[') && label_end > 1
}

fn is_footnote_definition_start(line: &str) -> bool {
    let trimmed_end = line.trim_end();
    let leading_spaces = trimmed_end.bytes().take_while(|b| *b == b' ').count();
    if leading_spaces > 3 {
        return false;
    }

    let rest = &trimmed_end[leading_spaces..];
    let Some(label_end) = rest.find("]:") else {
        return false;
    };
    rest.starts_with("[^") && label_end > 2
}

fn is_reference_definition_title_continuation(line: &str) -> bool {
    let (_, indent_bytes) = leading_indent_columns_and_bytes(line);
    if indent_bytes == 0 {
        return false;
    }

    let trimmed = line[indent_bytes..].trim();
    (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        || (trimmed.starts_with('(') && trimmed.ends_with(')'))
}

fn is_block_html_start(line: &str) -> bool {
    parse_html_block_start(line).is_some()
}

fn collect_closed_html_comment_region(lines: &[String], start: usize) -> Option<usize> {
    match parse_html_block_start(&lines[start])? {
        HtmlBlockStart::Comment => {}
        HtmlBlockStart::Tag { .. } => return None,
    }

    if lines[start].contains("-->") {
        return Some(start + 1);
    }

    let mut index = start + 1;
    while index < lines.len() {
        if lines[index].contains("-->") {
            return Some(index + 1);
        }
        index += 1;
    }

    None
}

fn collect_block_html_region(lines: &[String], start: usize) -> usize {
    match parse_html_block_start(&lines[start]) {
        Some(HtmlBlockStart::Comment) => collect_closed_html_comment_region(lines, start)
            .unwrap_or_else(|| collect_html_fallback_region(lines, start)),
        Some(HtmlBlockStart::Tag {
            name,
            self_closing,
            closes_same_line,
        }) => {
            if self_closing || closes_same_line {
                return start + 1;
            }

            let mut depth = 1usize;
            let mut index = start + 1;
            while index < lines.len() {
                if let Some(HtmlBlockStart::Tag {
                    name: nested_name,
                    self_closing,
                    closes_same_line,
                }) = parse_html_block_start(&lines[index])
                    && nested_name == name
                    && !self_closing
                    && !closes_same_line
                {
                    depth += 1;
                }

                if let Some(close_name) = parse_html_close_tag_name(&lines[index])
                    && close_name == name
                {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return index + 1;
                    }
                }

                index += 1;
            }
            collect_html_fallback_region(lines, start)
        }
        None => collect_until_blank_line(lines, start),
    }
}

fn collect_reference_definition_region(lines: &[String], start: usize) -> usize {
    let mut index = start + 1;
    while index < lines.len() && is_reference_definition_title_continuation(&lines[index]) {
        index += 1;
    }
    index
}

fn collect_footnote_definition_region(lines: &[String], start: usize) -> usize {
    let mut index = start + 1;
    while index < lines.len() {
        let line = &lines[index];
        if line.trim().is_empty() {
            index += 1;
            continue;
        }

        let (indent_columns, _) = leading_indent_columns_and_bytes(line);
        if indent_columns > 0 {
            index += 1;
            continue;
        }

        break;
    }
    index
}

fn is_display_math_start(line: &str) -> bool {
    strip_fence_indent(line)
        .map(str::trim_end)
        .is_some_and(|rest| rest.starts_with("$$") || rest.starts_with("\\["))
}

fn collect_display_math_region(lines: &[String], start: usize) -> usize {
    let opener = strip_fence_indent(&lines[start])
        .map(str::trim_end)
        .unwrap_or_default();
    // 单行闭合（$$...$$ 或 \[...\]）：区域仅一行
    if opener.starts_with("$$") && opener != "$$" && opener[2..].contains("$$") {
        return start + 1;
    }
    if opener.starts_with("\\[") && opener != "\\[" && opener[2..].contains("\\]") {
        return start + 1;
    }

    let close_marker = if opener.starts_with("\\[") { "\\]" } else { "$$" };
    let mut index = start + 1;
    while index < lines.len() {
        if lines[index].trim() == close_marker {
            return index + 1;
        }

        if lines[index].trim().is_empty() {
            let mut lookahead = index + 1;
            while lookahead < lines.len() && lines[lookahead].trim().is_empty() {
                lookahead += 1;
            }

            if lookahead >= lines.len() || looks_like_root_block_start(lines, lookahead) {
                return lookahead;
            }
        }

        index += 1;
    }

    lines.len()
}

fn parse_html_block_start(line: &str) -> Option<HtmlBlockStart> {
    let rest = strip_fence_indent(line)?.trim_end();
    if rest.starts_with("<!--") {
        return Some(HtmlBlockStart::Comment);
    }

    let tagged = rest.strip_prefix('<')?;
    if tagged.starts_with('/') {
        return None;
    }

    let name_len = tagged
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .count();
    if name_len == 0 {
        return None;
    }

    let name = &tagged[..name_len];
    // 行内标签（u/kbd/span/mark/del/sup/sub 等）不构成块级 HTML：
    // `<u>xxx</u>` 这类整行内联 HTML 应按段落行内解析（否则被当成 HtmlBlock 原文展示）
    if is_inline_tag(name) {
        return None;
    }
    let suffix = &tagged[name_len..];
    let next = suffix.chars().next()?;
    if !matches!(next, '>' | ' ' | '\t' | '/') {
        return None;
    }

    Some(HtmlBlockStart::Tag {
        name: name.to_string(),
        self_closing: rest.ends_with("/>") || is_html_void_block_tag(name),
        closes_same_line: rest.contains(&format!("</{name}>")),
    })
}

fn is_html_void_block_tag(name: &str) -> bool {
    matches!(name.to_ascii_lowercase().as_str(), "br" | "hr" | "img")
}

fn parse_html_close_tag_name(line: &str) -> Option<String> {
    let rest = strip_fence_indent(line)?.trim_end();
    let tagged = rest.strip_prefix("</")?;
    let name_len = tagged
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .count();
    if name_len == 0 {
        return None;
    }

    let name = &tagged[..name_len];
    let suffix = &tagged[name_len..];
    let next = suffix.chars().next()?;
    if !matches!(next, '>' | ' ' | '\t') {
        return None;
    }

    Some(name.to_string())
}

fn collect_quote_raw_region(lines: &[String], start: usize) -> usize {
    let mut index = start;
    while index < lines.len() {
        let line = &lines[index];
        if line.trim().is_empty() || !is_quote_start(line) {
            break;
        }
        index += 1;
    }
    index
}

fn quote_content_starts_unsupported(lines: &[String], index: usize) -> bool {
    let line = &lines[index];
    is_block_html_start(line)
        || is_footnote_definition_start(line)
        || is_reference_definition_start(line)
        || is_root_table_candidate_line(line)
        || is_display_math_start(line)
        || BlockKind::parse_atx_heading_line(line).is_some()
        || BlockKind::parse_separator_line(line)
        || lines
            .get(index + 1)
            .and_then(|next| BlockKind::parse_setext_underline(next))
            .is_some()
}

fn collect_unsupported_quote_region(lines: &[String], start: usize) -> Option<usize> {
    if start >= lines.len() {
        return None;
    }

    let line = &lines[start];
    if is_block_html_start(line) {
        return Some(collect_block_html_region(lines, start));
    }
    if is_footnote_definition_start(line) {
        return Some(collect_footnote_definition_region(lines, start));
    }
    if is_reference_definition_start(line) {
        return Some(collect_reference_definition_region(lines, start));
    }
    if is_root_table_candidate_line(line) {
        return Some(collect_root_table_candidate_region(lines, start));
    }
    if is_display_math_start(line) {
        return Some(collect_display_math_region(lines, start));
    }
    if BlockKind::parse_atx_heading_line(line).is_some() || BlockKind::parse_separator_line(line) {
        return Some(start + 1);
    }
    if lines
        .get(start + 1)
        .and_then(|next| BlockKind::parse_setext_underline(next))
        .is_some()
    {
        return Some((start + 2).min(lines.len()));
    }

    None
}

fn collect_list_item_region(lines: &[String], start: usize, marker_indent_columns: usize) -> usize {
    let mut index = start + 1;
    let mut pending_blank_lines = 0usize;
    while index < lines.len() {
        let line = &lines[index];
        if line.trim().is_empty() {
            pending_blank_lines += 1;
            index += 1;
            continue;
        }

        if parse_list_marker(line)
            .is_some_and(|marker| marker.indent_columns <= marker_indent_columns)
        {
            return index.saturating_sub(pending_blank_lines);
        }

        if parse_list_marker(line).is_some() {
            pending_blank_lines = 0;
            index += 1;
            continue;
        }

        let (indent_columns, _) = leading_indent_columns_and_bytes(line);
        // 同级或更浅缩进的分割线终止列表项区域（CommonMark：thematic break 终止列表项，
        // 无空行时也不得吸入项内）
        if indent_columns <= marker_indent_columns && BlockKind::parse_separator_line(line) {
            return index.saturating_sub(pending_blank_lines);
        }
        if indent_columns > marker_indent_columns || pending_blank_lines == 0 {
            pending_blank_lines = 0;
            index += 1;
            continue;
        }

        return index.saturating_sub(pending_blank_lines);
    }
    index
}

fn looks_like_root_block_start(lines: &[String], index: usize) -> bool {
    let line = &lines[index];
    if line.trim().is_empty() {
        return true;
    }

    parse_opening_fence(line).is_some()
        || is_block_html_start(line)
        || is_footnote_definition_start(line)
        || is_reference_definition_start(line)
        || strip_indented_code_prefix(line).is_some()
        || parse_list_marker(line).is_some()
        || is_quote_start(line)
        || BlockKind::parse_atx_heading_line(line).is_some()
        || BlockKind::parse_separator_line(line)
        || lines
            .get(index + 1)
            .and_then(|next| BlockKind::parse_setext_underline(next))
            .is_some()
        || is_root_table_candidate_line(line)
        || is_display_math_start(line)
}

fn build_code_block(
    language: Option<String>,
    content: String,
) -> BlockNode {
    BlockNode::leaf(BlockRecord::new(
            BlockKind::CodeBlock { language },
            InlineTextTree::plain(content),
        ),
    )
}

fn collect_fenced_code_block(
    lines: &[String],
    start: usize,
) -> Option<(BlockNode, usize)> {
    let fence = parse_opening_fence(&lines[start])?;
    let closing_index = find_matching_closing_fence(lines, start, &fence)?;
    if is_mermaid_info_string(fence.language.as_ref().map(|language| language.as_ref())) {
        let raw = lines[start..=closing_index].join("\n");
        return Some((
            BlockNode::leaf(BlockRecord::mermaid(raw)),
            closing_index + 1,
        ));
    }
    // `math`/`latex` info 的围栏按展示公式块处理（Typora 风格）
    if is_math_info_string(fence.language.as_ref().map(|language| language.as_ref())) {
        let raw = lines[start..=closing_index].join("\n");
        return Some((
            BlockNode::leaf(BlockRecord::math(raw)),
            closing_index + 1,
        ));
    }

    // Length is known: closing_index - (start + 1). slice.to_vec()
    // allocates the exact capacity in one shot, vs Vec::new() + while-push
    // which doubles the buffer 2-3 times for any non-trivial code block.
    let code_lines = lines[start + 1..closing_index].to_vec();

    Some((
        build_code_block(fence.language.clone(), code_lines.join("\n")),
        closing_index + 1,
    ))
}

fn collect_indented_code_block(
    lines: &[String],
    start: usize,
) -> Option<(BlockNode, usize)> {
    let stripped = strip_indented_code_prefix(&lines[start])?;
    let mut code_lines = vec![stripped.to_string()];
    let mut code_index = start + 1;
    while code_index < lines.len() {
        if let Some(stripped) = strip_indented_code_prefix(&lines[code_index]) {
            code_lines.push(stripped.to_string());
            code_index += 1;
        } else if lines[code_index].trim().is_empty() {
            code_lines.push(String::new());
            code_index += 1;
        } else {
            break;
        }
    }

    Some((
        build_code_block(None, code_lines.join("\n")),
        code_index,
    ))
}

fn raw_block( markdown: String) -> BlockNode {
    BlockNode::leaf(BlockRecord::raw_markdown(markdown))
}

fn comment_block( markdown: String) -> BlockNode {
    BlockNode::leaf(BlockRecord::comment(markdown))
}

fn html_or_raw_block( markdown: String) -> BlockNode {
    // 偏好「HTML 标签转换为 Markdown 语法」：单一容器标签按原生块解析
    if crate::markdown::html_to_md_enabled()
        && let Some(node) = convert_html_container_to_native(&markdown)
    {
        return node;
    }
    let document = parse_html_document(&markdown);
    if document.safety == HtmlSafetyClass::Semantic {
        // 复用分类时的解析结果，不再二次解析
        BlockNode::leaf(BlockRecord::html_with_document(markdown, document))
    } else {
        raw_block(markdown)
    }
}

/// 把单一容器标签的 HTML 块转换为原生 Markdown 块：
/// `<h1>..</h1>` → 标题，`<p>/<div>/<center> ..</..>` → 段落（内部内容走行内解析）。
/// 仅处理单一容器（`<tag attrs>inner</tag>` 单行，或开/闭标签各占首尾行）。
fn convert_html_container_to_native(markdown: &str) -> Option<BlockNode> {
    let (name, inner) = unwrap_simple_container(markdown.trim())?;
    let title = InlineTextTree::from_markdown(inner.trim());
    let record = match name.as_str() {
        "h1" => BlockRecord::new(BlockKind::Heading { level: 1 }, title),
        "h2" => BlockRecord::new(BlockKind::Heading { level: 2 }, title),
        "h3" => BlockRecord::new(BlockKind::Heading { level: 3 }, title),
        "h4" => BlockRecord::new(BlockKind::Heading { level: 4 }, title),
        "h5" => BlockRecord::new(BlockKind::Heading { level: 5 }, title),
        "h6" => BlockRecord::new(BlockKind::Heading { level: 6 }, title),
        "p" | "div" | "center" => BlockRecord::new(BlockKind::Paragraph, title),
        _ => return None,
    };
    Some(BlockNode::leaf(record))
}

/// 去掉单一容器标签：`<tag attrs>inner</tag>`（单行）或 `<tag>` 与 `</tag>` 各占首尾行。
/// 内层存在同名嵌套标签或块级标签（p/div/center/h1-6/section/ul/ol/table 等）时不处理
///（返回 None 走 HtmlBlock 原文路径，避免块级结构被拍平为纯文本）。
fn unwrap_simple_container(markdown: &str) -> Option<(String, String)> {
    let rest = markdown.strip_prefix('<')?;
    let name_len = rest
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric())
        .count();
    if name_len == 0 {
        return None;
    }
    let name = rest[..name_len].to_ascii_lowercase();
    let open_end = rest.find('>')?;
    let inner = &rest[open_end + 1..];
    let close = format!("</{name}>");
    let inner = inner.strip_suffix(&close)?;
    let inner_trimmed = inner.trim();
    if inner_trimmed.contains(&format!("<{name}")) || inner_trimmed.contains(&close) {
        return None;
    }
    // 内层含块级标签：保持 HtmlBlock 原文（不在本转换范围）
    for block_tag in ["<p", "<div", "<center", "<h1", "<h2", "<h3", "<h4", "<h5", "<h6", "<section", "<ul", "<ol", "<table", "<blockquote", "<pre"] {
        if inner_trimmed.contains(block_tag) {
            return None;
        }
    }
    Some((name, inner_trimmed.to_string()))
}


fn math_or_raw_block( markdown: String) -> BlockNode {
    if parse_display_math_source(&markdown).is_some() {
        return BlockNode::leaf(BlockRecord::math(markdown));
    }
    // 单行展示公式候选解析失败（如 `$$ 文本 $$ $$ 公式 $$` 同行多段）：
    // 回退为段落走行内解析（行内 $$ 公式仍可渲染、内容不丢）；
    // 多行失败保持 RawMarkdown 无损保留。
    if !markdown.contains('\n') {
        return BlockNode::leaf(BlockRecord::new(
            BlockKind::Paragraph,
            InlineTextTree::from_markdown(&markdown),
        ));
    }
    raw_block(markdown)
}

fn collect_comment_block(
    lines: &[String],
    start: usize,
) -> Option<(BlockNode, usize)> {
    let end = collect_closed_html_comment_region(lines, start)?;
    Some((comment_block(lines[start..end].join("\n")), end))
}

fn native_block(
    kind: BlockKind,
    markdown: String,
) -> BlockNode {
    BlockNode::leaf(BlockRecord::new(kind, InlineTextTree::from_markdown(&markdown)),
    )
}

fn standalone_image_block( markdown: String) -> BlockNode {
    BlockNode::leaf(BlockRecord::paragraph(markdown.trim().to_string()))
}

fn is_standalone_image_paragraph(lines: &[String]) -> bool {
    lines.len() == 1 && parse_standalone_image(&lines[0]).is_some()
}

fn starts_with_standalone_image_child_paragraph(lines: &[String]) -> bool {
    if lines.is_empty() || !is_standalone_image_paragraph(&lines[..1]) {
        return false;
    }

    lines.get(1).is_none_or(|next| {
        next.trim().is_empty()
            || parse_list_marker(next).is_some()
            || is_quote_start(next)
            || parse_opening_fence(next).is_some()
            || strip_indented_code_prefix(next).is_some()
            || is_block_html_start(next)
            || is_footnote_definition_start(next)
            || is_reference_definition_start(next)
            || is_root_table_candidate_line(next)
            || is_display_math_start(next)
    })
}

fn append_markdown_to_block(block: &mut BlockNode, separator: &str, markdown: &str) {
    let mut title = block.record.title.clone();
    if !separator.is_empty() {
        title.append_tree(InlineTextTree::plain(separator.to_string()));
    }
    title.append_tree(InlineTextTree::from_markdown(markdown));
    block.record.set_title(title);
}

fn plain_text_paragraph_block( text: String) -> BlockNode {
    BlockNode::leaf(BlockRecord::paragraph(text))
}

fn append_quote_separator_children(
    children: &mut Vec<BlockNode>,
    count: usize,
) {
    for _ in 0..count {
        children.push(native_block(BlockKind::Paragraph, String::new()));
    }
}

fn build_native_footnote_definition_block(
    lines: &[String],
) -> Option<BlockNode> {
    let (id, first_line) = parse_footnote_definition_head(lines.first()?)?;
    let mut body_lines = Vec::new();
    if !first_line.is_empty() {
        body_lines.push(first_line);
    }

    for line in lines.iter().skip(1) {
        if line.trim().is_empty() {
            body_lines.push(String::new());
        } else {
            body_lines.push(
                strip_leading_columns(line, 4)
                    .unwrap_or(line.as_str())
                    .to_string(),
            );
        }
    }

    let children = build_blocks_from_lines_internal(&body_lines, false)
            .into_iter()
            .map(|root| root.node)
            .collect::<Vec<_>>();
    let mut block = BlockNode::leaf(BlockRecord::new(BlockKind::FootnoteDefinition, InlineTextTree::plain(id)),
    );
    block.children.extend(children);
    Some(block)
}

/// 根级块及其在源文档中的行区间（`start_line` 含、`end_line` 不含）。
/// 仅根级块携带行区间；嵌套子块的区间对前端不可见（编辑以根块为单位）。
#[derive(Clone, Debug)]
pub struct RootBlock {
    pub node: BlockNode,
    pub start_line: usize,
    pub end_line: usize,
}

impl RootBlock {
    fn spanned(node: BlockNode, start_line: usize, end_line: usize) -> Self {
        Self {
            node,
            start_line,
            end_line,
        }
    }
}

/// 解析 Markdown 全文为根级块序列（携带行区间）。
pub fn parse_root_blocks(markdown: &str) -> Vec<RootBlock> {
    let lines = markdown
        .split('\n')
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    build_blocks_from_lines_internal(&lines, true)
}

/// 从 Markdown 行序列构建块。
///
/// 仅为运行时编辑器可安全编辑的语法创建原生块；更复杂的合法 Markdown
/// 区域回退为 [`BlockKind::RawMarkdown`]，保存时原样保留。
pub fn build_blocks_from_lines(lines: &[String]) -> Vec<RootBlock> {
    build_blocks_from_lines_internal(lines, true)
}

fn build_blocks_from_lines_internal(
    lines: &[String],
    allow_root_footnote_definitions: bool,
) -> Vec<RootBlock> {
        let mut roots: Vec<RootBlock> = Vec::new();
        let mut index = 0;

        while index < lines.len() {
            let line = &lines[index];
            if line.trim().is_empty() {
                let blank_start = index;
                while index < lines.len() && lines[index].trim().is_empty() {
                    index += 1;
                }

                let blank_run_len = index - blank_start;
                let previous_root_is_list_item = roots
                    .last()
                    .map(|block: &RootBlock| block.node.record.kind.is_list_item())
                    .unwrap_or(false);
                let next_root_is_list_item = lines
                    .get(index)
                    .is_some_and(|line| parse_list_marker(line).is_some());
                let preserved_empty_blocks = if roots.is_empty() {
                    blank_run_len
                } else if previous_root_is_list_item && next_root_is_list_item {
                    blank_run_len
                } else {
                    blank_run_len.saturating_sub(1)
                };

                // 每个保留的空段落对应一个空行，区间逐行分配。
                for i in 0..preserved_empty_blocks {
                    roots.push(RootBlock::spanned(
                        native_block(BlockKind::Paragraph, String::new()),
                        blank_start + i,
                        blank_start + i + 1,
                    ));
                }
                continue;
            }

            if parse_opening_fence(line).is_some() {
                let Some((block, next_index)) = collect_fenced_code_block(lines, index) else {
                    let paragraph = collect_paragraph_block(lines, index);
                    roots.push(RootBlock::spanned(paragraph.0, index, paragraph.1));
                    index = paragraph.1;
                    continue;
                };

                roots.push(RootBlock::spanned(block, index, next_index));
                index = next_index;
                continue;
            }

            if let Some((block, end)) = collect_comment_block(lines, index) {
                roots.push(RootBlock::spanned(block, index, end));
                index = end;
                continue;
            }

            if is_block_html_start(line) {
                let end = collect_block_html_region(lines, index);
                roots.push(RootBlock::spanned(
                    html_or_raw_block(lines[index..end].join("\n")),
                    index,
                    end,
                ));
                index = end;
                continue;
            }

            if is_footnote_definition_start(line) {
                let end = collect_footnote_definition_region(lines, index);
                if allow_root_footnote_definitions {
                    if let Some(block) =
                        build_native_footnote_definition_block(&lines[index..end])
                    {
                        roots.push(RootBlock::spanned(block, index, end));
                    } else {
                        roots.push(RootBlock::spanned(
                            raw_block(lines[index..end].join("\n")),
                            index,
                            end,
                        ));
                    }
                } else {
                    roots.push(RootBlock::spanned(
                        raw_block(lines[index..end].join("\n")),
                        index,
                        end,
                    ));
                }
                index = end;
                continue;
            }

            if is_reference_definition_start(line) {
                let end = collect_reference_definition_region(lines, index);
                roots.push(RootBlock::spanned(
                    raw_block(lines[index..end].join("\n")),
                    index,
                    end,
                ));
                index = end;
                continue;
            }

            // setext 标题只能由段落行构成：列表项/引用/ATX 标题/分割线起始的行不参与
            //（`- [ ] x\n---` 应为列表项 + 分割线，而非把列表项行当 setext 文本）
            let setext_ineligible = parse_list_marker(line).is_some()
                || is_quote_start(line)
                || BlockKind::parse_atx_heading_line(line).is_some()
                || BlockKind::parse_separator_line(line);
            if !setext_ineligible
                && let Some(level) = lines
                    .get(index + 1)
                    .and_then(|next| BlockKind::parse_setext_underline(next))
            {
                roots.push(RootBlock::spanned(
                    native_block(BlockKind::Heading { level }, line.trim_end().to_string()),
                    index,
                    index + 2,
                ));
                index += 2;
                continue;
            }

            if parse_standalone_image(line).is_some() {
                roots.push(RootBlock::spanned(
                    standalone_image_block(line.to_string()),
                    index,
                    index + 1,
                ));
                index += 1;
                continue;
            }

            if strip_indented_code_prefix(line).is_some() {
                let Some((block, next_index)) = collect_indented_code_block(lines, index)
                else {
                    unreachable!("indented code prefix disappeared after detection");
                };

                roots.push(RootBlock::spanned(block, index, next_index));
                index = next_index;
                continue;
            }

            if parse_list_marker(line).is_some() {
                let (blocks, next_index) = collect_list_blocks(lines, index);
                roots.extend(blocks);
                index = next_index;
                continue;
            }

            if is_quote_start(line) {
                let (block, next_index) = collect_quote_block(lines, index);
                roots.push(RootBlock::spanned(block, index, next_index));
                index = next_index;
                continue;
            }

            if let Some((level, content)) = BlockKind::parse_atx_heading_line(line) {
                roots.push(RootBlock::spanned(
                    native_block(BlockKind::Heading { level }, content),
                    index,
                    index + 1,
                ));
                index += 1;
                continue;
            }

            if BlockKind::parse_separator_line(line) {
                roots.push(RootBlock::spanned(
                    BlockNode::leaf(BlockRecord::new(
                        BlockKind::Separator,
                        InlineTextTree::plain(String::new()),
                    )),
                    index,
                    index + 1,
                ));
                index += 1;
                continue;
            }

            if is_root_table_candidate_line(line) {
                let end = collect_root_table_candidate_region(lines, index);
                let region = &lines[index..end];
                if let Some(table) = parse_root_table_region(region) {
                    roots.push(RootBlock::spanned(
                        BlockNode::leaf(BlockRecord::table(table)),
                        index,
                        end,
                    ));
                } else {
                    // 表格候选区域解析失败：逐行降级为普通段落，区间逐行分配。
                    roots.extend(region.iter().cloned().enumerate().map(|(i, line)| {
                        RootBlock::spanned(
                            plain_text_paragraph_block(line),
                            index + i,
                            index + i + 1,
                        )
                    }));
                }
                index = end;
                continue;
            }

            if let Some(end) = collect_pipeless_table_region(lines, index)
                && let Some(table) = parse_root_table_region(&lines[index..end])
            {
                roots.push(RootBlock::spanned(
                    BlockNode::leaf(BlockRecord::table(table)),
                    index,
                    end,
                ));
                index = end;
                continue;
            }

            if is_display_math_start(line) {
                let end = collect_display_math_region(lines, index);
                roots.push(RootBlock::spanned(
                    math_or_raw_block(lines[index..end].join("\n")),
                    index,
                    end,
                ));
                index = end;
                continue;
            }

            let paragraph = collect_paragraph_block(lines, index);
            roots.push(RootBlock::spanned(paragraph.0, index, paragraph.1));
            index = paragraph.1;
        }

        roots
    }

    fn collect_paragraph_block(
        lines: &[String],
        start: usize,
    ) -> (BlockNode, usize) {
        let mut paragraph_lines = vec![lines[start].to_string()];
        let mut index = start + 1;
        while index < lines.len() {
            if (lines[index].trim().is_empty() || looks_like_root_block_start(lines, index))
                && !paragraph_can_continue_through_boundary(&paragraph_lines, lines, index)
            {
                break;
            }
            paragraph_lines.push(lines[index].to_string());
            index += 1;
        }

        (
            native_block(BlockKind::Paragraph, paragraph_lines.join("\n")),
            index,
        )
    }

    fn collect_quote_block(
        lines: &[String],
        start: usize,
    ) -> (BlockNode, usize) {
        let end = collect_quote_raw_region(lines, start);
        let region = &lines[start..end];
        let mut dequoted = Vec::with_capacity(region.len());
        for line in region {
            if line.trim().is_empty() {
                dequoted.push(String::new());
                continue;
            }

            let Some(content) = strip_one_quote_level(line) else {
                return (raw_block(region.join("\n")), end);
            };
            dequoted.push(content);
        }

        let Some(block) = build_native_quote_block(&dequoted) else {
            return (raw_block(region.join("\n")), end);
        };

        (block, end)
    }

    fn build_native_quote_block(
        lines: &[String],
    ) -> Option<BlockNode> {
        if let Some(header_index) = lines.iter().position(|line| !line.trim().is_empty())
            && let Some((variant, title)) = CalloutVariant::parse_header_line(&lines[header_index])
        {
            return build_native_callout_block(&lines[header_index + 1..],
                variant,
                title,
            );
        }

        let mut title_markdown = String::new();
        let mut children = Vec::new();
        let mut index = 0usize;
        let mut pending_blank_lines = 0usize;
        let mut saw_child = false;

        while index < lines.len() {
            let line = &lines[index];
            if line.trim().is_empty() {
                pending_blank_lines += 1;
                index += 1;
                continue;
            }

            if is_table_candidate_line(line) {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines);
                }
                let table_end = collect_table_candidate_region(lines, index);
                let table_region = &lines[index..table_end];
                if let Some(table) = parse_table_region(table_region) {
                    children.push(BlockNode::leaf(BlockRecord::table(table)));
                } else {
                    children.push(raw_block(table_region.join("\n")));
                }
                saw_child = true;
                pending_blank_lines = 0;
                index = table_end;
                continue;
            }

            if is_footnote_definition_start(line) {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines);
                }
                let footnote_end = collect_footnote_definition_region(lines, index);
                if let Some(footnote) =
                    build_native_footnote_definition_block(&lines[index..footnote_end])
                {
                    children.push(footnote);
                    saw_child = true;
                    pending_blank_lines = 0;
                    index = footnote_end;
                    continue;
                }
            }

            if let Some((comment, consumed)) = collect_comment_block(lines, index) {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines);
                }
                children.push(comment);
                saw_child = true;
                pending_blank_lines = 0;
                index = consumed;
                continue;
            }

            if is_block_html_start(line) {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines);
                }
                let html_end = collect_block_html_region(lines, index);
                children.push(html_or_raw_block(lines[index..html_end].join("\n")));
                saw_child = true;
                pending_blank_lines = 0;
                index = html_end;
                continue;
            }

            if is_display_math_start(line) {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines);
                }
                let math_end = collect_display_math_region(lines, index);
                children.push(math_or_raw_block(lines[index..math_end].join("\n")));
                saw_child = true;
                pending_blank_lines = 0;
                index = math_end;
                continue;
            }

            if let Some(unsupported_end) = collect_unsupported_quote_region(lines, index) {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines);
                }
                children.push(raw_block(lines[index..unsupported_end].join("\n")));
                saw_child = true;
                pending_blank_lines = 0;
                index = unsupported_end;
                continue;
            }

            if is_quote_start(line) {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines);
                }
                let (quote, consumed) = collect_quote_block(lines, index);
                if quote.record.kind == BlockKind::RawMarkdown {
                    return None;
                }
                children.push(quote);
                saw_child = true;
                pending_blank_lines = 0;
                index = consumed;
                continue;
            }

            if parse_list_marker(line).is_some() {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines);
                }
                let (list_blocks, consumed) = collect_list_blocks(lines, index);
                if list_blocks
                    .iter()
                    .any(|block| block.node.record.kind == BlockKind::RawMarkdown)
                {
                    return None;
                }
                children.extend(list_blocks.into_iter().map(|root| root.node));
                saw_child = true;
                pending_blank_lines = 0;
                index = consumed;
                continue;
            }

            if parse_opening_fence(line).is_some()
                && let Some((code_block, consumed)) = collect_fenced_code_block(lines, index)
            {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines);
                }
                children.push(code_block);
                saw_child = true;
                pending_blank_lines = 0;
                index = consumed;
                continue;
            }

            if starts_with_standalone_image_child_paragraph(&lines[index..]) {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines);
                }
                children.push(standalone_image_block(line.to_string()));
                saw_child = true;
                pending_blank_lines = 0;
                index += 1;
                continue;
            }

            if strip_indented_code_prefix(line).is_some()
                && let Some((code_block, consumed)) = collect_indented_code_block(lines, index)
            {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines);
                }
                children.push(code_block);
                saw_child = true;
                pending_blank_lines = 0;
                index = consumed;
                continue;
            }

            let mut paragraph_lines = vec![line.clone()];
            index += 1;
            while index < lines.len() {
                let next = &lines[index];
                if next.trim().is_empty()
                    || is_quote_start(next)
                    || parse_list_marker(next).is_some()
                    || parse_opening_fence(next).is_some()
                    || strip_indented_code_prefix(next).is_some()
                    || quote_content_starts_unsupported(lines, index)
                {
                    break;
                }

                paragraph_lines.push(next.clone());
                index += 1;
            }

            if is_standalone_image_paragraph(&paragraph_lines) {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines);
                }
                children.push(standalone_image_block(paragraph_lines.join("\n")));
                saw_child = true;
                pending_blank_lines = 0;
                continue;
            }

            if saw_child {
                if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
                    append_quote_separator_children(&mut children, pending_blank_lines);
                }
                children.push(native_block(BlockKind::Paragraph,
                    paragraph_lines.join("\n"),
                ));
                pending_blank_lines = 0;
                continue;
            }

            if !title_markdown.is_empty() {
                title_markdown.push_str(if pending_blank_lines > 0 {
                    "\n\n"
                } else {
                    "\n"
                });
            }
            title_markdown.push_str(&paragraph_lines.join("\n"));
            pending_blank_lines = 0;
        }

        if pending_blank_lines > 0 && (!title_markdown.is_empty() || !children.is_empty()) {
            append_quote_separator_children(&mut children, pending_blank_lines);
        }

        let mut block = native_block(BlockKind::Quote, title_markdown);
        block.children.extend(children);
        Some(block)
    }

    fn build_native_callout_block(
        lines: &[String],
        variant: CalloutVariant,
        title: String,
    ) -> Option<BlockNode> {
        let mut children = Vec::new();
        let mut index = 0usize;
        let mut pending_blank_lines = 0usize;

        while index < lines.len() {
            let line = &lines[index];
            if line.trim().is_empty() {
                pending_blank_lines += 1;
                index += 1;
                continue;
            }

            if pending_blank_lines > 0 {
                append_quote_separator_children(&mut children, pending_blank_lines);
                pending_blank_lines = 0;
            }

            if is_table_candidate_line(line) {
                let table_end = collect_table_candidate_region(lines, index);
                let table_region = &lines[index..table_end];
                if let Some(table) = parse_table_region(table_region) {
                    children.push(BlockNode::leaf(BlockRecord::table(table)));
                } else {
                    children.push(raw_block(table_region.join("\n")));
                }
                index = table_end;
                continue;
            }

            if is_footnote_definition_start(line) {
                let footnote_end = collect_footnote_definition_region(lines, index);
                if let Some(footnote) =
                    build_native_footnote_definition_block(&lines[index..footnote_end])
                {
                    children.push(footnote);
                    index = footnote_end;
                    continue;
                }
            }

            if let Some((comment, consumed)) = collect_comment_block(lines, index) {
                children.push(comment);
                index = consumed;
                continue;
            }

            if is_block_html_start(line) {
                let html_end = collect_block_html_region(lines, index);
                children.push(html_or_raw_block(lines[index..html_end].join("\n")));
                index = html_end;
                continue;
            }

            if is_display_math_start(line) {
                let math_end = collect_display_math_region(lines, index);
                children.push(math_or_raw_block(lines[index..math_end].join("\n")));
                index = math_end;
                continue;
            }

            if let Some(unsupported_end) = collect_unsupported_quote_region(lines, index) {
                children.push(raw_block(lines[index..unsupported_end].join("\n")));
                index = unsupported_end;
                continue;
            }

            if is_quote_start(line) {
                let (quote, consumed) = collect_quote_block(lines, index);
                if quote.record.kind == BlockKind::RawMarkdown {
                    return None;
                }
                children.push(quote);
                index = consumed;
                continue;
            }

            if parse_list_marker(line).is_some() {
                let (list_blocks, consumed) = collect_list_blocks(lines, index);
                if list_blocks
                    .iter()
                    .any(|block| block.node.record.kind == BlockKind::RawMarkdown)
                {
                    return None;
                }
                children.extend(list_blocks.into_iter().map(|root| root.node));
                index = consumed;
                continue;
            }

            if parse_opening_fence(line).is_some()
                && let Some((code_block, consumed)) = collect_fenced_code_block(lines, index)
            {
                children.push(code_block);
                index = consumed;
                continue;
            }

            if starts_with_standalone_image_child_paragraph(&lines[index..]) {
                children.push(standalone_image_block(line.to_string()));
                index += 1;
                continue;
            }

            if strip_indented_code_prefix(line).is_some()
                && let Some((code_block, consumed)) = collect_indented_code_block(lines, index)
            {
                children.push(code_block);
                index = consumed;
                continue;
            }

            let mut paragraph_lines = vec![line.clone()];
            index += 1;
            while index < lines.len() {
                let next = &lines[index];
                if next.trim().is_empty()
                    || is_quote_start(next)
                    || parse_list_marker(next).is_some()
                    || parse_opening_fence(next).is_some()
                    || strip_indented_code_prefix(next).is_some()
                    || quote_content_starts_unsupported(lines, index)
                {
                    break;
                }

                paragraph_lines.push(next.clone());
                index += 1;
            }

            children.push(native_block(BlockKind::Paragraph,
                paragraph_lines.join("\n"),
            ));
        }

        if pending_blank_lines > 0 {
            append_quote_separator_children(&mut children, pending_blank_lines);
        }

        let mut block = BlockNode::leaf(BlockRecord::new(
                BlockKind::Callout(variant),
                InlineTextTree::from_markdown(&title),
            ),
        );
        block.children.extend(children);
        Some(block)
    }

    fn collect_list_blocks(
        lines: &[String],
        start: usize,
    ) -> (Vec<RootBlock>, usize) {
        let mut roots = Vec::new();
        let mut index = start;

        while index < lines.len() {
            let Some(marker) = parse_list_marker(&lines[index]) else {
                break;
            };

            let item_end = collect_list_item_region(lines, index, marker.indent_columns);
            // 围栏开在列表标记行（如 `1. ```html`）时，围栏行文本需要保留以构造原文
            let marker_fence = parse_opening_fence(&marker.text).map(|fence| (fence, marker.text.clone()));
            let mut block = native_block(marker.kind.clone(), marker.text);
            let mut body_index = index + 1;
            let mut pending_blank_lines = 0usize;
            let mut fallback_raw = false;
            let mut saw_child = false;

            // 标记行开围栏：续行去内容缩进后收集到闭合围栏，作为该项的代码块子块
            //（mermaid/math 围栏与根级一致各自特判）；未闭合则维持普通子块流程
            if let Some((fence, fence_line)) = marker_fence {
                let content_dedented =
                    dedent_lines(&lines[body_index..item_end], marker.content_indent_columns);
                if let Some(closing_index) = find_matching_closing_fence(&content_dedented, 0, &fence) {
                    let raw = std::iter::once(fence_line.as_str())
                        .chain(content_dedented[..=closing_index].iter().map(String::as_str))
                        .collect::<Vec<_>>()
                        .join("\n");
                    let child = if is_mermaid_info_string(fence.language.as_deref()) {
                        BlockNode::leaf(BlockRecord::mermaid(raw))
                    } else if is_math_info_string(fence.language.as_deref()) {
                        BlockNode::leaf(BlockRecord::math(raw))
                    } else {
                        let code_lines = content_dedented[..closing_index].to_vec();
                        build_code_block(fence.language.clone(), code_lines.join("\n"))
                    };
                    block.children.push(child);
                    block.record.set_title(InlineTextTree::plain(String::new()));
                    body_index += closing_index + 1;
                    saw_child = true;
                }
            }

            while body_index < item_end {
                let line = &lines[body_index];
                if line.trim().is_empty() {
                    pending_blank_lines += 1;
                    body_index += 1;
                    continue;
                }

                let (line_indent_columns, _) = leading_indent_columns_and_bytes(line);
                if line_indent_columns > marker.indent_columns {
                    let anchor_dedented =
                        dedent_lines(&lines[body_index..item_end], line_indent_columns);

                    if parse_list_marker(&anchor_dedented[0]).is_some() {
                        let (children, consumed) =
                            collect_list_blocks(&anchor_dedented, 0);
                        block.children.extend(children.into_iter().map(|root| root.node));
                        body_index += consumed;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }

                    if is_quote_start(&anchor_dedented[0]) {
                        let (quote, consumed) = collect_quote_block(&anchor_dedented, 0);
                        if quote.record.kind == BlockKind::RawMarkdown {
                            fallback_raw = true;
                            break;
                        }

                        block.children.extend(vec![quote]);
                        body_index += consumed;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }

                    // 项内分割线（内容级缩进的 ---）：作为分割线子块而非标题文本
                    if BlockKind::parse_separator_line(&anchor_dedented[0]) {
                        block.children.push(BlockNode::leaf(BlockRecord::new(
                            BlockKind::Separator,
                            InlineTextTree::plain(String::new()),
                        )));
                        body_index += 1;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }

                    if parse_opening_fence(&anchor_dedented[0]).is_some()
                        && let Some((code_block, consumed)) =
                            collect_fenced_code_block(&anchor_dedented, 0)
                    {
                        block.children.extend(vec![code_block]);
                        body_index += consumed;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }

                    if is_root_table_candidate_line(&anchor_dedented[0]) {
                        let table_end = collect_root_table_candidate_region(&anchor_dedented, 0);
                        let table_region = &anchor_dedented[..table_end];
                        let child = if let Some(table) = parse_root_table_region(table_region) {
                            BlockNode::leaf(BlockRecord::table(table))
                        } else {
                            raw_block(table_region.join("\n"))
                        };
                        block.children.extend(vec![child]);
                        body_index += table_end;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }

                    if starts_with_standalone_image_child_paragraph(&anchor_dedented) {
                        block.children.extend(vec![standalone_image_block(anchor_dedented[0].clone())],
                        );
                        body_index += 1;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }

                    if line_indent_columns >= marker.content_indent_columns {
                        let content_dedented = dedent_lines(
                            &lines[body_index..item_end],
                            marker.content_indent_columns,
                        );
                        if strip_indented_code_prefix(&content_dedented[0]).is_some() {
                            let Some((code_block, consumed)) =
                                collect_indented_code_block(&content_dedented, 0)
                            else {
                                unreachable!(
                                    "indented code prefix disappeared after child detection"
                                );
                            };

                            block.children.extend(vec![code_block]);
                            body_index += consumed;
                            pending_blank_lines = 0;
                            saw_child = true;
                            continue;
                        }
                    }

                    if is_reference_definition_start(&anchor_dedented[0]) {
                        let consumed = collect_reference_definition_region(&anchor_dedented, 0);
                        block.children.extend(vec![raw_block(anchor_dedented[..consumed].join("\n"))],
                        );
                        body_index += consumed;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }

                    if let Some((comment, consumed)) =
                        collect_comment_block(&anchor_dedented, 0)
                    {
                        block.children.extend(vec![comment]);
                        body_index += consumed;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }

                    if is_block_html_start(&anchor_dedented[0]) {
                        let consumed = collect_block_html_region(&anchor_dedented, 0);
                        block.children.extend(vec![html_or_raw_block(anchor_dedented[..consumed].join("\n"),
                            )],
                        );
                        body_index += consumed;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }

                    if is_footnote_definition_start(&anchor_dedented[0]) {
                        let consumed = collect_footnote_definition_region(&anchor_dedented, 0);
                        block.children.extend(vec![raw_block(anchor_dedented[..consumed].join("\n"))],
                        );
                        body_index += consumed;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }

                    if is_display_math_start(&anchor_dedented[0]) {
                        let consumed = collect_display_math_region(&anchor_dedented, 0);
                        block.children.extend(vec![math_or_raw_block(anchor_dedented[..consumed].join("\n"),
                            )],
                        );
                        body_index += consumed;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }

                    let should_promote_plain_child = pending_blank_lines > 0
                        || saw_child
                        || block.record.title.visible_text().is_empty()
                        || parse_standalone_image(&block.record.title_markdown())
                            .is_some();
                    if should_promote_plain_child {
                        let (paragraph, consumed) =
                            collect_paragraph_block(&anchor_dedented, 0);
                        block.children.extend(vec![paragraph]);
                        body_index += consumed;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }
                }

                if line_indent_columns >= marker.content_indent_columns {
                    let content_dedented =
                        dedent_lines(&lines[body_index..item_end], marker.content_indent_columns);
                    if strip_indented_code_prefix(&content_dedented[0]).is_some() {
                        let Some((code_block, consumed)) =
                            collect_indented_code_block(&content_dedented, 0)
                        else {
                            unreachable!("indented code prefix disappeared after detection");
                        };

                        block.children.extend(vec![code_block]);
                        body_index += consumed;
                        pending_blank_lines = 0;
                        saw_child = true;
                        continue;
                    }
                }

                let trimmed = line.trim_start_matches([' ', '\t']);
                append_markdown_to_block(
                    &mut block,
                    if pending_blank_lines > 0 {
                        "\n\n"
                    } else {
                        "\n"
                    },
                    trimmed,
                );
                pending_blank_lines = 0;
                body_index += 1;
            }

            if fallback_raw {
                roots.push(RootBlock::spanned(
                    raw_block(lines[index..item_end].join("\n")),
                    index,
                    item_end,
                ));
            } else {
                roots.push(RootBlock::spanned(block, index, item_end));
            }
            index = item_end;
        }

        (roots, index)
    }
