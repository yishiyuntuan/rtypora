fn main() {
    tauri_build::build();

    // tauri-build 生成的 Windows 资源（含 comctl32 v6 清单）只通过
    // rustc-link-arg-bins 链接，cargo test 的测试程序拿不到清单，启动时会因
    // 加载 comctl32 v5 缺少 TaskDialogIndirect 入口点而失败（0xc0000139）。
    // 这里把同一份资源补链到测试目标。
    #[cfg(windows)]
    {
        let out_dir = std::env::var("OUT_DIR").unwrap();
        let resource = std::path::Path::new(&out_dir).join("libresource.a");
        if resource.exists() {
            println!("cargo:rustc-link-arg-tests={}", resource.display());
        }
    }
}
