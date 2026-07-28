//! LaTeX 数学公式渲染：移植自 velotype `components/latex/mod.rs` 的渲染管线。
//! 经 ratex（parser → layout → svg）在 Rust 端渲染为自包含 SVG（内嵌字体字形）。
//! velotype 的磁盘缓存在此简化为进程内缓存；颜色/字号由前端按当前主题传入。

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, Once};

use crate::markdown::block::math::parse_display_math_source;

/// 展示公式字号缩放（与 velotype 一致）。
const DISPLAY_MATH_SCALE: f32 = 1.25;
/// 行内公式字号缩放（与 velotype 一致）。
const INLINE_MATH_SCALE: f32 = 1.12;

static LATEX_CACHE: LazyLock<Mutex<HashMap<String, Option<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static FONT_INIT: Once = Once::new();

/// ratex 的 Unicode 回落字体（CJK 等）默认走无衬线系统字体（如微软雅黑），
/// 与公式的衬线风格不一致。这里默认指定衬线中文字体（宋体），
/// 主题 token math_cjk_font 或 RATEX_UNICODE_FONT 环境变量可覆盖。
fn init_unicode_fallback_font() {
    FONT_INIT.call_once(|| {
        if std::env::var("RATEX_UNICODE_FONT").is_err() {
            // Windows 宋体（衬线）；不存在时 ratex 自动回退系统字体发现。
            // SAFETY: 在首次渲染前经 Once 单线程初始化，此时没有其他线程读该变量。
            unsafe {
                std::env::set_var("RATEX_UNICODE_FONT", r"C:\Windows\Fonts\simsun.ttc#SimSun");
            }
        }
    });
}

/// 设置公式中文回落字体（主题 token math_cjk_font 驱动）。
/// spec 格式：`路径` 或 `路径#索引` 或 `路径#字体族名`。
/// 注意：ratex 首次加载字体后进程内不再重读该变量，运行中切换需重启应用生效。
#[tauri::command]
pub fn set_math_unicode_font(spec: &str) -> bool {
    if spec.trim().is_empty() {
        return false;
    }
    // SAFETY: 与 init 相同的单写入点约定；前端仅经本命令修改该变量。
    unsafe {
        std::env::set_var("RATEX_UNICODE_FONT", spec);
    }
    // 字体变化后旧缓存失效，清空重渲
    if let Ok(mut cache) = LATEX_CACHE.lock() {
        cache.clear();
    }
    true
}

/// 渲染展示公式（`$$...$$` 或 ```math 围栏原文）为 SVG；语法错误返回 None（前端回退源码展示）。
#[tauri::command]
pub fn render_display_math(
    raw: &str,
    color: Option<&str>,
    base_font_size: Option<f32>,
) -> Option<String> {
    let body = parse_display_math_source(raw)
        .map(|source| source.body)
        .or_else(|| parse_math_fence_body(raw))?;
    render_cached(&body, color, base_font_size, DISPLAY_MATH_SCALE)
}

/// 从 ```math / ```latex 围栏提取 LaTeX 正文。
fn parse_math_fence_body(raw: &str) -> Option<String> {
    let lines: Vec<&str> = raw.trim_matches('\n').split('\n').collect();
    if lines.len() < 2 {
        return None;
    }
    let opening = lines[0].trim_end();
    if !(opening.starts_with("```") || opening.starts_with("~~~")) {
        return None;
    }
    let marker = opening.chars().next()?;
    let info = opening.trim_start_matches(marker).trim();
    if !info.eq_ignore_ascii_case("math") && !info.eq_ignore_ascii_case("latex") {
        return None;
    }
    let closing = lines.last()?.trim();
    if !closing.starts_with("```") && !closing.starts_with("~~~") {
        return None;
    }
    Some(lines[1..lines.len() - 1].join("\n"))
}

/// 渲染行内公式（LaTeX 正文）为 SVG；语法错误返回 None。
#[tauri::command]
pub fn render_inline_math(
    body: &str,
    color: Option<&str>,
    base_font_size: Option<f32>,
) -> Option<String> {
    render_cached(body, color, base_font_size, INLINE_MATH_SCALE)
}

fn render_cached(
    latex: &str,
    color: Option<&str>,
    base_font_size: Option<f32>,
    scale: f32,
) -> Option<String> {
    let font_size = base_font_size.unwrap_or(16.0) * scale;
    let color = color.unwrap_or("#000000").to_string();
    let key = format!("{latex}|{color}|{font_size}");
    if let Some(cached) = LATEX_CACHE.lock().ok()?.get(&key) {
        return cached.clone();
    }
    // 直接渲染失败后，按宏别名表重试一次（容错常见非标准写法，如 \part → \partial）
    let rendered = render_latex_to_svg(latex, &color, font_size)
        .ok()
        .or_else(|| {
            let aliased = alias_nonstandard_macros(latex);
            (aliased != latex)
                .then(|| render_latex_to_svg(&aliased, &color, font_size).ok())
                .flatten()
        });
    // 进程内缓存上限（防无界增长）：满 500 条清空重建（命中率损失可接受）
    let mut cache = LATEX_CACHE.lock().ok()?;
    if cache.len() >= 500 {
        cache.clear();
    }
    cache.insert(key, rendered.clone());
    rendered
}

/// 常见非标准宏的容错别名（仅在直接渲染失败时应用）。
const MACRO_ALIASES: &[(&str, &str)] = &[("\\part", "\\partial")];

fn alias_nonstandard_macros(latex: &str) -> String {
    let mut out = latex.to_string();
    for (from, to) in MACRO_ALIASES {
        out = replace_macro(&out, from, to);
    }
    out
}

/// 替换宏名，仅当其后不是 ASCII 字母时生效（宏边界，`\partial` 中的 `\part` 不受影响）。
fn replace_macro(text: &str, from: &str, to: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find(from) {
        result.push_str(&rest[..pos]);
        let after = &rest[pos + from.len()..];
        if after.chars().next().is_some_and(|ch| ch.is_ascii_alphabetic()) {
            result.push_str(from);
        } else {
            result.push_str(to);
        }
        rest = after;
    }
    result.push_str(rest);
    result
}

/// LaTeX 表达式 → 自包含 SVG 文本（ratex，默认黑色替换为主题色）。
/// physics 包宏先展开为 ratex 支持的语法（\dv/\pdv/\abs/\norm/\bra/\ket/\qty 等）。
fn render_latex_to_svg(latex: &str, color: &str, font_size: f32) -> Result<String, ()> {
    init_unicode_fallback_font();
    let expanded = expand_physics_macros(latex);
    let parsed = ratex_parser::parse(&expanded).map_err(|_| ())?;
    let layout = ratex_layout::layout(&parsed, &ratex_layout::LayoutOptions::default());
    let display_list = ratex_layout::to_display_list(&layout);
    let svg = ratex_svg::render_to_svg(
        &display_list,
        &ratex_svg::SvgOptions {
            font_size: f64::from(font_size.max(1.0)),
            padding: f64::from((font_size * 0.35).max(4.0)),
            embed_glyphs: true,
            ..ratex_svg::SvgOptions::default()
        },
    );
    Ok(recolor_default_black(&svg, &css_color_to_rgba(color)))
}

/// ratex 输出默认黑色 rgba(0,0,0,1)，替换为主题正文色（与 velotype 一致）。
fn recolor_default_black(svg: &str, color: &str) -> String {
    svg.replace("rgba(0,0,0,1)", color)
        .replace("rgba(0, 0, 0, 1)", color)
}

/// CSS 颜色（#rgb/#rrggbb/#rrggbbaa/rgb()/rgba()/具名黑白色）→ rgba(r,g,b,a)。
fn css_color_to_rgba(color: &str) -> String {
    let color = color.trim();
    let hex = color.strip_prefix('#').unwrap_or(color);
    let (r, g, b, a) = match hex.len() {
        3 => (
            dup(&hex[0..1]),
            dup(&hex[1..2]),
            dup(&hex[2..3]),
            255,
        ),
        6 => (
            u8::from_str_radix(&hex[0..2], 16).unwrap_or(0),
            u8::from_str_radix(&hex[2..4], 16).unwrap_or(0),
            u8::from_str_radix(&hex[4..6], 16).unwrap_or(0),
            255,
        ),
        8 => (
            u8::from_str_radix(&hex[0..2], 16).unwrap_or(0),
            u8::from_str_radix(&hex[2..4], 16).unwrap_or(0),
            u8::from_str_radix(&hex[4..6], 16).unwrap_or(0),
            u8::from_str_radix(&hex[6..8], 16).unwrap_or(255),
        ),
        // rgb()/rgba() 或其他格式：原样透传（多数为合法 CSS 颜色）
        _ => return color.to_string(),
    };
    fn dup(s: &str) -> u8 {
        u8::from_str_radix(&s.repeat(2), 16).unwrap_or(0)
    }
    format!("rgba({r},{g},{b},{:.3})", f32::from(a) / 255.0)
}


// ---------- physics 包宏展开 ----------

/// 支持的 physics 包宏名。
const PHYSICS_MACROS: &[&str] = &[
    "braket", "ket", "bra", "norm", "abs", "qty", "eval", "pdv", "dv", "dd",
];

/// 把 physics 包常用宏展开为 ratex 支持的 LaTeX（ratex 不内置 physics 包）。
/// pub 仅供集成测试调试。
pub fn debug_expand_physics(latex: &str) -> String {
    expand_physics_macros(latex)
}

fn expand_physics_macros(latex: &str) -> String {
    let mut out = String::with_capacity(latex.len());
    let mut rest = latex;
    while let Some(pos) = rest.find('\\') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        let name_len = after.bytes().take_while(|b| b.is_ascii_alphabetic()).count();
        let name = &after[..name_len];
        if name.is_empty() || !PHYSICS_MACROS.contains(&name) {
            out.push_str(&rest[pos..pos + 1 + name_len]);
            rest = &rest[pos + 1 + name_len..];
            continue;
        }
        let (replacement, consumed) = expand_physics_macro(name, after);
        out.push_str(&replacement);
        rest = &rest[pos + 1 + consumed..];
    }
    out.push_str(rest);
    out
}

/// 展开单个 physics 宏；返回（展开结果，在 after 中消耗的字节数）。
/// 参数不完整时原样返回（不破坏输入）。
fn expand_physics_macro(name: &str, after: &str) -> (String, usize) {
    match name {
        "dd" => {
            // read_braced_group 返回的 consumed 已是 after 内绝对偏移（含宏名长度）
            let (replacement, consumed) = match read_braced_group(after, name.len()) {
                Some((group, consumed)) => (format!("\\mathrm{{d}}{group}"), consumed),
                None => ("\\mathrm{d}".to_string(), name.len()),
            };
            (replacement, consumed)
        }
        "dv" | "pdv" => {
            let diff = if name == "dv" { "\\mathrm{d}" } else { "\\partial" };
            let (power, cursor) = read_bracket_opt(after, name.len());
            let Some((g1, c1)) = read_braced_group(after, cursor) else {
                return (format!("\\{name}"), name.len());
            };
            // c1/c2 为 after 内绝对偏移（含宏名），游标直接取绝对位置
            if let Some((g2, c2)) = read_braced_group(after, c1) {
                (
                    // 分子：d^n f（阶数挂微分算符，算符与变量间留空格避免连成未知宏）
                    format!(
                        "\\frac{{{}}}{{{}}}",
                        format!("{} {}", apply_power(diff, &power), g1),
                        apply_power(&format!("{diff} {g2}"), &power)
                    ),
                    c2,
                )
            } else {
                (
                    format!(
                        "\\frac{{{}}}{{{}}}",
                        apply_power(diff, &power),
                        apply_power(&format!("{diff} {g1}"), &power)
                    ),
                    c1,
                )
            }
        }
        "abs" => wrap_group(after, name, "\\left| ", " \\right|"),
        "norm" => wrap_group(after, name, "\\left\\| ", " \\right\\|"),
        "bra" => wrap_group(after, name, "\\left\\langle ", " \\right|"),
        "ket" => wrap_group(after, name, "\\left| ", " \\right\\rangle"),
        "braket" => {
            let Some((g1, c1)) = read_braced_group(after, name.len()) else {
                return (format!("\\{name}"), name.len());
            };
            let Some((g2, c2)) = read_braced_group(after, c1) else {
                return (format!("\\{name}"), name.len());
            };
            (
                format!("\\left\\langle {g1} \\middle| {g2} \\right\\rangle"),
                c2,
            )
        }
        "eval" => {
            let Some((g1, c1)) = read_braced_group(after, name.len()) else {
                return (format!("\\{name}"), name.len());
            };
            if let Some((g2, c2)) = read_braced_group(after, c1) {
                if let Some((g3, c3)) = read_braced_group(after, c2) {
                    return (
                        format!("\\left. {g1} \\right|_{{{g2}}}^{{{g3}}}"),
                        c3,
                    );
                }
                return (format!("\\left. {g1} \\right|_{{{g2}}}"), c2);
            }
            (format!("\\left. {g1} \\right|"), c1)
        }
        "qty" => expand_qty(after, name.len()),
        _ => (format!("\\{name}"), name.len()),
    }
}

/// \abs / \norm / \bra / \ket 的「一个分组包一层定界符」展开。
fn wrap_group(after: &str, name: &str, open: &str, close: &str) -> (String, usize) {
    // read_braced_group 返回的 consumed 已是 after 内绝对偏移（含宏名长度）
    match read_braced_group(after, name.len()) {
        Some((group, consumed)) => (format!("{open}{group}{close}"), consumed),
        None => (format!("\\{name}"), name.len()),
    }
}

/// \qty(...) / \qty[...] / \qty{...} 展开为对应的 \left...\right...。
fn expand_qty(after: &str, name_len: usize) -> (String, usize) {
    let rest = &after[name_len..];
    let (open_ch, close_ch, left, right) = match rest.chars().next() {
        Some('(') => ('(', ')', "\\left( ", " \\right)"),
        Some('[') => ('[', ']', "\\left[ ", " \\right]"),
        Some('{') => ('{', '}', "\\left\\{ ", " \\right\\}"),
        _ => return ("\\qty".to_string(), name_len),
    };
    match read_balanced(rest, open_ch, close_ch) {
        Some((body, consumed)) => (format!("{left}{body}{right}"), name_len + consumed),
        None => ("\\qty".to_string(), name_len),
    }
}

/// 读取 `{...}` 分组（支持嵌套花括号）；start 为 after 内的字节偏移。
fn read_braced_group(after: &str, start: usize) -> Option<(String, usize)> {
    let bytes = after.as_bytes();
    if bytes.get(start) != Some(&b'{') {
        return None;
    }
    let mut depth = 0usize;
    let mut end = start;
    while end < bytes.len() {
        match bytes[end] {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some((after[start + 1..end].to_string(), end + 1));
                }
            }
            _ => {}
        }
        end += 1;
    }
    None
}

/// 读取配对括号（圆/方）内容；text 首字符须为 open。
fn read_balanced(text: &str, open: char, close: char) -> Option<(String, usize)> {
    let mut depth = 0usize;
    for (idx, ch) in text.char_indices() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some((text[open.len_utf8()..idx].to_string(), idx + ch.len_utf8()));
            }
        }
    }
    None
}

/// 读取可选 `[n]` 阶数参数，返回（阶数字符串，消耗字节数；无括号时游标停在 start）。
fn read_bracket_opt(after: &str, start: usize) -> (String, usize) {
    read_balanced(&after[start..], '[', ']')
        .map(|(body, consumed)| (body, start + consumed))
        .unwrap_or_else(|| (String::new(), start))
}

/// 有阶数参数时加 `^{n}`。
fn apply_power(text: &str, power: &str) -> String {
    if power.is_empty() {
        text.to_string()
    } else {
        format!("{text}^{{{power}}}")
    }
}
