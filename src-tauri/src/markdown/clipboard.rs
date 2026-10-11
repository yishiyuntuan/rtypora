//! 剪贴板内容到表格的转换命令。
//!
//! 前端粘贴时把剪贴板的 text/html 交给这里：
//! - `table_markdown_from_clipboard`：HTML 表格或 TSV 文本 → 规范 Markdown 管道表格；
//! - `table_matrix_from_text`：TSV 文本 → 二维字符串矩阵（用于已聚焦表格时的按区域填充）；
//! - `table_to_csv`：原生表格数据 → RFC4180 CSV。
//!
//! 所有解析都在 Rust 侧完成，前端不自行拼 Markdown。

use super::inline::tree::InlineTextTree;
use super::table::{serialize_table_markdown_lines, TableColumnAlignment, TableData};

/// 输入文本长度上限（字符数），防止超大剪贴板拖垮解析。
const MAX_INPUT_CHARS: usize = 5_000_000;
/// 单元格总数上限，防止恶意巨型表格。
const MAX_CELLS: usize = 10_000;

/// 从剪贴板内容生成 Markdown 管道表格。
/// HTML 含 `<table` 时优先解析首个表格；否则按 TSV 处理纯文本。
/// 无法识别为表格时返回 None，前端退化为普通文本粘贴。
#[tauri::command]
pub fn table_markdown_from_clipboard(text: Option<String>, html: Option<String>) -> Option<String> {
    // HTML 路径优先
    if let Some(html) = html.as_deref() {
        if html.len() <= MAX_INPUT_CHARS && html.to_ascii_lowercase().contains("<table") {
            // 首个 table 无有效行时落回纯文本路径
            if let Some(matrix) = parse_html_table(html) {
                if !over_cell_limit(&matrix) {
                    return Some(serialize_matrix(&matrix));
                }
                return None;
            }
        }
    }
    // 纯文本 TSV 路径
    let text = text?;
    if text.len() > MAX_INPUT_CHARS {
        return None;
    }
    let matrix = parse_tsv_matrix(&text)?;
    if over_cell_limit(&matrix) {
        return None;
    }
    Some(serialize_matrix(&matrix))
}

/// 把 TSV 文本解析为二维字符串矩阵（供前端在已聚焦表格中按区域填充）。
#[tauri::command]
pub fn table_matrix_from_text(text: String) -> Option<Vec<Vec<String>>> {
    if text.len() > MAX_INPUT_CHARS {
        return None;
    }
    let matrix = parse_tsv_matrix(&text)?;
    if over_cell_limit(&matrix) {
        return None;
    }
    Some(matrix)
}

/// 把原生表格数据序列化为 RFC4180 CSV（表头 + 全部数据行）。
/// 单元格取其行内树的全部片段文本拼接。
#[tauri::command]
pub fn table_to_csv(table: TableData) -> String {
    let mut output = String::new();
    let mut write_row = |cells: &[InlineTextTree]| {
        let fields: Vec<String> = cells.iter().map(cell_plain_text).collect();
        output.push_str(&encode_csv_record(&fields));
        output.push_str("\r\n");
    };
    write_row(&table.header);
    for row in &table.rows {
        write_row(row);
    }
    output
}

/// 拼接行内树全部片段的纯文本。
fn cell_plain_text(tree: &InlineTextTree) -> String {
    tree.fragments
        .iter()
        .map(|fragment| fragment.text.as_str())
        .collect()
}

/// RFC4180 记录编码：含逗号/引号/换行的字段加引号，引号双写。
fn encode_csv_record(fields: &[String]) -> String {
    let mut record = String::new();
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            record.push(',');
        }
        let needs_quote = field.contains([',', '"', '\n', '\r']);
        if needs_quote {
            record.push('"');
            record.push_str(&field.replace('"', "\"\""));
            record.push('"');
        } else {
            record.push_str(field);
        }
    }
    record
}

fn over_cell_limit(matrix: &[Vec<String>]) -> bool {
    matrix.iter().map(Vec::len).sum::<usize>() > MAX_CELLS
}

/// 把矩阵包成 TableData（全 default 对齐）并走既有序列化路径生成规范管道表格。
/// `|` 与 `\` 的转义由序列化器完成；单元格内换行以 `<br>` 字面量保留。
fn serialize_matrix(matrix: &[Vec<String>]) -> String {
    let to_tree = |cell: &str| InlineTextTree::plain(cell.replace('\n', "<br>").replace('\r', ""));
    let header: Vec<InlineTextTree> = matrix[0].iter().map(|cell| to_tree(cell)).collect();
    let rows: Vec<Vec<InlineTextTree>> = matrix[1..]
        .iter()
        .map(|row| row.iter().map(|cell| to_tree(cell)).collect())
        .collect();
    let alignments = vec![TableColumnAlignment::Default; header.len()];
    let table = TableData {
        header,
        rows,
        alignments,
    };
    serialize_table_markdown_lines(&table).join("\n")
}

/// 解析 TSV 文本为矩阵；不满足表格特征（≥2 个非空行且至少一行含制表符）时返回 None。
/// 引号规则参照 RFC4180：`"..."` 包裹的单元格内可含换行与制表符，`""` 还原为 `"`。
fn parse_tsv_matrix(text: &str) -> Option<Vec<Vec<String>>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_quotes {
            match ch {
                '"' => {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        cell.push('"');
                    } else {
                        in_quotes = false;
                    }
                }
                _ => cell.push(ch),
            }
            continue;
        }
        match ch {
            '"' if cell.is_empty() => in_quotes = true,
            '\t' => {
                row.push(std::mem::take(&mut cell));
            }
            '\n' => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            '\r' => {} // 吞掉 CR，统一按 \n 分行
            _ => cell.push(ch),
        }
    }
    // 末尾无换行的残余单元格/行
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }

    // 丢弃全空行
    rows.retain(|r| r.iter().any(|c| !c.trim().is_empty()));
    let has_tab_structure = rows.iter().any(|r| r.len() >= 2);
    if rows.len() < 2 || !has_tab_structure {
        return None;
    }
    // 归一化列数：以表头列数为准，不足补空、超出截断
    let columns = rows[0].len();
    for row in &mut rows {
        row.resize(columns, String::new());
        row.truncate(columns);
    }
    Some(rows)
}

/// 从 HTML 中解析首个 `<table>`：抽取 `<tr>` 及其 `<td>/<th>` 文本。
/// 剥离内部标签并解码常见实体；无有效行时返回 None。
/// 无 `<table`、无有效 `<tr>` 或单元格全空时返回 None（调用方落回纯文本路径）。
fn parse_html_table(html: &str) -> Option<Vec<Vec<String>>> {
    let lower = html.to_ascii_lowercase();
    let table_start = lower.find("<table")?;
    // 只处理首个 table，到 </table> 或串尾为止
    let scope_end = lower[table_start..]
        .find("</table>")
        .map(|offset| table_start + offset)
        .unwrap_or(lower.len());
    let scope = &html[table_start..scope_end];
    let scope_lower = &lower[table_start..scope_end];

    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut cursor = 0;
    while let Some(rel) = scope_lower[cursor..].find("<tr") {
        let tr_open = cursor + rel;
        // 确认是标签名（后跟空白或 '>'）
        let after = tr_open + 3;
        if !scope_lower[after..]
            .chars()
            .next()
            .is_some_and(|c| c.is_whitespace() || c == '>')
        {
            cursor = after;
            continue;
        }
        let tr_end = scope_lower[after..]
            .find("</tr>")
            .map(|offset| after + offset)
            .unwrap_or(scope.len());
        let row_html = &scope[after..tr_end];
        let row_lower = &scope_lower[after..tr_end];

        let mut cells: Vec<String> = Vec::new();
        let mut cell_cursor = 0;
        while let Some(rel) = find_next_cell_open(row_lower, cell_cursor) {
            let open_end = row_lower[rel..]
                .find('>')
                .map(|offset| rel + offset + 1)
                .unwrap_or(row_html.len());
            let close_tag = if row_lower[rel..].starts_with("<th") {
                "</th>"
            } else {
                "</td>"
            };
            let close = row_lower[open_end..]
                .find(close_tag)
                .map(|offset| open_end + offset)
                .unwrap_or(row_html.len());
            cells.push(decode_entities(&strip_tags(&row_html[open_end..close])));
            cell_cursor = close + close_tag.len().min(row_html.len().saturating_sub(close));
        }
        if !cells.is_empty() {
            rows.push(cells);
        }
        cursor = tr_end;
    }

    if rows.is_empty()
        || rows
            .iter()
            .all(|row| row.iter().all(|cell| cell.trim().is_empty()))
    {
        return None;
    }
    // 归一化列数
    let columns = rows[0].len();
    for row in &mut rows {
        row.resize(columns, String::new());
        row.truncate(columns);
    }
    Some(rows)
}

/// 从 `from` 起找下一个 `<td`/`<th` 开标签位置。
fn find_next_cell_open(lower: &str, from: usize) -> Option<usize> {
    let td = lower[from..].find("<td").map(|o| from + o);
    let th = lower[from..].find("<th").map(|o| from + o);
    match (td, th) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) | (None, Some(a)) => Some(a),
        (None, None) => None,
    }
    .filter(|pos| {
        lower[*pos + 3..]
            .chars()
            .next()
            .is_some_and(|c| c.is_whitespace() || c == '>')
    })
}

/// 剥离 HTML 标签，保留文本内容；`<br>` 转为换行。
fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut chars = html.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '<' {
            // 读到 '>' 为止；<br> 转换行
            let mut tag = String::new();
            for c in chars.by_ref() {
                if c == '>' {
                    break;
                }
                tag.push(c);
            }
            let name = tag.trim().trim_end_matches('/').to_ascii_lowercase();
            if name == "br" {
                out.push('\n');
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// 解码常见 HTML 实体。
fn decode_entities(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .trim()
        .to_string()
}
