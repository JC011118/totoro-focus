//! Applying engine effects (SPEC §4.9) with no engine lock held.
//!
//! Order within one call: save once (Persist, with the call's own `Now`), then
//! SettingsChanged, Show/HideCompanion, ActivityChanged, Transition, PlaySound
//! in the order the engine returned them, then exactly one `focus://snapshot`,
//! then the tray. Persist is applied synchronously, so a WillSleep observer or
//! the Exit handler has saved before it returns. Show/HideCompanion and
//! ActivityChanged are applied latest-wins (see `shell`).

use focus_core::engine::{ActivityLevel, Command, Effect, Engine, SystemEvent, Transition};
use focus_core::events;
use focus_core::settings::Settings;
use focus_core::snapshot::Snapshot;
use focus_core::timer::PauseReason;
use focus_core::Now;
use tauri::{AppHandle, Emitter, Manager};

use crate::shell::{Outcome, Shell};
use crate::{actions, companion, platform, tray};

/// Whether the call's snapshot is always emitted (commands, system events) or
/// only when its display key changed (ticks, `get_snapshot`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Emit {
    Always,
    IfChanged,
}

/// Apply `outcome` and return its snapshot.
pub fn apply(app: &AppHandle, shell: &Shell, outcome: Outcome, emit: Emit) -> Snapshot {
    let Outcome {
        now,
        effects,
        persist,
        snapshot,
    } = outcome;
    if let Some(state) = &persist {
        shell.persist(snapshot.seq, now, state);
    }
    for effect in effects {
        apply_one(app, shell, effect);
    }
    if shell.admit_emit(&snapshot, emit == Emit::Always) {
        if let Err(e) = app.emit(events::SNAPSHOT, &snapshot) {
            log::warn!("emitting the snapshot failed: {e}");
        }
    }
    tray::update(app, &snapshot);
    if emit == Emit::Always {
        shell.ticker.kick();
    }
    snapshot
}

fn apply_one(app: &AppHandle, shell: &Shell, effect: Effect) {
    match effect {
        Effect::Persist => {} // already saved, once, before everything else
        Effect::SettingsChanged { previous, current } => settings_changed(app, &previous, &current),
        // Both are latest-wins: they apply the newest wanted state (recorded under
        // the engine lock), not this effect's payload, so a call whose effects are
        // applied after a newer call's never undoes the newer state.
        Effect::ShowCompanion | Effect::HideCompanion => companion::sync_visibility(app),
        Effect::ActivityChanged { .. } => {
            shell.sync_activity(platform::set_activity);
        }
        Effect::Transition(t) => {
            log_transition(&t);
            if let Err(e) = app.emit(events::TRANSITION, &t) {
                log::warn!("emitting a transition failed: {e}");
            }
        }
        Effect::PlaySound(cue) => {
            log::debug!("chime: {cue:?}");
            platform::play_chime(app);
        }
    }
}

/// React to settings the shell owns. The hotkey is registered by `set_hotkey`
/// before the engine stores it; the tray title and labels follow the snapshot.
fn settings_changed(app: &AppHandle, previous: &Settings, current: &Settings) {
    if previous.active_pack != current.active_pack {
        let resolved = actions::resolve_pack(app, &current.active_pack);
        if let Err(e) = app.emit(events::PACK_CHANGED, &resolved) {
            log::warn!("emitting pack-changed failed: {e}");
        }
    }
}

fn log_transition(t: &Transition) {
    match t {
        Transition::Paused {
            reason: PauseReason::Suspend,
            ..
        } => log::info!("session paused (suspend or observation gap)"),
        Transition::Paused {
            reason: PauseReason::ClockJump,
            ..
        } => log::info!("session paused: the wall clock jumped forward"),
        other => log::debug!("transition: {other:?}"),
    }
}

/// Run a system event (Tick, Suspend, Wake, ClockChanged) and apply it.
/// Returns `None` before setup or during shutdown.
pub fn system_event(app: &AppHandle, event: SystemEvent) -> Option<Snapshot> {
    let shell = app.try_state::<Shell>()?;
    if shell.is_shutting_down() {
        return None;
    }
    let outcome = shell.call_system(event);
    if event == SystemEvent::Tick {
        warn_on_stall(&outcome.effects);
    }
    let emit = if event == SystemEvent::Tick {
        Emit::IfChanged
    } else {
        Emit::Always
    };
    Some(apply(app, &shell, outcome, emit))
}

/// A tick that paused with reason `suspend` means the ticker (or the whole
/// process) stalled longer than the observation gap; it earned nothing (§4.5).
fn warn_on_stall(effects: &[Effect]) {
    let stalled = effects.iter().any(|e| {
        matches!(
            e,
            Effect::Transition(Transition::Paused {
                reason: PauseReason::Suspend,
                ..
            })
        )
    });
    if stalled {
        log::warn!("observation gap exceeded on a tick (ticker stall or missed sleep); the gap earned nothing");
    }
}

/// Idempotent shutdown (SPEC §8.2): `command(Quit)` and a synchronous save.
/// A companion drag still waiting in the debouncer is folded into the same
/// save, so a move made just before quitting is kept. Called from
/// `RunEvent::Exit` on the main thread, so it never round-trips to the main
/// thread: no emits, no window or tray calls.
pub fn shutdown(app: &AppHandle) {
    let Some(shell) = app.try_state::<Shell>() else {
        return;
    };
    let pending = || {
        app.try_state::<companion::CompanionState>()
            .and_then(|c| c.take_pending())
    };
    if shutdown_shell(&shell, pending, platform::set_activity) {
        shell.ticker.kick();
        log::info!("shutdown complete");
    }
}

/// Shutdown's engine work, save and activity release, with no Tauri runtime
/// involved. Returns `false` (doing nothing) when shutdown already ran.
///
/// `pending_position` is read only after the shutdown flag is set: from then on
/// `actions::store_widget_position` leaves positions alone (it checks the flag
/// under the engine lock), so the debouncer can neither lose the drag nor save
/// it after the Quit checkpoint.
pub fn shutdown_shell(
    shell: &Shell,
    pending_position: impl FnOnce() -> Option<(f64, f64)>,
    set_activity: impl FnOnce(ActivityLevel),
) -> bool {
    if !shell.begin_shutdown() {
        return false;
    }
    let position = pending_position();
    let mut refused = Vec::new();
    let result = shell.call(|engine, now| Ok(quit_effects(engine, now, position, &mut refused)));
    for e in refused {
        log::error!("{e}");
    }
    // `quit_effects` never fails, so the call always produces an outcome.
    if let Ok(outcome) = result {
        if let Some(state) = &outcome.persist {
            shell.persist(outcome.snapshot.seq, outcome.now, state);
        }
    }
    // Quit paused any running session; release the activity it held.
    shell.sync_activity(set_activity);
    true
}

/// Under the engine lock: store a not-yet-saved companion drag, then Quit
/// (pause + Persist), so one synchronous save holds both. A refusal of either
/// is reported in `refused` (logged after the lock is dropped); the other still
/// runs, and both use the same `now`.
fn quit_effects(
    engine: &mut Engine,
    now: Now,
    position: Option<(f64, f64)>,
    refused: &mut Vec<String>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(pos) = position {
        match actions::widget_position_update(engine, now, pos) {
            Ok(fx) => effects.extend(fx.unwrap_or_default()),
            Err(e) => refused.push(format!(
                "the last companion position {pos:?} was not saved: {e}"
            )),
        }
    }
    match engine.command(Command::Quit, now) {
        Ok(fx) => effects.extend(fx),
        Err(e) => refused.push(format!("quit was refused by the engine: {e}")),
    }
    effects
}
