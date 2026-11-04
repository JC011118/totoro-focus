//! The global hotkey (SPEC §8.2 Hotkey). Default `Ctrl+Alt+T` (⌃⌥T).
//!
//! - A binding is valid iff `Shortcut::from_str` accepts it and it has a Ctrl,
//!   Alt or Super modifier (function keys are exempt).
//! - The stored form is canonical `Ctrl+Alt+Shift+Super+<Key>`; bindings are
//!   compared as parsed `Shortcut`s, never as strings.
//! - `set_hotkey(Some)`: validate, unregister the old binding, register the new
//!   one; on failure re-register the old one and return `InvalidSettings`.
//! - The handler acts only on `Pressed` (Toggle).
//! - While the settings recorder captures keys the binding is unregistered; it
//!   is restored on `set_hotkey_capture(false)`, on settings close, or after 30 s.
//! - A startup registration failure keeps the setting and is reported in
//!   `AppInfo.hotkeyError`.
//!
//! Every function that registers or unregisters runs on the main thread (sync
//! commands, window events, setup; the capture timeout dispatches there). The
//! state mutex is never held across a global-shortcut call.

use std::str::FromStr;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use focus_core::engine::{Command, CommandError};
use focus_core::settings::SettingsPatch;
use focus_core::snapshot::Snapshot;
use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use crate::actions;
use crate::shell::Shell;

pub const MAX_LEN: usize = 64;
pub const CAPTURE_TIMEOUT: Duration = Duration::from_secs(30);

const MODIFIER_TOKENS: [&str; 11] = [
    "CTRL",
    "CONTROL",
    "ALT",
    "OPTION",
    "SHIFT",
    "CMD",
    "COMMAND",
    "SUPER",
    "COMMANDORCONTROL",
    "CMDORCTRL",
    "CMDORCONTROL",
];

// ---------------------------------------------------------------- pure rules

/// Validate an accelerator such as `Ctrl+Alt+T` (case-insensitive, aliases
/// allowed). Returns a gentle, user-facing reason on failure.
pub fn parse(input: &str) -> Result<Shortcut, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err("The shortcut is empty.".into());
    }
    if s.len() > MAX_LEN {
        return Err(format!("A shortcut can be at most {MAX_LEN} characters."));
    }
    let hk = Shortcut::from_str(s).map_err(|_| describe_parse_failure(s))?;
    if !hk
        .mods
        .intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER)
        && !is_function_key(hk.key)
    {
        return Err("A shortcut needs Control, Option or Command (function keys excepted).".into());
    }
    Ok(hk)
}

fn describe_parse_failure(s: &str) -> String {
    let tokens: Vec<&str> = s.split('+').map(str::trim).collect();
    if tokens.iter().any(|t| t.is_empty()) {
        return format!("“{s}” has an empty part; write it like Ctrl+Alt+T.");
    }
    if let Some(key) = tokens.last() {
        let modifier = MODIFIER_TOKENS.contains(&key.to_uppercase().as_str());
        if !modifier && Shortcut::from_str(key).is_err() {
            return format!("“{key}” isn't a key that can be used in a shortcut.");
        }
    }
    format!("“{s}” isn't a shortcut; use modifiers and then one key, like Ctrl+Alt+T.")
}

pub fn is_function_key(code: Code) -> bool {
    use Code::*;
    matches!(
        code,
        F1 | F2
            | F3
            | F4
            | F5
            | F6
            | F7
            | F8
            | F9
            | F10
            | F11
            | F12
            | F13
            | F14
            | F15
            | F16
            | F17
            | F18
            | F19
            | F20
            | F21
            | F22
            | F23
            | F24
    )
}

/// Canonical stored form: `Ctrl+Alt+Shift+Super+<Key>`, with letters and
/// digits as `A`/`7` and every other key by its W3C code name (`Space`,
/// `ArrowUp`, `F5`, …). It parses back to the same `Shortcut`.
pub fn canonical(hk: &Shortcut) -> String {
    let mut parts: Vec<String> = [
        (Modifiers::CONTROL, "Ctrl"),
        (Modifiers::ALT, "Alt"),
        (Modifiers::SHIFT, "Shift"),
        (Modifiers::SUPER, "Super"),
    ]
    .into_iter()
    .filter(|(flag, _)| hk.mods.contains(*flag))
    .map(|(_, name)| name.to_string())
    .collect();
    parts.push(key_name(hk.key));
    parts.join("+")
}

fn key_name(code: Code) -> String {
    let name = code.to_string();
    let short = name
        .strip_prefix("Key")
        .or_else(|| name.strip_prefix("Digit"))
        .filter(|rest| rest.len() == 1);
    short.map_or_else(|| name.clone(), str::to_string)
}

fn unavailable(hk: &Shortcut, detail: &str) -> String {
    format!(
        "{} couldn't be registered; another app may be using it ({detail}).",
        canonical(hk)
    )
}

// ---------------------------------------------------------------- runtime state

#[derive(Debug, Default)]
struct Inner {
    /// What this process has registered right now.
    registered: Option<Shortcut>,
    /// Generation of the active capture, if the recorder is capturing.
    capture: Option<u64>,
    next_capture: u64,
    /// Why the saved binding is not registered (for `AppInfo.hotkeyError`).
    error: Option<String>,
}

#[derive(Debug, Default)]
pub struct HotkeyState(Mutex<Inner>);

impl HotkeyState {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn capturing(&self) -> bool {
        self.lock().capture.is_some()
    }

    fn set_registered(&self, hk: Option<Shortcut>) {
        self.lock().registered = hk;
    }

    fn set_error(&self, error: Option<String>) {
        if let Some(e) = &error {
            log::warn!("global hotkey unavailable: {e}");
        }
        self.lock().error = error;
    }

    /// Start (or restart) a capture: returns its generation and the binding to
    /// unregister.
    fn begin_capture(&self) -> (u64, Option<Shortcut>) {
        let mut inner = self.lock();
        inner.next_capture += 1;
        inner.capture = Some(inner.next_capture);
        (inner.next_capture, inner.registered.take())
    }

    /// End the capture if one is active (and matches `generation`, when given).
    fn finish_capture(&self, generation: Option<u64>) -> bool {
        let mut inner = self.lock();
        match (inner.capture, generation) {
            (Some(active), Some(g)) if active != g => false,
            (Some(_), _) => {
                inner.capture = None;
                true
            }
            (None, _) => false,
        }
    }

    /// End any capture and return what is registered right now.
    fn end_capture_and_current(&self) -> Option<Shortcut> {
        let mut inner = self.lock();
        inner.capture = None;
        inner.registered
    }
}

pub fn plugin() -> TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                on_pressed(app);
            }
        })
        .build()
}

fn on_pressed(app: &AppHandle) {
    if app
        .try_state::<HotkeyState>()
        .is_some_and(|s| s.capturing())
    {
        return;
    }
    if app.try_state::<Shell>().is_some() {
        actions::run_logged(app, Command::Toggle);
    }
}

fn saved_binding(app: &AppHandle) -> Option<String> {
    app.try_state::<Shell>()?
        .read(|engine, _, _| engine.settings().hotkey.clone())
}

/// Register the persisted binding at startup. A failure keeps the setting and
/// is reported through `AppInfo.hotkeyError`.
pub fn register_saved(app: &AppHandle) {
    let Some(state) = app.try_state::<HotkeyState>() else {
        return;
    };
    let Some(text) = saved_binding(app) else {
        log::info!("global hotkey is disabled");
        return;
    };
    match parse(&text) {
        Err(reason) => state.set_error(Some(format!(
            "The saved shortcut “{text}” can't be used: {reason}"
        ))),
        Ok(hk) => match app.global_shortcut().register(hk) {
            Ok(()) => {
                state.set_registered(Some(hk));
                state.set_error(None);
                log::info!("global hotkey {} registered", canonical(&hk));
            }
            Err(e) => state.set_error(Some(unavailable(&hk, &e.to_string()))),
        },
    }
}

/// Unregister `current`, register `new`.
fn swap(app: &AppHandle, current: Option<Shortcut>, new: Option<Shortcut>) -> Result<(), String> {
    if current == new {
        return Ok(());
    }
    let gs = app.global_shortcut();
    if let Some(old) = current {
        if let Err(e) = gs.unregister(old) {
            log::warn!("unregistering {} failed: {e}", canonical(&old));
        }
    }
    if let Some(n) = new {
        gs.register(n)
            .map_err(|e| unavailable(&n, &e.to_string()))?;
    }
    Ok(())
}

/// Re-register the saved binding (nothing else is registered at this point).
fn restore(app: &AppHandle, state: &HotkeyState, saved: Option<Shortcut>) {
    state.set_registered(None);
    if let Some(hk) = saved {
        match app.global_shortcut().register(hk) {
            Ok(()) => {
                state.set_registered(Some(hk));
                state.set_error(None);
            }
            Err(e) => state.set_error(Some(unavailable(&hk, &e.to_string()))),
        }
    }
}

/// `set_hotkey`: `None` disables. Invalid or unregistrable bindings return
/// `InvalidSettings` and keep the previous binding.
pub fn set(app: &AppHandle, accelerator: Option<String>) -> Result<Snapshot, CommandError> {
    let invalid = |reason: String| CommandError::InvalidSettings { reason };
    let new = accelerator
        .as_deref()
        .map(parse)
        .transpose()
        .map_err(invalid)?;
    let state = app.state::<HotkeyState>();
    let saved = saved_binding(app).as_deref().and_then(|s| parse(s).ok());
    let current = state.end_capture_and_current();
    if let Err(reason) = swap(app, current, new) {
        restore(app, &state, saved);
        return Err(invalid(reason));
    }
    state.set_registered(new);
    let patch = SettingsPatch {
        hotkey: Some(new.as_ref().map(canonical)),
        ..SettingsPatch::default()
    };
    match actions::run(app, Command::UpdateSettings(patch)) {
        Ok(snapshot) => {
            state.set_error(None);
            log::info!(
                "global hotkey set to {}",
                new.as_ref().map_or_else(|| "none".to_string(), canonical)
            );
            Ok(snapshot)
        }
        Err(e) => {
            let _ = swap(app, new, None);
            restore(app, &state, saved);
            Err(e)
        }
    }
}

/// `set_hotkey_capture`: while active the binding is unregistered.
pub fn set_capture(app: &AppHandle, active: bool) {
    if !active {
        end_capture(app);
        return;
    }
    let Some(state) = app.try_state::<HotkeyState>() else {
        return;
    };
    let (generation, registered) = state.begin_capture();
    if let Some(hk) = registered {
        if let Err(e) = app.global_shortcut().unregister(hk) {
            log::warn!("unregistering {} for capture failed: {e}", canonical(&hk));
        }
    }
    let handle = app.clone();
    let timer = std::thread::Builder::new()
        .name("hotkey-capture-timeout".into())
        .spawn(move || {
            std::thread::sleep(CAPTURE_TIMEOUT);
            let main = handle.clone();
            let _ = handle.run_on_main_thread(move || end_capture_if(&main, Some(generation)));
        });
    if let Err(e) = timer {
        log::warn!("no capture timeout thread ({e}); the binding returns when settings closes");
    }
}

/// End a capture (recorder done, settings closed) and restore the saved binding.
pub fn end_capture(app: &AppHandle) {
    end_capture_if(app, None);
}

fn end_capture_if(app: &AppHandle, generation: Option<u64>) {
    let Some(state) = app.try_state::<HotkeyState>() else {
        return;
    };
    if !state.finish_capture(generation) {
        return;
    }
    let saved = saved_binding(app).as_deref().and_then(|s| parse(s).ok());
    restore(app, &state, saved);
}

pub fn error(app: &AppHandle) -> Option<String> {
    app.try_state::<HotkeyState>()?.lock().error.clone()
}

#[cfg(test)]
mod tests {
    use focus_core::settings::DEFAULT_HOTKEY;

    use super::*;

    fn canon(s: &str) -> String {
        canonical(&parse(s).unwrap_or_else(|e| panic!("{s}: {e}")))
    }

    #[test]
    fn default_hotkey_is_valid_and_canonical() {
        assert_eq!(canon(DEFAULT_HOTKEY), DEFAULT_HOTKEY);
        assert_eq!(
            parse(DEFAULT_HOTKEY).unwrap(),
            Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyT)
        );
    }

    #[test]
    fn canonical_order_and_key_names() {
        assert_eq!(canon("control+option+keyt"), "Ctrl+Alt+T");
        assert_eq!(canon("Shift+Cmd+Ctrl+Alt+k"), "Ctrl+Alt+Shift+Super+K");
        assert_eq!(canon("super+shift+1"), "Shift+Super+1");
        assert_eq!(canon("Ctrl+Alt+Digit7"), "Ctrl+Alt+7");
        assert_eq!(canon(" Ctrl + Alt + Space "), "Ctrl+Alt+Space");
        assert_eq!(canon("Alt+up"), "Alt+ArrowUp");
        assert_eq!(canon("Ctrl+;"), "Ctrl+Semicolon");
        assert_eq!(canon("F5"), "F5");
        assert_eq!(canon("Shift+F12"), "Shift+F12");
    }

    #[test]
    fn canonical_round_trips_for_recorder_keys() {
        let keys = [
            "A",
            "Z",
            "0",
            "9",
            "F1",
            "F20",
            "Space",
            "ArrowUp",
            "ArrowDown",
            "ArrowLeft",
            "ArrowRight",
            "Minus",
            "Equal",
            "Comma",
            "Period",
            "Slash",
            "Semicolon",
            "Quote",
            "BracketLeft",
            "BracketRight",
            "Backquote",
            "Backslash",
            "Enter",
            "Tab",
            "Escape",
            "Backspace",
            "Delete",
            "Home",
            "End",
            "PageUp",
            "PageDown",
            "Numpad5",
            "NumpadAdd",
            "MediaPlayPause",
        ];
        for key in keys {
            for mods in ["Ctrl", "Alt+Shift", "Ctrl+Alt+Shift+Super"] {
                let input = format!("{mods}+{key}");
                let first = parse(&input).unwrap_or_else(|e| panic!("{input}: {e}"));
                let stored = canonical(&first);
                assert_eq!(parse(&stored).unwrap(), first, "{input} → {stored}");
                assert_eq!(
                    canonical(&parse(&stored).unwrap()),
                    stored,
                    "{stored} is a fixed point"
                );
            }
        }
    }

    #[test]
    fn rejects_bindings_without_a_real_modifier() {
        for bad in ["T", "Space", "Shift+T", "shift+1", "Enter"] {
            let err = parse(bad).unwrap_err();
            assert!(err.contains("Control, Option or Command"), "{bad}: {err}");
        }
    }

    #[test]
    fn rejects_malformed_bindings_gently() {
        assert_eq!(parse("").unwrap_err(), "The shortcut is empty.");
        assert_eq!(parse("   ").unwrap_err(), "The shortcut is empty.");
        assert!(parse("Ctrl+Alt+").unwrap_err().contains("empty part"));
        assert!(parse("Ctrl++T").unwrap_err().contains("empty part"));
        assert!(parse("Ctrl+Alt+Banana")
            .unwrap_err()
            .contains("“Banana” isn't a key"));
        assert!(parse("Ctrl+Alt")
            .unwrap_err()
            .contains("modifiers and then one key"));
        assert!(parse("Ctrl+T+K")
            .unwrap_err()
            .contains("modifiers and then one key"));
        let long = format!("Ctrl+Alt+{}", "A".repeat(70));
        assert!(parse(&long).unwrap_err().contains("at most 64"));
        // Never leaks the upstream parser's text.
        for bad in ["Ctrl+Alt+Banana", "Ctrl+T+K", "Ctrl++T"] {
            assert!(!parse(bad).unwrap_err().contains("github"), "{bad}");
        }
    }

    #[test]
    fn bindings_compare_by_parsed_shortcut() {
        let p = |s: &str| parse(s).unwrap();
        assert_eq!(p("Ctrl+Alt+T"), p("control+option+KeyT"));
        assert_eq!(p("Alt+Ctrl+T"), p("Ctrl+Alt+T"));
        assert_ne!(p("Ctrl+Alt+T"), p("Ctrl+Alt+Shift+T"));
        assert_ne!(p("Ctrl+Alt+T"), p("Ctrl+Super+T"));
    }

    #[test]
    fn function_keys_are_exempt() {
        assert!(is_function_key(Code::F1));
        assert!(is_function_key(Code::F24));
        assert!(!is_function_key(Code::KeyF));
        assert!(parse("F13").is_ok());
    }

    #[test]
    fn capture_generations_only_end_their_own_capture() {
        let s = HotkeyState::default();
        s.set_registered(Some(parse("Ctrl+Alt+T").unwrap()));
        let (g1, taken) = s.begin_capture();
        assert!(
            taken.is_some(),
            "the binding is handed over for unregistering"
        );
        assert!(s.capturing());
        // Restarting the recorder starts a new generation; the old timeout is stale.
        let (g2, taken) = s.begin_capture();
        assert_ne!(g1, g2);
        assert_eq!(taken, None);
        assert!(!s.finish_capture(Some(g1)), "a stale timeout does nothing");
        assert!(s.capturing());
        assert!(s.finish_capture(Some(g2)));
        assert!(!s.capturing());
        assert!(!s.finish_capture(None), "nothing to end twice");
        // An explicit end (settings closed) ends any generation.
        let _ = s.begin_capture();
        assert!(s.finish_capture(None));
    }

    #[test]
    fn setting_a_binding_ends_the_capture() {
        let s = HotkeyState::default();
        let _ = s.begin_capture();
        assert_eq!(s.end_capture_and_current(), None);
        assert!(!s.capturing());
    }
}
