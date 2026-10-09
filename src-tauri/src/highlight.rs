//! 代码块语法高亮：移植自 velotype `code_highlight.rs`。
//!
//! 基于 tree-sitter（20 种官方语法 + yaml/toml 补充语法），`highlight_code` 命令
//! 返回扁平 span（UTF-16 码元区间 + 11 个规范类名，与主题 token `code_syntax_*`
//! 后缀一一对应）；语言未知/纯文本/失败时返回空数组，前端按纯文本渲染。

use std::collections::HashMap;
use std::ops::Range;
use std::sync::LazyLock;

use tree_sitter_highlight::{Highlight, HighlightConfiguration, HighlightEvent, Highlighter};


/// 语法高亮注册表使用的规范语言键（与前端语言徽章/补全清单一一对应）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CodeLanguageKey {
    /// Rust 源代码。
    Rust,
    /// JavaScript（不含 JSX）。
    JavaScript,
    /// JavaScript（含 JSX 语法）。
    JavaScriptJsx,
    /// TypeScript（不含 TSX）。
    TypeScript,
    /// TypeScript（含 TSX 语法）。
    TypeScriptTsx,
    /// JSON 数据。
    Json,
    /// Markdown 源码。
    Markdown,
    /// POSIX 风格 shell 脚本。
    Bash,
    /// C 源代码。
    C,
    /// C++ 源代码。
    Cpp,
    /// C# 源代码。
    CSharp,
    /// CSS 样式表。
    Css,
    /// Go 源代码。
    Go,
    /// HTML 标记。
    Html,
    /// Java 源代码。
    Java,
    /// PHP 源代码。
    Php,
    /// Python 源代码。
    Python,
    /// Ruby 源代码。
    Ruby,
    /// YAML 配置。
    Yaml,
    /// TOML 配置。
    Toml,
    /// Mermaid 图表源码。
    Mermaid,
    /// 纯文本或未知语言的回落。
    PlainText,
}

/// 映射到主题颜色的语义高亮类（serde camelCase，前端拼接为 `ts-*` 类名上色）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CodeHighlightClass {
    /// 注释文本。
    Comment,
    /// 关键字或保留字。
    Keyword,
    /// 字符串字面量。
    String,
    /// 数字字面量。
    Number,
    /// 类型名。
    Type,
    /// 函数或可调用标识符。
    Function,
    /// 常量标识符。
    Constant,
    /// 变量标识符。
    Variable,
    /// 对象或记录属性。
    Property,
    /// 运算符。
    Operator,
    /// 标点符号。
    Punctuation,
}

/// 代码块内的高亮字节区间（tree-sitter 原始输出为字节偏移，命令层换算为 UTF-16）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeHighlightSpan {
    pub(crate) range: Range<usize>,
    pub(crate) class: CodeHighlightClass,
}

/// 代码块高亮结果（语言键 + 扁平 span 列表，span 按起点排序且互不重叠）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeHighlightResult {
    pub(crate) language: CodeLanguageKey,
    pub(crate) spans: Vec<CodeHighlightSpan>,
}

/// 围栏代码块信息串可接受的语言别名（如 `rs`/`rust`、`ts`/`typescript`）。
#[derive(Clone, Copy)]
struct LanguageDescriptor {
    key: CodeLanguageKey,
    aliases: &'static [&'static str],
}

const LANGUAGE_DESCRIPTORS: &[LanguageDescriptor] = &[
    LanguageDescriptor {
        key: CodeLanguageKey::Rust,
        aliases: &["rust", "rs"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::JavaScript,
        aliases: &["javascript", "js"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::JavaScriptJsx,
        aliases: &["jsx"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::TypeScript,
        aliases: &["typescript", "ts"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::TypeScriptTsx,
        aliases: &["tsx"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Json,
        aliases: &["json"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Markdown,
        aliases: &["markdown", "md"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Bash,
        aliases: &["bash", "sh", "shell", "zsh"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::C,
        aliases: &["c", "h"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Cpp,
        aliases: &["cpp", "cxx", "cc", "hpp", "hxx"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::CSharp,
        aliases: &["csharp", "cs", "c#"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Css,
        aliases: &["css"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Go,
        aliases: &["go", "golang"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Html,
        aliases: &["html"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Java,
        aliases: &["java"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Php,
        aliases: &["php"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Python,
        aliases: &["python", "py"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Ruby,
        aliases: &["ruby", "rb"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Yaml,
        aliases: &["yaml", "yml"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Toml,
        aliases: &["toml"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::PlainText,
        aliases: &["text", "txt", "plain"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Mermaid,
        aliases: &["mermaid"],
    },
];

const HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "comment",
    "constant",
    "constant.builtin",
    "constructor",
    "embedded",
    "function",
    "function.builtin",
    "keyword",
    "module",
    "number",
    "operator",
    "property",
    "property.builtin",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.special",
    "string",
    "string.special",
    "tag",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.parameter",
];

/// 惰性构建的 tree-sitter 高亮注册表（首次使用时编译全部语法与查询）。
struct CodeHighlightRegistry {
    configs: HashMap<CodeLanguageKey, HighlightConfiguration>,
}

static CODE_HIGHLIGHT_REGISTRY: LazyLock<CodeHighlightRegistry> =
    LazyLock::new(CodeHighlightRegistry::new);

impl CodeHighlightRegistry {
    fn new() -> Self {
        // 数据驱动注册：（语言键, 配置构建器）表 + 循环，替代逐条手写调用
        let builders: [(CodeLanguageKey, fn() -> Option<HighlightConfiguration>); 20] = [
            (CodeLanguageKey::Rust, build_rust_config),
            (CodeLanguageKey::JavaScript, build_javascript_config),
            (CodeLanguageKey::JavaScriptJsx, build_jsx_config),
            (CodeLanguageKey::TypeScript, build_typescript_config),
            (CodeLanguageKey::TypeScriptTsx, build_tsx_config),
            (CodeLanguageKey::Json, build_json_config),
            (CodeLanguageKey::Markdown, build_markdown_config),
            (CodeLanguageKey::Bash, build_bash_config),
            (CodeLanguageKey::C, build_c_config),
            (CodeLanguageKey::Cpp, build_cpp_config),
            (CodeLanguageKey::CSharp, build_csharp_config),
            (CodeLanguageKey::Css, build_css_config),
            (CodeLanguageKey::Go, build_go_config),
            (CodeLanguageKey::Html, build_html_config),
            (CodeLanguageKey::Java, build_java_config),
            (CodeLanguageKey::Php, build_php_config),
            (CodeLanguageKey::Python, build_python_config),
            (CodeLanguageKey::Ruby, build_ruby_config),
            (CodeLanguageKey::Yaml, build_yaml_config),
            (CodeLanguageKey::Toml, build_toml_config),
        ];
        let mut configs = HashMap::new();
        for (key, build) in builders {
            maybe_insert_config(&mut configs, key, build());
        }
        Self { configs }
    }

    fn config_for(&self, key: CodeLanguageKey) -> Option<&HighlightConfiguration> {
        self.configs.get(&key)
    }
}

fn maybe_insert_config(
    configs: &mut HashMap<CodeLanguageKey, HighlightConfiguration>,
    key: CodeLanguageKey,
    config: Option<HighlightConfiguration>,
) {
    if let Some(config) = config {
        configs.insert(key, config);
    }
}

fn configure_highlights(
    language: tree_sitter::Language,
    name: &'static str,
    highlights_query: &str,
    injections_query: &str,
    locals_query: &str,
) -> Option<HighlightConfiguration> {
    let mut config = HighlightConfiguration::new(
        language,
        name,
        highlights_query,
        injections_query,
        locals_query,
    )
    .ok()?;
    config.configure(HIGHLIGHT_NAMES);
    Some(config)
}

fn build_rust_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_rust::LANGUAGE.into(),
        "rust",
        tree_sitter_rust::HIGHLIGHTS_QUERY,
        tree_sitter_rust::INJECTIONS_QUERY,
        "",
    )
}

fn build_javascript_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_javascript::LANGUAGE.into(),
        "javascript",
        tree_sitter_javascript::HIGHLIGHT_QUERY,
        tree_sitter_javascript::INJECTIONS_QUERY,
        tree_sitter_javascript::LOCALS_QUERY,
    )
}

fn build_jsx_config() -> Option<HighlightConfiguration> {
    let query = format!(
        "{}\n{}",
        tree_sitter_javascript::HIGHLIGHT_QUERY,
        tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
    );
    configure_highlights(
        tree_sitter_javascript::LANGUAGE.into(),
        "javascript",
        &query,
        tree_sitter_javascript::INJECTIONS_QUERY,
        tree_sitter_javascript::LOCALS_QUERY,
    )
}

fn build_typescript_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "typescript",
        tree_sitter_typescript::HIGHLIGHTS_QUERY,
        "",
        tree_sitter_typescript::LOCALS_QUERY,
    )
}

fn build_tsx_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_typescript::LANGUAGE_TSX.into(),
        "tsx",
        tree_sitter_typescript::HIGHLIGHTS_QUERY,
        "",
        tree_sitter_typescript::LOCALS_QUERY,
    )
}

fn build_json_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_json::LANGUAGE.into(),
        "json",
        tree_sitter_json::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

fn build_markdown_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_md::LANGUAGE.into(),
        "markdown",
        tree_sitter_md::HIGHLIGHT_QUERY_BLOCK,
        tree_sitter_md::INJECTION_QUERY_BLOCK,
        "",
    )
}

fn build_bash_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_bash::LANGUAGE.into(),
        "bash",
        tree_sitter_bash::HIGHLIGHT_QUERY,
        "",
        "",
    )
}

fn build_c_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_c::LANGUAGE.into(),
        "c",
        tree_sitter_c::HIGHLIGHT_QUERY,
        "",
        "",
    )
}

fn build_cpp_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_cpp::LANGUAGE.into(),
        "cpp",
        tree_sitter_cpp::HIGHLIGHT_QUERY,
        "",
        "",
    )
}

fn build_csharp_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_c_sharp::LANGUAGE.into(),
        "c_sharp",
        tree_sitter_c_sharp::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

fn build_css_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_css::LANGUAGE.into(),
        "css",
        tree_sitter_css::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

fn build_go_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_go::LANGUAGE.into(),
        "go",
        tree_sitter_go::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

fn build_html_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_html::LANGUAGE.into(),
        "html",
        tree_sitter_html::HIGHLIGHTS_QUERY,
        tree_sitter_html::INJECTIONS_QUERY,
        "",
    )
}

fn build_java_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_java::LANGUAGE.into(),
        "java",
        tree_sitter_java::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

fn build_php_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_php::LANGUAGE_PHP.into(),
        "php",
        tree_sitter_php::HIGHLIGHTS_QUERY,
        tree_sitter_php::INJECTIONS_QUERY,
        "",
    )
}

fn build_python_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_python::LANGUAGE.into(),
        "python",
        tree_sitter_python::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

fn build_ruby_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_ruby::LANGUAGE.into(),
        "ruby",
        tree_sitter_ruby::HIGHLIGHTS_QUERY,
        "",
        tree_sitter_ruby::LOCALS_QUERY,
    )
}

fn build_yaml_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_yaml::LANGUAGE.into(),
        "yaml",
        tree_sitter_yaml::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

fn build_toml_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_toml::LANGUAGE.into(),
        "toml",
        tree_sitter_toml::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

fn descriptor_for_language(language: &str) -> Option<&'static LanguageDescriptor> {
    LANGUAGE_DESCRIPTORS.iter().find(|descriptor| {
        descriptor
            .aliases
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case(language))
    })
}

pub fn resolve_code_language_key(language: Option<&str>) -> Option<CodeLanguageKey> {
    let normalized = language?
        .split_whitespace()
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    descriptor_for_language(normalized).map(|descriptor| descriptor.key)
}

/// 围栏代码块语言自动补全清单：mermaid/math/plantuml 为特殊渲染围栏（图表/公式），
/// 与 tree-sitter 语法高亮语言一起按字典序返回（前端按前缀匹配过滤）。
#[tauri::command]
pub fn code_languages() -> Vec<String> {
    let mut languages: Vec<String> = LANGUAGE_DESCRIPTORS
        .iter()
        .map(|descriptor| descriptor.aliases[0].to_string())
        .collect();
    languages.push("mermaid".to_string());
    languages.push("math".to_string());
    languages.push("plantuml".to_string());
    languages.sort_by_key(|name| name.to_lowercase());
    languages.dedup();
    languages
}

pub fn highlight_code_block(
    language: Option<&str>,
    source: &str,
) -> Option<CodeHighlightResult> {
    let key = resolve_code_language_key(language)?;

    // 无配置（语言无语法支持）/解析失败/事件错误时的统一回落：空 span（前端按纯文本渲染）
    let empty = || CodeHighlightResult {
        language: key,
        spans: Vec::new(),
    };
    let Some(config) = CODE_HIGHLIGHT_REGISTRY.config_for(key) else {
        return Some(empty());
    };
    let mut highlighter = Highlighter::new();
    // tree-sitter-highlight 0.27 签名：highlight(config, source, encoding, cancellation, injection_callback)；
    // encoding=None 表示输入为 UTF-8 字节流（Some 常量用于 UTF-16LE/BE）
    let events = match highlighter.highlight(config, source.as_bytes(), None, None, |_| None) {
        Ok(events) => events,
        Err(_) => return Some(empty()),
    };
    // 事件流转扁平 span：Source 段套用生效类栈（active）栈顶类，
    // HighlightStart/End 维护栈；相邻同类 span 在 push_highlight_span 内合并
    let mut spans = Vec::new();
    let mut active = Vec::new();
    for event in events {
        let Ok(event) = event else {
            return Some(empty());
        };
        match event {
            HighlightEvent::Source { start, end } => {
                if let Some(class) = active.last().copied() {
                    push_highlight_span(&mut spans, start..end, class);
                }
            }
            HighlightEvent::HighlightStart(highlight) => {
                if let Some(class) = class_for_highlight(highlight) {
                    active.push(class);
                }
            }
            HighlightEvent::HighlightEnd => {
                active.pop();
            }
        }
    }
    Some(CodeHighlightResult {
        language: key,
        spans,
    })
}

fn push_highlight_span(
    spans: &mut Vec<CodeHighlightSpan>,
    range: Range<usize>,
    class: CodeHighlightClass,
) {
    if range.start >= range.end {
        return;
    }

    if let Some(last) = spans.last_mut()
        && last.class == class
        && last.range.end == range.start
    {
        last.range.end = range.end;
        return;
    }

    spans.push(CodeHighlightSpan { range, class });
}

fn class_for_highlight(highlight: Highlight) -> Option<CodeHighlightClass> {
    let name = HIGHLIGHT_NAMES.get(highlight.0)?;
    Some(match *name {
        "comment" => CodeHighlightClass::Comment,
        "keyword" | "tag" => CodeHighlightClass::Keyword,
        "string" | "string.special" | "embedded" => CodeHighlightClass::String,
        "number" => CodeHighlightClass::Number,
        "type" | "type.builtin" | "module" => CodeHighlightClass::Type,
        "function" | "function.builtin" | "constructor" => CodeHighlightClass::Function,
        "constant" | "constant.builtin" => CodeHighlightClass::Constant,
        "variable" | "variable.builtin" | "variable.parameter" => CodeHighlightClass::Variable,
        "property" | "property.builtin" | "attribute" => CodeHighlightClass::Property,
        "operator" => CodeHighlightClass::Operator,
        "punctuation" | "punctuation.bracket" | "punctuation.delimiter" | "punctuation.special" => {
            CodeHighlightClass::Punctuation
        }
        _ => return None,
    })
}


// ---------- Tauri 命令层 ----------

/// 高亮片段（前端按 class 上色，class 与主题 token code_syntax_* 后缀一致）。
/// start/end 为 UTF-16 码元偏移（前端直接 String.slice 使用）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HighlightSpanDto {
    pub start: usize,
    pub end: usize,
    #[serde(rename = "class")]
    pub class_name: CodeHighlightClass,
}

/// 代码块语法高亮（tree-sitter）：语言未知/纯文本/失败时返回空数组（前端按纯文本渲染）。
#[tauri::command]
pub fn highlight_code(language: Option<&str>, code: &str) -> Vec<HighlightSpanDto> {
    let Some(result) = highlight_code_block(language, code) else {
        return Vec::new();
    };
    if result.spans.is_empty() {
        return Vec::new();
    }
    // 字节偏移 → UTF-16 码元偏移：只收集 span 边界，排序去重后单趟推进计算，
    // 不再为整块代码分配 `code.len() + 1` 的前缀映射表。
    let mut boundaries: Vec<usize> = result
        .spans
        .iter()
        .flat_map(|span| [span.range.start, span.range.end])
        .map(|byte| byte.min(code.len()))
        .collect();
    boundaries.sort_unstable();
    boundaries.dedup();

    // 与 boundaries 平行的 UTF-16 偏移；每个分段只遍历一次，整体 O(code.len())。
    let mut utf16_at = Vec::with_capacity(boundaries.len());
    let mut utf16 = 0usize;
    let mut prev_byte = 0usize;
    for &byte in &boundaries {
        if let Some(segment) = code.get(prev_byte..byte) {
            utf16 += segment.chars().map(char::len_utf16).sum::<usize>();
        }
        utf16_at.push(utf16);
        prev_byte = byte;
    }
    let utf16_offset = |byte: usize| {
        let byte = byte.min(code.len());
        match boundaries.binary_search(&byte) {
            Ok(index) => utf16_at[index],
            // 边界集合由同一 clamp 逻辑生成，必然命中；兜底取最近前驱偏移。
            Err(index) => index.checked_sub(1).map_or(0, |i| utf16_at[i]),
        }
    };
    result
        .spans
        .iter()
        .map(|span| HighlightSpanDto {
            start: utf16_offset(span.range.start),
            end: utf16_offset(span.range.end),
            class_name: span.class,
        })
        .collect()
}
