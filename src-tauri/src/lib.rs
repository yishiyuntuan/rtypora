#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod markdown;
pub mod files;
pub mod highlight;
pub mod mermaid;
pub mod latex;

use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_autostart::ManagerExt;


#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--flag1", "--flag2"]),
        ))
        .invoke_handler(tauri::generate_handler![
            markdown::parse_markdown,
            markdown::parse_blocks,
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
            files::save_file,
            files::save_file_as,
            files::save_pasted_image,
            files::save_html_as,
            files::pick_folder,
            files::create_markdown_file,
            highlight::highlight_code,
            highlight::code_languages,
            markdown::lcp_offsets,
            markdown::block_template,
            markdown::merge_block_markdown,
            markdown::set_html_to_md,
            markdown::set_callout_unify,
            markdown::format_table_source,
            files::build_export_html,
            mermaid::render_mermaid,
            latex::render_display_math,
            latex::render_inline_math,
            latex::set_math_unicode_font
        ])
        .setup(|app| {
            // 获取自动启动管理器
            let autostart_manager = app.autolaunch();
            // 启用 autostart
            let _ = autostart_manager.enable();
            // 检查 enable 状态
            println!("registered for autostart? {}", autostart_manager.is_enabled().unwrap());
            // 禁用 autostart
            let _ = autostart_manager.disable();

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
