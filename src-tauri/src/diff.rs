//! 改动审阅：当前内容与比较基线（上次保存/Git HEAD/手动快照）的行级 diff（similar Myers 算法）。
//! 输出新文本侧的 UTF-16 区间（与块偏移同一约定）+ 被删/被改行的旧文本（「显示修改前内容」用），
//! 前端据此把变更映射到根块并渲染条纹 / 概览尺 / 旧内容块。Rust 端无状态，基线与当前内容都由前端传入。

use serde::Serialize;
use similar::{capture_diff_slices, Algorithm, DiffOp};

/// 单个变更块（连续的非 Equal 操作归并为一个 hunk）。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChangeHunk {
    /// "added"（纯新增）/ "modified"（新旧都有）/ "deleted"（纯删除）
    pub kind: &'static str,
    /// 变更在新文本中的 UTF-16 起始偏移；纯删除时为删除点位置
    pub start: usize,
    /// UTF-16 结束偏移（不含）；纯删除时 == start
    pub end: usize,
    /// 该 hunk 删除的旧行数（前端「-N」提示用）
    pub deleted_lines: usize,
    /// 被删除/被替换的旧行文本（「显示修改前内容」；纯新增为空数组）
    pub old_lines: Vec<String>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DiffSummary {
    pub hunks: Vec<ChangeHunk>,
    /// 新增行数（纯新增 hunk 的新行数合计）
    pub added: usize,
    /// 修改行数（替换 hunk 的新行数合计）
    pub modified: usize,
    /// 删除行数（纯删除 hunk 的旧行数 + 替换 hunk 溢出的旧行数）
    pub deleted: usize,
}

/// 行级 diff：old 为基线，new 为当前内容。
/// 行切分用 split('\n')（内容与换行符分开比较，与 line_utf16_starts 的口径一致）。
fn diff_lines(old: &str, new: &str) -> DiffSummary {
    let old_lines: Vec<&str> = old.split('\n').collect();
    let new_lines: Vec<&str> = new.split('\n').collect();

    // 新文本每行的 UTF-16 起始偏移 + 总长（单趟扫描）
    let mut starts = Vec::with_capacity(new_lines.len());
    let mut offset = 0usize;
    for line in &new_lines {
        starts.push(offset);
        offset += line.encode_utf16().count() + 1;
    }
    let total = offset - 1; // 换行比行数少一个
    let line_start = |idx: usize| -> usize {
        if idx < starts.len() { starts[idx] } else { total }
    };

    let ops = capture_diff_slices(Algorithm::Myers, &old_lines, &new_lines);
    let mut summary = DiffSummary { hunks: Vec::new(), added: 0, modified: 0, deleted: 0 };
    // 归并连续的 Delete/Insert/Replace 为一个 hunk（Equal 断开）；capture 已把相邻删+插合成 Replace
    // 累加器：(新起始行, 新结束行, 旧行文本累计, 插入行数)
    let mut cur: Option<(usize, usize, Vec<String>, usize)> = None;
    for op in &ops {
        let (ns, ne, old_range, ins) = match op {
            DiffOp::Equal { .. } => {
                if let Some((a, b, o, i)) = cur.take() {
                    push_hunk(&mut summary, a, b, o, i, &line_start);
                }
                continue;
            }
            DiffOp::Delete { old_index, old_len, new_index, .. } => {
                (*new_index, *new_index, Some((*old_index, *old_len)), 0)
            }
            DiffOp::Insert { new_index, new_len, .. } => (*new_index, *new_index + *new_len, None, *new_len),
            DiffOp::Replace { old_index, old_len, new_index, new_len, .. } => {
                (*new_index, *new_index + *new_len, Some((*old_index, *old_len)), *new_len)
            }
        };
        match &mut cur {
            Some((_, b, o, i)) => {
                *b = (*b).max(ne);
                if let Some((oi, ol)) = old_range {
                    o.extend(old_lines[oi..oi + ol].iter().map(|s| s.to_string()));
                }
                *i += ins;
            }
            None => {
                let olds = match old_range {
                    Some((oi, ol)) => old_lines[oi..oi + ol].iter().map(|s| s.to_string()).collect(),
                    None => Vec::new(),
                };
                cur = Some((ns, ne, olds, ins));
            }
        }
    }
    if let Some((a, b, o, i)) = cur.take() {
        push_hunk(&mut summary, a, b, o, i, &line_start);
    }
    summary
}

fn push_hunk(
    summary: &mut DiffSummary,
    new_start_line: usize,
    new_end_line: usize,
    old_lines: Vec<String>,
    ins: usize,
    line_start: &dyn Fn(usize) -> usize,
) {
    let del = old_lines.len();
    let kind = if del > 0 && ins > 0 { "modified" } else if ins > 0 { "added" } else { "deleted" };
    // 纯删除时新区间为空：退化为删除点（落在后一行行首，末行删除时落在全文末尾）
    let start = line_start(new_start_line);
    let end = if ins > 0 { line_start(new_end_line) } else { start };
    match kind {
        "added" => summary.added += ins,
        "deleted" => summary.deleted += del,
        _ => {
            summary.modified += ins;
            if del > ins {
                summary.deleted += del - ins;
            }
        }
    }
    summary.hunks.push(ChangeHunk { kind, start, end, deleted_lines: del, old_lines });
}

#[tauri::command]
pub fn diff_saved_content(old: &str, new: &str) -> DiffSummary {
    diff_lines(old, new)
}

/// 异步版本（大文档 diff 不阻塞主线程 IPC，与 text_stats_async 同理）。
#[tauri::command]
pub async fn diff_saved_content_async(old: String, new: String) -> DiffSummary {
    diff_lines(&old, &new)
}

/// Git HEAD 基线：取文件在最新提交中的内容（统一规范化 CRLF/CR→LF，与编辑器内存模型一致）。
/// 非 Git 仓库 / 文件未被跟踪 / git 不可用 → None（前端禁用该基线项）。
/// 实现走 git CLI（rev-parse 定位仓库根 + show HEAD:相对路径），不引入 git2 依赖。
#[tauri::command]
pub async fn git_head_content(path: String) -> Option<String> {
    use std::process::Command;
    let p = std::path::Path::new(&path);
    let dir = p.parent()?;
    // canonicalize 消除符号链接/相对成分，保证 strip_prefix 与 git 输出可比较
    let file_abs = std::fs::canonicalize(p).ok()?;
    let top = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(dir)
        .output()
        .ok()?;
    if !top.status.success() {
        return None;
    }
    let top_path = std::fs::canonicalize(String::from_utf8_lossy(&top.stdout).trim()).ok()?;
    let rel = file_abs.strip_prefix(&top_path).ok()?;
    // git 路径固定正斜杠
    let rel_str = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
    let show = Command::new("git").args(["show", &format!("HEAD:{rel_str}")]).current_dir(dir).output().ok()?;
    if !show.status.success() {
        return None; // 未跟踪（HEAD 中不存在）
    }
    let text = String::from_utf8(show.stdout).ok()?;
    Some(if text.contains('\r') { text.replace("\r\n", "\n").replace('\r', "\n") } else { text })
}
