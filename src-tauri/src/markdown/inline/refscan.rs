//! 引用定义扫描共享骨架：链接（link.rs）与图片（image.rs）的「引用定义扫描」
//! 使用同一套逐行扫描辅助——容器前缀剥离（引用/列表标记）、围栏代码块跟踪
//! （围栏内的 `[label]: url` 不识别）、HTML 块抑制（HTML 块内不扫描）、
//! 标题续行判定与转义感知查找。两侧曾各持一份完全相同的拷贝，此处收敛为单一来源。
//!
//! 移植自 velotype `components/markdown/`（link.rs 与 image.rs 的公共部分）。

/// 扫描引用定义时正处于的围栏代码块（围栏内的定义行不识别）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FenceInfo {
    pub(crate) ch: char,
    pub(crate) len: usize,
}

/// 抑制引用定义扫描的 HTML 块起始。
pub(crate) enum HtmlBlockStart {
    /// 以 `<!--` 开始的 HTML 注释。
    Comment,
    /// HTML 标签块（闭合行为取决于标签形态）。
    Tag {
        name: String,
        self_closing: bool,
        closes_same_line: bool,
    },
}

/// 剥离行首的容器前缀（引用 `>` 与列表标记可多层叠加），返回内容部分。
pub(crate) fn strip_reference_scan_container_prefixes(mut line: &str) -> &str {
    loop {
        let original = line;
        if let Some(rest) = strip_reference_scan_quote_prefix(line) {
            line = rest;
            continue;
        }
        if let Some(rest) = strip_reference_scan_list_marker(line) {
            line = rest;
            continue;
        }
        if line == original {
            return line;
        }
    }
}

/// 剥离引用前缀（≤3 个前导空格 + `>` + 可选一个空格）。
fn strip_reference_scan_quote_prefix(line: &str) -> Option<&str> {
    let leading_spaces = line.bytes().take_while(|b| *b == b' ').count();
    if leading_spaces > 3 {
        return None;
    }

    let rest = &line[leading_spaces..];
    if !rest.starts_with('>') {
        return None;
    }

    Some(rest[1..].strip_prefix(' ').unwrap_or(&rest[1..]))
}

/// 剥离列表标记前缀（无序 `-`/`+`/`*` 或有序 `1.`/`1)`，分隔符为空格或 tab）。
fn strip_reference_scan_list_marker(line: &str) -> Option<&str> {
    let indent_bytes = line
        .chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .map(char::len_utf8)
        .sum::<usize>();
    let rest = &line[indent_bytes..];

    if let Some(marker @ ('-' | '*' | '+')) = rest.chars().next() {
        let after_marker = &rest[marker.len_utf8()..];
        return after_marker
            .strip_prefix(' ')
            .or_else(|| after_marker.strip_prefix('\t'));
    }

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

    Some(&rest[digit_len + 2..])
}

/// 解析起始围栏行（``` 或 ~~~，可带信息串；反引号围栏的信息串不得再含反引号）。
pub(crate) fn parse_reference_scan_opening_fence(line: &str) -> Option<FenceInfo> {
    let trimmed = line.trim_end();
    let ch = trimmed.chars().next()?;
    if !matches!(ch, '`' | '~') {
        return None;
    }
    let len = trimmed.chars().take_while(|current| *current == ch).count();
    let rest = &trimmed[ch.len_utf8() * len..];
    if ch == '`' && rest.contains('`') {
        return None;
    }
    (len >= 3).then_some(FenceInfo { ch, len })
}

/// 判断是否为匹配 opener 的闭合围栏行（同字符、长度 ≥ 起始、其后仅空白）。
pub(crate) fn is_reference_scan_closing_fence(line: &str, opener: FenceInfo) -> bool {
    let trimmed = line.trim_end();
    if !trimmed.starts_with(opener.ch) {
        return false;
    }

    let run_len = trimmed
        .chars()
        .take_while(|current| *current == opener.ch)
        .count();
    run_len >= opener.len && trimmed[opener.ch.len_utf8() * run_len..].trim().is_empty()
}

/// 识别 HTML 块起始行（注释 `<!--` 或标签块）；用于抑制块内的引用定义扫描。
pub(crate) fn parse_reference_scan_html_block_start(line: &str) -> Option<HtmlBlockStart> {
    let rest = line.trim_start().trim_end();
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
    let suffix = &tagged[name_len..];
    let next = suffix.chars().next()?;
    if !matches!(next, '>' | ' ' | '\t' | '/') {
        return None;
    }

    Some(HtmlBlockStart::Tag {
        name: name.to_string(),
        self_closing: rest.ends_with("/>"),
        closes_same_line: rest.contains(&format!("</{name}>")),
    })
}

/// 解析 HTML 闭合标签行（`</name>`）的标签名。
pub(crate) fn parse_reference_scan_html_close_tag_name(line: &str) -> Option<String> {
    let rest = line.trim_start().trim_end();
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

/// 判断该行是否为引用定义的标题续行（缩进后的 "..."、'...' 或 (...) 行）。
pub(crate) fn is_reference_definition_title_continuation(line: &str) -> bool {
    let indent_bytes = line
        .chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .map(char::len_utf8)
        .sum::<usize>();
    if indent_bytes == 0 {
        return false;
    }

    let trimmed = line[indent_bytes..].trim();
    (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        || (trimmed.starts_with('(') && trimmed.ends_with(')'))
}

/// 在 `close_quote` 之前反向寻找标题开引号（前一个字符须为空白、未被转义）。
pub(crate) fn find_open_title_quote(input: &str, close_quote: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    (0..close_quote).rev().find(|&index| {
        bytes[index] == b'"'
            && !is_escaped(input, index)
            && index > 0
            && bytes[index - 1].is_ascii_whitespace()
    })
}

/// 从 `start` 起找未被转义的 `target` 字节。
pub(crate) fn find_unescaped_char(input: &str, start: usize, target: u8) -> Option<usize> {
    let bytes = input.as_bytes();
    (start..bytes.len()).find(|&index| bytes[index] == target && !is_escaped(input, index))
}

/// ASCII 标点反转义（`\*` → `*` 等；其余反斜杠保留）。
pub(crate) fn unescape_ascii_punctuation(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.peek().is_some_and(|next| next.is_ascii_punctuation()) {
            output.push(chars.next().expect("peeked punctuation must exist"));
        } else {
            output.push(ch);
        }
    }
    output
}

/// `index` 处的字符是否被反斜杠转义（向前数连续反斜杠，奇数个即被转义）。
pub(crate) fn is_escaped(input: &str, index: usize) -> bool {
    if index == 0 {
        return false;
    }

    let bytes = input.as_bytes();
    let mut backslashes = 0usize;
    let mut cursor = index;
    while cursor > 0 {
        cursor -= 1;
        if bytes[cursor] == b'\\' {
            backslashes += 1;
        } else {
            break;
        }
    }
    backslashes % 2 == 1
}
