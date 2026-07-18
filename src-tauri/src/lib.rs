#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod markdown;

use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_autostart::ManagerExt;


#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--flag1", "--flag2"]),
        ))
        .invoke_handler(tauri::generate_handler![markdown::parse_markdown])
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
