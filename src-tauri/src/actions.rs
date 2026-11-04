//! Operations shared by IPC commands, the tray menu, the global hotkey, the
//! single-instance callback and window events. Each one follows the engine
//! lock discipline: engine work happens inside `Shell::call`/`Shell::read`
//! closures, and every Tauri call happens after the guard is dropped.

use focus_core::engine::{Command, CommandError, Effect};
use focus_core::events::{self, PlayEvent, COMPANION_WINDOW};
use focus_core::history::SessionRecord;
use focus_core::packs::{PackInfo, PackRegistry, ResolvedPack};
use focus_core::settings::{SettingsPatch, WidgetPosition};
use focus_core::snapshot::{AppInfo, Snapshot};
use tauri::{AppHandle, Emitter, Manager};

use crate::companion::{self, CompanionState};
use crate::effects::{self, Emit};
use crate::pack_protocol;
use crate::shell::Shell;
use crate::{hotkey, settings_window};

/// Default and maximum `get_history` sizes (newest first).
pub const HISTORY_DEFAULT: u32 = 50;
pub const HISTORY_MAX: u32 = 500;

/// Run one engine command and apply its effects; returns the fresh snapshot,
/// which has also been broadcast.
pub fn run(app: &AppHandle, cmd: Command) -> Result<Snapshot, CommandError> {
    let shell = app.state::<Shell>();
    let outcome = shell.call(|engine, now| engine.command(cmd, now))?;
    Ok(effects::apply(app, &shell, outcome, Emit::Always))
}

/// Like [`run`], for callers with no one to report an error to (tray, hotkey).
pub fn run_logged(app: &AppHandle, cmd: Command) {
    let label = format!("{cmd:?}");
    if let Err(e) = run(app, cmd) {
        log::info!("{label} was not applied: {e}");
    }
}

/// The tray's Pause/Resume item: resume a paused session, pause otherwise. The
/// choice is made under the same lock as the command.
pub fn pause_or_resume(app: &AppHandle) {
    let shell = app.state::<Shell>();
    let outcome = shell.call(|engine, now| {
        let paused = engine.active().is_some_and(|s| !s.is_running());
        engine.command(
            if paused {
                Command::Resume
            } else {
                Command::Pause
            },
            now,
        )
    });
    match outcome {
        Ok(outcome) => {
            effects::apply(app, &shell, outcome, Emit::Always);
        }
        Err(e) => log::info!("pause/resume was not applied: {e}"),
    }
}

/// A user show/hide of the companion (command, tray, Reopen, second launch,
/// close button). Clears the hide-during-focus flag (SPEC §4.10) and goes
/// through the call pipeline so the snapshot's `companionVisible` is stamped in
/// `seq` order.
pub fn set_companion_visible(app: &AppHandle, visible: bool) -> Option<Snapshot> {
    let shell = app.try_state::<Shell>()?;
    if shell.is_shutting_down() {
        return None;
    }
    let outcome = shell
        .call(|engine, _now| {
            engine.clear_hidden_for_focus();
            Ok(vec![if visible {
                Effect::ShowCompanion
            } else {
                Effect::HideCompanion
            }])
        })
        .ok()?;
    Some(effects::apply(app, &shell, outcome, Emit::Always))
}

pub fn toggle_companion(app: &AppHandle) {
    let Some(shell) = app.try_state::<Shell>() else {
        return;
    };
    let visible = shell.companion_wanted();
    set_companion_visible(app, !visible);
}

/// Tray "Custom duration…": show the companion, then open its duration picker.
pub fn open_picker(app: &AppHandle) {
    set_companion_visible(app, true);
    if let Err(e) = app.emit_to(COMPANION_WINDOW, events::OPEN_PICKER, ()) {
        log::warn!("emitting open-picker failed: {e}");
    }
}

pub fn open_settings(app: &AppHandle, tab: events::SettingsTab) {
    settings_window::open(app, tab);
}

/// Resolve pack `id` (falling back to the placeholder) for the renderer.
pub fn resolve_pack(app: &AppHandle, id: &str) -> ResolvedPack {
    let shell = app.state::<Shell>();
    let registry = shell.packs();
    registry.resolve_pack(id, shell.table(), pack_protocol::url_prefix())
}

fn active_pack_id(shell: &Shell) -> String {
    shell.read(|engine, _, _| engine.settings().active_pack.clone())
}

pub fn active_pack(app: &AppHandle) -> ResolvedPack {
    let id = active_pack_id(&app.state::<Shell>());
    resolve_pack(app, &id)
}

/// Rescan pack folders. If that changes what the active pack resolves to,
/// broadcast `focus://pack-changed`.
pub fn rescan_packs(app: &AppHandle) -> Vec<PackInfo> {
    let shell = app.state::<Shell>();
    let id = active_pack_id(&shell);
    let before = resolve_pack(app, &id);
    let fresh = PackRegistry::load(
        pack_protocol::bundled_pack(),
        Some(&shell.dirs.user_packs),
        shell.dirs.dev_packs.as_deref(),
    );
    let list = fresh.list();
    *shell.packs_mut() = fresh;
    let after = resolve_pack(app, &id);
    if after != before {
        if let Err(e) = app.emit(events::PACK_CHANGED, &after) {
            log::warn!("emitting pack-changed failed: {e}");
        }
    }
    list
}

/// Why `id` cannot be activated, for `PackUnavailable`.
pub fn unavailable_reason(list: &[PackInfo], id: &str) -> String {
    match list.iter().find(|p| p.id == id) {
        None => "no pack with this id was found".to_string(),
        Some(p) if p.errors.is_empty() => "the pack is not valid".to_string(),
        Some(p) => p.errors.join("; "),
    }
}

/// Persist `settings.activePack` (broadcasting pack-changed and a snapshot) and
/// return the resolved pack. Unknown or invalid ids → `PackUnavailable`.
pub fn set_active_pack(app: &AppHandle, id: String) -> Result<ResolvedPack, CommandError> {
    let shell = app.state::<Shell>();
    {
        let registry = shell.packs();
        if !registry.is_valid(&id) {
            let reason = unavailable_reason(&registry.list(), &id);
            return Err(CommandError::PackUnavailable { id, reason });
        }
    }
    let patch = SettingsPatch {
        active_pack: Some(id.clone()),
        ..SettingsPatch::default()
    };
    run(app, Command::UpdateSettings(patch))?;
    Ok(resolve_pack(app, &id))
}

/// Validate a replay, show the companion if hidden, then emit `focus://play` to
/// the companion only.
pub fn replay(app: &AppHandle, key: String) -> Result<(), CommandError> {
    let shell = app.state::<Shell>();
    shell.read(|engine, _, _| engine.check_replay(&key))?;
    if !shell.companion_wanted() {
        set_companion_visible(app, true);
    }
    if let Err(e) = app.emit_to(COMPANION_WINDOW, events::PLAY, PlayEvent { key }) {
        log::warn!("emitting play failed: {e}");
    }
    Ok(())
}

pub fn history_limit(limit: Option<u32>) -> usize {
    limit.unwrap_or(HISTORY_DEFAULT).min(HISTORY_MAX) as usize
}

pub fn history(app: &AppHandle, limit: Option<u32>) -> Vec<SessionRecord> {
    let limit = history_limit(limit);
    app.state::<Shell>()
        .read(|engine, _, _| engine.history().recent(limit))
}

/// Save a companion position (user drag, after debouncing, or a reset).
pub fn save_widget_position(app: &AppHandle, (x, y): (f64, f64)) {
    let Some(shell) = app.try_state::<Shell>() else {
        return;
    };
    let position = WidgetPosition { x, y };
    if shell.read(|engine, _, _| engine.settings().widget_position) == Some(position) {
        return;
    }
    let patch = SettingsPatch {
        widget_position: Some(Some(position)),
        ..SettingsPatch::default()
    };
    run_logged(app, Command::UpdateSettings(patch));
}

/// Move the companion to the default spot on the primary display and save it.
pub fn reset_companion_position(app: &AppHandle) {
    if let Some(target) = companion::reset_position(app) {
        save_widget_position(app, target);
    }
}

pub fn app_info(app: &AppHandle) -> AppInfo {
    let shell = app.state::<Shell>();
    let info = app.package_info();
    let window_mode = app
        .try_state::<CompanionState>()
        .map_or_else(companion::WindowMode::detect, |c| c.mode())
        .as_str();
    AppInfo {
        name: info.name.clone(),
        version: info.version.to_string(),
        user_packs_dir: shell.dirs.user_packs.display().to_string(),
        data_dir: shell.dirs.data.display().to_string(),
        window_mode: window_mode.to_string(),
        debug: cfg!(debug_assertions),
        hotkey_error: hotkey::error(app),
        limits: shell.read(|engine, _, _| engine.limits()),
        rules: shell.table().rules.clone(),
        unlocks: shell.table().unlocks.clone(),
    }
}

#[cfg(test)]
mod tests {
    use focus_core::packs::{PackFormatKind, PackSource};

    use super::*;

    fn info(id: &str, errors: &[&str]) -> PackInfo {
        PackInfo {
            id: id.into(),
            name: id.into(),
            author: None,
            license: None,
            description: None,
            source: PackSource::User,
            format_kind: PackFormatKind::Native,
            valid: errors.is_empty(),
            errors: errors.iter().map(|e| e.to_string()).collect(),
            warnings: vec![],
        }
    }

    #[test]
    fn history_limit_defaults_to_50_and_caps_at_500() {
        assert_eq!(history_limit(None), 50);
        assert_eq!(history_limit(Some(0)), 0);
        assert_eq!(history_limit(Some(7)), 7);
        assert_eq!(history_limit(Some(500)), 500);
        assert_eq!(history_limit(Some(10_000)), 500);
    }

    #[test]
    fn unavailable_reason_names_the_problem() {
        let list = vec![
            info("good", &[]),
            info("broken", &["no idle animation", "bad fps"]),
        ];
        assert_eq!(
            unavailable_reason(&list, "missing"),
            "no pack with this id was found"
        );
        assert_eq!(
            unavailable_reason(&list, "broken"),
            "no idle animation; bad fps"
        );
        assert_eq!(unavailable_reason(&list, "good"), "the pack is not valid");
    }
}
