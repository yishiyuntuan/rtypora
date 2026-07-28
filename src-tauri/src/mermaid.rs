//! Mermaid 图表渲染：移植自 velotype `components/mermaid/mod.rs` 的渲染管线。
//! 经 mermaid-rs-renderer 在 Rust 端把图表源码渲染为 SVG；webview 端按 CSS 尺寸显示，
//! 因此 velotype 的磁盘缓存与按宽度生成缩放副本两步在此简化为进程内缓存。

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{LazyLock, Mutex};

static MERMAID_CACHE: LazyLock<Mutex<HashMap<u64, Option<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 渲染 Mermaid 源码为 SVG 文本；接受 ```mermaid 围栏原文或图表正文（围栏在 Rust 端剥离）。
/// 不支持的图类型或语法错误返回 None（前端回退源码展示）。
#[tauri::command]
pub fn render_mermaid(source: &str) -> Option<String> {
    let body = strip_code_fence(source);
    let key = mermaid_cache_key(&body);
    if let Some(cached) = MERMAID_CACHE.lock().ok()?.get(&key) {
        return cached.clone();
    }
    let rendered = render_mermaid_raw(&body);
    // 进程内缓存上限（防无界增长）：满 500 条清空重建
    let mut cache = MERMAID_CACHE.lock().ok()?;
    if cache.len() >= 500 {
        cache.clear();
    }
    cache.insert(key, rendered.clone());
    rendered
}

/// 剥离 ```mermaid 开围栏与闭合围栏；无围栏时原样返回（去首尾空白）
fn strip_code_fence(source: &str) -> String {
    let mut lines: Vec<&str> = source.lines().collect();
    if lines.first().is_some_and(|line| line.trim_start().starts_with("```")) {
        lines.remove(0);
        if lines.last().is_some_and(|line| line.trim().starts_with("```")) {
            lines.pop();
        }
    }
    lines.join("\n").trim().to_string()
}

fn mermaid_cache_key(source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

/// 与 velotype 一致：先校验图类型，渲染后检查错误标记。
fn render_mermaid_raw(source: &str) -> Option<String> {
    if !looks_like_supported_mermaid_source(source) {
        return None;
    }
    let svg = mermaid_rs_renderer::render(source).ok()?;
    if svg.contains("class=\"error-text\"") || svg.contains("Syntax error in text") {
        return None;
    }
    Some(svg)
}

/// 首个有效行（跳过空行、frontmatter、`%%` 注释）须以已知图类型关键字开头。
fn looks_like_supported_mermaid_source(source: &str) -> bool {
    let mut in_frontmatter = false;
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "---" {
            in_frontmatter = !in_frontmatter;
            continue;
        }
        if in_frontmatter || trimmed.starts_with("%%") {
            continue;
        }

        let lower = trimmed.to_ascii_lowercase();
        return [
            "sequencediagram",
            "classdiagram",
            "statediagram",
            "erdiagram",
            "pie",
            "mindmap",
            "journey",
            "timeline",
            "gantt",
            "requirementdiagram",
            "gitgraph",
            "c4",
            "sankey",
            "quadrantchart",
            "zenuml",
            "block",
            "packet",
            "kanban",
            "architecture",
            "radar",
            "treemap",
            "xychart",
            "flowchart",
            "graph",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix));
    }
    false
}
