fn main() {
    // tauri-build / windres 不能在 MinGW 下处理非 ASCII 路径（项目路径含中文），
    // 这里把 icon 复制到 ASCII-only 路径并通过 window_icon_path 让它走那里。
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("icons").join("icon.ico");
    let dst = std::path::Path::new("C:/build/ecoalert-gnu/build-assets/icon.ico");
    if let Some(parent) = dst.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if src.exists() {
        let _ = std::fs::copy(&src, dst);
    }
    println!("cargo:rerun-if-changed={}", src.display());
    println!("cargo:rerun-if-changed=build.rs");

    tauri_build::try_build(
        tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new().window_icon_path(dst)),
    )
    .expect("failed to run tauri-build");
}
