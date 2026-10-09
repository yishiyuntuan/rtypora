//! 原生 Markdown 表格数据模型与解析/序列化工具。
//!
//! 渲染模式下，根级与引用类容器内的表格按原生块支持；
//! 更复杂的嵌套上下文（仍在运行时安全子集之外）继续走原文 Markdown
//! 回落路径。
//!
//! 移植自 velotype `components/table.rs`。

// 轴高亮、布局等类型为后续原生表格 UI 预留。
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

use crate::markdown::inline::tree::InlineTextTree;

/// 表格分隔行声明的水平对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TableColumnAlignment {
    /// 无显式对齐标记（`---`）。渲染按左对齐，但与 [`Left`](Self::Left) 保持区分，
    /// 未标记的列不会在下次序列化时被静默改写为带前导冒号的形式。
    Default,
    /// 显式左对齐（`:---`）。
    Left,
    /// 居中对齐（`:---:`）。
    Center,
    /// 右对齐（`---:`）。
    Right,
}

/// 渲染模式下原生表格维护 UI 可寻址的轴种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TableAxisKind {
    /// 表格行轴。
    Row,
    /// 表格列轴。
    Column,
}

/// 原生表格内的一个行或列标记。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableAxisMarker {
    pub kind: TableAxisKind,
    pub index: usize,
}

/// 预览或选中表格轴时的视觉强调级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TableAxisHighlight {
    /// 无轴强调。
    #[default]
    None,
    /// 悬停预览强调。
    Preview,
    /// 持久的选中轴强调。
    Selected,
}

/// 原生表格块的持久化单元格内容（表头 + 数据行 + 列对齐）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableData {
    pub header: Vec<InlineTextTree>,
    pub rows: Vec<Vec<InlineTextTree>>,
    pub alignments: Vec<TableColumnAlignment>,
}

impl PartialEq for TableData {
    fn eq(&self, other: &Self) -> bool {
        self.header == other.header
            && self.rows == other.rows
            && self.alignments == other.alignments
    }
}

impl Eq for TableData {}

impl TableData {
    /// 创建空表格：一行表头、`body_rows` 个数据行、`columns` 个左对齐列（均至少为 1）。
    pub fn new_empty(body_rows: usize, columns: usize) -> Self {
        let columns = columns.max(1);
        let header = (0..columns)
            .map(|_| InlineTextTree::plain(String::new()))
            .collect::<Vec<_>>();
        let rows = (0..body_rows.max(1))
            .map(|_| {
                (0..columns)
                    .map(|_| InlineTextTree::plain(String::new()))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let alignments = vec![TableColumnAlignment::Default; columns];
        Self {
            header,
            rows,
            alignments,
        }
    }

    pub(crate) fn column_count(&self) -> usize {
        self.header
            .len()
            .max(self.alignments.len())
            .max(self.rows.iter().map(Vec::len).max().unwrap_or(0))
            .max(1)
    }

    fn normalize_shape(&mut self) {
        let columns = self.column_count();
        while self.header.len() < columns {
            self.header.push(InlineTextTree::plain(String::new()));
        }
        while self.alignments.len() < columns {
            self.alignments.push(TableColumnAlignment::Default);
        }
        for row in &mut self.rows {
            while row.len() < columns {
                row.push(InlineTextTree::plain(String::new()));
            }
        }
    }

    /// 追加一个空数据行，保持当前列数不变。
    pub fn append_row(&mut self) {
        self.normalize_shape();
        let columns = self.column_count();
        self.rows.push(
            (0..columns)
                .map(|_| InlineTextTree::plain(String::new()))
                .collect(),
        );
    }

    /// 向表头与每个数据行各追加一个空单元格（即新增一列），并记录其对齐。
    pub fn append_column(&mut self, alignment: TableColumnAlignment) {
        self.normalize_shape();
        self.header.push(InlineTextTree::plain(String::new()));
        self.alignments.push(alignment);
        for row in &mut self.rows {
            row.push(InlineTextTree::plain(String::new()));
        }
    }

    /// 设置某列的对齐（列存在才生效）。
    pub fn set_column_alignment(&mut self, column: usize, alignment: TableColumnAlignment) {
        self.normalize_shape();
        if let Some(slot) = self.alignments.get_mut(column) {
            *slot = alignment;
        }
    }

    /// 按视觉序号交换两行：行 `0` 为表头，行 `1..=rows.len()` 为数据行。
    /// 表头与首个数据行交换即交换表头与数据内容——与行拖拽把手
    /// 把表头当作可移动的一行来对待的行为一致。
    pub fn swap_visual_rows(&mut self, row_a: usize, row_b: usize) {
        self.normalize_shape();
        let total = self.rows.len() + 1;
        if row_a >= total || row_b >= total || row_a == row_b {
            return;
        }
        match (row_a, row_b) {
            (0, other) | (other, 0) => {
                std::mem::swap(&mut self.header, &mut self.rows[other - 1]);
            }
            (a, b) => self.rows.swap(a - 1, b - 1),
        }
    }

    /// 跨表头、数据行与对齐向量交换两列。
    pub fn swap_columns(&mut self, col_a: usize, col_b: usize) {
        self.normalize_shape();
        let columns = self.column_count();
        if col_a >= columns || col_b >= columns || col_a == col_b {
            return;
        }

        self.header.swap(col_a, col_b);
        self.alignments.swap(col_a, col_b);
        for row in &mut self.rows {
            row.swap(col_a, col_b);
        }
    }

    /// 删除一个数据行。
    pub fn remove_body_row(&mut self, row_index: usize) {
        self.normalize_shape();
        if row_index >= self.rows.len() {
            return;
        }
        // 表格可以只剩表头；之后删除表头本身时由编辑器移除整个块。
        self.rows.remove(row_index);
    }

    /// 删除表头行：把首个数据行提升为新表头。
    /// 没有数据行时返回 false（表格不变）——管道表必须保留表头行。
    pub fn remove_header_row(&mut self) -> bool {
        self.normalize_shape();
        if self.rows.is_empty() {
            return false;
        }
        self.header = self.rows.remove(0);
        true
    }

    /// 删除一列，至少保留一列。
    pub fn remove_column(&mut self, col_index: usize) {
        self.normalize_shape();
        let columns = self.column_count();
        if columns <= 1 || col_index >= columns {
            return;
        }

        self.header.remove(col_index);
        self.alignments.remove(col_index);
        for row in &mut self.rows {
            row.remove(col_index);
        }
    }
}

/// 原生表格每行共享的响应式列宽比例。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableColumnLayout {
    fractions: Vec<f32>,
}

impl TableColumnLayout {
    pub fn equal(column_count: usize) -> Self {
        let column_count = column_count.max(1);
        let fraction = 1.0 / column_count as f32;
        Self {
            fractions: vec![fraction; column_count],
        }
    }

    #[cfg(test)]
    pub(crate) fn fractions(&self) -> &[f32] {
        &self.fractions
    }

    pub fn fraction(&self, column: usize) -> f32 {
        self.fractions
            .get(column)
            .copied()
            .unwrap_or_else(|| 1.0 / self.fractions.len().max(1) as f32)
    }

    pub fn from_preferred_widths(
        preferred_widths: &[f32],
        table_width: f32,
        min_column_width: f32,
    ) -> Self {
        if preferred_widths.is_empty() {
            return Self::equal(1);
        }

        let column_count = preferred_widths.len();
        let safe_table_width = table_width.max(1.0);
        let equal_share = safe_table_width / column_count as f32;
        if preferred_widths
            .iter()
            .all(|preferred| *preferred <= equal_share + f32::EPSILON)
        {
            return Self::equal(column_count);
        }

        let floor_width = min_column_width
            .max(0.0)
            .min(safe_table_width / column_count as f32);
        let weights = preferred_widths
            .iter()
            .map(|preferred| preferred.max(equal_share))
            .collect::<Vec<_>>();
        let mut assigned_widths = vec![0.0; column_count];
        let mut remaining_indices = (0..column_count).collect::<Vec<_>>();
        let mut remaining_width = safe_table_width;

        loop {
            if remaining_indices.is_empty() {
                break;
            }

            let weight_sum = remaining_indices
                .iter()
                .map(|index| weights[*index])
                .sum::<f32>();
            if weight_sum <= f32::EPSILON {
                let share = remaining_width / remaining_indices.len() as f32;
                for index in remaining_indices {
                    assigned_widths[index] = share;
                }
                break;
            }

            let mut newly_floored = Vec::new();
            for index in &remaining_indices {
                let width = remaining_width * (weights[*index] / weight_sum);
                if width < floor_width - f32::EPSILON {
                    newly_floored.push(*index);
                } else {
                    assigned_widths[*index] = width;
                }
            }

            if newly_floored.is_empty() {
                break;
            }

            if newly_floored.len() == remaining_indices.len() {
                let share = remaining_width / remaining_indices.len() as f32;
                for index in remaining_indices {
                    assigned_widths[index] = share;
                }
                break;
            }

            for index in &newly_floored {
                assigned_widths[*index] = floor_width;
                remaining_width -= floor_width;
            }
            remaining_indices.retain(|index| !newly_floored.contains(index));
        }

        let assigned_sum = assigned_widths.iter().sum::<f32>();
        if assigned_sum <= f32::EPSILON {
            return Self::equal(column_count);
        }

        let fractions = assigned_widths
            .into_iter()
            .map(|width| width / assigned_sum)
            .collect::<Vec<_>>();
        Self { fractions }
    }
}

/// 单元格在原生表格内的运行时位置（仅运行时使用，不持久化）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableCellPosition {
    /// 0 基视觉行：表头为行 `0`，首个数据行为 `1`。
    pub row: usize,
    pub column: usize,
}

impl TableCellPosition {
    pub fn is_header(self) -> bool {
        self.row == 0
    }

    pub fn body_row_index(self) -> Option<usize> {
        self.row.checked_sub(1)
    }
}

fn strip_table_indent(line: &str) -> Option<&str> {
    let indent = line.bytes().take_while(|b| *b == b' ').count();
    (indent <= 3).then_some(&line[indent..])
}

fn split_table_cells(line: &str) -> Option<Vec<String>> {
    let rest = strip_table_indent(line)?.trim_end();
    if rest.is_empty() {
        return None;
    }
    // 外管道可选（GFM）：存在则剥掉，使 `Name | Score` 这样的无管道行
    // 与 `| Name | Score |` 按同样方式拆分。
    let inner = rest.strip_prefix('|').unwrap_or(rest);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut escaping = false;

    for ch in inner.chars() {
        if escaping {
            match ch {
                '|' | '\\' => current.push(ch),
                _ => {
                    current.push('\\');
                    current.push(ch);
                }
            }
            escaping = false;
            continue;
        }

        match ch {
            '\\' => escaping = true,
            '|' => {
                cells.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    if escaping {
        current.push('\\');
    }
    cells.push(current.trim().to_string());
    Some(cells)
}

fn parse_alignment_cell(cell: &str) -> Option<TableColumnAlignment> {
    let trimmed = cell.trim();
    if trimmed.len() < 3 {
        return None;
    }

    let left = trimmed.starts_with(':');
    let right = trimmed.ends_with(':');
    let core = trimmed.trim_start_matches(':').trim_end_matches(':');
    if core.len() < 3 || !core.chars().all(|ch| ch == '-') {
        return None;
    }

    Some(match (left, right) {
        (true, true) => TableColumnAlignment::Center,
        (false, true) => TableColumnAlignment::Right,
        (true, false) => TableColumnAlignment::Left,
        (false, false) => TableColumnAlignment::Default,
    })
}

fn serialize_alignment(alignment: TableColumnAlignment) -> &'static str {
    match alignment {
        TableColumnAlignment::Default => "---",
        TableColumnAlignment::Left => ":---",
        TableColumnAlignment::Center => ":---:",
        TableColumnAlignment::Right => "---:",
    }
}

pub(crate) fn serialize_table_cell_markdown(tree: &InlineTextTree) -> String {
    tree.serialize_markdown()
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('\n', " ")
}

fn serialize_row<'a>(cells: impl IntoIterator<Item = &'a InlineTextTree>) -> String {
    let rendered = cells
        .into_iter()
        .map(serialize_table_cell_markdown)
        .collect::<Vec<_>>();
    format!("| {} |", rendered.join(" | "))
}

/// 判断一行是否为当前容器作用域内的候选原生表格行（缩进 ≤3 且以 `|` 开头）。
pub fn is_table_candidate_line(line: &str) -> bool {
    strip_table_indent(line)
        .map(str::trim_end)
        .is_some_and(|rest| rest.starts_with('|'))
}

/// `line` 中以管道分隔的单元格数：外管道按 GFM 视为可选，
/// 故 `Name | Score` 这样的无管道行也能识别。单列行返回 `None`——
/// 散文中偶然出现的 `|` 不会被误判为表格行。
pub fn table_row_column_count(line: &str) -> Option<usize> {
    split_table_cells(line)
        .map(|cells| cells.len())
        .filter(|count| *count >= 2)
}

/// `line` 是否可能是表格行（含无管道的 GFM 行）。
pub fn is_table_row_candidate(line: &str) -> bool {
    table_row_column_count(line).is_some()
}

/// 收集当前容器作用域内连续的表格候选区域（返回区域末尾，开区间）。
pub fn collect_table_candidate_region(lines: &[&str], start: usize) -> usize {
    let mut index = start + 1;
    while index < lines.len() && is_table_candidate_line(&lines[index]) {
        index += 1;
    }
    index
}

/// 把管道表格区域解析为原生表格数据（表头 + 分隔行对齐 + 数据行）。
pub fn parse_table_region(lines: &[&str]) -> Option<TableData> {
    if lines.len() < 2 {
        return None;
    }

    let header = split_table_cells(&lines[0])?;
    let alignment_cells = split_table_cells(&lines[1])?;
    if header.is_empty() || alignment_cells.len() != header.len() {
        return None;
    }

    let alignments = alignment_cells
        .iter()
        .map(|cell| parse_alignment_cell(cell))
        .collect::<Option<Vec<_>>>()?;

    let mut rows = Vec::new();
    for line in &lines[2..] {
        // GFM 把数据行归一化到表头宽度：短行补空单元格、长行丢弃尾部单元格，
        // 而不是让整张表失效。
        let mut cells = split_table_cells(line)?;
        cells.resize(header.len(), String::new());
        rows.push(
            cells
                .into_iter()
                .map(|cell| {
                    crate::markdown::inline::with_current_link_refs(|refs| {
                        InlineTextTree::from_markdown_with_link_references(&cell, refs)
                    })
                })
                .collect::<Vec<_>>(),
        );
    }

    Some(TableData {
        header: header
            .into_iter()
            .map(|cell| {
                crate::markdown::inline::with_current_link_refs(|refs| {
                    InlineTextTree::from_markdown_with_link_references(&cell, refs)
                })
            })
            .collect(),
        rows,
        alignments,
    })
}

/// 判断 `line` 是否为恰好 `columns` 个单元格的分隔行（每格都是合法对齐规格）。
fn is_delimiter_row(line: &str, columns: usize) -> bool {
    split_table_cells(line).is_some_and(|cells| {
        cells.len() == columns
            && cells
                .iter()
                .all(|cell| parse_alignment_cell(cell).is_some())
    })
}

/// 探测从 `start` 开始的表格（不要求外管道）：`lines[start]` 是多列表头
/// 且紧跟匹配的分隔行时返回区域末尾（开区间）。数据行延伸到下一个空行，
/// 与 GFM 一致。普通散文返回 `None`，偶然的 `|` 不会被误认为表格；
/// 单列无管道候选同样拒绝（与 setext 标题歧义）。
pub fn collect_pipeless_table_region(lines: &[&str], start: usize) -> Option<usize> {
    // 快速守卫：不含 `|` 的散文行直接排除（避免 split_table_cells 的全量分配）
    let line = lines.get(start)?;
    if !line.contains('|') {
        return None;
    }
    let header = split_table_cells(line)?;
    if header.len() < 2 {
        return None;
    }
    if !is_delimiter_row(lines.get(start + 1)?, header.len()) {
        return None;
    }

    let mut end = start + 2;
    while end < lines.len() && !lines[end].trim().is_empty() {
        end += 1;
    }
    Some(end)
}

/// 判断根级行是否为候选原生表格行。
pub fn is_root_table_candidate_line(line: &str) -> bool {
    is_table_candidate_line(line)
}

/// 收集根级连续的表格候选区域。
pub fn collect_root_table_candidate_region(lines: &[&str], start: usize) -> usize {
    collect_table_candidate_region(lines, start)
}

/// 把根级管道表格区域解析为原生表格数据。
pub fn parse_root_table_region(lines: &[&str]) -> Option<TableData> {
    parse_table_region(lines)
}

/// 解析单个表格数据行，归一化为 `columns` 个单元格（短补空、长截尾）。
/// 该行不是表格行时返回 `None`。
pub fn parse_table_body_row(line: &str, columns: usize) -> Option<Vec<InlineTextTree>> {
    let mut cells = split_table_cells(line)?;
    cells.resize(columns, String::new());
    Some(
        cells
            .into_iter()
            .map(|cell| InlineTextTree::from_markdown(&cell))
            .collect(),
    )
}

/// 把原生表格数据序列化为规范管道表格 Markdown 行。
pub fn serialize_table_markdown_lines(table: &TableData) -> Vec<String> {
    let mut lines = Vec::with_capacity(2 + table.rows.len());
    lines.push(serialize_row(table.header.iter()));
    lines.push(format!(
        "| {} |",
        table
            .alignments
            .iter()
            .map(|alignment| serialize_alignment(*alignment))
            .collect::<Vec<_>>()
            .join(" | ")
    ));
    lines.extend(table.rows.iter().map(|row| serialize_row(row.iter())));
    lines
}

/// 各对齐方式的分隔单元格最小宽度，保证分隔行可解析
///（`parse_alignment_cell` 要求至少三个连字符）。
fn delimiter_min_width(alignment: TableColumnAlignment) -> usize {
    match alignment {
        TableColumnAlignment::Default => 3,
        TableColumnAlignment::Left | TableColumnAlignment::Right => 4,
        TableColumnAlignment::Center => 5,
    }
}

/// 把原生表格数据序列化为管道表格 Markdown，每列补齐到统一宽度使管道对齐
///（Typora 的「格式化表格源码」）。
/// 宽度按 Unicode 字符数计（不补偿 CJK 全角宽度）；内容与对齐保持不变。
pub fn serialize_table_markdown_lines_padded(table: &TableData) -> Vec<String> {
    let columns = table.column_count();
    let header = table
        .header
        .iter()
        .map(serialize_table_cell_markdown)
        .collect::<Vec<_>>();
    let body = table
        .rows
        .iter()
        .map(|row| row.iter().map(serialize_table_cell_markdown).collect::<Vec<_>>())
        .collect::<Vec<_>>();

    // 单元格字符数只算一趟：列宽统计与 pad 填充共用，避免逐格重复 chars().count()
    let header_lens: Vec<usize> = header.iter().map(|cell| cell.chars().count()).collect();
    let body_lens: Vec<Vec<usize>> = body
        .iter()
        .map(|row| row.iter().map(|cell| cell.chars().count()).collect())
        .collect();

    let mut widths = vec![3usize; columns];
    for (i, width) in widths.iter_mut().enumerate() {
        *width = (*width).max(delimiter_min_width(
            table
                .alignments
                .get(i)
                .copied()
                .unwrap_or(TableColumnAlignment::Default),
        ));
        if let Some(len) = header_lens.get(i) {
            *width = (*width).max(*len);
        }
        for row_lens in &body_lens {
            if let Some(len) = row_lens.get(i) {
                *width = (*width).max(*len);
            }
        }
    }

    let pad = |cell: &str, len: usize, width: usize| {
        if len >= width {
            cell.to_string()
        } else {
            format!("{}{}", cell, " ".repeat(width - len))
        }
    };
    let padded_row = |cells: &[String], lens: &[usize]| {
        format!(
            "| {} |",
            cells
                .iter()
                .enumerate()
                .map(|(i, cell)| pad(cell, lens[i], widths.get(i).copied().unwrap_or(3)))
                .collect::<Vec<_>>()
                .join(" | ")
        )
    };

    let mut lines = Vec::with_capacity(2 + body.len());
    lines.push(padded_row(&header, &header_lens));
    lines.push(format!(
        "| {} |",
        (0..columns)
            .map(|i| {
                let width = widths[i];
                match table.alignments.get(i).copied().unwrap_or(TableColumnAlignment::Default) {
                    TableColumnAlignment::Default => "-".repeat(width),
                    TableColumnAlignment::Left => format!(":{}", "-".repeat(width - 1)),
                    TableColumnAlignment::Right => format!("{}:", "-".repeat(width - 1)),
                    TableColumnAlignment::Center => {
                        format!(":{}:", "-".repeat(width.saturating_sub(2)))
                    }
                }
            })
            .collect::<Vec<_>>()
            .join(" | ")
    ));
    lines.extend(body.iter().zip(&body_lens).map(|(row, lens)| padded_row(row, lens)));
    lines
}

#[cfg(test)]
mod tests {
    use super::{
        TableColumnAlignment, TableColumnLayout, TableData, collect_pipeless_table_region,
        collect_root_table_candidate_region, is_root_table_candidate_line, parse_root_table_region,
        serialize_table_markdown_lines,
    };
    use crate::markdown::inline::tree::InlineTextTree;

    fn assert_close(left: f32, right: f32) {
        assert!(
            (left - right).abs() < 0.0001,
            "expected {left} to be close to {right}"
        );
    }

    #[test]
    fn parses_valid_root_table_region() {
        let lines = vec![
            "| Left | Center | Right |",
            "| :--- | :---: | ---: |",
            "| a | b | c |",
        ];
        let table = parse_root_table_region(&lines).expect("table should parse");
        assert_eq!(table.alignments.len(), 3);
        assert_eq!(
            table.alignments,
            vec![
                TableColumnAlignment::Left,
                TableColumnAlignment::Center,
                TableColumnAlignment::Right
            ]
        );
        assert_eq!(table.header[0].serialize_markdown(), "Left");
        assert_eq!(table.rows[0][2].serialize_markdown(), "c");
    }

    #[test]
    fn rejects_invalid_alignment_row() {
        let lines = vec!["| Left | Right |", "| nope | --- |"];
        assert!(parse_root_table_region(&lines).is_none());
    }

    #[test]
    fn rejects_alignment_row_with_wrong_column_count() {
        let lines = vec!["| A | B | C |", "| --- | --- |"];
        assert!(parse_root_table_region(&lines).is_none());
    }

    #[test]
    fn preserves_explicit_left_alignment_colon() {
        // ":---" 是显式左对齐，必须在解析/序列化往返后存活，
        // 而不是被静默改写为裸的 "---"。
        let lines = vec![
            "| L | D | R |",
            "| :--- | --- | ---: |",
            "| a | b | c |",
        ];
        let table = parse_root_table_region(&lines).expect("table should parse");
        assert_eq!(
            table.alignments,
            vec![
                TableColumnAlignment::Left,
                TableColumnAlignment::Default,
                TableColumnAlignment::Right
            ]
        );
        assert_eq!(
            serialize_table_markdown_lines(&table)[1],
            "| :--- | --- | ---: |"
        );
    }

    #[test]
    fn pads_short_body_rows_and_truncates_long_ones() {
        let lines = vec![
            "| A | B | C |",
            "| --- | --- | --- |",
            "| short |",
            "| 1 | 2 | 3 | 4 |",
        ];
        let table = parse_root_table_region(&lines).expect("table should parse");
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0].len(), 3);
        assert_eq!(table.rows[0][0].serialize_markdown(), "short");
        assert!(table.rows[0][1].serialize_markdown().is_empty());
        assert!(table.rows[0][2].serialize_markdown().is_empty());
        assert_eq!(table.rows[1].len(), 3);
        assert_eq!(table.rows[1][2].serialize_markdown(), "3");
    }

    #[test]
    fn parses_pipeless_table() {
        let lines = vec![
            "Name | Score",
            "--- | ---",
            "Alice | 10",
            "Bob | 7",
        ];
        let end = collect_pipeless_table_region(&lines, 0).expect("region");
        assert_eq!(end, 4);
        let table = parse_root_table_region(&lines[..end]).expect("table should parse");
        assert_eq!(table.header.len(), 2);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.header[0].serialize_markdown(), "Name");
        assert_eq!(table.rows[1][1].serialize_markdown(), "7");
    }

    #[test]
    fn prose_with_pipe_is_not_a_pipeless_table() {
        let lines = vec!["this | that", "and the next line"];
        assert!(collect_pipeless_table_region(&lines, 0).is_none());
    }

    #[test]
    fn pipeless_table_requires_valid_delimiter_row() {
        let lines = vec!["Name | Score", "Alice | 10"];
        assert!(collect_pipeless_table_region(&lines, 0).is_none());
    }

    #[test]
    fn single_column_pipeless_is_not_a_table() {
        // 与 setext 标题歧义；不得被捕获为表格。
        let lines = vec!["Title", "---"];
        assert!(collect_pipeless_table_region(&lines, 0).is_none());
    }

    #[test]
    fn serializes_canonical_pipe_table() {
        let table = TableData {
            header: vec![
                InlineTextTree::from_markdown("**bold**"),
                InlineTextTree::from_markdown("[link](https://example.com)"),
            ],
            rows: vec![vec![
                InlineTextTree::plain("A | B"),
                InlineTextTree::plain("value"),
            ]],
            alignments: vec![TableColumnAlignment::Default, TableColumnAlignment::Right],
        };
        assert_eq!(
            serialize_table_markdown_lines(&table),
            vec![
                "| **bold** | [link](https://example.com) |",
                "| --- | ---: |",
                "| A \\| B | value |",
            ]
        );
    }

    #[test]
    fn detects_root_table_candidate_runs() {
        let lines = vec![
            "| A | B |",
            "| --- | --- |",
            "| 1 | 2 |",
            "paragraph",
        ];
        assert!(is_root_table_candidate_line(&lines[0]));
        assert_eq!(collect_root_table_candidate_region(&lines, 0), 3);
    }

    #[test]
    fn equal_share_fast_path_keeps_columns_uniform() {
        let layout = TableColumnLayout::from_preferred_widths(&[32.0, 64.0, 48.0], 360.0, 60.0);
        let fractions = layout.fractions();
        assert_eq!(fractions.len(), 3);
        assert_close(fractions[0], 1.0 / 3.0);
        assert_close(fractions[1], 1.0 / 3.0);
        assert_close(fractions[2], 1.0 / 3.0);
    }

    #[test]
    fn content_pressure_redistributes_width_across_the_whole_column() {
        let layout = TableColumnLayout::from_preferred_widths(&[48.0, 220.0, 48.0], 360.0, 60.0);
        let fractions = layout.fractions();
        assert_eq!(fractions.len(), 3);
        assert!(fractions[1] > fractions[0]);
        assert!(fractions[1] > fractions[2]);
        assert_close(fractions[0], fractions[2]);
    }

    #[test]
    fn minimum_column_floor_prevents_neighbor_collapse() {
        let layout = TableColumnLayout::from_preferred_widths(&[16.0, 900.0, 16.0], 300.0, 70.0);
        let fractions = layout.fractions();
        let widths = fractions
            .iter()
            .map(|fraction| fraction * 300.0)
            .collect::<Vec<_>>();
        assert!(widths[0] >= 70.0 - 0.001);
        assert!(widths[2] >= 70.0 - 0.001);
        assert_close(fractions.iter().sum::<f32>(), 1.0);
    }

    #[test]
    fn moderate_single_cell_growth_stays_equal_when_share_is_sufficient() {
        let layout = TableColumnLayout::from_preferred_widths(&[56.0, 92.0, 56.0], 360.0, 60.0);
        let fractions = layout.fractions();
        assert_close(fractions[0], 1.0 / 3.0);
        assert_close(fractions[1], 1.0 / 3.0);
        assert_close(fractions[2], 1.0 / 3.0);
    }

    #[test]
    fn append_row_preserves_column_count_and_creates_empty_cells() {
        let mut table = TableData::new_empty(1, 3);
        table.append_row();

        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[1].len(), 3);
        assert!(
            table.rows[1]
                .iter()
                .all(|cell| cell.serialize_markdown().is_empty())
        );
    }

    #[test]
    fn append_column_extends_every_row_and_uses_requested_alignment() {
        let mut table = TableData {
            header: vec![
                InlineTextTree::plain("A"),
                InlineTextTree::plain("B"),
            ],
            rows: vec![
                vec![
                    InlineTextTree::plain("1"),
                    InlineTextTree::plain("2"),
                ],
                vec![
                    InlineTextTree::plain("3"),
                    InlineTextTree::plain("4"),
                ],
            ],
            alignments: vec![TableColumnAlignment::Left, TableColumnAlignment::Right],
        };

        table.append_column(TableColumnAlignment::Right);

        assert_eq!(table.header.len(), 3);
        assert_eq!(table.rows[0].len(), 3);
        assert_eq!(table.rows[1].len(), 3);
        assert_eq!(
            table.alignments,
            vec![
                TableColumnAlignment::Left,
                TableColumnAlignment::Right,
                TableColumnAlignment::Right,
            ]
        );
        assert!(table.header[2].serialize_markdown().is_empty());
        assert!(table.rows[0][2].serialize_markdown().is_empty());
        assert!(table.rows[1][2].serialize_markdown().is_empty());
    }

    #[test]
    fn append_column_pads_missing_alignments_with_default() {
        let mut table = TableData {
            header: vec![InlineTextTree::plain("A")],
            rows: vec![vec![InlineTextTree::plain("1")]],
            alignments: Vec::new(),
        };

        table.append_column(TableColumnAlignment::Left);

        assert_eq!(
            table.alignments,
            vec![TableColumnAlignment::Default, TableColumnAlignment::Left]
        );
        assert_eq!(table.header.len(), 2);
        assert_eq!(table.rows[0].len(), 2);
    }

    #[test]
    fn set_column_alignment_updates_requested_column() {
        let mut table = TableData::new_empty(2, 3);
        table.set_column_alignment(1, TableColumnAlignment::Center);
        assert_eq!(
            table.alignments,
            vec![
                TableColumnAlignment::Default,
                TableColumnAlignment::Center,
                TableColumnAlignment::Default
            ]
        );
    }

    #[test]
    fn swap_visual_rows_exchanges_header_with_first_body_row() {
        let mut table = TableData {
            header: vec![InlineTextTree::plain("A")],
            rows: vec![
                vec![InlineTextTree::plain("1")],
                vec![InlineTextTree::plain("2")],
            ],
            alignments: vec![TableColumnAlignment::Left],
        };
        // 视觉行 0 是表头；与视觉行 1 交换即交换表头与首个数据行内容。
        table.swap_visual_rows(0, 1);
        assert_eq!(table.header[0].serialize_markdown(), "1");
        assert_eq!(table.rows[0][0].serialize_markdown(), "A");
        assert_eq!(table.rows[1][0].serialize_markdown(), "2");

        // 两个数据行（视觉行 1 和 2）像普通行一样交换。
        table.swap_visual_rows(1, 2);
        assert_eq!(table.rows[0][0].serialize_markdown(), "2");
        assert_eq!(table.rows[1][0].serialize_markdown(), "A");
    }

    #[test]
    fn swap_columns_exchanges_header_body_and_alignment() {
        let mut table = TableData {
            header: vec![
                InlineTextTree::plain("A"),
                InlineTextTree::plain("B"),
            ],
            rows: vec![vec![
                InlineTextTree::plain("1"),
                InlineTextTree::plain("2"),
            ]],
            alignments: vec![TableColumnAlignment::Left, TableColumnAlignment::Right],
        };
        table.swap_columns(0, 1);
        assert_eq!(table.header[0].serialize_markdown(), "B");
        assert_eq!(table.rows[0][0].serialize_markdown(), "2");
        assert_eq!(
            table.alignments,
            vec![TableColumnAlignment::Right, TableColumnAlignment::Left]
        );
    }

    #[test]
    fn remove_body_row_can_empty_the_table() {
        let mut table = TableData::new_empty(2, 2);
        table.remove_body_row(0);
        assert_eq!(table.rows.len(), 1);
        table.remove_body_row(0);
        // 最后一个数据行可以删除，留下仅表头的表格。
        assert!(table.rows.is_empty());
        // 越界删除是空操作。
        table.remove_body_row(0);
        assert!(table.rows.is_empty());
    }

    #[test]
    fn remove_header_row_promotes_first_body_row() {
        let mut table = parse_root_table_region(&[
            "| A | B |",
            "| --- | --- |",
            "| 1 | 2 |",
            "| 3 | 4 |",
        ])
        .expect("valid table");

        assert!(table.remove_header_row());
        assert_eq!(table.header[0].serialize_markdown(), "1");
        assert_eq!(table.header[1].serialize_markdown(), "2");
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0][0].serialize_markdown(), "3");

        // 提升最后剩余的一行后，留下仅表头的表格。
        assert!(table.remove_header_row());
        assert!(table.rows.is_empty());
        assert!(!table.remove_header_row());
        assert_eq!(table.header[0].serialize_markdown(), "3");
    }

    #[test]
    fn remove_column_preserves_at_least_one_column() {
        let mut table = TableData::new_empty(2, 2);
        table.remove_column(0);
        assert_eq!(table.column_count(), 1);
        table.remove_column(0);
        assert_eq!(table.column_count(), 1);
    }
}
