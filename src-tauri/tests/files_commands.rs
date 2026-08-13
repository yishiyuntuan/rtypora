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
fn 目录按修改时间排序() {
    let dir = std::env::temp_dir().join(format!("tauri-app-listdir-time-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let old_path = dir.join("old.md");
    let new_path = dir.join("new.md");
    std::fs::write(&old_path, "").unwrap();
    // 把 old 的修改时间拨到过去，确保序关系确定
    let past = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    filetime::FileTime::from_system_time(past);
    std::fs::write(&new_path, "").unwrap();
    filetime::set_file_mtime(&old_path, filetime::FileTime::from_system_time(past)).unwrap();

    let desc = list_dir(dir.to_str().unwrap(), Some("modified_desc"));
    let asc = list_dir(dir.to_str().unwrap(), Some("modified_asc"));
    std::fs::remove_dir_all(&dir).ok();

    let names: Vec<&str> = desc.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["new.md", "old.md"], "最近修改在前");
    let names: Vec<&str> = asc.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["old.md", "new.md"], "最早修改在前");
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
    assert_eq!(opened.line_ending, "lf");
    assert_eq!(opened.blocks.len(), tauri_app_lib::markdown::parse_blocks(doc).len());
    // 非 Markdown 文件拒绝
    assert!(tauri_app_lib::files::read_markdown_parsed(path.with_extension("txt").to_str().unwrap()).is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn 换行符规范化_crlf文档() {
    // CRLF 为主的 Windows 风格文档：内容规范化为 LF、风格记录为 crlf、解析不受影响
    let doc = "# 标题\r\n\r\n第一行\r\n第二行\r\n\r\n- 列表项\r\n\r\n```rust\r\nfn a() {}\r\n```\r\n";
    let dir = std::env::temp_dir().join(format!("tauri-app-crlf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("win.md");
    std::fs::write(&path, doc).unwrap();

    let opened = tauri_app_lib::files::read_markdown_parsed(path.to_str().unwrap()).expect("应读取成功");
    assert_eq!(opened.line_ending, "crlf", "CRLF 居多应记录为 crlf");
    assert!(!opened.content.contains('\r'), "内容必须规范化为 LF");
    // 与 LF 原文解析结果一致（块数与区间）
    let lf_doc = doc.replace("\r\n", "\n");
    let full = tauri_app_lib::markdown::parse_blocks(&lf_doc);
    assert_eq!(opened.blocks.len(), full.len(), "CRLF 规范化后块数应与 LF 解析一致");
    for (a, b) in opened.blocks.iter().zip(full.iter()) {
        assert_eq!(a.start, b.start);
        assert_eq!(a.end, b.end);
    }

    // LF 为主的文档记录为 lf
    let path2 = dir.join("unix.md");
    std::fs::write(&path2, "a\nb\r\nc\n").unwrap();
    let opened2 = tauri_app_lib::files::read_markdown_parsed(path2.to_str().unwrap()).expect("应读取成功");
    assert_eq!(opened2.line_ending, "lf", "LF 居多应记录为 lf");
    assert!(!opened2.content.contains('\r'));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn 粘贴图片路径与扩展名净化() {
    use tauri_app_lib::files::save_pasted_image;
    let dir = std::env::temp_dir().join(format!("tauri-app-paste-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir_str = dir.to_str().unwrap().to_string();
    let bytes = vec![1u8, 2, 3, 4];

    // 目录穿越：.. 组件 / 绝对路径一律拒绝
    assert!(save_pasted_image(bytes.clone(), &dir_str, Some("../escape"), None).is_none());
    assert!(save_pasted_image(bytes.clone(), &dir_str, Some("a/../../escape"), None).is_none());
    assert!(!dir.parent().unwrap().join("escape").exists(), "不得写出目标目录之外");
    // 扩展名净化：非 ASCII 字母数字剔除（防 x/../y 形式的文件名穿越）
    let ok = save_pasted_image(bytes.clone(), &dir_str, None, Some("p\"g/../x")).expect("净化后应成功");
    assert!(ok.starts_with("./paste-") && ok.ends_with(".pgx"), "扩展名仅保留字母数字: {ok}");
    // 正常 assets 子目录与扩展名
    let ok2 = save_pasted_image(bytes, &dir_str, Some("assets"), Some("jpeg")).expect("assets 子目录应成功");
    assert!(ok2.starts_with("./assets/paste-") && ok2.ends_with(".jpeg"), "{ok2}");
    assert!(dir.join("assets").is_dir(), "子目录已创建");
    // 空字节拒绝
    assert!(save_pasted_image(Vec::new(), &dir_str, None, None).is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn 图片dataurl大小上限() {
    use tauri_app_lib::files::read_image_data_url;
    let dir = std::env::temp_dir().join(format!("tauri-app-imgcap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // 超过 64MB 上限的文件拒绝读取（防内存 DoS）；小文件正常
    let big = dir.join("big.png");
    std::fs::write(&big, vec![0u8; 65 * 1024 * 1024]).unwrap();
    assert!(read_image_data_url(big.to_str().unwrap(), None).is_none(), "超限拒绝");
    let small = dir.join("small.png");
    std::fs::write(&small, b"\x89PNG").unwrap();
    let url = read_image_data_url(small.to_str().unwrap(), None).expect("小文件正常读取");
    assert!(url.starts_with("data:image/png;base64,"), "{url}");
    std::fs::remove_dir_all(&dir).ok();
}
