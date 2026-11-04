//! Checkpoints, quit, crash and relaunch recovery (SPEC §4.7).

use chrono::TimeDelta;
use focus_core::clock::Wall;
use focus_core::engine::{Command, Effect, SystemEvent, Transition};
use focus_core::history::{EndReason, HarvestItem, Outcome, SessionRecord};
use focus_core::rewards::AwardRecord;
use focus_core::settings::Settings;
use focus_core::snapshot::Phase;
use focus_core::store::PersistedState;
use focus_core::testkit::{effect_names, persist_count, transitions, H, HOUR, MIN, SEC};
use focus_core::timer::{PauseReason, PersistedSession, RunStatus, SessionKind};
use focus_core::FakeClock;

fn start25(h: &mut H) {
    h.ok(Command::StartFocus {
        minutes: None,
        task_label: None,
    });
}

fn active_ms(h: &H) -> u64 {
    h.snap().session.map_or(0, |s| s.active_ms)
}

fn checkpoint(
    id: &str,
    status: RunStatus,
    accrued: u64,
    planned: u64,
    clock: &FakeClock,
) -> PersistedSession {
    use focus_core::Clock;
    let now = clock.now().wall;
    PersistedSession {
        id: id.into(),
        kind: SessionKind::Focus,
        task_label: Some("Lecture".into()),
        planned_ms: planned,
        started_at: now - TimeDelta::minutes(40),
        accrued_ms: accrued,
        status,
        checkpoint_at: now - TimeDelta::minutes(1),
        paused_at: None,
        pause_reason: None,
        age_anchor: None,
    }
}

fn record(id: &str, clock: &FakeClock) -> SessionRecord {
    use focus_core::Clock;
    let now = clock.now().wall;
    SessionRecord {
        id: id.into(),
        kind: SessionKind::Focus,
        task_label: None,
        started_at: now - TimeDelta::minutes(30),
        ended_at: now - TimeDelta::minutes(5),
        planned_ms: 25 * MIN,
        active_ms: 25 * MIN,
        outcome: Outcome::Completed,
        end_reason: EndReason::Completed,
        ended_utc_offset_minutes: Some(0),
    }
}

fn award(points: u64, bonus: u64, clock: &FakeClock) -> AwardRecord {
    use focus_core::Clock;
    AwardRecord {
        points,
        bonus,
        completed: bonus > 0,
        awarded_at: clock.now().wall,
    }
}

fn reconciled_state() -> PersistedState {
    // A state that Engine::new would not change (base already unlocked).
    let mut s = PersistedState::default();
    s.friendship.unlocked.insert("base".into());
    s
}

fn h_with(state: PersistedState) -> H {
    H::with_state(state)
}

#[test]
fn checkpoint_cadence_while_running() {
    let mut h = H::new();
    start25(&mut h);
    let saves_after_start = h.saves;
    let mut persist_at = Vec::new();
    for s in 1..=60 {
        h.clock.advance(SEC);
        if persist_count(&h.tick()) > 0 {
            persist_at.push(s);
        }
    }
    assert_eq!(persist_at, vec![15, 30, 45, 60]);
    assert_eq!(h.saves, saves_after_start + 4);
    // Paused: no cadence.
    assert_eq!(persist_count(&h.ok(Command::Pause)), 1);
    assert_eq!(persist_count(&h.run(60 * SEC)), 0);
    // Idle: no cadence.
    h.ok(Command::End);
    assert_eq!(persist_count(&h.run(60 * SEC)), 0);
}

#[test]
fn failed_saves_retry_on_every_running_tick() {
    let mut h = H::new();
    start25(&mut h);
    h.run(14 * SEC);
    h.saving = false;
    assert_eq!(persist_count(&h.run(SEC)), 1);
    assert_eq!(
        persist_count(&h.run(SEC)),
        1,
        "not marked persisted, so it retries"
    );
    assert_eq!(persist_count(&h.run(SEC)), 1);
    h.saving = true;
    assert_eq!(persist_count(&h.run(SEC)), 1);
    assert_eq!(persist_count(&h.run(SEC)), 0);
}

#[test]
fn persisted_state_never_folds_past_the_last_observation() {
    let mut h = H::new();
    start25(&mut h);
    h.run(10 * MIN);
    h.clock.advance(5 * SEC);
    let now = h.now();
    let p = h.engine.persisted_state(now);
    let cp = p.active_session.unwrap();
    assert_eq!(cp.accrued_ms, 600_000);
    assert_eq!(cp.checkpoint_at, now.wall);
    assert_eq!(cp.status, RunStatus::Running);
    assert_eq!(p.schema_version, focus_core::store::SCHEMA_VERSION);
    assert_eq!(active_ms(&h), 600_000);
    // Pure: calling it again changes nothing.
    assert_eq!(
        h.engine
            .persisted_state(now)
            .active_session
            .unwrap()
            .accrued_ms,
        600_000
    );
}

#[test]
fn quit_then_relaunch_restores_paused() {
    let mut h = H::new();
    start25(&mut h);
    h.run(10 * MIN);
    h.ok(Command::Quit);
    let fx = h.relaunch(30 * MIN, false);
    assert_eq!(
        effect_names(&fx),
        ["Restored"],
        "a paused checkpoint restores unchanged"
    );
    assert_eq!(
        transitions(&fx),
        [Transition::Restored {
            session_id: "s1".into(),
            kind: SessionKind::Focus
        }]
    );
    let s = h.snap();
    assert_eq!(s.phase, Phase::Focus);
    let v = s.session.unwrap();
    assert_eq!(v.status, RunStatus::Paused);
    assert_eq!(v.pause_reason, Some(PauseReason::Quit));
    assert_eq!(v.active_ms, 600_000);
    assert_eq!(v.remaining_label, "15:00");
    // Closed time counts toward the pause age.
    assert_eq!(
        h.engine.active().unwrap().pause_age_ms(h.now().wall),
        Some(30 * MIN)
    );
    assert!(!h.engine.is_running());
    assert_eq!(h.activity, focus_core::ActivityLevel::None);
}

#[test]
fn relaunch_after_a_crash_restores_the_running_checkpoint_paused() {
    let mut h = H::new();
    start25(&mut h);
    h.run(10 * MIN + 7 * SEC); // last checkpoint at 10:00
    let fx = h.relaunch(60 * MIN, false);
    assert_eq!(effect_names(&fx), ["Persist", "Restored"]);
    let v = h.snap().session.unwrap();
    assert_eq!(v.id, "s1");
    assert_eq!(v.status, RunStatus::Paused);
    assert_eq!(v.pause_reason, Some(PauseReason::Restored));
    assert_eq!(v.active_ms, 600_000);
    // Closed-app time earns nothing.
    h.run(30 * MIN);
    assert_eq!(active_ms(&h), 600_000);
    h.ok(Command::Resume);
    h.run(15 * MIN);
    assert_eq!(h.engine.friendship().total_points, 30);
    assert_eq!(h.engine.history().len(), 1);
}

#[test]
fn crash_loses_at_most_one_checkpoint_interval() {
    let mut h = H::new();
    start25(&mut h);
    h.run(10 * MIN + 14 * SEC);
    h.relaunch(0, false);
    assert_eq!(active_ms(&h), 600_000);
}

#[test]
fn monotonic_origin_does_not_matter_across_relaunch() {
    for counting in [false, true] {
        let mut h = H::new();
        start25(&mut h);
        h.run(10 * MIN);
        h.relaunch(20 * MIN, counting);
        assert_eq!(active_ms(&h), 600_000, "counting={counting}");
        h.ok(Command::Resume);
        h.run(15 * MIN);
        assert_eq!(
            h.engine.friendship().total_points,
            30,
            "counting={counting}"
        );
    }
}

#[test]
fn relaunch_after_a_long_close_expires_with_minutes_kept() {
    let mut h = H::new();
    start25(&mut h);
    h.run(10 * MIN);
    let checkpoint_wall = h.now().wall;
    let fx = h.relaunch(3 * HOUR, false);
    assert_eq!(effect_names(&fx), ["Persist", "FocusInterrupted"]);
    match &transitions(&fx)[0] {
        Transition::FocusInterrupted { record, award } => {
            assert_eq!(record.end_reason, EndReason::PauseExpired);
            assert_eq!(record.ended_at, checkpoint_wall);
            assert_eq!(record.active_ms, 600_000);
            assert_eq!(award.as_ref().unwrap().points, 10);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(h.snap().phase, Phase::Idle);
    assert_eq!(h.snap().today.harvest, vec![HarvestItem::Leaf]);
    assert!(!fx
        .iter()
        .any(|e| matches!(e, Effect::ShowCompanion | Effect::ActivityChanged { .. })));
    // The expiry was saved.
    assert!(h.saved().active_session.is_none());
    assert_eq!(h.saved().friendship.total_points, 10);
}

#[test]
fn relaunch_keeps_a_user_pause_and_its_anchor() {
    let mut h = H::new();
    start25(&mut h);
    h.run(10 * MIN);
    h.ok(Command::Pause);
    h.clock.advance(30 * MIN);
    h.ok(Command::Quit);
    h.relaunch(60 * MIN, false);
    assert_eq!(
        h.snap().session.unwrap().pause_reason,
        Some(PauseReason::User)
    );
    h.clock.advance(29 * MIN);
    h.tick();
    assert!(h.engine.active().is_some(), "119 min");
    h.clock.advance(2 * MIN);
    let fx = h.tick();
    assert!(matches!(
        transitions(&fx)[..],
        [Transition::FocusInterrupted { .. }]
    ));
}

#[test]
fn awaiting_choice_is_not_persisted() {
    let mut h = H::new();
    h.ok(Command::StartFocus {
        minutes: Some(1),
        task_label: None,
    });
    h.run(MIN);
    assert_eq!(h.snap().phase, Phase::AwaitingChoice);
    let fx = h.relaunch(MIN, false);
    assert!(fx.is_empty(), "{fx:?}");
    let s = h.snap();
    assert_eq!(s.phase, Phase::Idle);
    assert_eq!(s.today.harvest, vec![HarvestItem::Acorn]);
    assert_eq!(s.friendship.total_points, 6);
}

#[test]
fn crash_before_the_completion_save() {
    let mut h = H::new();
    start25(&mut h);
    h.run(24 * MIN + 50 * SEC); // last saved at 24:45
    h.saving = false;
    let fx = h.run(10 * SEC);
    assert!(transitions(&fx)
        .iter()
        .any(|t| matches!(t, Transition::FocusCompleted { .. })));
    // Crash: the completion never reached the disk.
    h.saving = true;
    h.relaunch(MIN, false);
    let v = h.snap().session.unwrap();
    assert_eq!(v.status, RunStatus::Paused);
    assert_eq!(v.remaining_ms, 15_000);
    h.ok(Command::Resume);
    let fx = h.run(15 * SEC);
    assert!(transitions(&fx)
        .iter()
        .any(|t| matches!(t, Transition::FocusCompleted { .. })));
    assert_eq!(h.engine.friendship().total_points, 30);
    assert_eq!(h.engine.history().len(), 1);
}

#[test]
fn crash_after_the_completion_save() {
    let mut h = H::new();
    start25(&mut h);
    h.run(25 * MIN);
    let fx = h.relaunch(MIN, true);
    assert!(fx.is_empty());
    assert_eq!(h.snap().phase, Phase::Idle);
    assert_eq!(h.engine.friendship().total_points, 30);
    assert_eq!(h.engine.history().len(), 1);
    // Relaunching again changes nothing.
    assert!(h.relaunch(MIN, false).is_empty());
    assert_eq!(h.engine.friendship().total_points, 30);
}

#[test]
fn a_clean_state_produces_no_effects() {
    let h = h_with(reconciled_state());
    assert!(h.launch_fx.is_empty());
    assert_eq!(h.engine.revision(), 0);
    assert!(h.engine.baseline().is_some());
    assert!(!h.engine.is_suspended());
    assert!(!h.engine.hidden_for_focus());
}

#[test]
fn a_fresh_install_reconciles_and_saves() {
    let h = H::new();
    assert_eq!(effect_names(&h.launch_fx), ["Persist"]);
    assert!(h.engine.friendship().unlocked.contains("base"));
}

#[test]
fn checkpoint_already_in_history_is_dropped() {
    let clock = FakeClock::default_start();
    let mut state = reconciled_state();
    state.history.push(record("x", &clock));
    state
        .friendship
        .awards
        .insert("x".into(), award(25, 5, &clock));
    state.friendship.total_points = 30;
    state.friendship.unlocked.insert("wave".into());
    state.active_session = Some(checkpoint(
        "x",
        RunStatus::Running,
        1_490_000,
        25 * MIN,
        &clock,
    ));
    let h = H::with(
        Default::default(),
        focus_core::rewards::UnlockTable::builtin(),
        state,
        clock,
    );
    assert_eq!(effect_names(&h.launch_fx), ["Persist"]);
    assert!(h.engine.active().is_none());
    assert_eq!(h.engine.friendship().total_points, 30);
    assert_eq!(h.engine.history().len(), 1);
    assert!(h.saved().active_session.is_none());
}

#[test]
fn checkpoint_already_awarded_is_dropped() {
    let clock = FakeClock::default_start();
    let mut state = reconciled_state();
    state
        .friendship
        .awards
        .insert("x".into(), award(10, 0, &clock));
    state.friendship.total_points = 10;
    state.active_session = Some(checkpoint(
        "x",
        RunStatus::Paused,
        10 * MIN,
        25 * MIN,
        &clock,
    ));
    let h = H::with(
        Default::default(),
        focus_core::rewards::UnlockTable::builtin(),
        state,
        clock,
    );
    assert_eq!(effect_names(&h.launch_fx), ["Persist"]);
    assert!(h.engine.active().is_none());
    assert_eq!(h.engine.friendship().total_points, 10);
}

#[test]
fn invalid_planned_durations_are_dropped() {
    for planned in [0, 30_000, 181 * MIN] {
        let clock = FakeClock::default_start();
        let mut state = reconciled_state();
        state.active_session = Some(checkpoint("x", RunStatus::Running, 0, planned, &clock));
        let h = H::with(
            Default::default(),
            focus_core::rewards::UnlockTable::builtin(),
            state,
            clock,
        );
        assert_eq!(effect_names(&h.launch_fx), ["Persist"], "{planned}");
        assert!(h.engine.active().is_none());
        assert_eq!(
            h.engine.friendship().total_points,
            0,
            "no bonus for a planned-0 checkpoint"
        );
    }
}

#[test]
fn an_empty_checkpoint_id_is_dropped() {
    let clock = FakeClock::default_start();
    let mut state = reconciled_state();
    state.active_session = Some(checkpoint("", RunStatus::Running, 0, 25 * MIN, &clock));
    let h = H::with(
        Default::default(),
        focus_core::rewards::UnlockTable::builtin(),
        state,
        clock,
    );
    assert!(h.engine.active().is_none());
}

#[test]
fn a_fully_served_checkpoint_completes_silently() {
    for status in [RunStatus::Running, RunStatus::Paused] {
        let clock = FakeClock::default_start();
        let mut state = reconciled_state();
        let cp = checkpoint("x", status, 25 * MIN, 25 * MIN, &clock);
        let checkpoint_at = cp.checkpoint_at;
        state.active_session = Some(cp);
        let h = H::with(
            Default::default(),
            focus_core::rewards::UnlockTable::builtin(),
            state,
            clock,
        );
        assert_eq!(
            effect_names(&h.launch_fx),
            ["Persist"],
            "no transition, no sound ({status:?})"
        );
        assert_eq!(h.snap().phase, Phase::Idle);
        let r = &h.engine.history().records()[0];
        assert_eq!(r.outcome, Outcome::Completed);
        assert_eq!(r.ended_at, checkpoint_at);
        assert_eq!(r.active_ms, 25 * MIN);
        assert_eq!(r.task_label.as_deref(), Some("Lecture"));
        assert_eq!(h.engine.friendship().total_points, 30);
        assert!(h.engine.friendship().unlocked.contains("wave"));
        assert!(h.saved().active_session.is_none());
        assert_eq!(h.saved().history.len(), 1);
    }
}

#[test]
fn an_overfull_checkpoint_is_clamped_and_completes() {
    let clock = FakeClock::default_start();
    let mut state = reconciled_state();
    state.active_session = Some(checkpoint(
        "x",
        RunStatus::Running,
        99 * MIN,
        25 * MIN,
        &clock,
    ));
    let h = H::with(
        Default::default(),
        focus_core::rewards::UnlockTable::builtin(),
        state,
        clock,
    );
    assert_eq!(h.engine.history().records()[0].active_ms, 25 * MIN);
    assert_eq!(h.engine.friendship().total_points, 30);
}

#[test]
fn future_pause_stamps_are_clamped_to_now() {
    let clock = FakeClock::default_start();
    let mut state = reconciled_state();
    let mut cp = checkpoint("x", RunStatus::Paused, 10 * MIN, 25 * MIN, &clock);
    let future = cp.checkpoint_at + TimeDelta::days(1);
    cp.paused_at = Some(future);
    cp.age_anchor = Some(future);
    cp.pause_reason = Some(PauseReason::User);
    state.active_session = Some(cp);
    let mut h = H::with(
        Default::default(),
        focus_core::rewards::UnlockTable::builtin(),
        state,
        clock,
    );
    assert_eq!(effect_names(&h.launch_fx), ["Persist", "Restored"]);
    let launch = h.now().wall;
    assert_eq!(h.engine.active().unwrap().paused_at(), Some(launch));
    assert_eq!(h.engine.active().unwrap().age_anchor(), Some(launch));
    h.clock.advance(120 * MIN);
    h.tick();
    assert!(h.engine.active().is_some());
    h.clock.advance(1);
    let fx = h.tick();
    match &transitions(&fx)[0] {
        Transition::FocusInterrupted { record, .. } => assert_eq!(record.ended_at, launch),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_break_checkpoint_restores_as_a_paused_break() {
    let mut h = H::new();
    h.ok(Command::StartBreak { minutes: None });
    h.run(2 * MIN);
    h.relaunch(MIN, false);
    let s = h.snap();
    assert_eq!(s.phase, Phase::Break);
    let v = s.session.unwrap();
    assert_eq!(v.kind, SessionKind::Break);
    assert_eq!(v.status, RunStatus::Paused);
    assert_eq!(v.active_ms, 2 * MIN);
    assert!(matches!(
        transitions(&h.launch_fx)[..],
        [Transition::Restored {
            kind: SessionKind::Break,
            ..
        }]
    ));
    h.ok(Command::Resume);
    let fx = h.run(3 * MIN);
    assert!(transitions(&fx)
        .iter()
        .any(|t| matches!(t, Transition::BreakFinished { .. })));
    assert_eq!(h.engine.friendship().total_points, 0);
}

#[test]
fn a_break_checkpoint_closed_too_long_ends_silently() {
    let mut h = H::new();
    h.ok(Command::StartBreak { minutes: None });
    h.run(2 * MIN);
    let fx = h.relaunch(3 * HOUR, false);
    assert_eq!(effect_names(&fx), ["Persist", "BreakEnded"]);
    assert_eq!(h.snap().phase, Phase::Idle);
    assert!(h.engine.history().is_empty());
}

#[test]
fn load_repairs_settings_history_and_friendship() {
    let clock = FakeClock::default_start();
    let mut state = PersistedState {
        settings: Settings {
            last_focus_minutes: 999,
            palette: "evening".into(), // not unlocked
            active_pack: "Bad Id".into(),
            ..Settings::default()
        },
        ..PersistedState::default()
    };
    state.history = vec![
        record("a", &clock),
        record("b", &clock),
        record("a", &clock),
    ];
    state
        .friendship
        .awards
        .insert("a".into(), award(25, 5, &clock));
    state
        .friendship
        .awards
        .insert("b".into(), award(25, 5, &clock));
    state.friendship.total_points = 0; // stored total below the ledger
    let h = H::with(
        Default::default(),
        focus_core::rewards::UnlockTable::builtin(),
        state,
        clock,
    );
    assert_eq!(effect_names(&h.launch_fx), ["Persist"]);
    assert!(!h
        .launch_fx
        .iter()
        .any(|e| matches!(e, Effect::SettingsChanged { .. })));
    let s = h.engine.settings();
    assert_eq!(s.last_focus_minutes, 25);
    assert_eq!(s.palette, "auto");
    assert_eq!(s.active_pack, "placeholder");
    assert_eq!(h.engine.history().len(), 2);
    assert_eq!(h.engine.friendship().total_points, 60);
    assert!(h.engine.friendship().unlocked.contains("stretch"));
    // The repaired state is what gets saved.
    assert_eq!(h.saved().history.len(), 2);
    assert_eq!(h.saved().friendship.total_points, 60);
}

#[test]
fn an_unlocked_palette_survives_load() {
    let clock = FakeClock::default_start();
    let mut state = reconciled_state();
    state.friendship.total_points = 200;
    for id in ["base", "wave", "stretch", "umbrella", "evening"] {
        state.friendship.unlocked.insert(id.into());
    }
    state.settings.palette = "evening".into();
    let h = H::with(
        Default::default(),
        focus_core::rewards::UnlockTable::builtin(),
        state,
        clock,
    );
    assert!(h.launch_fx.is_empty());
    assert_eq!(h.engine.settings().palette, "evening");
}

#[test]
fn ids_continue_across_relaunch() {
    let mut h = H::new();
    start25(&mut h);
    h.run(MIN);
    h.ok(Command::End);
    h.relaunch(MIN, false);
    let fx = h.ok(Command::StartFocus {
        minutes: None,
        task_label: None,
    });
    assert!(
        matches!(&transitions(&fx)[0], Transition::FocusStarted { session_id, .. } if session_id == "s2")
    );
}

#[test]
fn colliding_ids_from_the_id_source_are_skipped() {
    // A state whose history already holds the ids the counter will produce.
    let clock = FakeClock::default_start();
    let mut state = reconciled_state();
    state.history = vec![record("s1", &clock), record("s2", &clock)];
    state
        .friendship
        .awards
        .insert("s1".into(), award(25, 5, &clock));
    state
        .friendship
        .awards
        .insert("s2".into(), award(25, 5, &clock));
    state.friendship.total_points = 60;
    let mut h = H::with(
        Default::default(),
        focus_core::rewards::UnlockTable::builtin(),
        state,
        clock,
    );
    let fx = h.ok(Command::StartFocus {
        minutes: Some(1),
        task_label: None,
    });
    assert!(
        matches!(&transitions(&fx)[0], Transition::FocusStarted { session_id, .. } if session_id == "s3")
    );
    h.run(MIN);
    assert_eq!(
        h.engine.friendship().total_points,
        66,
        "the new session was awarded"
    );
}

#[test]
fn settings_friendship_and_history_persist_across_restarts() {
    let mut h = H::new();
    h.ok(Command::UpdateSettings(
        focus_core::settings::UserSettingsPatch {
            sound: Some(false),
            hide_during_focus: Some(true),
            menu_bar_title: Some(false),
            ..Default::default()
        }
        .into(),
    ));
    h.ok(Command::StartFocus {
        minutes: Some(30),
        task_label: Some("Essay".into()),
    });
    h.run(30 * MIN);
    h.ok(Command::Quit);
    h.relaunch(8 * HOUR, false);
    let s = h.engine.settings();
    assert!(!s.sound && s.hide_during_focus && !s.menu_bar_title);
    assert_eq!(s.last_focus_minutes, 30);
    assert_eq!(h.engine.friendship().total_points, 35);
    assert_eq!(
        h.engine.history().records()[0].task_label.as_deref(),
        Some("Essay")
    );
}

#[test]
fn restored_session_persists_through_another_relaunch() {
    let mut h = H::new();
    start25(&mut h);
    h.run(10 * MIN);
    h.relaunch(5 * MIN, false);
    h.relaunch(5 * MIN, true);
    let v = h.snap().session.unwrap();
    assert_eq!(v.pause_reason, Some(PauseReason::Restored));
    assert_eq!(v.active_ms, 600_000);
    // The anchor is still the original checkpoint time: 10 min of closed time so far.
    assert_eq!(
        h.engine.active().unwrap().pause_age_ms(h.now().wall),
        Some(10 * MIN)
    );
}

#[test]
fn suspend_saves_the_checkpoint_for_a_crash_during_sleep() {
    let mut h = H::new();
    start25(&mut h);
    h.run(10 * MIN + 5 * SEC);
    h.ev(SystemEvent::Suspend);
    // The machine never wakes cleanly: the app is relaunched later.
    h.relaunch(30 * MIN, false);
    let v = h.snap().session.unwrap();
    assert_eq!(v.active_ms, 10 * MIN + 5 * SEC, "the suspend save is exact");
    assert_eq!(v.pause_reason, Some(PauseReason::Suspend));
}

/// Focus 25, run 10 min, user pause, then a +3 h wall jump that `event` detects while
/// the app runs. Returns the harness, the true pause stamp and the event's effects.
fn paused_then_clock_jump(event: SystemEvent) -> (H, Wall, Vec<Effect>) {
    let mut h = H::new();
    start25(&mut h);
    h.run(10 * MIN);
    h.ok(Command::Pause);
    let paused_at = h.now().wall;
    h.clock.jump_wall(3 * HOUR as i64);
    let fx = h.ev(event);
    (h, paused_at, fx)
}

#[test]
fn a_detected_clock_change_while_paused_is_saved() {
    for event in [SystemEvent::ClockChanged, SystemEvent::Tick] {
        let (h, paused_at, fx) = paused_then_clock_jump(event);
        assert_eq!(effect_names(&fx), ["Persist"], "{event:?}");
        let shifted = paused_at + TimeDelta::hours(3);
        assert_eq!(h.engine.active().unwrap().age_anchor(), Some(shifted));
        let cp = h.saved().active_session.as_ref().unwrap();
        assert_eq!(cp.age_anchor, Some(shifted), "{event:?}: anchor on disk");
        assert_eq!(cp.paused_at, Some(paused_at), "pausedAt never shifts");
        assert_eq!(cp.pause_reason, Some(PauseReason::User));
    }
}

#[test]
fn crash_after_a_clock_change_while_paused_stays_paused() {
    let (mut h, paused_at, _) = paused_then_clock_jump(SystemEvent::ClockChanged);
    // Crash: no Quit, relaunched a second later.
    let fx = h.relaunch(SEC, false);
    assert!(
        matches!(transitions(&fx)[..], [Transition::Restored { .. }]),
        "a jump detected while the app runs never counts as pause time: {fx:?}"
    );
    let v = h.snap().session.unwrap();
    assert_eq!(v.status, RunStatus::Paused);
    assert_eq!(v.pause_reason, Some(PauseReason::User));
    assert_eq!(v.active_ms, 10 * MIN);
    let s = h.engine.active().unwrap();
    assert_eq!(s.age_anchor(), Some(paused_at + TimeDelta::hours(3)));
    assert_eq!(s.pause_age_ms(h.now().wall), Some(SEC));
    // Expiry still comes 120 min after the shifted anchor, recorded at the true stamp.
    h.clock.advance(120 * MIN - SEC);
    assert!(h.tick().is_empty());
    h.clock.advance(1);
    let fx = h.tick();
    match &transitions(&fx)[..] {
        [Transition::FocusInterrupted { record, .. }] => {
            assert_eq!(record.end_reason, EndReason::PauseExpired);
            assert_eq!(record.ended_at, paused_at);
            assert_eq!(record.active_ms, 10 * MIN);
        }
        other => panic!("expected expiry, got {other:?}"),
    }
}

#[test]
fn crash_during_sleep_after_a_clock_change_while_paused_stays_paused() {
    let mut h = H::new();
    start25(&mut h);
    h.run(10 * MIN);
    h.ok(Command::Pause);
    let paused_at = h.now().wall;
    h.clock.jump_wall(3 * HOUR as i64);
    // Suspend arrives before any tick saw the jump; its observation shifts the anchor.
    let fx = h.ev(SystemEvent::Suspend);
    assert_eq!(effect_names(&fx), ["Persist"]);
    h.sleep_mono_stopped(30 * MIN);
    // The battery dies during sleep: no Wake, relaunched later.
    let fx = h.relaunch(10 * MIN, false);
    assert!(matches!(
        transitions(&fx)[..],
        [Transition::Restored { .. }]
    ));
    let s = h.engine.active().unwrap();
    assert_eq!(s.age_anchor(), Some(paused_at + TimeDelta::hours(3)));
    assert_eq!(
        s.pause_age_ms(h.now().wall),
        Some(40 * MIN),
        "sleep and closed time count; the detected jump does not"
    );
}

#[test]
fn a_backward_clock_change_while_paused_is_saved() {
    let mut h = H::new();
    start25(&mut h);
    h.run(10 * MIN);
    h.ok(Command::Pause);
    let paused_at = h.now().wall;
    h.clock.jump_wall(-(3 * HOUR as i64));
    let fx = h.ev(SystemEvent::ClockChanged);
    assert_eq!(effect_names(&fx), ["Persist"]);
    let shifted = paused_at - TimeDelta::hours(3);
    assert_eq!(
        h.saved().active_session.as_ref().unwrap().age_anchor,
        Some(shifted)
    );
    h.clock.advance(20 * MIN);
    assert!(h.tick().is_empty());
    // Crash, relaunched 5 min later: 25 min of pause age, not 0.
    h.relaunch(5 * MIN, false);
    let s = h.engine.active().unwrap();
    assert_eq!(s.age_anchor(), Some(shifted));
    assert_eq!(s.pause_age_ms(h.now().wall), Some(25 * MIN));
}

#[test]
fn skew_within_tolerance_while_paused_saves_nothing() {
    let mut h = H::new();
    start25(&mut h);
    h.run(10 * MIN);
    h.ok(Command::Pause);
    let saves = h.saves;
    h.clock.jump_wall(60 * SEC as i64);
    assert!(h.ev(SystemEvent::ClockChanged).is_empty());
    h.clock.jump_wall(-(60 * SEC as i64));
    assert!(h.tick().is_empty());
    assert_eq!(h.saves, saves);
}

#[test]
fn a_paused_clock_change_save_does_not_bump_the_revision() {
    let (h, _, _) = paused_then_clock_jump(SystemEvent::ClockChanged);
    let mut g = H::new();
    start25(&mut g);
    g.run(10 * MIN);
    g.ok(Command::Pause);
    assert_eq!(h.engine.revision(), g.engine.revision());
}
