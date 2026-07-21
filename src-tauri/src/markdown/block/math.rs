/// 展示数学（`$$...$$`）源码解析：移植自 velotype `components/latex/mod.rs` 的纯解析半。
/// LaTeX → SVG 渲染管线不移植，前端后续用 KaTeX 渲染 `body`。

use serde::{Deserialize, Serialize};

/// 从 Markdown 保留的展示数学源码。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayMathSource {
    /// 完整 Markdown 源码，含 `$$` 定界符。
    pub raw: String,
    /// 定界符之间的 LaTeX 正文。
    pub body: String,
}

/// 围栏代码块 info string 是否声明数学公式（`math`/`latex`，Typora 风格）。
pub fn is_math_info_string(info: Option<&str>) -> bool {
    info.and_then(|info| info.split_whitespace().next())
        .is_some_and(|first| first.eq_ignore_ascii_case("math") || first.eq_ignore_ascii_case("latex"))
}

/// 剥离行首不超过 3 列的空格缩进；超过则不是合法展示数学行。
fn strip_display_indent(line: &str) -> Option<&str> {
    let indent = line.bytes().take_while(|byte| *byte == b' ').count();
    (indent <= 3).then_some(&line[indent..])
}

/// 把原始 `$$...$$` 或 `\[...\]` Markdown 块解析为其中的 LaTeX 正文。
pub fn parse_display_math_source(raw: &str) -> Option<DisplayMathSource> {
    let raw = raw.trim_matches('\n').to_string();
    let lines = raw.split('\n').collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }

    if lines.len() == 1 {
        let line = strip_display_indent(lines[0])?.trim_end();
        if let Some(body_and_close) = line.strip_prefix("$$") {
            let close = body_and_close.find("$$")?;
            // 单行仅允许一对 $$：闭包后仍有内容则不视为展示公式块，
            // 交由调用方回退（如 `$$ 文本 $$ $$ 公式 $$` 同行多段，避免吞掉后段）。
            if !body_and_close[close + 2..].trim().is_empty() {
                return None;
            }
            let body = body_and_close[..close].trim().to_string();
            return Some(DisplayMathSource { raw, body });
        }
        if let Some(body_and_close) = line.strip_prefix("\\[") {
            // `\[ body \]` 单行形式（同样要求闭包后无内容）
            let close = body_and_close.find("\\]")?;
            if !body_and_close[close + 2..].trim().is_empty() {
                return None;
            }
            let body = body_and_close[..close].trim().to_string();
            if body.is_empty() {
                return None;
            }
            return Some(DisplayMathSource { raw, body });
        }
        return None;
    }

    let opener = strip_display_indent(lines[0])?.trim_end();
    let closer = lines.last()?.trim();
    if (opener == "$$" && closer == "$$") || (opener == "\\[" && closer == "\\]") {
        let body = lines[1..lines.len() - 1].join("\n");
        return Some(DisplayMathSource { raw, body });
    }
    None
}
