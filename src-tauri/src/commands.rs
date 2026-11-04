//! IPC commands. Names, argument names and return types are the contract with
//! the renderer (docs/SPEC.md §10); the frontend calls them in camelCase
//! (`startFocus({ minutes, taskLabel })`). Every command that changes state
//! returns the fresh Snapshot, and the same Snapshot is also broadcast as
//! `focus://snapshot` to every window.

use focus_core::engine::CommandError;
use focus_core::history::SessionRecord;
use focus_core::packs::{PackInfo, ResolvedPack};
use focus_core::events::SettingsTab;
use focus_core::settings::UserSettingsPatch;
use focus_core::snapshot::{AppInfo, Snapshot};
use tauri::AppHandle;

type CmdResult<T> = Result<T, CommandError>;

#[tauri::command]
pub fn get_snapshot(app: AppHandle) -> Snapshot {
    let _ = app;
    todo!("shell: get_snapshot")
}

#[tauri::command]
pub fn start_focus(app: AppHandle, minutes: Option<u32>, task_label: Option<String>) -> CmdResult<Snapshot> {
    let _ = (app, minutes, task_label);
    todo!("shell: start_focus")
}

#[tauri::command]
pub fn start_break(app: AppHandle, minutes: Option<u32>) -> CmdResult<Snapshot> {
    let _ = (app, minutes);
    todo!("shell: start_break")
}

#[tauri::command]
pub fn pause(app: AppHandle) -> CmdResult<Snapshot> {
    let _ = app;
    todo!("shell: pause")
}

#[tauri::command]
pub fn resume(app: AppHandle) -> CmdResult<Snapshot> {
    let _ = app;
    todo!("shell: resume")
}

#[tauri::command]
pub fn end_session(app: AppHandle) -> CmdResult<Snapshot> {
    let _ = app;
    todo!("shell: end_session")
}

#[tauri::command]
pub fn dismiss(app: AppHandle) -> CmdResult<Snapshot> {
    let _ = app;
    todo!("shell: dismiss")
}

/// Renderer-editable settings only (hotkey, pack and position have dedicated paths).
#[tauri::command]
pub fn update_settings(app: AppHandle, patch: UserSettingsPatch) -> CmdResult<Snapshot> {
    let _ = (app, patch);
    todo!("shell: update_settings")
}

/// `None` disables the hotkey. Invalid or unregistrable accelerators return
/// `InvalidSettings` and keep the previous binding.
#[tauri::command]
pub fn set_hotkey(app: AppHandle, accelerator: Option<String>) -> CmdResult<Snapshot> {
    let _ = (app, accelerator);
    todo!("shell: set_hotkey")
}

/// While the settings hotkey recorder is active the global shortcut is unregistered
/// (restored on false, on settings close, or after 30 s).
#[tauri::command]
pub fn set_hotkey_capture(app: AppHandle, active: bool) {
    let _ = (app, active);
    todo!("shell: set_hotkey_capture")
}

/// Newest first; default limit 50, max 500.
#[tauri::command]
pub fn get_history(app: AppHandle, limit: Option<u32>) -> Vec<SessionRecord> {
    let _ = (app, limit);
    todo!("shell: get_history")
}

/// Rescans pack directories.
#[tauri::command]
pub fn list_packs(app: AppHandle) -> Vec<PackInfo> {
    let _ = app;
    todo!("shell: list_packs")
}

#[tauri::command]
pub fn get_active_pack(app: AppHandle) -> ResolvedPack {
    let _ = app;
    todo!("shell: get_active_pack")
}

/// Persists `settings.activePack`, broadcasts `focus://pack-changed` and a snapshot.
/// Unknown or invalid ids → `PackUnavailable`.
#[tauri::command]
pub fn set_active_pack(app: AppHandle, id: String) -> CmdResult<ResolvedPack> {
    let _ = (app, id);
    todo!("shell: set_active_pack")
}

/// Validated by `Engine::check_replay` (`FocusRunning`, `NotUnlocked`); shows the
/// companion if hidden, then emits `focus://play {key}` to the companion only.
#[tauri::command]
pub fn replay_animation(app: AppHandle, key: String) -> CmdResult<()> {
    let _ = (app, key);
    todo!("shell: replay_animation")
}

#[tauri::command]
pub fn show_companion(app: AppHandle) {
    let _ = app;
    todo!("shell: show_companion")
}

#[tauri::command]
pub fn hide_companion(app: AppHandle) {
    let _ = app;
    todo!("shell: hide_companion")
}

/// Moves the companion to the default spot on the primary display and saves it.
#[tauri::command]
pub fn reset_companion_position(app: AppHandle) {
    let _ = app;
    todo!("shell: reset_companion_position")
}

/// Opens (creating on demand, `settings.html#<tab>`) and activates the settings
/// window; if it already exists, emits `focus://open-tab` to it.
#[tauri::command]
pub fn open_settings(app: AppHandle, tab: Option<SettingsTab>) {
    let _ = (app, tab);
    todo!("shell: open_settings")
}

#[tauri::command]
pub fn get_app_info(app: AppHandle) -> AppInfo {
    let _ = app;
    todo!("shell: get_app_info")
}

/// Pauses and checkpoints any session, then exits.
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    let _ = app;
    todo!("shell: quit_app")
}
