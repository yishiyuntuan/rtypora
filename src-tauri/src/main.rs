// 防止 Windows 发布版弹出额外的控制台窗口，勿删除！！
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// 薄入口：全部应用逻辑在 tauri_app_lib（lib.rs），便于集成测试引用 crate
fn main() {
    tauri_app_lib::run()
}
