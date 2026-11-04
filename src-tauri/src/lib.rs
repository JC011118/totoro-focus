//! App shell: owns windows, tray, hotkey, ticker, sleep/wake observers, pack
//! protocol and persistence I/O; applies `focus_core::Effect`s. The renderer
//! only renders. See docs/SPEC.md §1, §8.

mod commands;

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::start_focus,
            commands::start_break,
            commands::pause,
            commands::resume,
            commands::end_session,
            commands::dismiss,
            commands::update_settings,
            commands::set_hotkey,
            commands::get_history,
            commands::list_packs,
            commands::get_active_pack,
            commands::set_active_pack,
            commands::replay_animation,
            commands::show_companion,
            commands::hide_companion,
            commands::open_settings,
            commands::set_hotkey_capture,
            commands::reset_companion_position,
            commands::get_app_info,
            commands::quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the app");
}
