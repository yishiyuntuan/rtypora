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

#[test]
fn 按名称创建文件() {
    use tauri_app_lib::files::create_markdown_file;
    let dir = std::env::temp_dir().join(format!("tauri-app-create-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir_str = dir.to_str().unwrap().to_string();

    // 自动补 .md 后缀
    let path = create_markdown_file(&dir_str, Some("笔记".into())).expect("应创建成功");
    assert!(path.ends_with("笔记.md"), "自动补后缀: {path}");
    // 已有 .md 不重复补
    let path2 = create_markdown_file(&dir_str, Some("doc.md".into())).expect("应创建成功");
    assert!(path2.ends_with("doc.md"));
    // 重名拒绝（不覆盖）
    assert!(create_markdown_file(&dir_str, Some("笔记".into())).is_none());
    // 路径分隔符被剔除（落盘在当前目录）
    let path3 = create_markdown_file(&dir_str, Some("../evil".into())).expect("应创建成功");
    assert!(path3.starts_with(&dir_str), "不得逃逸目录: {path3}");
    assert!(path3.ends_with("evil.md"));
    // 空白名拒绝
    assert!(create_markdown_file(&dir_str, Some("   ".into())).is_none());
    // 未指定名称走 untitled 自动编号
    let path4 = create_markdown_file(&dir_str, None).expect("应创建成功");
    assert!(path4.ends_with("untitled.md"));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn 大文件渐进打开_首屏块与尾部接缝自愈() {
    use tauri_app_lib::markdown::parse_blocks;
    // 构造 >128KB 的 ASCII 文档（空行分隔段落 + 末尾代码围栏；ASCII 下 UTF-16 偏移=字节偏移）
    let mut doc = String::new();
    for i in 0..3000 {
        doc.push_str(&format!("paragraph {i} with some text to fill the line properly\n\n"));
    }
    doc.push_str("```rust\nfn main() {}\n\n// tail comment\n```\n\nlast paragraph\n");
    let dir = std::env::temp_dir().join(format!("tauri-app-parsed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("big.md");
    std::fs::write(&path, &doc).unwrap();

    let opened = tauri_app_lib::files::read_markdown_parsed(path.to_str().unwrap()).expect("应读取成功");
    let tail_from = opened.tail_from.expect("大文件应有尾部偏移");
    assert!(!opened.blocks.is_empty(), "首屏块不应为空");
    // tail_from = 首屏末块起点（末块可能被截断，尾部从它重解析）
    let last = opened.blocks.last().unwrap();
    assert_eq!(Some(tail_from), last.start);

    // 前端流程模拟：尾部重解析（偏移换算为绝对偏移）并原位替换末块
    let tail: Vec<_> = parse_blocks(&doc[tail_from..])
        .into_iter()
        .map(|mut b| {
            b.start = b.start.map(|s| s + tail_from);
            b.end = b.end.map(|e| e + tail_from);
            b
        })
        .collect();
    let mut combined: Vec<_> = opened.blocks[..opened.blocks.len() - 1].to_vec();
    combined.extend(tail);

    // 不变量：拼接结果的块区间序列必须与全量解析一致（接缝无错位/无丢块）
    let full = parse_blocks(&doc);
    assert_eq!(combined.len(), full.len(), "拼接后块数应与全量解析一致");
    for (a, b) in combined.iter().zip(full.iter()) {
        assert_eq!(a.start, b.start, "块起点不一致");
        assert_eq!(a.end, b.end, "块终点不一致");
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn 小文件渐进打开_无尾部() {
    let doc = "# 标题\n\n正文\n";
    let dir = std::env::temp_dir().join(format!("tauri-app-parsed-small-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("small.md");
    std::fs::write(&path, doc).unwrap();

    let opened = tauri_app_lib::files::read_markdown_parsed(path.to_str().unwrap()).expect("应读取成功");
    assert!(opened.tail_from.is_none(), "小文件无尾部");
    assert_eq!(opened.blocks.len(), tauri_app_lib::markdown::parse_blocks(doc).len());
    // 非 Markdown 文件拒绝
    assert!(tauri_app_lib::files::read_markdown_parsed(path.with_extension("txt").to_str().unwrap()).is_none());
    std::fs::remove_dir_all(&dir).ok();
}
