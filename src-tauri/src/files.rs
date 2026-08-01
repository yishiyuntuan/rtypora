//! 文件操作命令：打开/读取 Markdown 文件、列目录（供侧边栏文件树）。
//! 文件对话框仅在 Rust 端使用（tauri-plugin-dialog 的 DialogExt），前端无需插件权限。

use serde::Serialize;
use tauri_plugin_dialog::{DialogExt, FileDialogBuilder};

use crate::markdown;

/// 对话框种类（打开文件 / 选择文件夹 / 保存文件）。
enum DialogMode {
    PickFile,
    PickFolder,
    SaveFile,
}

/// 回调式文件对话框（异步命令专用）。
/// macOS 上阻塞式（blocking_*）对话框在同步命令里会卡死主线程（窗口一直转圈），
/// 回调式由插件内部调度到主线程执行，结果经 oneshot 通道传回。
async fn pick_dialog(
    builder: FileDialogBuilder<tauri::Wry>,
    mode: DialogMode,
) -> Option<std::path::PathBuf> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let send = move |picked| {
        let _ = tx.send(picked);
    };
    match mode {
        DialogMode::PickFile => builder.pick_file(send),
        DialogMode::PickFolder => builder.pick_folder(send),
        DialogMode::SaveFile => builder.save_file(send),
    }
    rx.await.ok()??.into_path().ok()
}

/// 大文件渐进加载：首屏前缀的目标字节数（前缀安全截断后解析返回，
/// 尾部由前端后台再解析补齐，避免整棵块树的大 IPC 阻塞首屏）。
const OPEN_PREFIX_TARGET_BYTES: usize = 128 * 1024;

/// 已打开的 Markdown 文件（路径 + 全文）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedFile {
    pub path: String,
    pub content: String,
}

/// 已打开的 Markdown 文件 + 首屏解析块（渐进加载入口）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedFileParsed {
    pub path: String,
    pub content: String,
    /// 首屏块（前缀解析结果；tail_from 为 None 时即全文块树）
    pub blocks: Vec<markdown::model::BlockDto>,
    /// 尾部重解析起点（UTF-16 偏移 = 首屏末块起点；末块可能被截断，
    /// 尾部带完整上下文重解析后原位替换，接缝自愈）；None 表示无需尾部
    pub tail_from: Option<usize>,
    /// 原文档主导换行风格（"crlf" | "lf"）：内容已规范化为 LF，
    /// 保存时由前端按此还原，保持文件原有的换行约定
    pub line_ending: String,
}

/// 换行符规范化：CRLF/CR → LF（解析器与 UTF-16 偏移体系只认 \n）；
/// 返回规范化文本与原文档主导风格（CRLF 居多记 "crlf"，否则 "lf"）。
fn normalize_line_endings(text: &str) -> (String, &'static str) {
    let crlf = text.matches("\r\n").count();
    let lone_lf = text.matches('\n').count() - crlf;
    let style = if crlf > lone_lf { "crlf" } else { "lf" };
    if !text.contains('\r') {
        return (text.to_string(), style);
    }
    (text.replace("\r\n", "\n").replace('\r', "\n"), style)
}

/// 前缀安全截断点：target 之后首个「非代码围栏内空行」的行首。
/// 截在空行之前（前缀不以空行结尾）：否则前缀解析会在末尾多产出一个空段落块，
/// 与尾部重解析拼接后比全量解析多一块。空行本身是跨块安全边界
/// （松列表/表格/段落均在空行处分块），围栏内不切（避免代码块被截成两截）。
fn safe_prefix_end(markdown: &str, target_bytes: usize) -> usize {
    let mut in_fence = false;
    let mut pos = 0usize;
    for line in markdown.split_inclusive('\n') {
        let line_start = pos;
        pos += line.len();
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
        }
        if line_start >= target_bytes && !in_fence && line.trim().is_empty() {
            return line_start;
        }
    }
    markdown.len()
}

/// 读取结果 + 首屏解析：小文档全量解析（tail_from = None），
/// 大文档只解析安全前缀，尾部偏移交给前端后台增量补齐。
/// 内容统一规范化为 LF（line_ending 记录原文档风格供保存还原）。
fn parsed_open(path: String, content: String) -> OpenedFileParsed {
    let (content, line_ending) = normalize_line_endings(&content);
    if content.len() <= OPEN_PREFIX_TARGET_BYTES {
        let blocks = markdown::parse_blocks(&content);
        return OpenedFileParsed {
            path,
            content,
            blocks,
            tail_from: None,
            line_ending: line_ending.to_string(),
        };
    }
    let cut = safe_prefix_end(&content, OPEN_PREFIX_TARGET_BYTES);
    let blocks = markdown::parse_blocks(&content[..cut]);
    // 末块起点作为尾部重解析起点（末块可能被截断，需带上下文重解析自愈）；
    // 前缀无块（极端：整块超长的围栏/HTML 块）时从 0 全量重来
    let tail_from = if cut < content.len() {
        Some(blocks.last().and_then(|b| b.start).unwrap_or(0))
    } else {
        None
    };
    OpenedFileParsed {
        path,
        content,
        blocks,
        tail_from,
        line_ending: line_ending.to_string(),
    }
}

/// 目录条目（侧边栏文件树节点）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_markdown: bool,
}

fn is_markdown_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

/// 弹出文件选择框选择并读取 Markdown 文件；取消或读取失败返回 None。
#[tauri::command]
pub async fn open_markdown_file(app: tauri::AppHandle) -> Option<OpenedFile> {
    let path = pick_dialog(
        app.dialog()
            .file()
            .add_filter("Markdown", &["md", "markdown"]),
        DialogMode::PickFile,
    )
    .await?;
    let content = std::fs::read_to_string(&path).ok()?;
    Some(OpenedFile {
        path: path.to_string_lossy().to_string(),
        content,
    })
}

/// 按路径读取 Markdown 文件（侧边栏文件树点击）；非 Markdown 文件或读取失败返回 None。
#[tauri::command]
pub fn read_markdown_file(path: &str) -> Option<OpenedFile> {
    if !is_markdown_name(path) {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    Some(OpenedFile {
        path: path.to_string(),
        content,
    })
}

/// 打开 Markdown 文件并返回首屏解析块（大文件渐进加载）。
#[tauri::command]
pub async fn open_markdown_parsed(app: tauri::AppHandle) -> Option<OpenedFileParsed> {
    let path = pick_dialog(
        app.dialog()
            .file()
            .add_filter("Markdown", &["md", "markdown"]),
        DialogMode::PickFile,
    )
    .await?;
    let content = std::fs::read_to_string(&path).ok()?;
    Some(parsed_open(path.to_string_lossy().to_string(), content))
}

/// 按路径读取 Markdown 文件并返回首屏解析块（大文件渐进加载）。
#[tauri::command]
pub fn read_markdown_parsed(path: &str) -> Option<OpenedFileParsed> {
    if !is_markdown_name(path) {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    Some(parsed_open(path.to_string(), content))
}

/// 列出目录内容：文件夹在前，文件在后，各自排序（不递归）。
/// sort: "asc" 名称升序（默认）| "desc" 名称降序 |
/// "created_asc"/"created_desc" 创建时间升/降序 | "modified_asc"/"modified_desc" 修改时间升/降序
#[tauri::command]
pub fn list_dir(path: &str, sort: Option<&str>) -> Vec<DirEntry> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // 跳过隐藏文件/目录
        if name.starts_with('.') {
            continue;
        }
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let item = DirEntry {
            is_markdown: !is_dir && is_markdown_name(&name),
            path: entry.path().to_string_lossy().to_string(),
            name,
            is_dir,
        };
        if is_dir {
            dirs.push(item);
        } else {
            files.push(item);
        }
    }
    let mode = sort.unwrap_or("asc");
    let by_name = |a: &DirEntry, b: &DirEntry| a.name.to_lowercase().cmp(&b.name.to_lowercase());
    // 时间戳取不到（权限/删除竞争）时按最早处理（排升序在前、降序在后）
    let time_of = |e: &DirEntry, created: bool| {
        std::fs::metadata(&e.path)
            .and_then(|m| if created { m.created() } else { m.modified() })
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    };
    match mode {
        "desc" => {
            dirs.sort_by(|a, b| by_name(a, b).reverse());
            files.sort_by(|a, b| by_name(a, b).reverse());
        }
        "created_asc" | "created_desc" | "modified_asc" | "modified_desc" => {
            let created = mode.starts_with("created");
            let by_time = |a: &DirEntry, b: &DirEntry| {
                let ord = time_of(a, created).cmp(&time_of(b, created));
                if mode.ends_with("_desc") { ord.reverse() } else { ord }
            };
            dirs.sort_by(by_time);
            files.sort_by(by_time);
        }
        _ => {
            dirs.sort_by(by_name);
            files.sort_by(by_name);
        }
    }
    dirs.extend(files);
    dirs
}

// ---------- 图片加载 ----------

use base64::Engine;
use std::path::PathBuf;

/// URL 百分号编码解码（Typora 等编辑器会对含中文/空格的图片路径做编码）。
/// 无 % 时返回 None；非法 % 序列原样保留（路径语境，不做表单 + 号转换）。
fn percent_decode(s: &str) -> Option<String> {
    if !s.contains('%') {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h = (bytes[i + 1] as char).to_digit(16);
            let l = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (h, l) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    Some(String::from_utf8_lossy(&out).into_owned())
}

/// 解析本地图片路径：相对路径按 base_dir 拼接；候选依次为原文、百分号解码后路径
///（目录名本身含合法 % 序列时原文优先，避免误解码），返回第一个存在的文件。
fn resolve_local_image(source: &str, base_dir: Option<&str>) -> Option<PathBuf> {
    let mut candidates = vec![source.to_string()];
    if let Some(decoded) = percent_decode(source) {
        if decoded != source {
            candidates.push(decoded);
        }
    }
    for cand in candidates {
        let p = std::path::Path::new(&cand);
        let resolved: PathBuf = if p.is_absolute() {
            p.to_path_buf()
        } else {
            PathBuf::from(base_dir?).join(p)
        };
        if resolved.is_file() {
            return Some(resolved);
        }
    }
    None
}


/// 读取本地图片为 data URL（相对路径基于文档目录解析，支持百分号编码路径）；
/// 远程 http(s) URL 原样返回。读取失败返回 None（前端显示占位）。
#[tauri::command]
pub fn read_image_data_url(source: &str, base_dir: Option<&str>) -> Option<String> {
    if source.starts_with("http://") || source.starts_with("https://") {
        return Some(source.to_string());
    }
    let resolved = resolve_local_image(source, base_dir)?;
    let bytes = std::fs::read(&resolved).ok()?;
    let mime = match resolved
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("bmp") => "image/bmp",
        Some("ico") => "image/x-icon",
        Some("avif") => "image/avif",
        _ => "application/octet-stream",
    };
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:{mime};base64,{encoded}"))
}

/// 解析本地图片为存在的绝对路径（编辑器内 <img> 走 asset 协议：
/// 浏览器原生缓存 + 流式读取，滚动重挂载不再重复 base64 解码）。
/// 远程 URL 返回 None（前端直用原地址）；文件不存在返回 None（前端显示占位）。
#[tauri::command]
pub fn resolve_image_path(source: &str, base_dir: Option<&str>) -> Option<String> {
    if source.starts_with("http://") || source.starts_with("https://") {
        return None;
    }
    let resolved = resolve_local_image(source, base_dir)?;
    Some(resolved.to_string_lossy().to_string())
}

/// 保存到当前路径；写失败返回错误信息。
#[tauri::command]
pub fn save_file(path: &str, content: &str) -> Result<(), String> {
    std::fs::write(path, content).map_err(|e| e.to_string())
}

/// 另存为：弹出保存对话框并写入；取消返回 None，写失败返回错误信息。
#[tauri::command]
pub async fn save_file_as(app: tauri::AppHandle, content: String) -> Option<Result<String, String>> {
    let path = pick_dialog(
        app.dialog()
            .file()
            .add_filter("Markdown", &["md", "markdown"])
            .set_file_name("untitled.md"),
        DialogMode::SaveFile,
    )
    .await?;
    let path_str = path.to_string_lossy().to_string();
    Some(std::fs::write(&path, content).map(|_| path_str).map_err(|e| e.to_string()))
}

/// 粘贴图片保存：把剪贴板图片字节写入目标目录（文档目录或 assets 子目录），
/// 返回供 Markdown 引用的相对路径（如 `./assets/paste-1712345678.png`）。
#[tauri::command]
pub fn save_pasted_image(
    bytes: Vec<u8>,
    base_dir: &str,
    sub_dir: Option<&str>,
    extension: Option<&str>,
) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    let dir = match sub_dir {
        Some(sub) if !sub.is_empty() => {
            let dir = std::path::Path::new(base_dir).join(sub);
            std::fs::create_dir_all(&dir).ok()?;
            dir
        }
        _ => std::path::PathBuf::from(base_dir),
    };
    let ext = extension.unwrap_or("png");
    let name = format!("paste-{}.{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis(), ext);
    let path = dir.join(&name);
    std::fs::write(&path, bytes).ok()?;
    // 返回相对引用路径
    Some(match sub_dir {
        Some(sub) if !sub.is_empty() => format!("./{}/{}", sub.trim_matches('/'), name),
        _ => format!("./{}", name),
    })
}

/// 源 Markdown 文件名 → 导出 HTML 建议文件名（大小写不敏感去除 .md/.markdown 后缀）。
pub fn html_export_name(source_name: &str) -> String {
    let lower = source_name.to_ascii_lowercase();
    let stem = if lower.ends_with(".markdown") {
        &source_name[..source_name.len() - ".markdown".len()]
    } else if lower.ends_with(".md") {
        &source_name[..source_name.len() - ".md".len()]
    } else {
        source_name
    };
    format!("{stem}.html")
}

/// 装配独立导出 HTML 文档：模板与 title 转义在 Rust 完成；
/// 前端只负责从渲染 DOM 提取内容 HTML 与样式文本（视图层提取）。
#[tauri::command]
pub fn build_export_html(content_html: &str, css_text: &str, title: &str) -> String {
    let escaped_title = title
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    format!(
        "<!DOCTYPE html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"utf-8\">\n<title>{escaped_title}</title>\n<style>\n{css_text}\n</style>\n</head>\n<body>\n<div class=\"t-root\"><div class=\"t-measure\">\n{content_html}\n</div></div>\n</body>\n</html>"
    )
}

/// 导出 HTML：弹出保存对话框（.html 过滤器，建议文件名由源文件名推导）并写入；取消返回 None。
#[tauri::command]
pub async fn save_html_as(app: tauri::AppHandle, content: String, source_name: String) -> Option<Result<String, String>> {
    let path = pick_dialog(
        app.dialog()
            .file()
            .add_filter("HTML", &["html", "htm"])
            .set_file_name(html_export_name(&source_name)),
        DialogMode::SaveFile,
    )
    .await?;
    let path_str = path.to_string_lossy().to_string();
    Some(std::fs::write(&path, content).map(|_| path_str).map_err(|e| e.to_string()))
}

/// 选择文件夹对话框；取消返回 None。
#[tauri::command]
pub async fn pick_folder(app: tauri::AppHandle) -> Option<String> {
    let path = pick_dialog(app.dialog().file(), DialogMode::PickFolder).await?;
    Some(path.to_string_lossy().to_string())
}

/// 弹出文件选择框选择并读取主题包文件（JSON/JSONC/YAML）；取消或读取失败返回 None。
/// 前端隐藏 file input 的程序化点击在 WKWebView 不弹窗，主题导入统一走原生对话框。
#[tauri::command]
pub async fn pick_theme_file(app: tauri::AppHandle) -> Option<String> {
    let path = pick_dialog(
        app.dialog()
            .file()
            .add_filter("主题包", &["json", "jsonc", "yaml", "yml"]),
        DialogMode::PickFile,
    )
    .await?;
    std::fs::read_to_string(&path).ok()
}

/// 在当前文件夹创建新的 Markdown 文件，返回路径。
/// 指定 `name` 时按该名称创建（剥离路径分隔符限制在当前目录、自动补 `.md` 后缀、
/// 已存在则拒绝不覆盖，返回 None）；未指定时 untitled.md（重名自动编号）。
#[tauri::command]
pub fn create_markdown_file(dir: &str, name: Option<String>) -> Option<String> {
    let dir = std::path::Path::new(dir);
    if !dir.is_dir() {
        return None;
    }
    if let Some(name) = name {
        // 名称净化：去首尾空白与前导分隔符/点、剔除路径分隔符（保证落盘在当前目录）
        let cleaned = name
            .trim()
            .trim_start_matches(['/', '\\', '.'])
            .replace(['/', '\\'], "");
        let cleaned = cleaned.trim();
        if cleaned.is_empty() {
            return None;
        }
        let file_name = if cleaned.to_ascii_lowercase().ends_with(".md") {
            cleaned.to_string()
        } else {
            format!("{cleaned}.md")
        };
        let path = dir.join(&file_name);
        // 已存在则拒绝（不覆盖已有文件）
        if path.exists() {
            return None;
        }
        std::fs::write(&path, "").ok()?;
        return Some(path.to_string_lossy().to_string());
    }
    for i in 0..100 {
        let name = if i == 0 {
            "untitled.md".to_string()
        } else {
            format!("untitled-{i}.md")
        };
        let path = dir.join(&name);
        if !path.exists() {
            std::fs::write(&path, "").ok()?;
            return Some(path.to_string_lossy().to_string());
        }
    }
    None
}
