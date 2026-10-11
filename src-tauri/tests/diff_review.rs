//! 改动审阅 diff 命令的集成测试：hunk 旧行文本（old_lines）与 Git HEAD 基线命令。
use tauri_app_lib::diff::{diff_saved_content, git_head_content};

#[test]
fn 新增hunk不带旧行() {
    let r = diff_saved_content("a\nb", "a\nx\nb");
    assert_eq!(r.added, 1);
    assert_eq!(r.hunks.len(), 1);
    assert_eq!(r.hunks[0].kind, "added");
    assert!(r.hunks[0].old_lines.is_empty());
}

#[test]
fn 修改hunk携带被替换的旧行() {
    let r = diff_saved_content("a\n旧一\n旧二\nb", "a\n新\nb");
    assert_eq!(r.modified, 1);
    assert_eq!(r.deleted, 1); // 替换溢出的一行计入删除
    assert_eq!(r.hunks.len(), 1);
    assert_eq!(r.hunks[0].kind, "modified");
    assert_eq!(r.hunks[0].old_lines, vec!["旧一", "旧二"]);
}

#[test]
fn 删除hunk携带全部旧行且区间退化为点() {
    let r = diff_saved_content("a\nd1\nd2\nd3\nb", "a\nb");
    assert_eq!(r.deleted, 3);
    assert_eq!(r.hunks.len(), 1);
    assert_eq!(r.hunks[0].kind, "deleted");
    assert_eq!(r.hunks[0].old_lines, vec!["d1", "d2", "d3"]);
    assert_eq!(r.hunks[0].start, r.hunks[0].end);
}

#[test]
fn 中文旧行内容与偏移坐标() {
    let r = diff_saved_content("第一段\n旧中文行\n", "第一段\n新中文行\n末行\n");
    assert_eq!(r.hunks.len(), 1);
    assert_eq!(r.hunks[0].old_lines, vec!["旧中文行"]);
    // 新文本「新中文行」行的 UTF-16 起点 = 「第一段\n」= 4 码元
    assert_eq!(r.hunks[0].start, 4);
}

#[test]
fn git基线_非仓库返回空() {
    // 系统临时目录下的隔离目录不会是 git 仓库
    let dir = std::env::temp_dir().join(format!("tauri-diff-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.md");
    std::fs::write(&file, "hi").unwrap();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let r = rt.block_on(git_head_content(file.to_string_lossy().to_string()));
    assert!(r.is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn git基线_被跟踪文件取到内容() {
    // 本crate所在仓库（git 可用时）：README.md 是被跟踪文件
    let repo_file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../README.md");
    if !repo_file.exists() {
        return; // 无仓库布局（如打包环境）跳过
    }
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let r = rt.block_on(git_head_content(repo_file.to_string_lossy().to_string()));
    let content = r.expect("被跟踪文件应取到 HEAD 内容");
    assert!(!content.is_empty());
}
