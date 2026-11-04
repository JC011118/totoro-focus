//! IPC commands. Names, argument names and return types are the contract with
//! the renderer (docs/SPEC.md §10); the frontend calls them in camelCase
//! (`startFocus({ minutes, taskLabel })`). Every command that changes state
//! returns the fresh Snapshot, and the same Snapshot is also broadcast as
//! `focus://snapshot` to every window.

use focus_core::engine::CommandError;
use focus_core::history::SessionRecord;
use focus_core::packs::{PackInfo, ResolvedPack};
use focus_core::settings::SettingsPatch;
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

#[tauri::command]
pub fn update_settings(app: AppHandle, patch: SettingsPatch) -> CmdResult<Snapshot> {
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

/// Persists `settings.activePack` and broadcasts `focus://pack-changed`.
#[tauri::command]
pub fn set_active_pack(app: AppHandle, id: String) -> CmdResult<ResolvedPack> {
    let _ = (app, id);
    todo!("shell: set_active_pack")
}

/// Emits `focus://play {key}` to the companion. Refused (`SessionActive`) while a
/// focus session is running: unlocks never play during active focus.
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

/// Opens (or focuses) the settings window, optionally on `tab`
/// (`general` | `history` | `companion` | `start`); emits `focus://open-tab`.
#[tauri::command]
pub fn open_settings(app: AppHandle, tab: Option<String>) {
    let _ = (app, tab);
    todo!("shell: open_settings")
}

/// The companion's task-label field gained (true) or lost (false) focus. On macOS
/// the panel may become key only while this is true, so ordinary button clicks
/// never take keyboard focus from the frontmost app.
#[tauri::command]
pub fn set_text_input_active(app: AppHandle, active: bool) {
    let _ = (app, active);
    todo!("shell: set_text_input_active")
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
