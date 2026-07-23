//! 文件命令集成测试：目录排序（list_dir）与导出文件名推导（html_export_name）。

use tauri_app_lib::files::{html_export_name, list_dir};

#[test]
fn 导出文件名推导() {
    assert_eq!(html_export_name("note.md"), "note.html");
    assert_eq!(html_export_name("note.markdown"), "note.html");
    assert_eq!(html_export_name("Note.MD"), "Note.html");
    assert_eq!(html_export_name("document"), "document.html");
}

#[test]
fn 导出html装配与转义() {
    let html = tauri_app_lib::files::build_export_html("<p>内容</p>", ":root { --t-x: 1px; }", "a<b>&\"标题");
    assert!(html.contains("<title>a&lt;b&gt;&amp;&quot;标题</title>"), "title 必须转义: {html}");
    assert!(html.contains("<p>内容</p>"));
    assert!(html.contains(":root { --t-x: 1px; }"));
    assert!(html.starts_with("<!DOCTYPE html>"));
}

#[test]
fn 目录排序升降序() {
    let dir = std::env::temp_dir().join(format!("tauri-app-listdir-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("b.md"), "").unwrap();
    std::fs::write(dir.join("a.md"), "").unwrap();
    std::fs::create_dir_all(dir.join("zdir")).unwrap();

    let asc = list_dir(dir.to_str().unwrap(), None);
    let desc = list_dir(dir.to_str().unwrap(), Some("desc"));
    std::fs::remove_dir_all(&dir).ok();

    let names: Vec<&str> = asc.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["zdir", "a.md", "b.md"], "文件夹在前，文件按名称升序");
    let names: Vec<&str> = desc.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["zdir", "b.md", "a.md"], "文件夹在前，文件按名称降序");
}
