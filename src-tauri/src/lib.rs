#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod markdown;
pub mod files;
pub mod highlight;
pub mod latex;

use tauri_plugin_autostart::MacosLauncher;

// macOS：隐藏/显示原生红绿灯窗口按钮（悬停左上角时显示，偏好 traffic_light_autohide 驱动）；
// 其他平台为空操作，保证前端 invoke 不报错。
#[tauri::command]
fn set_traffic_lights_visible(window: tauri::WebviewWindow, visible: bool) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSWindow, NSWindowButton};
        if let Ok(ptr) = window.ns_window() {
            unsafe {
                let ns_window = &*(ptr as *const NSWindow);
                for button in [
                    NSWindowButton::CloseButton,
                    NSWindowButton::MiniaturizeButton,
                    NSWindowButton::ZoomButton,
                ] {
                    if let Some(btn) = ns_window.standardWindowButton(button) {
                        btn.setHidden(!visible);
                    }
                }
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (window, visible);
}

// 前端主题应用并挂载完成后调用：显示主窗口（配合 visible(false) 创建，消除启动白闪）
#[tauri::command]
fn show_main_window(app: tauri::AppHandle) {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}


#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            markdown::parse_markdown,
            markdown::parse_blocks,
            markdown::parse_blocks_async,
            markdown::parse_markdown_async,
            markdown::text_stats_async,
            markdown::serialize_markdown,
            markdown::toggle_task_markdown,
            markdown::text_stats,
            markdown::detect_block_shortcut,
            markdown::inline_shortcut,
            markdown::inline_html_autoclose,
            markdown::html_container_tag_between,
            markdown::html_closing_tag_at,
            markdown::parse_html_color,
            files::open_markdown_file,
            files::read_markdown_file,
            files::open_markdown_parsed,
            files::read_markdown_parsed,
            files::list_dir,
            files::read_image_data_url,
            files::resolve_image_path,
            files::save_file,
            files::save_file_as,
            files::save_pasted_image,
            files::save_html_as,
            files::pick_folder,
            files::pick_theme_file,
            files::create_markdown_file,
            highlight::highlight_code,
            highlight::code_languages,
            markdown::lcp_offsets,
            markdown::block_template,
            markdown::merge_block_markdown,
            markdown::looks_like_markdown,
            markdown::set_html_to_md,
            markdown::set_callout_unify,
            markdown::format_table_source,
            files::build_export_html,
            latex::render_display_math,
            latex::render_inline_math,
            latex::set_math_unicode_font,
            set_traffic_lights_visible,
            show_main_window
        ])
        .setup(|app| {
            // 窗口按平台创建：macOS 使用原生标题栏（Overlay 样式，原生红绿灯覆盖在左上角，
            // 内容区占满整个窗口，拖拽与菜单按钮由前端标题栏接管）；
            // Windows/Linux 保持无边框，由前端自绘窗口控制按钮。
            // 先隐藏创建（visible=false），前端主题应用并挂载后经 show_main_window 显示——
            // 消除暗色主题下「HTML 到达前原生白底」的启动白闪
            let mut builder = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("tauri-editor")
            .inner_size(1300.0, 750.0)
            .resizable(true)
            .fullscreen(false)
            .visible(false);
            #[cfg(target_os = "macos")]
            {
                builder = builder
                    .decorations(true)
                    .title_bar_style(tauri::TitleBarStyle::Overlay)
                    .hidden_title(true);
            }
            #[cfg(not(target_os = "macos"))]
            {
                builder = builder.decorations(false);
            }
            builder.build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
