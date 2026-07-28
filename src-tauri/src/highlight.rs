//! Code-block syntax highlighting support.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::LazyLock;

use tree_sitter_highlight::{Highlight, HighlightConfiguration, HighlightEvent, Highlighter};


/// Canonical language key used by the syntax-highlighting registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CodeLanguageKey {
    /// Rust source code.
    Rust,
    /// JavaScript without JSX.
    JavaScript,
    /// JavaScript with JSX syntax.
    JavaScriptJsx,
    /// TypeScript without TSX.
    TypeScript,
    /// TypeScript with TSX syntax.
    TypeScriptTsx,
    /// JSON data.
    Json,
    /// Markdown source.
    Markdown,
    /// POSIX-like shell scripts.
    Bash,
    /// C source code.
    C,
    /// C++ source code.
    Cpp,
    /// C# source code.
    CSharp,
    /// CSS stylesheets.
    Css,
    /// Go source code.
    Go,
    /// HTML markup.
    Html,
    /// Java source code.
    Java,
    /// PHP source code.
    Php,
    /// Python source code.
    Python,
    /// Ruby source code.
    Ruby,
    /// YAML configuration.
    Yaml,
    /// TOML configuration.
    Toml,
    /// Mermaid diagram source.
    Mermaid,
    /// Plain text or unknown language fallback.
    PlainText,
}

/// Semantic highlight classes mapped onto theme colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CodeHighlightClass {
    /// Comment text.
    Comment,
    /// Language keyword or reserved word.
    Keyword,
    /// String literal.
    String,
    /// Numeric literal.
    Number,
    /// Type name.
    Type,
    /// Function or callable identifier.
    Function,
    /// Constant identifier.
    Constant,
    /// Variable identifier.
    Variable,
    /// Object or record property.
    Property,
    /// Operator token.
    Operator,
    /// Punctuation token.
    Punctuation,
}

/// Highlighted byte range inside a code block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeHighlightSpan {
    pub(crate) range: Range<usize>,
    pub(crate) class: CodeHighlightClass,
}

/// Highlight result cached on a code block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeHighlightResult {
    pub(crate) language: CodeLanguageKey,
    pub(crate) spans: Vec<CodeHighlightSpan>,
}

/// Language aliases accepted from fenced-code info strings.
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

/// Lazily built tree-sitter highlighter registry.
struct CodeHighlightRegistry {
    configs: HashMap<CodeLanguageKey, HighlightConfiguration>,
}

static CODE_HIGHLIGHT_REGISTRY: LazyLock<CodeHighlightRegistry> =
    LazyLock::new(CodeHighlightRegistry::new);

impl CodeHighlightRegistry {
    fn new() -> Self {
        let mut configs = HashMap::new();
                maybe_insert_config(&mut configs, CodeLanguageKey::Rust, build_rust_config());
                maybe_insert_config(
            &mut configs,
            CodeLanguageKey::JavaScript,
            build_javascript_config(),
        );
                maybe_insert_config(
            &mut configs,
            CodeLanguageKey::JavaScriptJsx,
            build_jsx_config(),
        );
                maybe_insert_config(
            &mut configs,
            CodeLanguageKey::TypeScript,
            build_typescript_config(),
        );
                maybe_insert_config(
            &mut configs,
            CodeLanguageKey::TypeScriptTsx,
            build_tsx_config(),
        );
                maybe_insert_config(&mut configs, CodeLanguageKey::Json, build_json_config());
                maybe_insert_config(
            &mut configs,
            CodeLanguageKey::Markdown,
            build_markdown_config(),
        );
                maybe_insert_config(&mut configs, CodeLanguageKey::Bash, build_bash_config());
                maybe_insert_config(&mut configs, CodeLanguageKey::C, build_c_config());
                maybe_insert_config(&mut configs, CodeLanguageKey::Cpp, build_cpp_config());
                maybe_insert_config(&mut configs, CodeLanguageKey::CSharp, build_csharp_config());
                maybe_insert_config(&mut configs, CodeLanguageKey::Css, build_css_config());
                maybe_insert_config(&mut configs, CodeLanguageKey::Go, build_go_config());
                maybe_insert_config(&mut configs, CodeLanguageKey::Html, build_html_config());
                maybe_insert_config(&mut configs, CodeLanguageKey::Java, build_java_config());
                maybe_insert_config(&mut configs, CodeLanguageKey::Php, build_php_config());
                maybe_insert_config(&mut configs, CodeLanguageKey::Python, build_python_config());
                maybe_insert_config(&mut configs, CodeLanguageKey::Ruby, build_ruby_config());
                maybe_insert_config(&mut configs, CodeLanguageKey::Yaml, build_yaml_config());
                maybe_insert_config(&mut configs, CodeLanguageKey::Toml, build_toml_config());
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

        if let Some(config) = CODE_HIGHLIGHT_REGISTRY.config_for(key) {
        let mut highlighter = Highlighter::new();
        let events = match highlighter.highlight(config, source.as_bytes(), None, |_| None) {
            Ok(events) => events,
            Err(_) => {
                return Some(CodeHighlightResult {
                    language: key,
                    spans: Vec::new(),
                });
            }
        };

        let mut spans = Vec::new();
        let mut active = Vec::new();
        for event in events {
            let Ok(event) = event else {
                return Some(CodeHighlightResult {
                    language: key,
                    spans: Vec::new(),
                });
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

        return Some(CodeHighlightResult {
            language: key,
            spans,
        });
    }

    Some(CodeHighlightResult {
        language: key,
        spans: Vec::new(),
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
    // 字节偏移 → UTF-16 码元偏移（一次性前缀映射）
    let mut byte_to_utf16 = vec![0usize; code.len() + 1];
    let mut utf16 = 0;
    for (byte, ch) in code.char_indices() {
        byte_to_utf16[byte] = utf16;
        utf16 += ch.len_utf16();
    }
    byte_to_utf16[code.len()] = utf16;
    result
        .spans
        .iter()
        .map(|span| HighlightSpanDto {
            start: byte_to_utf16[span.range.start.min(code.len())],
            end: byte_to_utf16[span.range.end.min(code.len())],
            class_name: span.class,
        })
        .collect()
}
