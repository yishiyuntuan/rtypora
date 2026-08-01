//! 图片路径解析回归测试：百分号编码的相对路径（Typora 导出风格，
//! 如 回溯算法.assets → %E5%9B%9E%E6%BA%AF%E7%AE%97%E6%B3%95.assets）应能解析到真实文件。

use std::fs;
use std::path::PathBuf;

/// 建一个唯一临时目录，返回其路径（测试结束手动清理）
fn make_temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rtypora-img-test-{}-{}",
        tag,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn percent_encoded_relative_image_resolves() {
    let base = make_temp_dir("pct");
    let assets = base.join("回溯算法.assets");
    fs::create_dir_all(&assets).unwrap();
    let img = assets.join("3ef3d4dfa2a42b89c0ee01155258a365.jpeg");
    fs::write(&img, b"\xff\xd8\xff").unwrap();

    let base_str = base.to_string_lossy().to_string();
    // 原文编码路径（与问题反馈中的 Markdown 一致）
    let encoded = "%E5%9B%9E%E6%BA%AF%E7%AE%97%E6%B3%95.assets/3ef3d4dfa2a42b89c0ee01155258a365.jpeg";
    let resolved = tauri_app_lib::files::resolve_image_path(encoded, Some(&base_str));
    assert_eq!(resolved.as_deref(), Some(img.to_string_lossy().as_ref()));

    // 未编码路径仍然正常
    let plain = "回溯算法.assets/3ef3d4dfa2a42b89c0ee01155258a365.jpeg";
    let resolved = tauri_app_lib::files::resolve_image_path(plain, Some(&base_str));
    assert_eq!(resolved.as_deref(), Some(img.to_string_lossy().as_ref()));

    // 不存在的文件返回 None（前端显示占位）
    assert_eq!(
        tauri_app_lib::files::resolve_image_path("nope/missing.png", Some(&base_str)),
        None
    );

    let _ = fs::remove_dir_all(&base);
}
