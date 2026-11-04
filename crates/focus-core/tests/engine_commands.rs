//! Engine commands (SPEC §4.0–4.1, §4.10): validation, toggle, labels, settings, quit.

use focus_core::clock::{Mono, Now};
use focus_core::engine::{ActivityLevel, Command, CommandError, Effect, Transition};
use focus_core::settings::{SettingsPatch, UserSettingsPatch};
use focus_core::snapshot::Phase;
use focus_core::testkit::{effect_names, transitions, H, MIN, SEC};
use focus_core::timer::{PauseReason, RunStatus, SessionKind};

fn start_focus(minutes: Option<u32>, label: Option<&str>) -> Command {
    Command::StartFocus {
        minutes,
        task_label: label.map(str::to_string),
    }
}

fn settings_cmd(p: UserSettingsPatch) -> Command {
    Command::UpdateSettings(p.into())
}

/// Assert `cmd` fails with `err` and changes nothing observable.
fn assert_rejected(h: &mut H, cmd: Command, err: CommandError) {
    let now = h.now();
    let snap = h.engine.snapshot(now, &*h.clock);
    let saved = h.engine.persisted_state(now);
    let rev = h.engine.revision();
    let baseline = h.engine.baseline();
    assert_eq!(h.cmd(cmd.clone()), Err(err), "{cmd:?}");
    assert_eq!(
        h.engine.snapshot(now, &*h.clock),
        snap,
        "{cmd:?} changed the snapshot"
    );
    assert_eq!(
        h.engine.persisted_state(now),
        saved,
        "{cmd:?} changed persisted state"
    );
    assert_eq!(h.engine.revision(), rev);
    assert_eq!(h.engine.baseline(), baseline, "{cmd:?} observed time");
}

#[test]
fn start_focus_defaults_to_last_duration_without_label() {
    let mut h = H::new();
    let fx = h.ok(start_focus(None, None));
    assert_eq!(
        effect_names(&fx),
        ["Persist", "ActivityChanged", "FocusStarted"]
    );
    assert_eq!(
        fx[1],
        Effect::ActivityChanged {
            level: ActivityLevel::Focus
        }
    );
    assert_eq!(
        fx[2],
        Effect::Transition(Transition::FocusStarted {
            session_id: "s1".into(),
            planned_ms: 1_500_000
        })
    );
    let s = h.snap();
    assert_eq!(s.phase, Phase::Focus);
    let v = s.session.unwrap();
    assert_eq!(v.id, "s1");
    assert_eq!(v.kind, SessionKind::Focus);
    assert_eq!(v.status, RunStatus::Running);
    assert_eq!(v.task_label, None);
    assert_eq!(v.planned_ms, 1_500_000);
    assert_eq!(v.remaining_ms, 1_500_000);
    assert_eq!(v.remaining_label, "25:00");
    assert_eq!(v.started_at, h.now().wall);
    assert_eq!(h.activity, ActivityLevel::Focus);
    // The start was saved with a running checkpoint.
    let cp = h.saved().active_session.clone().unwrap();
    assert_eq!(cp.status, RunStatus::Running);
    assert_eq!(cp.id, "s1");
}

#[test]
fn start_focus_custom_updates_last_duration() {
    let mut h = H::new();
    let fx = h.ok(start_focus(Some(37), None));
    assert_eq!(
        effect_names(&fx),
        [
            "Persist",
            "SettingsChanged",
            "ActivityChanged",
            "FocusStarted"
        ]
    );
    match &fx[1] {
        Effect::SettingsChanged { previous, current } => {
            assert_eq!(previous.last_focus_minutes, 25);
            assert_eq!(current.last_focus_minutes, 37);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(h.engine.settings().last_focus_minutes, 37);
    h.ok(Command::End);
    let fx = h.ok(Command::Toggle);
    assert!(transitions(&fx).contains(&Transition::FocusStarted {
        session_id: "s2".into(),
        planned_ms: 37 * MIN
    }));
    // Same duration again: no SettingsChanged.
    h.ok(Command::End);
    let fx = h.ok(start_focus(Some(37), None));
    assert_eq!(
        effect_names(&fx),
        ["Persist", "ActivityChanged", "FocusStarted"]
    );
}

#[test]
fn start_focus_bounds() {
    let mut h = H::new();
    for m in [0, 181, u32::MAX] {
        assert_rejected(
            &mut h,
            start_focus(Some(m), None),
            CommandError::InvalidMinutes { min: 1, max: 180 },
        );
    }
    for m in [1, 180] {
        h.ok(start_focus(Some(m), None));
        assert_eq!(h.snap().session.unwrap().planned_ms, u64::from(m) * MIN);
        h.ok(Command::End);
    }
    assert_eq!(h.snap().phase, Phase::Idle);
}

#[test]
fn start_while_a_session_exists_is_rejected() {
    let mut h = H::new();
    h.ok(start_focus(None, None));
    assert_rejected(&mut h, start_focus(None, None), CommandError::SessionActive);
    assert_rejected(
        &mut h,
        Command::StartBreak { minutes: None },
        CommandError::SessionActive,
    );
    // SessionActive wins over invalid minutes.
    assert_rejected(
        &mut h,
        start_focus(Some(0), None),
        CommandError::SessionActive,
    );
    h.ok(Command::Pause);
    assert_rejected(&mut h, start_focus(None, None), CommandError::SessionActive);
    h.ok(Command::End);
    h.ok(Command::StartBreak { minutes: None });
    assert_rejected(&mut h, start_focus(None, None), CommandError::SessionActive);
    assert_rejected(
        &mut h,
        Command::StartBreak { minutes: Some(5) },
        CommandError::SessionActive,
    );
}

#[test]
fn task_label_is_normalized_and_recorded() {
    let mut h = H::new();
    h.ok(start_focus(None, Some("  Review\none\tlecture  ")));
    assert_eq!(
        h.snap().session.unwrap().task_label.as_deref(),
        Some("Review one lecture")
    );
    h.run(MIN);
    let fx = h.ok(Command::End);
    match &transitions(&fx)[0] {
        Transition::FocusInterrupted { record, .. } => {
            assert_eq!(record.task_label.as_deref(), Some("Review one lecture"))
        }
        other => panic!("{other:?}"),
    }
    // Whitespace-only label → none; overlong → 80 chars.
    h.ok(start_focus(None, Some(" \n\t ")));
    assert_eq!(h.snap().session.unwrap().task_label, None);
    h.ok(Command::End);
    h.ok(start_focus(None, Some(&"y".repeat(200))));
    assert_eq!(
        h.snap()
            .session
            .unwrap()
            .task_label
            .unwrap()
            .chars()
            .count(),
        80
    );
}

#[test]
fn toggle_matrix() {
    // idle → FocusStarted with the last duration and no label.
    let mut h = H::new();
    let fx = h.ok(Command::Toggle);
    assert_eq!(
        transitions(&fx),
        [Transition::FocusStarted {
            session_id: "s1".into(),
            planned_ms: 25 * MIN
        }]
    );
    assert_eq!(h.snap().session.unwrap().task_label, None);
    // focus running → Paused{User}.
    h.run(5 * SEC);
    let fx = h.ok(Command::Toggle);
    assert_eq!(
        transitions(&fx),
        [Transition::Paused {
            session_id: "s1".into(),
            kind: SessionKind::Focus,
            reason: PauseReason::User
        }]
    );
    // focus paused → Resumed.
    let fx = h.ok(Command::Toggle);
    assert_eq!(
        transitions(&fx),
        [Transition::Resumed {
            session_id: "s1".into(),
            kind: SessionKind::Focus
        }]
    );
    // awaiting(after focus) → FocusStarted.
    h.run(25 * MIN);
    assert_eq!(h.snap().phase, Phase::AwaitingChoice);
    let fx = h.ok(Command::Toggle);
    assert!(matches!(
        transitions(&fx)[..],
        [Transition::FocusStarted {
            planned_ms: 1_500_000,
            ..
        }]
    ));
    h.ok(Command::End);
    // break running → Paused{User}; break paused → Resumed.
    h.ok(Command::StartBreak { minutes: None });
    let fx = h.ok(Command::Toggle);
    assert!(matches!(
        transitions(&fx)[..],
        [Transition::Paused {
            kind: SessionKind::Break,
            reason: PauseReason::User,
            ..
        }]
    ));
    let fx = h.ok(Command::Toggle);
    assert!(matches!(
        transitions(&fx)[..],
        [Transition::Resumed {
            kind: SessionKind::Break,
            ..
        }]
    ));
    // awaiting(after break) → FocusStarted.
    h.run(5 * MIN);
    assert_eq!(h.engine.awaiting().unwrap().after, SessionKind::Break);
    let fx = h.ok(Command::Toggle);
    assert!(matches!(
        transitions(&fx)[..],
        [Transition::FocusStarted {
            planned_ms: 1_500_000,
            ..
        }]
    ));
    assert!(h.engine.awaiting().is_none());
}

#[test]
fn toggle_never_fails_even_with_out_of_range_saved_minutes() {
    // A config with a narrower range than the saved duration: load sanitizes it into
    // range, and Toggle (which never fails) starts that duration.
    let cfg = focus_core::EngineConfig {
        min_minutes: 30,
        max_minutes: 60,
        ..Default::default()
    };
    let mut state = focus_core::store::PersistedState::default();
    state.settings.last_focus_minutes = 25;
    let mut h = H::with(
        cfg,
        focus_core::rewards::UnlockTable::builtin(),
        state,
        focus_core::FakeClock::default_start(),
    );
    assert_eq!(h.engine.settings().last_focus_minutes, 30);
    let fx = h.cmd(Command::Toggle).unwrap();
    assert!(
        matches!(transitions(&fx)[..], [Transition::FocusStarted { planned_ms, .. }] if planned_ms == 30 * MIN)
    );
    assert!(h.cmd(Command::Toggle).is_ok());
    assert!(h.cmd(Command::Toggle).is_ok());
}

#[test]
fn pause_and_resume_are_idempotent() {
    let mut h = H::new();
    h.ok(start_focus(None, None));
    h.run(10 * SEC);
    let fx = h.ok(Command::Pause);
    assert_eq!(effect_names(&fx), ["Persist", "ActivityChanged", "Paused"]);
    let rev = h.engine.revision();
    assert_eq!(h.ok(Command::Pause), vec![]);
    assert_eq!(h.engine.revision(), rev);
    let fx = h.ok(Command::Resume);
    assert_eq!(effect_names(&fx), ["Persist", "ActivityChanged", "Resumed"]);
    let rev = h.engine.revision();
    assert_eq!(h.ok(Command::Resume), vec![]);
    assert_eq!(h.engine.revision(), rev);
}

#[test]
fn pause_freezes_remaining_time_and_resume_continues() {
    let mut h = H::new();
    h.ok(start_focus(None, None));
    h.run(10 * MIN);
    h.ok(Command::Pause);
    h.clock.advance(60 * MIN);
    h.tick();
    let v = h.snap().session.unwrap();
    assert_eq!(v.remaining_ms, 900_000);
    assert_eq!(v.remaining_label, "15:00");
    assert_eq!(v.status, RunStatus::Paused);
    h.ok(Command::Resume);
    assert_eq!(h.snap().session.unwrap().remaining_ms, 900_000);
    let fx = h.run(15 * MIN);
    assert!(transitions(&fx)
        .iter()
        .any(|t| matches!(t, Transition::FocusCompleted { .. })));
    assert_eq!(h.engine.friendship().total_points, 30);
}

#[test]
fn commands_without_a_session() {
    let mut h = H::new();
    for c in [Command::Pause, Command::Resume, Command::End] {
        assert_rejected(&mut h, c, CommandError::NoSession);
    }
    // Also while awaiting a choice.
    h.ok(start_focus(Some(1), None));
    h.run(MIN);
    assert_eq!(h.snap().phase, Phase::AwaitingChoice);
    for c in [Command::Pause, Command::Resume, Command::End] {
        assert_rejected(&mut h, c, CommandError::NoSession);
    }
}

#[test]
fn dismiss_rules() {
    let mut h = H::new();
    assert_rejected(&mut h, Command::Dismiss, CommandError::NotAwaitingChoice);
    h.ok(start_focus(Some(1), None));
    assert_rejected(&mut h, Command::Dismiss, CommandError::NotAwaitingChoice);
    h.run(MIN);
    let saves = h.saves;
    let rev = h.engine.revision();
    let fx = h.ok(Command::Dismiss);
    assert_eq!(
        fx,
        vec![Effect::Transition(Transition::Dismissed)],
        "no Persist on dismiss"
    );
    assert_eq!(h.saves, saves);
    assert_eq!(h.snap().phase, Phase::Idle);
    assert_eq!(h.engine.revision(), rev + 1);
    assert_rejected(&mut h, Command::Dismiss, CommandError::NotAwaitingChoice);
}

#[test]
fn update_settings_is_all_or_nothing() {
    let mut h = H::new();
    let bad = UserSettingsPatch {
        sound: Some(false),
        last_focus_minutes: Some(0),
        ..Default::default()
    };
    let now = h.now();
    let snap = h.engine.snapshot(now, &*h.clock);
    match h.cmd(settings_cmd(bad)) {
        Err(CommandError::InvalidSettings { reason }) => assert!(!reason.is_empty()),
        other => panic!("{other:?}"),
    }
    assert!(h.engine.settings().sound);
    assert_eq!(h.engine.snapshot(now, &*h.clock), snap);
}

#[test]
fn update_settings_while_running_leaves_the_session_alone() {
    let mut h = H::new();
    h.ok(start_focus(None, None));
    h.run(5 * MIN);
    let fx = h.ok(settings_cmd(UserSettingsPatch {
        sound: Some(false),
        reduced_motion: Some(true),
        ..Default::default()
    }));
    assert_eq!(effect_names(&fx), ["Persist", "SettingsChanged"]);
    match &fx[1] {
        Effect::SettingsChanged { previous, current } => {
            assert!(previous.sound && !current.sound);
            assert!(!previous.reduced_motion && current.reduced_motion);
        }
        other => panic!("{other:?}"),
    }
    let v = h.snap().session.unwrap();
    assert_eq!(v.status, RunStatus::Running);
    assert_eq!(v.active_ms, 5 * MIN);
    h.run(MIN);
    assert_eq!(h.snap().session.unwrap().active_ms, 6 * MIN);
    // Saved.
    assert!(!h.saved().settings.sound);
}

#[test]
fn update_settings_noop_patch_changes_nothing() {
    let mut h = H::new();
    let rev = h.engine.revision();
    assert_eq!(h.ok(settings_cmd(UserSettingsPatch::default())), vec![]);
    assert_eq!(
        h.ok(settings_cmd(UserSettingsPatch {
            sound: Some(true),
            ..Default::default()
        })),
        vec![]
    );
    assert_eq!(h.engine.revision(), rev);
}

#[test]
fn update_settings_privileged_fields_via_internal_patch() {
    let mut h = H::new();
    let fx = h.ok(Command::UpdateSettings(SettingsPatch {
        hotkey: Some(None),
        active_pack: Some("my-pack".into()),
        widget_position: Some(Some(focus_core::settings::WidgetPosition {
            x: 4.0,
            y: 8.0,
        })),
        ..Default::default()
    }));
    assert_eq!(effect_names(&fx), ["Persist", "SettingsChanged"]);
    assert_eq!(h.engine.settings().hotkey, None);
    assert_eq!(h.engine.settings().active_pack, "my-pack");
    assert_rejected(
        &mut h,
        Command::UpdateSettings(SettingsPatch {
            active_pack: Some("Bad!".into()),
            ..Default::default()
        }),
        CommandError::InvalidSettings {
            reason: "invalid pack id \"Bad!\"".into(),
        },
    );
}

#[test]
fn palette_must_be_unlocked() {
    let mut h = H::new();
    let palette = |id: &str| {
        settings_cmd(UserSettingsPatch {
            palette: Some(id.into()),
            ..Default::default()
        })
    };
    for id in ["evening", "moss", "wave", "base", "unknown"] {
        match h.cmd(palette(id)) {
            Err(CommandError::InvalidSettings { .. }) => {}
            other => panic!("{id}: {other:?}"),
        }
    }
    h.ok(palette("forest"));
    assert_eq!(h.engine.settings().palette, "forest");
    h.ok(palette("auto"));
    assert!(h.engine.palette_allowed("forest") && h.engine.palette_allowed("auto"));
    assert!(!h.engine.palette_allowed("evening"));
    // Earn 160+ points: one 180-minute session (185 points) unlocks "evening".
    h.ok(start_focus(Some(180), None));
    h.run_step(180 * MIN, MIN);
    assert!(h.engine.friendship().unlocked.contains("evening"));
    h.ok(palette("evening"));
    assert_eq!(h.engine.settings().palette, "evening");
    // Unlocked but not a palette.
    match h.cmd(palette("wave")) {
        Err(CommandError::InvalidSettings { .. }) => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn quit_rules() {
    // Idle: always Persist.
    let mut h = H::new();
    assert_eq!(h.ok(Command::Quit), vec![Effect::Persist]);
    // Running: fold + Paused{Quit}.
    h.ok(start_focus(None, None));
    h.run(10 * MIN);
    let fx = h.ok(Command::Quit);
    assert_eq!(effect_names(&fx), ["Persist", "ActivityChanged", "Paused"]);
    assert!(matches!(
        transitions(&fx)[..],
        [Transition::Paused {
            reason: PauseReason::Quit,
            ..
        }]
    ));
    let cp = h.saved().active_session.clone().unwrap();
    assert_eq!(cp.status, RunStatus::Paused);
    assert_eq!(cp.pause_reason, Some(PauseReason::Quit));
    assert_eq!(cp.accrued_ms, 10 * MIN);
    // Paused: keeps its reason.
    let mut h = H::new();
    h.ok(start_focus(None, None));
    h.run(MIN);
    h.ok(Command::Pause);
    assert_eq!(h.ok(Command::Quit), vec![Effect::Persist]);
    assert_eq!(
        h.saved().active_session.as_ref().unwrap().pause_reason,
        Some(PauseReason::User)
    );
    // Awaiting: Persist (awaiting itself is not persisted).
    let mut h = H::new();
    h.ok(start_focus(Some(1), None));
    h.run(MIN);
    assert_eq!(h.ok(Command::Quit), vec![Effect::Persist]);
}

#[test]
fn start_break_rules() {
    let mut h = H::new();
    let fx = h.ok(Command::StartBreak { minutes: None });
    assert_eq!(
        effect_names(&fx),
        ["Persist", "ActivityChanged", "BreakStarted"]
    );
    assert_eq!(
        fx[1],
        Effect::ActivityChanged {
            level: ActivityLevel::Break
        }
    );
    assert_eq!(
        fx[2],
        Effect::Transition(Transition::BreakStarted {
            session_id: "s1".into(),
            planned_ms: 300_000
        })
    );
    let v = h.snap().session.unwrap();
    assert_eq!(v.kind, SessionKind::Break);
    assert_eq!(v.task_label, None);
    assert_eq!(h.snap().phase, Phase::Break);
    assert_eq!(
        h.engine.settings().last_focus_minutes,
        25,
        "breaks never change the last duration"
    );
    h.ok(Command::End);
    assert_rejected(
        &mut h,
        Command::StartBreak { minutes: Some(0) },
        CommandError::InvalidMinutes { min: 1, max: 180 },
    );
    assert_rejected(
        &mut h,
        Command::StartBreak { minutes: Some(181) },
        CommandError::InvalidMinutes { min: 1, max: 180 },
    );
    h.ok(Command::StartBreak { minutes: Some(10) });
    assert_eq!(h.snap().session.unwrap().planned_ms, 10 * MIN);
    assert_eq!(h.engine.settings().last_focus_minutes, 25);
}

#[test]
fn stale_now_is_tolerated() {
    let mut h = H::new();
    h.ok(start_focus(None, None));
    h.run(10 * SEC);
    let b = h.engine.baseline().unwrap();
    let stale = Now {
        mono: Mono(b.mono.0 - 500),
        wall: b.wall - chrono::TimeDelta::milliseconds(500),
    };
    let fx = h.engine.command(Command::Pause, stale).unwrap();
    assert!(matches!(
        transitions(&fx)[..],
        [Transition::Paused {
            reason: PauseReason::User,
            ..
        }]
    ));
    assert_eq!(
        h.engine.baseline().unwrap().mono,
        b.mono,
        "baseline never moves backward"
    );
    assert_eq!(h.engine.active().unwrap().accrued_ms, 10 * SEC);
    let fx = h.engine.system(focus_core::SystemEvent::Tick, stale);
    assert!(fx.is_empty());
}

#[test]
fn ids_are_unique_and_sequential() {
    let mut h = H::new();
    for i in 1..=5 {
        let fx = h.ok(start_focus(Some(1), None));
        assert!(
            matches!(&transitions(&fx)[0], Transition::FocusStarted { session_id, .. } if *session_id == format!("s{i}"))
        );
        h.ok(Command::End);
    }
    assert_eq!(h.engine.history().len(), 5);
}
