fn main() {
    // Only the commands listed here get generated `allow-*` permissions; anything
    // not granted in `capabilities/` is unreachable from the webview.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "get_settings",
            "update_settings",
            "get_platform_info",
            "hide_picker",
            "open_settings",
        ]),
    ))
    .expect("failed to run tauri-build");
}
