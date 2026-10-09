//! 引用式链接与自动链接辅助。
//! 移植自 velotype `components/markdown/link.rs`。

use std::collections::HashMap;

use super::image::normalize_reference_label;
// 引用定义扫描骨架（围栏跟踪/HTML 块抑制/容器前缀剥离/转义感知查找）集中在 refscan，
// 与 image.rs 共用单一来源
use super::refscan::{
    HtmlBlockStart, find_open_title_quote, find_unescaped_char, is_escaped,
    is_reference_definition_title_continuation, is_reference_scan_closing_fence,
    parse_reference_scan_html_block_start, parse_reference_scan_html_close_tag_name,
    parse_reference_scan_opening_fence, strip_reference_scan_container_prefixes,
    unescape_ascii_punctuation,
};


/// 引用式链接的全局引用定义（`[label]: url "title"`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkReferenceDefinition {
    pub(crate) destination: String,
    #[allow(dead_code)]
    pub(crate) title: Option<String>,
}

pub type LinkReferenceDefinitions = HashMap<String, LinkReferenceDefinition>;

pub(crate) fn parse_link_reference_definitions(markdown: &str) -> LinkReferenceDefinitions {
    let lines = markdown.split('\n').collect::<Vec<_>>();
    let normalized_lines = lines
        .iter()
        .map(|line| strip_reference_scan_container_prefixes(line).to_string())
        .collect::<Vec<_>>();
    let normalized_refs = normalized_lines
        .iter()
        .map(|line| line.as_str())
        .collect::<Vec<_>>();
    let mut definitions = LinkReferenceDefinitions::new();
    let mut index = 0usize;
    let mut active_fence = None;
    let mut active_html_tag: Option<String> = None;
    let mut active_html_comment = false;

    while index < lines.len() {
        let line = normalized_refs[index];

        if let Some(fence) = active_fence {
            if is_reference_scan_closing_fence(line, fence) {
                active_fence = None;
            }
            index += 1;
            continue;
        }

        if active_html_comment {
            if line.contains("-->") || line.trim().is_empty() {
                active_html_comment = false;
            }
            index += 1;
            continue;
        }

        if let Some(tag_name) = active_html_tag.clone() {
            if line.trim().is_empty()
                || parse_reference_scan_html_close_tag_name(line).as_deref() == Some(&tag_name)
            {
                active_html_tag = None;
            }
            index += 1;
            continue;
        }

        if let Some(fence) = parse_reference_scan_opening_fence(line) {
            if !is_reference_scan_closing_fence(line, fence) {
                active_fence = Some(fence);
            }
            index += 1;
            continue;
        }

        if let Some(html_start) = parse_reference_scan_html_block_start(line) {
            match html_start {
                HtmlBlockStart::Comment => {
                    if !line.contains("-->") {
                        active_html_comment = true;
                    }
                }
                HtmlBlockStart::Tag {
                    name,
                    self_closing,
                    closes_same_line,
                } => {
                    if !self_closing && !closes_same_line {
                        active_html_tag = Some(name);
                    }
                }
            }
            index += 1;
            continue;
        }

        let Some((label, definition, consumed)) =
            parse_link_reference_definition(&normalized_refs, index)
        else {
            index += 1;
            continue;
        };

        definitions.entry(label).or_insert(definition);
        index += consumed;
    }

    definitions
}

pub(crate) fn is_supported_autolink_target(target: &str) -> bool {
    if target
        .strip_prefix("mailto:")
        .is_some_and(|address| !address.is_empty() && address.contains('@'))
    {
        return true;
    }

    // 纯数据层不依赖 http 客户端，仅按前缀判定 http/https scheme。
    ["http://", "https://"]
        .iter()
        .any(|prefix| target.starts_with(prefix) && target.len() > prefix.len())
}

fn parse_link_reference_definition(
    lines: &[&str],
    start: usize,
) -> Option<(String, LinkReferenceDefinition, usize)> {
    let line = lines.get(start)?;
    let trimmed_end = line.trim_end();
    let leading_spaces = trimmed_end.bytes().take_while(|b| *b == b' ').count();
    if leading_spaces > 3 {
        return None;
    }

    let rest = &trimmed_end[leading_spaces..];
    if !rest.starts_with('[') {
        return None;
    }

    let label_end = find_unescaped_char(rest, 1, b']')?;
    if rest.as_bytes().get(label_end + 1) != Some(&b':') {
        return None;
    }

    let label = normalize_reference_label(&rest[1..label_end])?;
    let mut target = rest[label_end + 2..].trim_start().to_string();
    let mut consumed = 1usize;

    if let Some(next_line) = lines.get(start + 1) {
        if is_reference_definition_title_continuation(next_line) {
            if !target.is_empty() {
                target.push(' ');
            }
            target.push_str(next_line.trim());
            consumed += 1;
        }
    }

    let (destination, title) = parse_link_target(&target)?;
    Some((
        label,
        LinkReferenceDefinition { destination, title },
        consumed,
    ))
}


pub(crate) fn parse_link_target(inner: &str) -> Option<(String, Option<String>)> {
    if inner.is_empty() {
        return None;
    }

    if inner.ends_with('"') {
        let close_quote = inner.len() - 1;
        if !is_escaped(inner, close_quote) {
            if let Some(open_quote) = find_open_title_quote(inner, close_quote) {
                let destination = inner[..open_quote.saturating_sub(1)].trim_end();
                let title = inner[open_quote + 1..close_quote].to_string();
                if destination.is_empty() {
                    return None;
                }
                return Some((normalize_link_destination(destination), Some(title)));
            }
        }
    }

    Some((normalize_link_destination(inner), None))
}

fn normalize_link_destination(destination: &str) -> String {
    let destination = unescape_ascii_punctuation(destination);
    if destination.starts_with('<')
        && destination.ends_with('>')
        && is_supported_autolink_target(&destination[1..destination.len() - 1])
    {
        destination[1..destination.len() - 1].to_string()
    } else {
        destination
    }
}


#[cfg(test)]
mod tests {
    use super::{
        LinkReferenceDefinition, is_supported_autolink_target, parse_link_reference_definitions,
    };

    #[test]
    fn parses_link_reference_definitions_with_title_and_first_wins() {
        let definitions = parse_link_reference_definitions(
            "[Ref Link]: https://first.example \"Caption\"\n[ref link]: https://second.example",
        );
        assert_eq!(
            definitions.get("ref link"),
            Some(&LinkReferenceDefinition {
                destination: "https://first.example".to_string(),
                title: Some("Caption".to_string()),
            })
        );
    }

    #[test]
    fn parses_container_scoped_link_reference_definitions_and_skips_raw_blocks() {
        let definitions = parse_link_reference_definitions(
            [
                "> [quoted ref]: https://quoted.example \"Quoted\"",
                "- [list ref]: https://list.example",
                "1) [ordered ref]: https://ordered.example",
                "> ```md",
                "> [code ref]: https://ignored-code.example",
                "> ```",
                "",
                "<div>",
                "[html ref]: https://ignored-html.example",
                "</div>",
            ]
            .join("\n")
            .as_str(),
        );

        assert_eq!(
            definitions.get("quoted ref"),
            Some(&LinkReferenceDefinition {
                destination: "https://quoted.example".to_string(),
                title: Some("Quoted".to_string()),
            })
        );
        assert_eq!(
            definitions.get("list ref"),
            Some(&LinkReferenceDefinition {
                destination: "https://list.example".to_string(),
                title: None,
            })
        );
        assert_eq!(
            definitions.get("ordered ref"),
            Some(&LinkReferenceDefinition {
                destination: "https://ordered.example".to_string(),
                title: None,
            })
        );
        assert!(!definitions.contains_key("code ref"));
        assert!(!definitions.contains_key("html ref"));
    }

    #[test]
    fn supports_http_https_and_mailto_autolinks() {
        assert!(is_supported_autolink_target("https://example.com"));
        assert!(is_supported_autolink_target("http://example.com"));
        assert!(is_supported_autolink_target("mailto:test@example.com"));
        assert!(!is_supported_autolink_target("./relative/path"));
        assert!(!is_supported_autolink_target("span>x</span"));
    }
}
