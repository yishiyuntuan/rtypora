//! Mermaid 围栏识别：移植自 velotype `components/mermaid/mod.rs` 的纯解析半。
//! Mermaid → SVG 渲染管线不移植，前端后续用 mermaid.js 渲染。

/// 围栏代码块 info string 是否声明了 Mermaid 内容。
pub fn is_mermaid_info_string(info: Option<&str>) -> bool {
    info.and_then(|info| info.split_whitespace().next())
        .is_some_and(|first| {
            first.eq_ignore_ascii_case("mermaid") || first.eq_ignore_ascii_case("mmd")
        })
}
