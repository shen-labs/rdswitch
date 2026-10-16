fn main() {
    let mut attrs = tauri_build::Attributes::new();
    let is_windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");

    // RustDesk 已安裝為系統服務時，切換伺服器必須以系統管理員身分呼叫 `rustdesk --option`。
    if is_windows {
        attrs = attrs.windows_attributes(
            tauri_build::WindowsAttributes::new().app_manifest(include_str!("windows-admin.manifest")),
        );
    }

    // 測試執行檔（只限測試，不含正式的主程式，它已經有自己的 manifest）：
    // 嵌入宣告 Common-Controls v6 的一般權限 manifest，否則使用 mock 應用程式的測試在 Windows 上
    // 會因找不到新版控制項的函式而無法啟動（STATUS_ENTRYPOINT_NOT_FOUND）。
    if is_windows && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-tests=/MANIFESTINPUT:{dir}/windows-test.manifest");
        println!("cargo:rerun-if-changed=windows-test.manifest");
    }

    tauri_build::try_build(attrs).expect("failed to run tauri-build");
}
