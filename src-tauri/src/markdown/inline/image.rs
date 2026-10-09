//! 独立图片语法、引用与源解析。
//! 移植自 velotype `components/markdown/image.rs`（GPUI 图片加载侧已剥离，
//! 保留语法解析与本地路径/远程 URL 分类）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

// 引用定义扫描骨架（围栏跟踪/HTML 块抑制/容器前缀剥离/转义感知查找）集中在 refscan，
// 与 link.rs 共用单一来源
use super::refscan::{
    HtmlBlockStart, find_open_title_quote, find_unescaped_char, is_escaped,
    is_reference_definition_title_continuation, is_reference_scan_closing_fence,
    parse_reference_scan_html_block_start, parse_reference_scan_html_close_tag_name,
    parse_reference_scan_opening_fence, strip_reference_scan_container_prefixes,
    unescape_ascii_punctuation,
};


/// 解析出的独立图片表达式（`![alt](target)` / `![alt][label]`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageSyntax {
    pub alt: String,
    pub target: ImageTarget,
}

/// 行内图片/文本片段（仅原生表格单元格渲染使用）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TableCellInlineImageSegment {
    Text(String),
    Image {
        markdown: String,
        syntax: ImageSyntax,
    },
}

/// 引用解析前的图片目标形式。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageTarget {
    /// 直接图片目标：`![alt](src "title")`。
    Direct { src: String, title: Option<String> },
    /// 引用图片目标：`![alt][label]`。
    Reference { label: String },
}

/// 引用式图片标签的全局引用定义。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImageReferenceDefinition {
    pub(crate) src: String,
    pub(crate) title: Option<String>,
}

pub(crate) type ImageReferenceDefinitions = HashMap<String, ImageReferenceDefinition>;

/// 已解析的图片目标，可用于路径或 URL 加载。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedImageTarget {
    pub(crate) src: String,
    pub(crate) title: Option<String>,
}

/// 本地路径 / 远程 URL 分类后的具体图片源。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ImageResolvedSource {
    /// 文件系统路径（尽可能相对于当前文档解析）。
    Local(PathBuf),
    /// HTTP(S) image URL，由上层（前端）负责加载。
    Remote(String),
}

impl ImageSyntax {
    pub(crate) fn resolve_target(
        &self,
        reference_definitions: &ImageReferenceDefinitions,
    ) -> Option<ResolvedImageTarget> {
        match &self.target {
            ImageTarget::Direct { src, title } => Some(ResolvedImageTarget {
                src: src.clone(),
                title: title.clone(),
            }),
            ImageTarget::Reference { label } => {
                let definition = reference_definitions.get(label)?;
                Some(ResolvedImageTarget {
                    src: definition.src.clone(),
                    title: definition.title.clone(),
                })
            }
        }
    }
}

/// 判断图片源是否为远程 URL（http:// 或 https:// 前缀）。
pub(crate) fn is_remote_image_source(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://")
}

pub(crate) fn resolve_image_source(source: &str, base_dir: Option<&Path>) -> ImageResolvedSource {
    if is_remote_image_source(source) {
        return ImageResolvedSource::Remote(source.to_string());
    }

    let path = Path::new(source);
    if path.is_absolute() {
        return ImageResolvedSource::Local(path.to_path_buf());
    }

    let resolved = base_dir
        .map(|dir| dir.join(path))
        .unwrap_or_else(|| path.to_path_buf());
    ImageResolvedSource::Local(resolved)
}

pub(crate) fn parse_standalone_image(markdown: &str) -> Option<ImageSyntax> {
    if markdown.contains('\n') || markdown.contains('\r') {
        return None;
    }
    let markdown = markdown.trim();
    if markdown.is_empty() {
        return None;
    }
    if !markdown.starts_with("![") {
        return None;
    }

    let bytes = markdown.as_bytes();
    let mut alt_end = None;
    for index in 2..bytes.len() {
        if bytes[index] == b']' && !is_escaped(markdown, index) {
            alt_end = Some(index);
            break;
        }
    }
    let alt_end = alt_end?;

    let alt = unescape_ascii_punctuation(&markdown[2..alt_end]);
    match bytes.get(alt_end + 1) {
        Some(b'(') if markdown.ends_with(')') => {
            let inner = &markdown[alt_end + 2..markdown.len() - 1];
            let (src, title) = parse_image_target(inner)?;
            Some(ImageSyntax {
                alt,
                target: ImageTarget::Direct { src, title },
            })
        }
        Some(b'[') if markdown.ends_with(']') => {
            let raw_label = &markdown[alt_end + 2..markdown.len() - 1];
            let label_source = if raw_label.is_empty() {
                alt.as_str()
            } else {
                raw_label
            };
            let label = normalize_reference_label(label_source)?;
            Some(ImageSyntax {
                alt,
                target: ImageTarget::Reference { label },
            })
        }
        None => {
            let label = normalize_reference_label(&alt)?;
            Some(ImageSyntax {
                alt,
                target: ImageTarget::Reference { label },
            })
        }
        _ => None,
    }
}

pub(crate) fn parse_table_cell_inline_images(markdown: &str) -> Vec<TableCellInlineImageSegment> {
    let mut segments = Vec::new();
    let mut text_start = 0usize;
    let mut cursor = 0usize;
    let mut found_image = false;

    while cursor < markdown.len() {
        let parsed_image = if markdown[cursor..].starts_with("![") && !is_escaped(markdown, cursor)
        {
            parse_inline_image_at(markdown, cursor)
        } else {
            None
        };
        if let Some((image_markdown, syntax, end)) = parsed_image {
            if is_link_wrapped_inline_image(markdown, cursor, end) {
                cursor += markdown[cursor..]
                    .chars()
                    .next()
                    .map(char::len_utf8)
                    .unwrap_or(1);
                continue;
            }

            if text_start < cursor {
                segments.push(TableCellInlineImageSegment::Text(
                    markdown[text_start..cursor].to_string(),
                ));
            }
            segments.push(TableCellInlineImageSegment::Image {
                markdown: image_markdown,
                syntax,
            });
            found_image = true;
            cursor = end;
            text_start = cursor;
            continue;
        }

        cursor += markdown[cursor..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(1);
    }

    if text_start < markdown.len() {
        segments.push(TableCellInlineImageSegment::Text(
            markdown[text_start..].to_string(),
        ));
    }

    if found_image {
        segments
    } else {
        vec![TableCellInlineImageSegment::Text(markdown.to_string())]
    }
}

fn parse_inline_image_at(markdown: &str, start: usize) -> Option<(String, ImageSyntax, usize)> {
    if !markdown[start..].starts_with("![") {
        return None;
    }

    let alt_end = find_unescaped_char(markdown, start + 2, b']')?;
    let alt = unescape_ascii_punctuation(&markdown[start + 2..alt_end]);
    let next = markdown.as_bytes().get(alt_end + 1).copied();

    match next {
        Some(b'(') => {
            let close = find_unescaped_char(markdown, alt_end + 2, b')')?;
            let inner = &markdown[alt_end + 2..close];
            let (src, title) = parse_image_target(inner)?;
            let end = close + 1;
            Some((
                markdown[start..end].to_string(),
                ImageSyntax {
                    alt,
                    target: ImageTarget::Direct { src, title },
                },
                end,
            ))
        }
        Some(b'[') => {
            let close = find_unescaped_char(markdown, alt_end + 2, b']')?;
            let raw_label = &markdown[alt_end + 2..close];
            let label_source = if raw_label.is_empty() {
                alt.as_str()
            } else {
                raw_label
            };
            let label = normalize_reference_label(label_source)?;
            let end = close + 1;
            Some((
                markdown[start..end].to_string(),
                ImageSyntax {
                    alt,
                    target: ImageTarget::Reference { label },
                },
                end,
            ))
        }
        _ => {
            let label = normalize_reference_label(&alt)?;
            let end = alt_end + 1;
            Some((
                markdown[start..end].to_string(),
                ImageSyntax {
                    alt,
                    target: ImageTarget::Reference { label },
                },
                end,
            ))
        }
    }
}

fn is_link_wrapped_inline_image(markdown: &str, start: usize, end: usize) -> bool {
    let mut cursor = 0usize;
    let mut open_label = None;
    while cursor < start {
        let byte = markdown.as_bytes()[cursor];
        if byte == b'[' && !is_escaped(markdown, cursor) {
            open_label = Some(cursor);
        } else if byte == b']' && !is_escaped(markdown, cursor) {
            open_label = None;
        }
        cursor += markdown[cursor..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(1);
    }

    open_label.is_some() && markdown[end..].starts_with("](")
}

pub(crate) fn parse_image_reference_definitions(markdown: &str) -> ImageReferenceDefinitions {
    let lines = markdown.split('\n').collect::<Vec<_>>();
    let normalized_lines = lines
        .iter()
        .map(|line| strip_reference_scan_container_prefixes(line).to_string())
        .collect::<Vec<_>>();
    let normalized_refs = normalized_lines
        .iter()
        .map(|line| line.as_str())
        .collect::<Vec<_>>();
    let mut definitions = ImageReferenceDefinitions::new();
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
            parse_image_reference_definition(&normalized_refs, index)
        else {
            index += 1;
            continue;
        };

        definitions.entry(label).or_insert(definition);
        index += consumed;
    }

    definitions
}

pub(crate) fn normalize_reference_label(label: &str) -> Option<String> {
    // 单趟拼接：只遍历单词一次，除首个外带前导分隔符推入输出，
    // 避免 split_whitespace().collect::<Vec<_>>() 的中间 Vec<&str> 分配
    //（随后 .join("") 还会再拷贝一次）。
    let mut normalized = String::with_capacity(label.len());
    for word in label.split_whitespace() {
        if !normalized.is_empty() {
            normalized.push(' ');
        }
        normalized.push_str(word);
    }
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_lowercase())
    }
}

fn parse_image_reference_definition(
    lines: &[&str],
    start: usize,
) -> Option<(String, ImageReferenceDefinition, usize)> {
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

    let (src, title) = parse_image_target(&target)?;
    Some((label, ImageReferenceDefinition { src, title }, consumed))
}


fn parse_image_target(inner: &str) -> Option<(String, Option<String>)> {
    if inner.is_empty() {
        return None;
    }

    if inner.ends_with('"') {
        let close_quote = inner.len() - 1;
        if !is_escaped(inner, close_quote) {
            if let Some(open_quote) = find_open_title_quote(inner, close_quote) {
                let src = inner[..open_quote.saturating_sub(1)].trim_end();
                let title = inner[open_quote + 1..close_quote].to_string();
                if src.is_empty() {
                    return None;
                }
                return Some((normalize_image_source(src), Some(title)));
            }
        }
    }

    Some((normalize_image_source(inner), None))
}


fn normalize_image_source(source: &str) -> String {
    let source = unescape_ascii_punctuation(source);
    if source.starts_with('<') && source.ends_with('>') {
        let inner = &source[1..source.len() - 1];
        // 代替原 Uri 校验：非空且不含空白/尖括号即视为合法 URI。
        if !inner.is_empty()
            && !inner
                .chars()
                .any(|ch| ch.is_whitespace() || matches!(ch, '<' | '>'))
        {
            return inner.to_string();
        }
    }
    source
}


#[cfg(test)]
mod tests {
    use super::{
        ImageReferenceDefinition, ImageResolvedSource, ImageSyntax, ImageTarget,
        TableCellInlineImageSegment, normalize_reference_label, parse_image_reference_definitions,
        parse_standalone_image, parse_table_cell_inline_images, resolve_image_source,
    };
    use std::path::Path;

    #[test]
    fn parses_standalone_image_without_title() {
        let parsed = parse_standalone_image("![alt](./img.png)").expect("image syntax");
        assert_eq!(parsed.alt, "alt");
        assert_eq!(
            parsed.target,
            ImageTarget::Direct {
                src: "./img.png".to_string(),
                title: None,
            }
        );
    }

    #[test]
    fn parses_standalone_image_with_surrounding_whitespace() {
        let three_space =
            parse_standalone_image("   ![alt](https://example.com/a.png)").expect("image syntax");
        assert_eq!(three_space.alt, "alt");
        assert_eq!(
            three_space.target,
            ImageTarget::Direct {
                src: "https://example.com/a.png".to_string(),
                title: None,
            }
        );

        let deeply_indented =
            parse_standalone_image("        ![alt](https://example.com/a.png)   ")
                .expect("image syntax");
        assert_eq!(deeply_indented, three_space);
        assert!(parse_standalone_image("   text ![alt](x)").is_none());
        assert!(parse_standalone_image("   ![alt](x)\n").is_none());
    }

    #[test]
    fn parses_image_target_with_escaped_punctuation_in_source() {
        let parsed = parse_standalone_image("![alt](https://example.com/typera\\_picgo/img.png)")
            .expect("image syntax");
        assert_eq!(
            parsed.target,
            ImageTarget::Direct {
                src: "https://example.com/typera_picgo/img.png".to_string(),
                title: None,
            }
        );
    }

    #[test]
    fn parses_standalone_image_with_underscores_in_alt_and_source() {
        let parsed = parse_standalone_image(
            "![1.1_进制转换例子](./NetworkEngineerSummer.assets/1.1_进制转换例子.jpg)",
        )
        .expect("image syntax");

        assert_eq!(parsed.alt, "1.1_进制转换例子");
        assert_eq!(
            parsed.target,
            ImageTarget::Direct {
                src: "./NetworkEngineerSummer.assets/1.1_进制转换例子.jpg".to_string(),
                title: None,
            }
        );
    }

    #[test]
    fn parses_standalone_image_with_title() {
        let parsed =
            parse_standalone_image("![alt](./img.png \"caption text\")").expect("image syntax");
        assert_eq!(parsed.alt, "alt");
        assert_eq!(
            parsed.target,
            ImageTarget::Direct {
                src: "./img.png".to_string(),
                title: Some("caption text".to_string()),
            }
        );
    }

    #[test]
    fn parses_reference_style_standalone_image() {
        let parsed =
            parse_standalone_image("![reference image][ref-image]").expect("reference image");
        assert_eq!(parsed.alt, "reference image");
        assert_eq!(
            parsed.target,
            ImageTarget::Reference {
                label: "ref-image".to_string(),
            }
        );
    }

    #[test]
    fn parses_collapsed_reference_style_standalone_image() {
        let parsed =
            parse_standalone_image("![collapsed image][]").expect("collapsed reference image");
        assert_eq!(parsed.alt, "collapsed image");
        assert_eq!(
            parsed.target,
            ImageTarget::Reference {
                label: "collapsed image".to_string(),
            }
        );
    }

    #[test]
    fn parses_shortcut_reference_style_standalone_image() {
        let parsed = parse_standalone_image("![shortcut image]").expect("shortcut reference image");
        assert_eq!(parsed.alt, "shortcut image");
        assert_eq!(
            parsed.target,
            ImageTarget::Reference {
                label: "shortcut image".to_string(),
            }
        );
    }

    #[test]
    fn rejects_mixed_or_wrapped_image_syntax() {
        assert!(parse_standalone_image("text ![alt](./img.png)").is_none());
        assert!(parse_standalone_image("[![alt](./img.png)](https://example.com)").is_none());
        assert!(parse_standalone_image("![][]").is_none());
        assert!(parse_standalone_image("![]").is_none());
    }

    #[test]
    fn parses_table_cell_inline_image_segments() {
        let segments = parse_table_cell_inline_images("image ![alt](https://example.com/x.png)");
        assert_eq!(
            segments,
            vec![
                TableCellInlineImageSegment::Text("image ".to_string()),
                TableCellInlineImageSegment::Image {
                    markdown: "![alt](https://example.com/x.png)".to_string(),
                    syntax: ImageSyntax {
                        alt: "alt".to_string(),
                        target: ImageTarget::Direct {
                            src: "https://example.com/x.png".to_string(),
                            title: None,
                        },
                    },
                },
            ]
        );
    }

    #[test]
    fn parses_multiple_table_cell_inline_images() {
        let segments = parse_table_cell_inline_images("![a](x.png) and ![b](y.png)");
        assert_eq!(segments.len(), 3);
        assert!(matches!(
            &segments[0],
            TableCellInlineImageSegment::Image { syntax, .. } if syntax.alt == "a"
        ));
        assert_eq!(
            segments[1],
            TableCellInlineImageSegment::Text(" and ".to_string())
        );
        assert!(matches!(
            &segments[2],
            TableCellInlineImageSegment::Image { syntax, .. } if syntax.alt == "b"
        ));
    }

    #[test]
    fn table_cell_inline_image_segments_keep_escaped_wrapped_and_broken_text() {
        assert_eq!(
            parse_table_cell_inline_images(r"\![alt](x.png)"),
            vec![TableCellInlineImageSegment::Text(
                r"\![alt](x.png)".to_string()
            )]
        );
        assert_eq!(
            parse_table_cell_inline_images("[![alt](x.png)](https://example.com)"),
            vec![TableCellInlineImageSegment::Text(
                "[![alt](x.png)](https://example.com)".to_string()
            )]
        );
        assert_eq!(
            parse_table_cell_inline_images("broken ![alt](x.png"),
            vec![TableCellInlineImageSegment::Text(
                "broken ![alt](x.png".to_string()
            )]
        );
    }

    #[test]
    fn table_cell_inline_reference_images_resolve() {
        let definitions = parse_image_reference_definitions(
            "[ref]: ./ref.png\n[collapsed]: ./collapsed.png\n[shortcut]: ./shortcut.png",
        );
        let segments = parse_table_cell_inline_images("![full][ref] ![collapsed][] ![shortcut]");
        let resolved = segments
            .iter()
            .filter_map(|segment| match segment {
                TableCellInlineImageSegment::Image { syntax, .. } => {
                    syntax.resolve_target(&definitions).map(|target| target.src)
                }
                TableCellInlineImageSegment::Text(_) => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(
            resolved,
            vec!["./ref.png", "./collapsed.png", "./shortcut.png"]
        );
    }

    #[test]
    fn parses_image_reference_definitions_with_title_and_first_wins() {
        let definitions = parse_image_reference_definitions(
            "[Ref Image]: ./first.png \"Caption\"\n[ref image]: ./second.png".trim(),
        );
        assert_eq!(
            definitions.get("ref image"),
            Some(&ImageReferenceDefinition {
                src: "./first.png".to_string(),
                title: Some("Caption".to_string()),
            })
        );
    }

    #[test]
    fn normalizes_reference_labels_case_and_whitespace_insensitively() {
        assert_eq!(
            normalize_reference_label("  Ref\t Image  "),
            Some("ref image".to_string())
        );
    }

    #[test]
    fn resolves_reference_targets() {
        let syntax = ImageSyntax {
            alt: "alt".to_string(),
            target: ImageTarget::Reference {
                label: "ref-image".to_string(),
            },
        };
        let definitions = parse_image_reference_definitions("[ref-image]: ./img.png \"Caption\"");
        let resolved = syntax
            .resolve_target(&definitions)
            .expect("resolved target");
        assert_eq!(resolved.src, "./img.png");
        assert_eq!(resolved.title.as_deref(), Some("Caption"));
    }

    #[test]
    fn resolves_collapsed_and_shortcut_reference_images() {
        let definitions = parse_image_reference_definitions(
            "[collapsed image]: ./collapsed.png\n[shortcut image]: ./shortcut.png",
        );

        let collapsed = parse_standalone_image("![collapsed image][]")
            .expect("collapsed reference image")
            .resolve_target(&definitions)
            .expect("resolved collapsed image");
        assert_eq!(collapsed.src, "./collapsed.png");

        let shortcut = parse_standalone_image("![shortcut image]")
            .expect("shortcut reference image")
            .resolve_target(&definitions)
            .expect("resolved shortcut image");
        assert_eq!(shortcut.src, "./shortcut.png");
    }

    #[test]
    fn unresolved_reference_target_returns_none() {
        let syntax = ImageSyntax {
            alt: "alt".to_string(),
            target: ImageTarget::Reference {
                label: "missing".to_string(),
            },
        };
        assert!(
            syntax
                .resolve_target(&parse_image_reference_definitions("[ref]: ./img.png"))
                .is_none()
        );
    }

    #[test]
    fn resolves_relative_and_remote_sources() {
        let local = resolve_image_source("images/pic.png", Some(Path::new("D:/docs")));
        assert_eq!(
            local,
            ImageResolvedSource::Local(Path::new("D:/docs").join("images/pic.png"))
        );

        let remote = resolve_image_source("https://example.com/img.gif", None);
        match remote {
            ImageResolvedSource::Remote(uri) => {
                assert_eq!(uri.to_string(), "https://example.com/img.gif");
            }
            other => panic!("expected remote source, got {other:?}"),
        }
    }

    #[test]
    fn parses_container_scoped_reference_definitions_in_source_order() {
        let definitions = parse_image_reference_definitions(
            [
                "> [quoted ref]: ./quoted.png \"Quoted\"",
                "- [list ref]: ./list.png",
                "1) [ordered ref]: ./ordered.png",
                "> > [quoted ref]: ./ignored.png",
            ]
            .join("\n")
            .as_str(),
        );

        assert_eq!(
            definitions.get("quoted ref"),
            Some(&ImageReferenceDefinition {
                src: "./quoted.png".to_string(),
                title: Some("Quoted".to_string()),
            })
        );
        assert_eq!(
            definitions.get("list ref"),
            Some(&ImageReferenceDefinition {
                src: "./list.png".to_string(),
                title: None,
            })
        );
        assert_eq!(
            definitions.get("ordered ref"),
            Some(&ImageReferenceDefinition {
                src: "./ordered.png".to_string(),
                title: None,
            })
        );
    }

    #[test]
    fn ignores_reference_definitions_inside_code_fences_and_html_blocks() {
        let definitions = parse_image_reference_definitions(
            [
                "> ```md",
                "> [code ref]: ./ignored-code.png",
                "> ```",
                "",
                "<div>",
                "[html ref]: ./ignored-html.png",
                "</div>",
                "",
                "> [live ref]: ./real.png",
            ]
            .join("\n")
            .as_str(),
        );

        assert!(!definitions.contains_key("code ref"));
        assert!(!definitions.contains_key("html ref"));
        assert_eq!(
            definitions.get("live ref"),
            Some(&ImageReferenceDefinition {
                src: "./real.png".to_string(),
                title: None,
            })
        );
    }
}
