//! 剪贴板表格转换命令的集成测试。

use tauri_app_lib::markdown::clipboard::{
    table_markdown_from_clipboard, table_matrix_from_text, table_to_csv,
};
use tauri_app_lib::markdown::inline::tree::InlineTextTree;
use tauri_app_lib::markdown::table::{TableColumnAlignment, TableData};

fn plain_cells(texts: &[&str]) -> Vec<InlineTextTree> {
    texts.iter().map(|t| InlineTextTree::plain(*t)).collect()
}

// ---------- table_markdown_from_clipboard：TSV 路径 ----------

#[test]
fn tsv_basic_becomes_pipe_table() {
    let md = table_markdown_from_clipboard(Some("a\tb\n1\t2".to_string()), None).unwrap();
    assert_eq!(md, "| a | b |\n| --- | --- |\n| 1 | 2 |");
}

#[test]
fn tsv_pipe_char_escaped() {
    let md = table_markdown_from_clipboard(Some("a|b\tc\nd\te".to_string()), None).unwrap();
    assert!(md.contains("a\\|b"), "管道符应转义为 \\|：{md}");
}

#[test]
fn tsv_quoted_multiline_cell_uses_br() {
    // 引号包裹的单元格内换行保留（序列化时替换为 <br>）
    let input = "name\tdesc\nfoo\t\"line1\nline2\"";
    let md = table_markdown_from_clipboard(Some(input.to_string()), None).unwrap();
    assert!(md.contains("line1<br>line2"), "单元格内换行应为 <br>：{md}");
}

#[test]
fn tsv_quoted_cell_restores_double_quote() {
    let input = "a\tb\n\"say \"\"hi\"\"\"\tc";
    let md = table_markdown_from_clipboard(Some(input.to_string()), None).unwrap();
    assert!(md.contains("say \"hi\""), "双写引号应还原：{md}");
}

#[test]
fn tsv_quoted_cell_with_tab() {
    let input = "a\tb\n\"x\ty\"\tc";
    let md = table_markdown_from_clipboard(Some(input.to_string()), None).unwrap();
    let lines: Vec<&str> = md.lines().collect();
    assert_eq!(lines.len(), 3, "引号内制表符不应拆列：{md}");
    assert!(md.contains("x\ty"), "引号内制表符应保留：{md}");
}

#[test]
fn tsv_single_line_without_tab_is_none() {
    assert_eq!(table_markdown_from_clipboard(Some("hello".to_string()), None), None);
}

#[test]
fn tsv_empty_input_is_none() {
    assert_eq!(table_markdown_from_clipboard(Some(String::new()), None), None);
    assert_eq!(table_markdown_from_clipboard(None, None), None);
}

#[test]
fn tsv_multi_line_without_tab_is_none() {
    assert_eq!(
        table_markdown_from_clipboard(Some("foo\nbar\nbaz".to_string()), None),
        None
    );
}

// ---------- table_markdown_from_clipboard：HTML 路径 ----------

#[test]
fn html_table_converted() {
    let html = "<table><tr><th>Name</th><th>Age</th></tr><tr><td>Alice</td><td>30</td></tr></table>";
    let md = table_markdown_from_clipboard(None, Some(html.to_string())).unwrap();
    assert_eq!(md, "| Name | Age |\n| --- | --- |\n| Alice | 30 |");
}

#[test]
fn html_table_strips_nested_tags_and_decodes_entities() {
    let html = "<TABLE><TR><TD><b>Tom</b> &amp; <i>Jerry</i></TD><TD>&lt;tag&gt; &quot;q&quot; &#39;x&#39; &nbsp;y</TD></TR><TR><TD>1</TD><TD>2</TD></TR></TABLE>";
    let md = table_markdown_from_clipboard(None, Some(html.to_string())).unwrap();
    assert!(md.contains("Tom & Jerry"), "嵌套标签应剥离、实体解码：{md}");
    assert!(md.contains("<tag> \"q\" 'x'  y"), "实体应解码：{md}");
}

#[test]
fn html_without_table_falls_back_to_text() {
    let md = table_markdown_from_clipboard(
        Some("a\tb\n1\t2".to_string()),
        Some("<p>not a table</p>".to_string()),
    )
    .unwrap();
    assert!(md.starts_with("| a | b |"), "应走 TSV 路径：{md}");
}

#[test]
fn html_table_without_valid_rows_falls_back_to_text() {
    let md = table_markdown_from_clipboard(
        Some("a\tb\n1\t2".to_string()),
        Some("<table></table>".to_string()),
    )
    .unwrap();
    assert!(md.starts_with("| a | b |"), "空 table 应落回 text 路径：{md}");
}

#[test]
fn html_preferred_over_text_when_table_present() {
    let md = table_markdown_from_clipboard(
        Some("x\ty\n1\t2".to_string()),
        Some("<table><tr><td>H1</td><td>H2</td></tr><tr><td>a</td><td>b</td></tr></table>".to_string()),
    )
    .unwrap();
    assert!(md.starts_with("| H1 | H2 |"), "HTML 应优先：{md}");
}

// ---------- 超限 ----------

#[test]
fn oversized_input_is_none() {
    let big = "a\tb\n".repeat(2_000_000); // 8M 字符
    assert_eq!(table_markdown_from_clipboard(Some(big), None), None);
}

#[test]
fn oversized_cell_count_is_none() {
    // 每个单元格 1 字符：101 列 × 101 行 > 10_000 单元格
    let row: String = std::iter::once("x".to_string())
        .chain(std::iter::repeat("\tx".to_string()).take(100))
        .collect();
    let big = format!("{row}\n").repeat(101);
    assert!(big.len() < 5_000_000);
    assert_eq!(table_markdown_from_clipboard(Some(big), None), None);
}

// ---------- table_matrix_from_text ----------

#[test]
fn matrix_single_line_single_column_is_none() {
    assert_eq!(table_matrix_from_text("hello".to_string()), None);
}

#[test]
fn matrix_multi_row_returns_2d() {
    let matrix = table_matrix_from_text("a\tb\n1\t2\n3\t4".to_string()).unwrap();
    assert_eq!(
        matrix,
        vec![
            vec!["a".to_string(), "b".to_string()],
            vec!["1".to_string(), "2".to_string()],
            vec!["3".to_string(), "4".to_string()],
        ]
    );
}

#[test]
fn matrix_quoted_multiline_cell() {
    let matrix = table_matrix_from_text("a\tb\n\"x\ny\"\tz".to_string()).unwrap();
    assert_eq!(matrix[1][0], "x\ny");
}

// ---------- table_to_csv ----------

#[test]
fn csv_basic() {
    let table = TableData {
        header: plain_cells(&["Name", "Age"]),
        rows: vec![plain_cells(&["Alice", "30"])],
        alignments: vec![TableColumnAlignment::Default; 2],
    };
    assert_eq!(table_to_csv(table), "Name,Age\r\nAlice,30\r\n");
}

#[test]
fn csv_quotes_comma_quote_newline() {
    let table = TableData {
        header: plain_cells(&["a,b", "c"]),
        rows: vec![plain_cells(&["say \"hi\"", "x\ny"])],
        alignments: vec![TableColumnAlignment::Default; 2],
    };
    assert_eq!(
        table_to_csv(table),
        "\"a,b\",c\r\n\"say \"\"hi\"\"\",\"x\ny\"\r\n"
    );
}

#[test]
fn csv_chinese_passthrough() {
    let table = TableData {
        header: plain_cells(&["姓名", "备注"]),
        rows: vec![plain_cells(&["张三", "你好，世界"])],
        alignments: vec![TableColumnAlignment::Default; 2],
    };
    // 全角逗号不是 RFC4180 分隔符，无需加引号
    assert_eq!(table_to_csv(table), "姓名,备注\r\n张三,你好，世界\r\n");
}
