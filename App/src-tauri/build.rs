fn main() {
    // 检测目标工具链：gnu 需要特殊处理非 ASCII 路径，msvc 可以直接使用原始路径
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let is_gnu = target_env == "gnu";

    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("icons").join("icon.ico");

    let icon_path = if is_gnu {
        // tauri-build / windres 不能在 MinGW 下处理非 ASCII 路径（项目路径含中文），
        // 把 icon 复制到 ASCII-only 路径并通过 window_icon_path 让它走那里。
        let dst = std::path::Path::new("C:/build/ecoalert-gnu/build-assets/icon.ico");
        if let Some(parent) = dst.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if src.exists() {
            let _ = std::fs::copy(&src, dst);
        }
        dst.to_path_buf()
    } else {
        // MSVC 工具链可以直接处理非 ASCII 路径
        src.clone()
    };

    println!("cargo:rerun-if-changed={}", src.display());
    println!("cargo:rerun-if-changed=build.rs");

    tauri_build::try_build(
        tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new().window_icon_path(&icon_path)),
    )
    .expect("failed to run tauri-build");
}
