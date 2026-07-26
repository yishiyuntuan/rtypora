//! Markdown 模块：移植自 velotype 的块模型、行内引擎、解析器与序列化器。
//! Rust 端无状态，仅提供「全文 → 块树」的解析命令，文档内容由前端持有。

pub mod block;
pub mod inline;
pub mod model;
pub mod table;

/// 每行的 UTF-16 码元起始偏移（`split('\n')` 与解析器一致的行切分）。
fn line_utf16_starts(markdown: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut offset = 0;
    for line in markdown.split('\n') {
        starts.push(offset);
        offset += line.encode_utf16().count() + 1;
    }
    starts
}

/// 解析入口共用：根块行区间 → 带 UTF-16 偏移的 DTO（偏移相对传入文本起点）。
fn parse_to_dtos(markdown: &str) -> Vec<model::BlockDto> {
    let roots = block::document::parse_root_blocks(markdown);
    let starts = line_utf16_starts(markdown);
    let total = markdown.encode_utf16().count();
    roots
        .iter()
        .map(|root| {
            let start = starts[root.start_line];
            // end_line 指向块后一行：减 1 去掉其前的换行；末行无换行时取全文长度。
            let end = if root.end_line < starts.len() {
                starts[root.end_line] - 1
            } else {
                total
            };
            model::BlockDto::from_node(&root.node, Some((start, end)))
        })
        .collect()
}

/// 解析 Markdown 全文，返回块树（JSON）。
/// 根块带 `start`/`end`（UTF-16 码元偏移，不含尾随换行），前端可据此切片做逐块编辑。
#[tauri::command]
pub fn parse_markdown(markdown: &str) -> Vec<model::BlockDto> {
    parse_to_dtos(markdown)
}

/// HTML 标签转 Markdown 开关（偏好设置 html_to_md 驱动）：
/// 开启后 h1-h6/p/div/center 容器标签按原生块解析（内联样式标签由行内引擎始终映射）。
#[tauri::command]
pub fn set_html_to_md(enabled: bool) {
    HTML_TO_MD.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

static HTML_TO_MD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// 是否启用容器 HTML 标签 → Markdown 块转换（document.rs 的 HTML 块处理读取）
pub(crate) fn html_to_md_enabled() -> bool {
    HTML_TO_MD.load(std::sync::atomic::Ordering::Relaxed)
}

/// 警告框语法统一转换开关（偏好设置 callout_unify 驱动）：
/// 开启后 Obsidian 别名标记（[!hint] 等 + 折叠后缀）与 :::type / !!! type
/// 容器语法按 GitHub 五变体解析，保存时落源为标准 [!TYPE] 引用格式；
/// 关闭则这些扩展语法不识别（按普通引用/文本保留原文）。
#[tauri::command]
pub fn set_callout_unify(enabled: bool) {
    CALLOUT_UNIFY.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

static CALLOUT_UNIFY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// 是否启用警告框扩展语法统一转换（document.rs 的警告框解析读取）
pub(crate) fn callout_unify_enabled() -> bool {
    CALLOUT_UNIFY.load(std::sync::atomic::Ordering::Relaxed)
}

/// 批量「序列化对齐」最长公共前缀（UTF-16 码元数，与 JS `String.length` 一致）：
/// 整块 DTO 序列化一次，再对每组光标前片段 DTO 求其序列化与整块的公共前缀长度。
/// 用于源码/WYSIWYG 切换的光标精确映射——一次调用代替前端逐节点往返。
#[tauri::command]
pub fn lcp_offsets(
    full_blocks: Vec<model::BlockDto>,
    before_parts: Vec<Vec<model::BlockDto>>,
) -> Vec<usize> {
    let full = serialize_markdown(full_blocks);
    before_parts
        .into_iter()
        .map(|part| lcp_utf16(&full, &serialize_markdown(part)))
        .collect()
}

fn lcp_utf16(a: &str, b: &str) -> usize {
    a.encode_utf16()
        .zip(b.encode_utf16())
        .take_while(|(x, y)| x == y)
        .count()
}

/// 斜杠命令的块模板（返回结构见 BlockTemplate）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockTemplate {
    /// 起始 Markdown 文本
    pub markdown: String,
    /// 建议光标位置（UTF-16 码元偏移）
    pub caret_offset: usize,
}

/// 斜杠命令的块模板：生成所选语法的起始 Markdown 与光标位置。
/// 表格经 `TableData` 序列化（与表格解析/往返同一规则，无平行实现）。
/// 警告框（callout）经 `variant` 指定类型（NOTE/TIP/IMPORTANT/WARNING/CAUTION），
/// 类型清单复用 `CalloutVariant` 解析（单一来源），未知类型回落 NOTE。
#[tauri::command]
pub fn block_template(
    kind: &str,
    rows: Option<usize>,
    cols: Option<usize>,
    variant: Option<String>,
) -> BlockTemplate {
    let (markdown, caret_offset) = match kind {
        "table" => {
            let mut table = table::TableData::new_empty(rows.unwrap_or(2), cols.unwrap_or(2));
            for (index, cell) in table.header.iter_mut().enumerate() {
                *cell = inline::tree::InlineTextTree::plain(format!("列{}", index + 1));
            }
            let markdown = table::serialize_table_markdown_lines(&table).join("\n");
            (markdown, 2)
        }
        "mathBlock" => ("$$\n\n$$".to_string(), 3),
        "mermaidBlock" => ("```mermaid\n\n```".to_string(), 11),
        "callout" => {
            let callout = variant
                .as_deref()
                .and_then(|v| {
                    block::state::CalloutVariant::parse_header_line(&format!("[!{v}]"))
                })
                .map(|(v, _)| v)
                .unwrap_or(block::state::CalloutVariant::Note);
            let markdown = format!("> [!{}]\n> ", callout.marker());
            let caret = markdown.len();
            (markdown, caret)
        }
        "sectionBlock" => ("<section>\n\n</section>".to_string(), "<section>\n".len()),
        "image" => ("![]()".to_string(), 3),
        // 链接：光标落在 URL 括号内（文字占位「链接」），与图片模板同一交互
        "link" => ("[链接]()".to_string(), 4),
        // 行内公式：两个 $ 之间输入（单行模板，Enter 整块提交渲染）
        "inlineMath" => ("$$".to_string(), 1),
        // 脚注定义与链接引用定义（右键菜单「插入」）
        "footnoteDef" => ("[^1]: ".to_string(), "[^1]: ".len()),
        "linkRef" => ("[1]: url \"title\"".to_string(), 5),
        // 内容目录（[TOC] 段落，渲染为文档大纲）与 YAML Front Matter（文档头插入）
        "toc" => ("[TOC]".to_string(), 5),
        "yamlFrontMatter" => ("---\ntitle: \n---\n".to_string(), 11),
        _ => (String::new(), 0),
    };
    BlockTemplate {
        markdown,
        caret_offset,
    }
}

/// 块首退格合并：把当前段落的序列化文本并入上一块源码（块类型沿用上块），
/// 接缝不加换行/空格（行内合并规则在 Rust 统一维护）。
#[tauri::command]
pub fn merge_block_markdown(prev_source: &str, appended_markdown: &str) -> String {
    format!("{}{}", prev_source, appended_markdown.trim())
}

/// 解析 Markdown 片段（单个或少数块的源码），返回块树（JSON）。
/// 偏移相对片段起点；前端在编辑提交后只重解析受影响的区域做增量更新，
/// 避免整棵树重解析（未变化的块 id 保持稳定，前端局部重渲染）。
#[tauri::command]
pub fn parse_blocks(markdown: &str) -> Vec<model::BlockDto> {
    parse_to_dtos(markdown)
}

/// 把块树序列化为规范 Markdown（用于往返测试与后续保存规范化）。
#[tauri::command]
pub fn serialize_markdown(blocks: Vec<model::BlockDto>) -> String {
    let nodes = blocks
        .into_iter()
        .map(model::BlockDto::into_node)
        .collect::<Vec<_>>();
    block::tree::serialize_blocks(&nodes)
}

/// 格式化表格源码：解析单个表格块并按列宽对齐管道（Typora「格式化表格源码」）。
/// 内容与对齐不变，仅重排空白；输入不是合法表格时返回 None（前端保持原样）。
#[tauri::command]
pub fn format_table_source(markdown: &str) -> Option<String> {
    let lines = markdown.lines().map(str::to_string).collect::<Vec<_>>();
    let table = table::parse_root_table_region(&lines)?;
    Some(table::serialize_table_markdown_lines_padded(&table).join("\n"))
}

/// 任务列表勾选：把源码中第 occurrence 个（0 基，默认首个）任务标记替换为勾选状态。
/// occurrence 为任务项在块树 DFS 前序中的序号（嵌套任务项经根块源码定位）；
/// 同一轮取三种写法中位置最早者，兼容 `[ ]`/`[x]`/`[X]` 混排。
/// Markdown 文本的增删改一律在 Rust 端完成，前端只做切片拼接。
#[tauri::command]
pub fn toggle_task_markdown(source: &str, checked: bool, occurrence: Option<usize>) -> String {
    let marker = if checked { "[x]" } else { "[ ]" };
    let target = occurrence.unwrap_or(0);
    let mut rest = source;
    let mut offset = 0usize;
    for seen in 0..=target {
        // 本轮最早出现的任务标记位置（三种写法取最小）
        let pos = ["[ ]", "[x]", "[X]"]
            .iter()
            .filter_map(|pat| rest.find(pat))
            .min();
        let Some(pos) = pos else { break };
        if seen == target {
            let abs = offset + pos;
            return format!("{}{}{}", &source[..abs], marker, &source[abs + 3..]);
        }
        offset += pos + 3;
        rest = &rest[pos + 3..];
    }
    source.to_string()
}

/// 全文统计：行数/词数/字符数（字符数为 UTF-16 码元数，与 JS `String.length` 一致）。
/// 词数统计与 velotype 一致：每个 CJK 字符计 1 词，拉丁词按空白分隔。
#[tauri::command]
pub fn text_stats(markdown: &str) -> TextStats {
    TextStats {
        words: count_words(markdown),
        chars: markdown.encode_utf16().count(),
        lines: count_visual_lines(markdown),
    }
}

/// 视觉行数统计：段落分隔空行不计（WYSIWYG 中段落间空行不占行），
/// 代码块围栏内的空行照常计，文档结尾的空段落（Enter 新建的空块）计一行。
/// 与编辑器中看到的内容行一致（一次回车 = +1 行）。
fn count_visual_lines(markdown: &str) -> usize {
    if markdown.is_empty() {
        return 1;
    }
    let mut count = 0;
    let mut in_fence = false;
    for line in markdown.split('\n') {
        let trimmed = line.trim_end();
        // 围栏开闭行本身计一行（粗略判定，统计用途足够）
        let t = trimmed.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            count += 1;
            continue;
        }
        if !trimmed.is_empty() || in_fence {
            count += 1;
        }
    }
    // 结尾的空段落（源码以空行结束，WYSIWYG 中光标所在的空块占一行）；
    // 围栏内尾部的空行已在循环中计过，不重复加
    if markdown.ends_with("\n\n") && !in_fence {
        count += 1;
    }
    count
}

/// text_stats 的返回结构。
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStats {
    pub words: usize,
    pub chars: usize,
    pub lines: usize,
}

/// CJK 词数统计（移植自 velotype `editor/status_bar.rs`）。
fn count_words(text: &str) -> usize {
    let mut count = 0;
    let mut in_latin_word = false;

    for ch in text.chars() {
        if is_cjk_char(ch) {
            if in_latin_word {
                count += 1;
                in_latin_word = false;
            }
            count += 1;
        } else if ch.is_whitespace() {
            if in_latin_word {
                count += 1;
                in_latin_word = false;
            }
        } else {
            in_latin_word = true;
        }
    }
    if in_latin_word {
        count += 1;
    }
    count
}

fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        // CJK Unified Ideographs
        0x4E00..=0x9FFF
        // CJK Unified Ideographs Extension A
        | 0x3400..=0x4DBF
        // CJK Unified Ideographs Extension B
        | 0x20000..=0x2A6DF
        // CJK Compatibility Ideographs
        | 0xF900..=0xFAFF
        // CJK Radicals Supplement / Kangxi Radicals
        | 0x2E80..=0x2EFF
        // CJK Symbols and Punctuation / Halfwidth & Fullwidth Forms
        | 0x3000..=0x303F
        | 0xFF00..=0xFFEF
        // Hiragana / Katakana
        | 0x3040..=0x30FF
    )
}

/// 块级 Markdown 快捷输入检测（`# `、`- `、`1. `、`> `、`- [ ] `、``` fence、`---`、`<section>`），
/// 返回目标块类型与标记前缀长度；前端据此做 DOM 结构转换。
#[tauri::command]
pub fn detect_block_shortcut(line: &str) -> Option<ShortcutHit> {
    // `<section>` 图文排版容器（仅开标签行；前端回车补全闭合标签进入原文编辑）
    if line.trim().eq_ignore_ascii_case("<section>") {
        return Some(ShortcutHit {
            kind: model::BlockKindDto::SectionBlock,
            prefix_len: line.encode_utf16().count(),
        });
    }
    // 围栏代码块（``` 或 ~~~，可带语言标记）
    if let Some(fence) = block::document::parse_opening_fence(line) {
        return Some(ShortcutHit {
            kind: model::BlockKindDto::CodeBlock {
                language: fence.language.map(|lang| lang.to_string()),
            },
            prefix_len: line.encode_utf16().count(),
        });
    }
    // 分割线（---/___/***）
    if block::state::BlockKind::parse_separator_line(line) {
        return Some(ShortcutHit {
            kind: model::BlockKindDto::Separator,
            prefix_len: line.encode_utf16().count(),
        });
    }
    let (kind, prefix_len) = block::state::BlockKind::detect_markdown_shortcut(line)?;
    Some(ShortcutHit {
        kind: kind.into(),
        prefix_len,
    })
}

/// detect_block_shortcut 的命中结构。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutHit {
    #[serde(flatten)]
    pub kind: model::BlockKindDto,
    pub prefix_len: usize,
}

/// 行内 Markdown 快捷输入检测：光标前文本以完整行内结构结尾时返回命中
/// （图片/链接/粗体/斜体/删除线/行内代码），前端据此把命中区间替换为样式元素。
/// `match_len` 为 UTF-16 码元长度，与 JS `String.length` 一致。
#[tauri::command]
pub fn inline_shortcut(text: &str) -> Option<InlineShortcutHit> {
    scan_bracket_link(text, true)
        .or_else(|| scan_bracket_link(text, false))
        .or_else(|| scan_wrapped(text, "**", "bold"))
        .or_else(|| scan_wrapped(text, "~~", "strikethrough"))
        .or_else(|| scan_wrapped(text, "`", "code"))
        .or_else(|| scan_italic(text))
}

/// 输入 `>` 完成 HTML 开始标签时的自动闭合：返回应插入的闭合标签文本
/// （已知标签白名单；void/自闭合/未知标签不触发）。
#[tauri::command]
pub fn inline_html_autoclose(text: &str) -> Option<String> {
    inline::html::inline_html_autoclose(text)
}

/// Enter 展开判定：光标位于 `<name ...>` 与 `</name>` 之间且标签为块级容器时
/// 返回标签名（前端据此把闭标签拆到下一行，光标落中间行）。
#[tauri::command]
pub fn html_container_tag_between(before: &str, after: &str) -> Option<String> {
    inline::html::html_container_tag_between(before, after)
}

/// Enter 跳过判定：光标位于 `<name ...>` 与 `</name>` 之间（开闭同名）时返回
/// 闭标签文本（前端据此把光标移到闭标签之后）。
#[tauri::command]
pub fn html_closing_tag_at(before: &str, after: &str) -> Option<String> {
    inline::html::html_closing_tag_at(before, after)
}

/// 字体颜色面板的色值解析：接受 CSS 颜色（名字/#hex/rgb()/rgba()/hsl()/hsla()/
/// currentColor/transparent），另兼容裸 RGB 三元组（`207,34,46` → `rgb(207,34,46)`）。
/// 返回 HtmlCssColor（serde JSON），前端直接作为 htmlStyle.color 使用。
#[tauri::command]
pub fn parse_html_color(text: &str) -> Option<inline::html::HtmlCssColor> {
    let text = text.trim();
    inline::html::parse_css_color(text).or_else(|| {
        text.contains(',')
            .then(|| inline::html::parse_css_color(&format!("rgb({text})")))?
    })
}

/// inline_shortcut 的命中结构。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineShortcutHit {
    pub kind: String,
    /// 光标前需要替换掉的 Markdown 源码长度（UTF-16 码元）。
    pub match_len: usize,
    /// 结构内的可见文本（链接/图片为标签，样式为正文）。
    pub text: String,
    /// 链接/图片目标地址。
    pub dest: Option<String>,
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// 扫描 `[label](dest)$` / `![alt](dest)$` 结尾结构。
fn scan_bracket_link(text: &str, image: bool) -> Option<InlineShortcutHit> {
    let close = text.strip_suffix(')')?;
    let open_paren = close.rfind("](")?;
    let dest = &close[open_paren + 2..];
    if dest.is_empty() || dest.contains(|ch: char| ch.is_whitespace() || ch == ')') {
        return None;
    }
    let before_paren = &close[..open_paren];
    let open_bracket = before_paren.rfind('[')?;
    let label = &before_paren[open_bracket + 1..];
    let mut prefix = &before_paren[..open_bracket];
    // 链接标签非空；图片 alt 可空，且 `[` 前必须有 `!`
    if !image && label.is_empty() {
        return None;
    }
    if image {
        prefix = prefix.strip_suffix('!')?;
    }
    if label.contains(['[', ']']) {
        return None;
    }
    let matched = &text[prefix.len()..];
    Some(InlineShortcutHit {
        kind: if image { "image" } else { "link" }.to_string(),
        match_len: utf16_len(matched),
        text: label.to_string(),
        dest: Some(dest.to_string()),
    })
}

/// 扫描 `marker...marker$` 包裹结构（粗体 `**`、删除线 `~~`、行内代码 `` ` ``）。
fn scan_wrapped(text: &str, marker: &str, kind: &str) -> Option<InlineShortcutHit> {
    let body = text.strip_suffix(marker)?;
    let start = body.rfind(marker)?;
    let inner = &body[start + marker.len()..];
    let marker_char = marker.chars().next()?;
    if inner.is_empty() || inner.contains(marker_char) {
        return None;
    }
    let matched = &text[start..];
    Some(InlineShortcutHit {
        kind: kind.to_string(),
        match_len: utf16_len(matched),
        text: inner.to_string(),
        dest: None,
    })
}

/// 扫描斜体 `*...*$`（单侧 `*`，不与其他 `*` 相邻）。
fn scan_italic(text: &str) -> Option<InlineShortcutHit> {
    let body = text.strip_suffix('*')?;
    if body.ends_with('*') {
        return None;
    }
    // 从后往前找孤立的 `*`（左右两侧都不是 `*`）
    for (idx, ch) in body.char_indices().rev() {
        if ch != '*' {
            continue;
        }
        let left_is_star = idx > 0 && body[..idx].ends_with('*');
        let right_is_star = body[idx + 1..].starts_with('*');
        if left_is_star || right_is_star {
            continue;
        }
        let inner = &body[idx + 1..];
        if inner.is_empty() || inner.contains('*') {
            return None;
        }
        let matched = &text[idx..];
        return Some(InlineShortcutHit {
            kind: "italic".to_string(),
            match_len: utf16_len(matched),
            text: inner.to_string(),
            dest: None,
        });
    }
    None
}
