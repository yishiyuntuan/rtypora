use tauri_app_lib::markdown::parse_blocks;
#[test]
fn probe() {
    for (name, src) in [
        ("末行2空格", "1. aaaaaaa\n2. bbbbbb\n3. cccccc\n  4. ddddd"),
        ("末行1空格", "1. aaaaaaa\n2. bbbbbb\n3. cccccc\n 4. ddddd"),
        ("空行分隔+末行2空格", "1. aaaaaaa\n\n2. bbbbbb\n\n3. cccccc\n\n  4. ddddd"),
    ] {
        println!("=== {} ===", name);
        for b in parse_blocks(src) {
            println!("kind={:?} children={:?} title={:?}", b.kind,
                b.children.iter().map(|c| format!("{:?}", c.kind)).collect::<Vec<_>>(),
                b.title.fragments.iter().map(|f| f.text.as_str()).collect::<Vec<_>>());
        }
    }
}
