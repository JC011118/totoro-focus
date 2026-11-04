//! Non-macOS stand-ins. The companion is a plain always-on-top window there;
//! sleep/wake, App Nap and the chime are not wired up.

use focus_core::engine::ActivityLevel;
use tauri::tray::TrayIcon;
use tauri::{AppHandle, WebviewWindow};

use super::activity_for;

pub fn install_observers(_app: &AppHandle) {
    log::info!("sleep/wake, clock-change and display observers are macOS-only");
}

pub fn set_activity(level: ActivityLevel) {
    log::debug!(
        "activity {level:?} → {:?} (no-op on this OS)",
        activity_for(level)
    );
}

pub fn play_chime(_app: &AppHandle) {
    log::debug!("chime (no sound on this OS)");
}

pub fn style_tray(_tray: &TrayIcon) {}

/// App-level hide is a macOS concept.
pub fn exempt_from_app_hide(_window: &WebviewWindow) {}

/// Panel mode is never selected off macOS (`WindowMode::Other`).
pub fn make_panel(_window: &WebviewWindow) -> tauri::Result<()> {
    log::warn!("panel mode requested on a non-macOS build; using a plain window");
    Ok(())
}

pub fn panel_set_visible(_app: &AppHandle, _visible: bool) {}
