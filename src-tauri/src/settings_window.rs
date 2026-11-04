//! The settings window (SPEC §8.2): label `settings`, URL
//! `settings.html#<tab>`, created on demand, activated with `set_focus()`
//! (activation is intended here, unlike the companion) and destroyed on close.
//! `focus://open-tab` is emitted only when the window already exists; a new
//! window reads its initial tab from `location.hash`.

use focus_core::events::{self, OpenTabEvent, SettingsTab, SETTINGS_WINDOW};
use tauri::{
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

use crate::hotkey;

pub const SIZE: (f64, f64) = (560.0, 640.0);
pub const MIN_SIZE: (f64, f64) = (460.0, 480.0);

pub fn url(tab: SettingsTab) -> String {
    format!("settings.html#{}", tab.as_str())
}

pub fn title(app_name: &str) -> String {
    format!("{app_name} Settings")
}

/// Open (or focus) the settings window on `tab`.
pub fn open(app: &AppHandle, tab: SettingsTab) {
    if let Some(window) = app.get_webview_window(SETTINGS_WINDOW) {
        focus_existing(app, &window, tab);
        return;
    }
    // Callers are often on the main thread inside a sync command or a menu
    // event; build the webview from a helper thread (which blocks on the main
    // thread, holding no lock) rather than nesting window creation in them.
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("settings-window".into())
        .spawn(move || create(&app, tab));
    if let Err(e) = spawned {
        log::error!("could not open settings: {e}");
    }
}

fn focus_existing(app: &AppHandle, window: &WebviewWindow, tab: SettingsTab) {
    let _ = window.show();
    let _ = window.unminimize();
    if let Err(e) = window.set_focus() {
        log::warn!("focusing settings failed: {e}");
    }
    if let Err(e) = app.emit_to(SETTINGS_WINDOW, events::OPEN_TAB, OpenTabEvent { tab }) {
        log::warn!("emitting open-tab failed: {e}");
    }
}

fn create(app: &AppHandle, tab: SettingsTab) {
    let built = WebviewWindowBuilder::new(app, SETTINGS_WINDOW, WebviewUrl::App(url(tab).into()))
        .title(title(&app.package_info().name))
        .inner_size(SIZE.0, SIZE.1)
        .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
        .resizable(true)
        .visible(true)
        .focused(true)
        .build();
    match built {
        Ok(window) => {
            let handle = app.clone();
            window.on_window_event(move |event| {
                if matches!(event, WindowEvent::Destroyed) {
                    // Closing settings ends a hotkey recording (restores the binding).
                    hotkey::end_capture(&handle);
                }
            });
            if let Err(e) = window.set_focus() {
                log::warn!("focusing settings failed: {e}");
            }
        }
        Err(e) => match app.get_webview_window(SETTINGS_WINDOW) {
            // Lost a race with another open request: just focus it.
            Some(window) => focus_existing(app, &window, tab),
            None => log::error!("could not create the settings window: {e}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_carries_the_tab_in_the_hash() {
        assert_eq!(url(SettingsTab::General), "settings.html#general");
        assert_eq!(url(SettingsTab::Start), "settings.html#start");
        assert_eq!(url(SettingsTab::History), "settings.html#history");
        assert_eq!(url(SettingsTab::Companion), "settings.html#companion");
        assert_eq!(url(SettingsTab::default()), "settings.html#general");
    }

    #[test]
    fn title_uses_the_configured_name() {
        assert_eq!(title("Example Focus"), "Example Focus Settings");
    }
}
