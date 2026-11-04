//! App shell: owns windows, tray, hotkey, ticker, sleep/wake observers, pack
//! protocol and persistence I/O; applies `focus_core::Effect`s. The renderer
//! only renders. See docs/SPEC.md §1, §8.
//!
//! Module map:
//! - `shell`: the one Engine behind one mutex, the lock-disciplined call
//!   pipeline, seq/companion_visible stamping, save and emit gates.
//! - `effects`: applying Outcomes in SPEC §4.9 order; system events; shutdown.
//! - `actions` / `commands`: operations shared by IPC, tray, hotkey; IPC wrappers.
//! - `companion` (+ `panel` on macOS), `settings_window`, `tray`, `hotkey`,
//!   `pack_protocol`, `ticker`, `platform` (macOS integration or no-ops).

mod actions;
mod commands;
mod companion;
mod effects;
mod hotkey;
mod instance_lock;
mod pack_protocol;
#[cfg(target_os = "macos")]
mod panel;
mod platform;
mod settings_window;
mod setup;
mod shell;
mod ticker;
mod tray;

use tauri::plugin::TauriPlugin;
use tauri::{RunEvent, Wry};

/// Every IPC command, in the order of `build.rs` and `generate_handler!` (kept
/// in sync by a test).
pub const COMMANDS: [&str; 22] = [
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
    "take_settings_tab",
    "get_app_info",
    "quit_app",
];

fn log_plugin() -> TauriPlugin<Wry> {
    use tauri_plugin_log::{Builder, RotationStrategy, Target, TargetKind};
    let (level, own) = if cfg!(debug_assertions) {
        (log::LevelFilter::Info, log::LevelFilter::Debug)
    } else {
        (log::LevelFilter::Info, log::LevelFilter::Info)
    };
    let mut builder = Builder::new()
        .clear_targets()
        .target(Target::new(TargetKind::LogDir { file_name: None }))
        .rotation_strategy(RotationStrategy::KeepOne)
        .max_file_size(1_000_000)
        .level(level)
        .level_for("focus_companion_lib", own)
        .level_for("tao", log::LevelFilter::Warn)
        .level_for("wry", log::LevelFilter::Warn);
    if cfg!(debug_assertions) {
        builder = builder.target(Target::new(TargetKind::Stdout));
    }
    builder.build()
}

pub fn run() {
    let builder = tauri::Builder::default()
        // FIRST plugin: a second process exits inside this plugin's setup; the
        // callback runs on another thread and shows the companion.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            actions::set_companion_visible(app, true);
        }))
        .plugin(log_plugin())
        .plugin(hotkey::plugin())
        .register_uri_scheme_protocol(pack_protocol::SCHEME, |ctx, request| {
            pack_protocol::handle(ctx.app_handle(), request.uri().path())
        })
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
            commands::take_settings_tab,
            commands::get_app_info,
            commands::quit_app,
        ]);
    #[cfg(target_os = "macos")]
    let builder = builder
        .plugin(tauri_nspanel::init())
        .activate_ignoring_other_apps(false);
    let builder = builder.setup(|app| setup::run(app.handle()));

    let app = match builder.build(tauri::generate_context!()) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("could not start: {e}");
            std::process::exit(1);
        }
    };
    // Accessory (no Dock icon) before `run`, so tao applies it at launch.
    #[cfg(target_os = "macos")]
    let mut app = app;
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);

    app.run(|handle, event| match event {
        // Closing the settings window must never quit the app.
        RunEvent::ExitRequested {
            code: None, api, ..
        } => api.prevent_exit(),
        // Cmd+Q, logout, tray Quit / quit_app (app.exit): pause + save.
        RunEvent::Exit => effects::shutdown(handle),
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => {
            actions::set_companion_visible(handle, true);
        }
        _ => {}
    });
}

#[cfg(test)]
mod pipeline_tests;
#[cfg(test)]
mod tests;
