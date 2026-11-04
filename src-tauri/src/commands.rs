//! IPC commands. Names, argument names and return types are the contract with
//! the renderer (docs/SPEC.md §10); the frontend calls them in camelCase
//! (`startFocus({ minutes, taskLabel })`). Every command that changes state
//! returns the fresh Snapshot, and the same Snapshot is also broadcast as
//! `focus://snapshot` to every window.
//!
//! These are synchronous commands, so they run on the main thread: each one
//! follows the engine lock discipline through `actions` / `Shell::call`.

use focus_core::engine::{Command, CommandError, SystemEvent};
use focus_core::events::SettingsTab;
use focus_core::history::SessionRecord;
use focus_core::packs::{PackInfo, ResolvedPack};
use focus_core::settings::UserSettingsPatch;
use focus_core::snapshot::{AppInfo, Snapshot};
use tauri::{AppHandle, Manager};

use crate::shell::Shell;
use crate::{actions, effects, hotkey};

type CmdResult<T> = Result<T, CommandError>;

/// Runs a Tick first (SPEC §4.7), applies its effects, and returns its snapshot.
#[tauri::command]
pub fn get_snapshot(app: AppHandle) -> Snapshot {
    effects::system_event(&app, SystemEvent::Tick)
        .unwrap_or_else(|| app.state::<Shell>().snapshot_now())
}

#[tauri::command]
pub fn start_focus(
    app: AppHandle,
    minutes: Option<u32>,
    task_label: Option<String>,
) -> CmdResult<Snapshot> {
    actions::run(
        &app,
        Command::StartFocus {
            minutes,
            task_label,
        },
    )
}

#[tauri::command]
pub fn start_break(app: AppHandle, minutes: Option<u32>) -> CmdResult<Snapshot> {
    actions::run(&app, Command::StartBreak { minutes })
}

#[tauri::command]
pub fn pause(app: AppHandle) -> CmdResult<Snapshot> {
    actions::run(&app, Command::Pause)
}

#[tauri::command]
pub fn resume(app: AppHandle) -> CmdResult<Snapshot> {
    actions::run(&app, Command::Resume)
}

#[tauri::command]
pub fn end_session(app: AppHandle) -> CmdResult<Snapshot> {
    actions::run(&app, Command::End)
}

#[tauri::command]
pub fn dismiss(app: AppHandle) -> CmdResult<Snapshot> {
    actions::run(&app, Command::Dismiss)
}

/// Renderer-editable settings only (hotkey, pack and position have dedicated paths).
#[tauri::command]
pub fn update_settings(app: AppHandle, patch: UserSettingsPatch) -> CmdResult<Snapshot> {
    actions::run(&app, Command::UpdateSettings(patch.into()))
}

/// `None` disables the hotkey. Invalid or unregistrable accelerators return
/// `InvalidSettings` and keep the previous binding.
#[tauri::command]
pub fn set_hotkey(app: AppHandle, accelerator: Option<String>) -> CmdResult<Snapshot> {
    hotkey::set(&app, accelerator)
}

/// While the settings hotkey recorder is active the global shortcut is unregistered
/// (restored on false, on settings close, or after 30 s).
#[tauri::command]
pub fn set_hotkey_capture(app: AppHandle, active: bool) {
    hotkey::set_capture(&app, active);
}

/// Newest first; default limit 50, max 500.
#[tauri::command]
pub fn get_history(app: AppHandle, limit: Option<u32>) -> Vec<SessionRecord> {
    actions::history(&app, limit)
}

/// Rescans pack directories.
#[tauri::command]
pub fn list_packs(app: AppHandle) -> Vec<PackInfo> {
    actions::rescan_packs(&app)
}

#[tauri::command]
pub fn get_active_pack(app: AppHandle) -> ResolvedPack {
    actions::active_pack(&app)
}

/// Persists `settings.activePack`, broadcasts `focus://pack-changed` and a snapshot.
/// Unknown or invalid ids → `PackUnavailable`.
#[tauri::command]
pub fn set_active_pack(app: AppHandle, id: String) -> CmdResult<ResolvedPack> {
    actions::set_active_pack(&app, id)
}

/// Validated by `Engine::check_replay` (`FocusRunning`, `NotUnlocked`); shows the
/// companion if hidden, then emits `focus://play {key}` to the companion only.
#[tauri::command]
pub fn replay_animation(app: AppHandle, key: String) -> CmdResult<()> {
    actions::replay(&app, key)
}

#[tauri::command]
pub fn show_companion(app: AppHandle) {
    actions::set_companion_visible(&app, true);
}

#[tauri::command]
pub fn hide_companion(app: AppHandle) {
    actions::set_companion_visible(&app, false);
}

/// Moves the companion to the default spot on the primary display and saves it.
#[tauri::command]
pub fn reset_companion_position(app: AppHandle) {
    actions::reset_companion_position(&app);
}

/// Opens (creating on demand, `settings.html#<tab>`) and activates the settings
/// window; if it already exists, emits `focus://open-tab` to it.
#[tauri::command]
pub fn open_settings(app: AppHandle, tab: Option<SettingsTab>) {
    actions::open_settings(&app, tab.unwrap_or_default());
}

/// The tab most recently requested by `open_settings` (then cleared). The
/// settings page calls this once its `focus://open-tab` listener is live, so a
/// request made while the page was still loading is not lost.
#[tauri::command]
pub fn take_settings_tab() -> Option<SettingsTab> {
    crate::settings_window::take_requested_tab()
}

#[tauri::command]
pub fn get_app_info(app: AppHandle) -> AppInfo {
    actions::app_info(&app)
}

/// Pauses and checkpoints any session, then exits (the pause and the save
/// happen in `RunEvent::Exit` → `effects::shutdown`).
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}
