//! The companion's NSPanel subclass (SPEC §8.1 rule 1). Exactly one
//! `tauri_panel!` invocation lives in this module: the macro emits
//! module-level `use` items, so a second invocation would collide.
//!
//! The panel can never become key or main, so clicking a companion button
//! leaves keyboard focus in the user's app (the companion has no text inputs,
//! rule 7). `acceptsFirstMouse` is not a panel key: it comes from
//! `WebviewWindowBuilder::accept_first_mouse(true)` on the WKWebView.

tauri_nspanel::tauri_panel! {
    panel!(CompanionPanel {
        config: {
            can_become_key_window: false,
            can_become_main_window: false,
            is_floating_panel: true
        }
    })
}
