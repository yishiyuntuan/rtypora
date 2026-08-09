//! 文档树 → Markdown 序列化：移植自 velotype `editor/tree.rs` 的 `markdown_text`
//! 与 `editor/persistence.rs` 的安全代码围栏工具。
//!
//! velotype 的 `DocumentTree` 还维护滚动虚拟化快照与结构变更 API（GPUI 编辑器
//! 状态），web 端不需要，这里只保留「BlockNode 树 → 规范 Markdown」的遍历。

use super::document::BlockNode;
use super::state::{BlockKind, CalloutVariant};
use crate::markdown::inline::image::parse_standalone_image;
use crate::markdown::table::serialize_table_markdown_lines;

/// 把块树序列化为规范 Markdown 全文。
pub fn serialize_blocks(roots: &[BlockNode]) -> String {
    let mut lines = Vec::new();
    collect_root_markdown_lines(roots, &mut lines);
    lines.join("\n")
}

fn is_empty_root_paragraph(node: &BlockNode) -> bool {
    node.record.kind == BlockKind::Paragraph
        && node.record.title.visible_text().is_empty()
        && node.children.is_empty()
}

/// 序号推进用的「空段落」判定（与前端 numberedOrdinals 同规则：空段落不打断编号，
/// 独立图片段落等内容块打断）
fn is_empty_paragraph_like(node: &BlockNode) -> bool {
    node.record.kind == BlockKind::Paragraph
        && node.record.title.visible_text().trim().is_empty()
        && node.children.is_empty()
        && parse_standalone_image(&node.record.title_markdown()).is_none()
}

/// 有序列表序号推进：NumberedListItem 返回序号（Some(n)）；
/// 空段落记为组间间隔（gap，不立即重置）；其余块重置并返回 None。
/// 规则（与前端 numberedOrdinals 逐字一致，源即所见）：
/// 组首项采用源标记数字（缺省 1）；组内递增；**空行后遇源标记 1. 视为新列表重启为 1**
fn next_numbered_ordinal(node: &BlockNode, ordinal: &mut usize, gap: bool) -> Option<usize> {
    if node.record.kind == BlockKind::NumberedListItem {
        *ordinal = if *ordinal == 0 {
            node.record.list_start.unwrap_or(1)
        } else if gap && node.record.list_start == Some(1) {
            1
        } else {
            *ordinal + 1
        };
        Some(*ordinal)
    } else if is_empty_paragraph_like(node) {
        None
    } else {
        *ordinal = 0;
        None
    }
}

fn collect_root_markdown_lines(blocks: &[BlockNode], lines: &mut Vec<String>) {
    let mut pending_empty_roots = 0usize;
    let mut wrote_non_empty_root = false;
    let mut previous_was_list_item = false;
    let mut numbered_ordinal = 0usize;

    for node in blocks {
        if is_empty_root_paragraph(node) {
            pending_empty_roots += 1;
            continue;
        }

        let current_is_list_item = node.record.kind.is_list_item();
        if wrote_non_empty_root {
            let separator_count = if previous_was_list_item && current_is_list_item {
                pending_empty_roots
            } else {
                pending_empty_roots + 1
            };
            lines.extend(std::iter::repeat_n(String::new(), separator_count));
        } else if pending_empty_roots > 0 {
            lines.extend(std::iter::repeat_n(String::new(), pending_empty_roots));
        }

        let list_ordinal = next_numbered_ordinal(node, &mut numbered_ordinal, pending_empty_roots > 0);
        collect_single_block_markdown_lines(node, 0, lines, list_ordinal);
        wrote_non_empty_root = true;
        pending_empty_roots = 0;
        previous_was_list_item = current_is_list_item;
    }

    if wrote_non_empty_root {
        if pending_empty_roots > 0 {
            lines.extend(std::iter::repeat_n(String::new(), pending_empty_roots + 1));
        }
    } else if pending_empty_roots > 1 {
        lines.extend(std::iter::repeat_n(String::new(), pending_empty_roots));
    }
}

fn collect_single_block_markdown_lines(node: &BlockNode, list_depth: usize, lines: &mut Vec<String>, list_ordinal: Option<usize>) {
    match &node.record.kind {
        BlockKind::Table => {
            if let Some(table) = node.record.table.as_ref() {
                // 列表项内的表格子块必须带项内容列缩进，否则重解析时
                // 未缩进的管道行被懒惰续行吞入项标题（表格子块丢失）
                let indentation = "  ".repeat(list_depth);
                lines.extend(
                    serialize_table_markdown_lines(table)
                        .into_iter()
                        .map(|line| format!("{indentation}{line}")),
                );
            }
        }
        BlockKind::CodeBlock { language } => {
            let indentation = "  ".repeat(list_depth);
            let lang_str = language.as_deref().unwrap_or("");
            let fence = safe_code_fence_with_info(
                &node.record.title.visible_text(),
                language.as_deref(),
            );
            lines.push(format!("{indentation}{fence}{lang_str}"));
            let content = node.record.title.visible_text();
            for code_line in content.split('\n') {
                lines.push(format!("{indentation}{code_line}"));
            }
            lines.push(format!("{indentation}{fence}"));
        }
        BlockKind::Quote => {
            let title_markdown =
                CalloutVariant::escape_plain_quote_header(&node.record.title_markdown());
            let indentation = "  ".repeat(list_depth);
            if !title_markdown.is_empty() || node.children.is_empty() {
                for line in title_markdown.split('\n') {
                    lines.push(format!("{indentation}> {line}"));
                }
            }

            if !node.children.is_empty() {
                let mut child_lines = Vec::new();
                collect_markdown_lines(&node.children, list_depth, &mut child_lines, false);
                lines.extend(
                    child_lines
                        .into_iter()
                        .map(|line| format!("{indentation}> {line}")),
                );
            }
        }
        BlockKind::Callout(variant) => {
            let indentation = "  ".repeat(list_depth);
            lines.push(format!(
                "{indentation}> {}",
                variant.header_markdown(&node.record.title_markdown())
            ));
            if !node.children.is_empty() {
                let mut child_lines = Vec::new();
                collect_markdown_lines(&node.children, list_depth, &mut child_lines, false);
                lines.extend(
                    child_lines
                        .into_iter()
                        .map(|line| format!("{indentation}> {line}")),
                );
            }
        }
        BlockKind::FootnoteDefinition => {
            let indentation = "  ".repeat(list_depth);
            let id = node.record.title.visible_text();
            if node.children.is_empty() {
                lines.push(format!("{indentation}[^{}]:" , id));
                return;
            }

            let first_child = &node.children[0];
            let first_is_paragraph = first_child.record.kind == BlockKind::Paragraph;
            if first_is_paragraph {
                let first_title = first_child.record.title_markdown();
                let mut first_lines = first_title.split('\n');
                let first_line = first_lines.next().unwrap_or_default();
                lines.push(format!("{indentation}[^{}]: {}", id, first_line));
                for line in first_lines {
                    if line.is_empty() {
                        lines.push(String::new());
                    } else {
                        lines.push(format!("{indentation}    {line}"));
                    }
                }

                if node.children.len() > 1 {
                    lines.push(String::new());
                    collect_markdown_lines(&node.children[1..], 2, lines, true);
                }
            } else {
                lines.push(format!("{indentation}[^{}]:" , id));
                collect_markdown_lines(&node.children, 2, lines, true);
            }
        }
        BlockKind::RawMarkdown
        | BlockKind::Comment
        | BlockKind::HtmlBlock
        | BlockKind::MathBlock
        | BlockKind::MermaidBlock => {
            let indentation = "  ".repeat(list_depth);
            let raw_markdown = node
                .record
                .raw_fallback
                .clone()
                .unwrap_or_else(|| node.record.title_markdown());
            for line in raw_markdown.split('\n') {
                if indentation.is_empty() {
                    lines.push(line.to_string());
                } else {
                    lines.push(format!("{indentation}{line}"));
                }
            }
        }
        BlockKind::BulletedListItem
        | BlockKind::TaskListItem { .. }
        | BlockKind::NumberedListItem => {
            // 有序列表序号按兄弟位置序列化（1. 2. 3. ...），与渲染层同规则
            lines.push(node.record.markdown_line(list_depth, list_ordinal));
            let child_list_depth = list_depth + 1;
            let mut previous_child_was_table = false;
            let mut child_ordinal = 0usize;
            let mut previous_child_was_gap = false;
            for child in &node.children {
                // 相邻两个表格子块之间必须有空行（否则重解析合并为一张表）
                if previous_child_was_table && child.record.kind == BlockKind::Table {
                    lines.push(String::new());
                }
                if list_child_requires_leading_blank_line(child) {
                    lines.push(String::new());
                }
                let child_list_ordinal =
                    next_numbered_ordinal(child, &mut child_ordinal, previous_child_was_gap);
                collect_single_block_markdown_lines(child, child_list_depth, lines, child_list_ordinal);
                previous_child_was_table = child.record.kind == BlockKind::Table;
                previous_child_was_gap = is_empty_paragraph_like(child);
            }
        }
        _ => {
            lines.push(node.record.markdown_line(list_depth, None));
            let child_list_depth = list_depth + usize::from(node.record.kind.is_list_item());
            collect_markdown_lines(&node.children, child_list_depth, lines, false);
        }
    }
}

fn list_child_requires_leading_blank_line(node: &BlockNode) -> bool {
    if node.record.kind != BlockKind::Paragraph || !node.children.is_empty() {
        return false;
    }

    let markdown = node.record.title_markdown();
    !markdown.is_empty() && parse_standalone_image(&markdown).is_none()
}

fn collect_markdown_lines(
    blocks: &[BlockNode],
    depth: usize,
    lines: &mut Vec<String>,
    blank_line_between_siblings: bool,
) {
    let mut first = true;
    let mut previous_was_list_item = false;
    let mut previous_was_table = false;
    let mut previous_was_gap = false;
    let mut numbered_ordinal = 0usize;
    for node in blocks {
        let current_is_list_item = node.record.kind.is_list_item();
        let current_is_table = node.record.kind == BlockKind::Table;
        // 相邻两个表格之间必须有空行（否则连续管道行重解析时合并为一张表），与空行规则无关
        if !first
            && ((blank_line_between_siblings && !(previous_was_list_item && current_is_list_item))
                || (previous_was_table && current_is_table))
        {
            lines.push(String::new());
        }
        first = false;

        let list_ordinal = next_numbered_ordinal(node, &mut numbered_ordinal, previous_was_gap);
        collect_single_block_markdown_lines(node, depth, lines, list_ordinal);
        previous_was_list_item = current_is_list_item;
        previous_was_table = current_is_table;
        previous_was_gap = is_empty_paragraph_like(node);
    }
}

fn longest_marker_run(text: &str, marker: char) -> usize {
    let mut longest = 0usize;
    let mut current = 0usize;

    for ch in text.chars() {
        if ch == marker {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }

    longest
}

/// 内容含反引号串时选用更长的围栏，保证代码块内容不会提前闭合。
pub fn safe_code_fence(content: &str) -> String {
    let longest_backticks = longest_marker_run(content, '`');
    if longest_backticks < 3 {
        return "```".to_string();
    }

    let longest_tildes = longest_marker_run(content, '~');
    "~".repeat(longest_tildes.max(2) + 1)
}

/// info string 含反引号时围栏必须用波浪号。
pub fn safe_code_fence_with_info(content: &str, info: Option<&str>) -> String {
    if info.is_some_and(|info| info.contains('`')) {
        let longest_tildes = longest_marker_run(content, '~');
        return "~".repeat(longest_tildes.max(2) + 1);
    }

    safe_code_fence(content)
}
