fn main() {
    // include_dir! does not track files on stable; rebuild when placeholder art changes.
    println!("cargo:rerun-if-changed=../packs/placeholder");
    // App commands get generated `allow-<command>` permissions so each window's
    // capability can allow exactly what it uses (minimal IPC surface).
    let commands: &[&str] = &[
        "get_snapshot",
        "start_focus",
        "start_break",
        "pause",
        "resume",
        "end_session",
        "dismiss",
        "update_settings",
        "set_hotkey",
        "get_history",
        "list_packs",
        "get_active_pack",
        "set_active_pack",
        "replay_animation",
        "show_companion",
        "hide_companion",
        "open_settings",
        "set_hotkey_capture",
        "reset_companion_position",
        "get_app_info",
        "quit_app",
    ];
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(commands)),
    )
    .expect("failed to run tauri-build");
}
