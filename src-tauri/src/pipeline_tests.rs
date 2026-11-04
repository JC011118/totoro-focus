//! `Shell` call-pipeline tests against the real engine and store (no Tauri
//! runtime needed): seq stamping, `Err` ⇒ nothing stamped, Persist with the
//! call's own `Now`, `mark_persisted` only after a successful save, companion
//! visibility stamping, the save gate on disk, latest-wins process activity,
//! and shutdown (Quit checkpoint + a pending companion drag in one save).
//!
//! They need the focus-core engine, store and packs implementations. While
//! those are still `todo!()` stubs (in an isolated module worktree), each test
//! detects the stub panic and skips itself; any other panic fails the test.

use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use focus_core::engine::{
    uuid_ids, ActivityLevel, Command, CommandError, Effect, Engine, EngineConfig, SystemEvent,
};
use focus_core::packs::PackRegistry;
use focus_core::rewards::UnlockTable;
use focus_core::settings::{SettingsPatch, WidgetPosition};
use focus_core::snapshot::Phase;
use focus_core::store::{PersistedState, Store};
use focus_core::timer::{PauseReason, RunStatus};
use focus_core::{Clock, SystemClock};

use crate::actions::{self, PositionCall};
use crate::shell::{activity_change, visibility_change, Dirs, Outcome, Shell};
use crate::{effects, pack_protocol};

/// Run `body`, skipping (not failing) when it hits an unimplemented stub.
fn with_core(name: &str, body: impl FnOnce()) {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(()) => {}
        Err(payload) => {
            let msg = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("");
            if msg.starts_with("not yet implemented") {
                eprintln!("skipping {name}: focus-core stub ({msg})");
            } else {
                resume_unwind(payload);
            }
        }
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "focus-shell-{tag}-{}-{}",
            std::process::id(),
            uuid_ids()()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A shell over a fresh engine; `store_dir` may be unusable on purpose.
fn shell_at(store_dir: &Path, state: PersistedState) -> Shell {
    let clock = SystemClock::new();
    let table = UnlockTable::builtin();
    let (engine, _) = Engine::new(
        EngineConfig::default(),
        table.clone(),
        state,
        clock.now(),
        uuid_ids(),
    );
    let packs = PackRegistry::load(pack_protocol::bundled_pack(), None, None);
    let dirs = Dirs {
        data: store_dir.to_path_buf(),
        user_packs: store_dir.join("packs"),
        dev_packs: None,
    };
    Shell::new(engine, clock, Store::new(store_dir), table, packs, dirs)
}

fn persist(shell: &Shell, o: &Outcome) {
    if let Some(state) = &o.persist {
        shell.persist(o.snapshot.seq, o.now, state);
    }
}

fn start_focus(shell: &Shell) -> Outcome {
    shell
        .call(|e, now| {
            e.command(
                Command::StartFocus {
                    minutes: Some(25),
                    task_label: None,
                },
                now,
            )
        })
        .expect("start focus")
}

fn has_persist(o: &Outcome) -> bool {
    o.effects.iter().any(|e| matches!(e, Effect::Persist))
}

#[test]
fn seq_strictly_increases_across_every_kind_of_call() {
    with_core("seq", || {
        let dir = TempDir::new("seq");
        let shell = shell_at(&dir.0, PersistedState::default());
        let a = shell
            .outcome_for(shell.read(|_, now, _| now), vec![])
            .snapshot
            .seq;
        let b = shell.snapshot_now().seq;
        let c = shell.call_system(SystemEvent::Tick).snapshot.seq;
        let d = start_focus(&shell).snapshot.seq;
        let e = shell.call_system(SystemEvent::Tick).snapshot.seq;
        assert!(a < b && b < c && c < d && d < e, "{a} {b} {c} {d} {e}");
    });
}

#[test]
fn a_refused_command_stamps_nothing_and_changes_nothing() {
    with_core("refused", || {
        let dir = TempDir::new("refused");
        let shell = shell_at(&dir.0, PersistedState::default());
        let before = shell.snapshot_now();
        let err = shell
            .call(|e, now| e.command(Command::Pause, now))
            .unwrap_err();
        assert_eq!(err, CommandError::NoSession);
        let after = shell.snapshot_now();
        assert_eq!(
            after.seq,
            before.seq + 1,
            "the refused call consumed no seq"
        );
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.phase, Phase::Idle);
    });
}

#[test]
fn persist_uses_the_calls_own_now_and_marks_only_after_success() {
    with_core("persist", || {
        let dir = TempDir::new("persist");
        let shell = shell_at(&dir.0, PersistedState::default());
        let started = start_focus(&shell);
        assert!(has_persist(&started));
        let state = started
            .persist
            .as_ref()
            .expect("persisted state copied out under the lock");
        let checkpoint = state.active_session.as_ref().expect("checkpoint");
        assert_eq!(
            checkpoint.checkpoint_at, started.now.wall,
            "same Now as the call"
        );
        persist(&shell, &started);

        let on_disk: PersistedState =
            serde_json::from_slice(&std::fs::read(dir.0.join("state.json")).unwrap()).unwrap();
        assert_eq!(on_disk.active_session.unwrap().id, checkpoint.id);

        // mark_persisted ran: an immediate tick is inside the 15 s checkpoint interval.
        let tick = shell.call_system(SystemEvent::Tick);
        assert!(
            !has_persist(&tick),
            "checkpoint cadence restarted by mark_persisted"
        );
    });
}

#[test]
fn a_failed_save_is_retried_on_the_next_running_tick() {
    with_core("failed-save", || {
        let dir = TempDir::new("failed-save");
        // A file where the data directory should be: every save fails.
        let blocked = dir.0.join("not-a-dir");
        std::fs::write(&blocked, b"x").unwrap();
        let shell = shell_at(&blocked, PersistedState::default());
        let started = start_focus(&shell);
        persist(&shell, &started);
        let tick = shell.call_system(SystemEvent::Tick);
        assert!(
            has_persist(&tick),
            "not marked persisted, so the tick checkpoints again"
        );
    });
}

#[test]
fn an_older_state_never_overwrites_a_newer_one_on_disk() {
    with_core("save-order", || {
        let dir = TempDir::new("save-order");
        let shell = shell_at(&dir.0, PersistedState::default());
        let older = start_focus(&shell);
        let newer = shell
            .call(|e, now| e.command(Command::End, now))
            .expect("end");
        persist(&shell, &newer);
        persist(&shell, &older); // arrives late: must be dropped
        let on_disk: PersistedState =
            serde_json::from_slice(&std::fs::read(dir.0.join("state.json")).unwrap()).unwrap();
        assert!(
            on_disk.active_session.is_none(),
            "the ended state stays on disk"
        );
        assert_eq!(on_disk.history.len(), 1);
    });
}

#[test]
fn companion_visibility_is_stamped_in_seq_order() {
    with_core("visibility", || {
        let dir = TempDir::new("visibility");
        let mut state = PersistedState::default();
        state.settings.hide_during_focus = true;
        let shell = shell_at(&dir.0, state);
        assert!(!shell.snapshot_now().companion_visible);
        shell.want_companion_at_launch();
        assert!(shell.snapshot_now().companion_visible);

        let started = start_focus(&shell);
        assert_eq!(visibility_change(&started.effects), Some(false));
        assert!(!started.snapshot.companion_visible, "hidden for focus");

        // A user show is a synthetic ShowCompanion through the same pipeline.
        let shown = shell
            .call(|e, _| {
                e.clear_hidden_for_focus();
                Ok(vec![Effect::ShowCompanion])
            })
            .unwrap();
        assert!(shown.snapshot.companion_visible);
        assert!(shown.persist.is_none());
    });
}

#[test]
fn settings_patches_flow_through_the_pipeline() {
    with_core("settings", || {
        let dir = TempDir::new("settings");
        let shell = shell_at(&dir.0, PersistedState::default());
        let patch = SettingsPatch {
            active_pack: Some("placeholder".into()),
            widget_position: Some(Some(WidgetPosition { x: 10.0, y: 40.0 })),
            ..SettingsPatch::default()
        };
        let o = shell
            .call(|e, now| e.command(Command::UpdateSettings(patch), now))
            .unwrap();
        persist(&shell, &o);
        let saved = shell.read(|e, _, _| e.settings().widget_position);
        assert_eq!(saved.map(|p| (p.x, p.y)), Some((10.0, 40.0)));
        let delay = shell.read(|e, now, clock: &dyn Clock| e.next_tick_delay_ms(now, clock));
        assert!((50..=60_000).contains(&delay), "{delay}");
    });
}

fn on_disk(dir: &Path) -> PersistedState {
    serde_json::from_slice(&std::fs::read(dir.join("state.json")).unwrap()).unwrap()
}

/// Apply only `o`'s ActivityChanged effects, the way `effects::apply` does,
/// recording every level handed to the OS.
fn apply_activity(shell: &Shell, o: &Outcome, applied: &Mutex<Vec<ActivityLevel>>) {
    for e in &o.effects {
        if let Effect::ActivityChanged { .. } = e {
            shell.sync_activity(|l| applied.lock().unwrap().push(l));
        }
    }
}

#[test]
fn activity_is_applied_latest_wins_whatever_order_effects_are_applied_in() {
    with_core("activity", || {
        let dir = TempDir::new("activity");
        let shell = shell_at(&dir.0, PersistedState::default());
        let applied = Mutex::new(Vec::new());

        // Three calls in seq order: start (Focus), end (None), start again (Focus).
        let first = start_focus(&shell);
        let ended = shell.call(|e, now| e.command(Command::End, now)).unwrap();
        let again = start_focus(&shell);
        assert_eq!(activity_change(&first.effects), Some(ActivityLevel::Focus));
        assert_eq!(activity_change(&ended.effects), Some(ActivityLevel::None));
        assert_eq!(activity_change(&again.effects), Some(ActivityLevel::Focus));
        assert_eq!(shell.activity_wanted(), ActivityLevel::Focus);

        // The newest call applies first and the older ones arrive late (the
        // review finding's interleaving): the running session keeps its activity.
        apply_activity(&shell, &again, &applied);
        apply_activity(&shell, &ended, &applied);
        apply_activity(&shell, &first, &applied);
        assert_eq!(*applied.lock().unwrap(), vec![ActivityLevel::Focus]);

        // Ending it is applied as soon as the newest call is, whatever comes late.
        let ended = shell.call(|e, now| e.command(Command::End, now)).unwrap();
        apply_activity(&shell, &again, &applied);
        apply_activity(&shell, &ended, &applied);
        apply_activity(&shell, &again, &applied);
        assert_eq!(
            *applied.lock().unwrap(),
            vec![ActivityLevel::Focus, ActivityLevel::None]
        );
    });
}

#[test]
fn a_break_holds_the_break_activity() {
    with_core("activity-break", || {
        let dir = TempDir::new("activity-break");
        let shell = shell_at(&dir.0, PersistedState::default());
        let o = shell
            .call(|e, now| e.command(Command::StartBreak { minutes: Some(5) }, now))
            .unwrap();
        assert_eq!(activity_change(&o.effects), Some(ActivityLevel::Break));
        assert_eq!(shell.activity_wanted(), ActivityLevel::Break);
        let applied = Mutex::new(Vec::new());
        apply_activity(&shell, &o, &applied);
        assert_eq!(*applied.lock().unwrap(), vec![ActivityLevel::Break]);
    });
}

#[test]
fn shutdown_saves_a_pending_companion_drag_with_the_quit_checkpoint() {
    with_core("shutdown-drag", || {
        let dir = TempDir::new("shutdown-drag");
        let shell = shell_at(&dir.0, PersistedState::default());
        let started = start_focus(&shell);
        persist(&shell, &started);
        let applied = Mutex::new(Vec::new());
        apply_activity(&shell, &started, &applied);

        // Dragged less than the debounce interval before Cmd+Q: still pending.
        assert!(effects::shutdown_shell(
            &shell,
            || Some((321.0, 456.0)),
            |l| applied.lock().unwrap().push(l),
        ));

        let state = on_disk(&dir.0);
        let pos = state.settings.widget_position.expect("the drag was saved");
        assert_eq!((pos.x, pos.y), (321.0, 456.0));
        let session = state.active_session.expect("the session is checkpointed");
        assert_eq!(session.status, RunStatus::Paused);
        assert_eq!(session.pause_reason, Some(PauseReason::Quit));
        // Quit released the activity the running focus session held.
        assert_eq!(
            *applied.lock().unwrap(),
            vec![ActivityLevel::Focus, ActivityLevel::None]
        );
        assert_eq!(shell.activity_wanted(), ActivityLevel::None);
    });
}

#[test]
fn shutdown_runs_once_and_keeps_the_saved_position_without_a_drag() {
    with_core("shutdown-once", || {
        let dir = TempDir::new("shutdown-once");
        let mut initial = PersistedState::default();
        initial.settings.widget_position = Some(WidgetPosition { x: 5.0, y: 50.0 });
        let shell = shell_at(&dir.0, initial);
        assert!(effects::shutdown_shell(&shell, || None, |_| {}));
        let state = on_disk(&dir.0);
        let pos = state.settings.widget_position.unwrap();
        assert_eq!((pos.x, pos.y), (5.0, 50.0));
        assert!(state.active_session.is_none());

        // Idempotent: a second call reads no position and runs no engine call.
        let before = shell.snapshot_now();
        assert!(!effects::shutdown_shell(
            &shell,
            || panic!("the pending position is read once"),
            |_| panic!("no activity change"),
        ));
        let after = shell.snapshot_now();
        assert_eq!(after.revision, before.revision);
        assert_eq!(
            after.seq,
            before.seq + 1,
            "the second shutdown stamped nothing"
        );
    });
}

#[test]
fn shutdown_skips_a_drag_the_engine_already_holds() {
    with_core("shutdown-same", || {
        let dir = TempDir::new("shutdown-same");
        let mut initial = PersistedState::default();
        initial.settings.widget_position = Some(WidgetPosition { x: 7.0, y: 70.0 });
        let shell = shell_at(&dir.0, initial);
        let revision = shell.snapshot_now().revision;
        assert!(effects::shutdown_shell(
            &shell,
            || Some((7.0, 70.0)),
            |_| {}
        ));
        // Quit while idle changes nothing observable, and the equal position adds
        // no settings change.
        assert_eq!(shell.snapshot_now().revision, revision);
        let pos = on_disk(&dir.0).settings.widget_position.unwrap();
        assert_eq!((pos.x, pos.y), (7.0, 70.0));
    });
}

#[test]
fn position_saves_defer_to_shutdown_once_it_has_begun() {
    with_core("position-shutdown", || {
        let dir = TempDir::new("position-shutdown");
        let shell = shell_at(&dir.0, PersistedState::default());

        match actions::store_widget_position(&shell, (11.0, 22.0)) {
            PositionCall::Stored(o) => {
                assert!(o.persist.is_some());
                persist(&shell, &o);
            }
            other => panic!("expected Stored, got {other:?}"),
        }
        let seq = shell.snapshot_now().seq;
        assert!(matches!(
            actions::store_widget_position(&shell, (11.0, 22.0)),
            PositionCall::Unchanged
        ));
        assert_eq!(
            shell.snapshot_now().seq,
            seq + 1,
            "an unchanged position stamps nothing"
        );

        // The debouncer commits after shutdown began: the drag stays pending for
        // shutdown to take, and the engine is never changed after Quit.
        assert!(shell.begin_shutdown());
        assert!(matches!(
            actions::store_widget_position(&shell, (99.0, 88.0)),
            PositionCall::LeftForShutdown
        ));
        let held = shell.read(|e, _, _| e.settings().widget_position).unwrap();
        assert_eq!((held.x, held.y), (11.0, 22.0));
    });
}

#[test]
fn a_refused_position_is_reported_and_changes_nothing() {
    with_core("position-refused", || {
        let dir = TempDir::new("position-refused");
        let shell = shell_at(&dir.0, PersistedState::default());
        let before = shell.snapshot_now();
        assert!(matches!(
            actions::store_widget_position(&shell, (f64::NAN, 1.0)),
            PositionCall::Refused(CommandError::InvalidSettings { .. })
        ));
        let after = shell.snapshot_now();
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.seq, before.seq + 1, "a refused call stamps nothing");
    });
}
