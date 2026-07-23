//! 文件操作命令：打开/读取 Markdown 文件、列目录（供侧边栏文件树）。
//! 文件对话框仅在 Rust 端使用（tauri-plugin-dialog 的 DialogExt），前端无需插件权限。

use serde::Serialize;
use tauri_plugin_dialog::DialogExt;

/// 已打开的 Markdown 文件（路径 + 全文）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedFile {
    pub path: String,
    pub content: String,
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
pub fn open_markdown_file(app: tauri::AppHandle) -> Option<OpenedFile> {
    let picked = app
        .dialog()
        .file()
        .add_filter("Markdown", &["md", "markdown"])
        .blocking_pick_file()?;
    let path = picked.into_path().ok()?;
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

/// 列出目录内容：文件夹在前，文件在后，各自按名称排序（sort: "asc" 默认升序 / "desc" 降序；不递归）。
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
    let desc = matches!(sort, Some("desc"));
    let by_name = |a: &DirEntry, b: &DirEntry| {
        let ord = a.name.to_lowercase().cmp(&b.name.to_lowercase());
        if desc { ord.reverse() } else { ord }
    };
    dirs.sort_by(by_name);
    files.sort_by(by_name);
    dirs.extend(files);
    dirs
}

// ---------- 图片加载 ----------

use base64::Engine;
use std::path::PathBuf;

/// 读取本地图片为 data URL（相对路径基于文档目录解析）；远程 http(s) URL 原样返回。
/// 读取失败返回 None（前端显示占位）。
#[tauri::command]
pub fn read_image_data_url(source: &str, base_dir: Option<&str>) -> Option<String> {
    if source.starts_with("http://") || source.starts_with("https://") {
        return Some(source.to_string());
    }
    let path = std::path::Path::new(source);
    let resolved: PathBuf = if path.is_absolute() {
        path.to_path_buf()
    } else {
        PathBuf::from(base_dir?).join(path)
    };
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

/// 保存到当前路径；写失败返回错误信息。
#[tauri::command]
pub fn save_file(path: &str, content: &str) -> Result<(), String> {
    std::fs::write(path, content).map_err(|e| e.to_string())
}

/// 另存为：弹出保存对话框并写入；取消返回 None，写失败返回错误信息。
#[tauri::command]
pub fn save_file_as(app: tauri::AppHandle, content: &str) -> Option<Result<String, String>> {
    let picked = app
        .dialog()
        .file()
        .add_filter("Markdown", &["md", "markdown"])
        .set_file_name("untitled.md")
        .blocking_save_file()?;
    let path = picked.into_path().ok()?;
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
pub fn save_html_as(app: tauri::AppHandle, content: &str, source_name: &str) -> Option<Result<String, String>> {
    let picked = app
        .dialog()
        .file()
        .add_filter("HTML", &["html", "htm"])
        .set_file_name(html_export_name(source_name))
        .blocking_save_file()?;
    let path = picked.into_path().ok()?;
    let path_str = path.to_string_lossy().to_string();
    Some(std::fs::write(&path, content).map(|_| path_str).map_err(|e| e.to_string()))
}

/// 选择文件夹对话框；取消返回 None。
#[tauri::command]
pub fn pick_folder(app: tauri::AppHandle) -> Option<String> {
    let picked = app.dialog().file().blocking_pick_folder()?;
    Some(picked.into_path().ok()?.to_string_lossy().to_string())
}

/// 在当前文件夹创建新的 Markdown 文件（untitled.md，重名自动编号），返回路径。
#[tauri::command]
pub fn create_markdown_file(dir: &str) -> Option<String> {
    let dir = std::path::Path::new(dir);
    if !dir.is_dir() {
        return None;
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
