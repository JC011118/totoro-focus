//! Effects, ordering and revision (§4.8–4.9), hide-during-focus (§4.10), the snapshot
//! view model (§6), ticker cadence (§8.2) and replay validation (§10).

use chrono::{TimeZone, Utc};
use focus_core::engine::{ActivityLevel, Command, CommandError, Effect, SystemEvent, Transition};
use focus_core::history::HarvestItem;
use focus_core::rewards::UnlockTable;
use focus_core::settings::UserSettingsPatch;
use focus_core::snapshot::Phase;
use focus_core::store::PersistedState;
use focus_core::testkit::{effect_name, effect_names, transitions, H, HOUR, MIN, SEC};
use focus_core::timer::RunStatus;
use focus_core::{Engine, EngineConfig, FakeClock};

fn start(minutes: Option<u32>) -> Command {
    Command::StartFocus {
        minutes,
        task_label: None,
    }
}

fn hide_setting(on: bool) -> Command {
    Command::UpdateSettings(
        UserSettingsPatch {
            hide_during_focus: Some(on),
            ..Default::default()
        }
        .into(),
    )
}

fn rank(e: &Effect) -> usize {
    match e {
        Effect::Persist => 0,
        Effect::SettingsChanged { .. } => 1,
        Effect::ShowCompanion | Effect::HideCompanion => 2,
        Effect::ActivityChanged { .. } => 3,
        Effect::Transition(_) => 4,
        Effect::PlaySound(_) => 5,
    }
}

fn assert_ordered(fx: &[Effect]) {
    assert!(
        fx.windows(2).all(|w| rank(&w[0]) <= rank(&w[1])),
        "{:?}",
        effect_names(fx)
    );
    assert!(fx.iter().filter(|e| matches!(e, Effect::Persist)).count() <= 1);
}

#[test]
fn hide_on_start_and_show_before_the_celebration() {
    let mut h = H::with_settings(|s| {
        s.hide_during_focus = true;
        s.last_focus_minutes = 1;
    });
    let fx = h.ok(start(None));
    assert_eq!(
        effect_names(&fx),
        [
            "Persist",
            "HideCompanion",
            "ActivityChanged",
            "FocusStarted"
        ]
    );
    assert!(h.engine.hidden_for_focus());
    assert!(!h.companion_visible);
    let fx = h.run(MIN);
    let done: Vec<_> = fx
        .iter()
        .filter(|e| !matches!(e, Effect::Persist))
        .map(effect_name)
        .collect();
    assert_eq!(
        done,
        [
            "ShowCompanion",
            "ActivityChanged",
            "FocusCompleted",
            "PlaySound"
        ]
    );
    assert!(h.companion_visible);
    assert!(!h.engine.hidden_for_focus());
}

#[test]
fn show_on_interruption_and_on_expiry() {
    let mut h = H::with_settings(|s| s.hide_during_focus = true);
    h.ok(start(None));
    h.run(MIN);
    let fx = h.ok(Command::End);
    assert_eq!(
        effect_names(&fx),
        [
            "Persist",
            "ShowCompanion",
            "ActivityChanged",
            "FocusInterrupted"
        ]
    );
    assert!(h.companion_visible);

    let mut h = H::with_settings(|s| s.hide_during_focus = true);
    h.ok(start(None));
    h.run(MIN);
    h.ok(Command::Pause);
    h.clock.advance(121 * MIN);
    let fx = h.tick();
    assert_eq!(
        effect_names(&fx),
        ["Persist", "ShowCompanion", "FocusInterrupted"]
    );
    assert!(h.companion_visible);
}

#[test]
fn pause_resume_suspend_wake_never_hide_or_show() {
    let mut h = H::with_settings(|s| s.hide_during_focus = true);
    h.ok(start(None));
    let mut fx = Vec::new();
    fx.extend(h.ok(Command::Pause));
    fx.extend(h.ok(Command::Resume));
    fx.extend(h.ev(SystemEvent::Suspend));
    h.sleep_mono_stopped(10 * MIN);
    fx.extend(h.ev(SystemEvent::Wake));
    fx.extend(h.ok(Command::Resume));
    fx.extend(h.ev(SystemEvent::ClockChanged));
    fx.extend(h.ok(Command::Quit));
    assert!(!fx
        .iter()
        .any(|e| matches!(e, Effect::ShowCompanion | Effect::HideCompanion)));
    assert!(h.engine.hidden_for_focus());
}

#[test]
fn turning_the_setting_off_mid_session_shows_the_companion() {
    let mut h = H::with_settings(|s| s.hide_during_focus = true);
    h.ok(start(None));
    let fx = h.ok(hide_setting(false));
    assert_eq!(
        effect_names(&fx),
        ["Persist", "SettingsChanged", "ShowCompanion"]
    );
    assert!(!h.engine.hidden_for_focus());
    assert!(h.companion_visible);
    // Completion no longer needs to show it.
    let fx = h.run(25 * MIN);
    assert!(!fx.iter().any(|e| matches!(e, Effect::ShowCompanion)));
}

#[test]
fn turning_the_setting_on_mid_session_waits_for_the_next_start() {
    let mut h = H::new();
    h.ok(start(Some(1)));
    let fx = h.ok(hide_setting(true));
    assert_eq!(effect_names(&fx), ["Persist", "SettingsChanged"]);
    assert!(h.companion_visible);
    let fx = h.run(MIN);
    assert!(!fx
        .iter()
        .any(|e| matches!(e, Effect::ShowCompanion | Effect::HideCompanion)));
    let fx = h.ok(start(Some(1)));
    assert!(fx.contains(&Effect::HideCompanion));
}

#[test]
fn a_user_show_or_hide_clears_the_flag() {
    let mut h = H::with_settings(|s| s.hide_during_focus = true);
    h.ok(start(Some(1)));
    h.user_show_hide(true);
    assert!(!h.engine.hidden_for_focus());
    let fx = h.run(MIN);
    assert!(!fx.iter().any(|e| matches!(e, Effect::ShowCompanion)));
}

#[test]
fn no_show_or_hide_when_the_setting_is_off() {
    let mut h = H::new();
    let mut fx = h.ok(start(Some(1)));
    fx.extend(h.run(MIN));
    fx.extend(h.ok(Command::StartBreak { minutes: Some(1) }));
    fx.extend(h.run(MIN));
    fx.extend(h.ok(start(None)));
    fx.extend(h.ok(Command::End));
    assert!(!fx
        .iter()
        .any(|e| matches!(e, Effect::ShowCompanion | Effect::HideCompanion)));
}

#[test]
fn breaks_never_hide_the_companion() {
    let mut h = H::with_settings(|s| s.hide_during_focus = true);
    let fx = h.ok(Command::StartBreak { minutes: None });
    assert!(!fx.contains(&Effect::HideCompanion));
    assert!(!h.engine.hidden_for_focus());
}

#[test]
fn activity_tracks_what_is_running() {
    let mut h = H::new();
    let level = |fx: &[Effect]| -> Vec<ActivityLevel> {
        fx.iter()
            .filter_map(|e| match e {
                Effect::ActivityChanged { level } => Some(*level),
                _ => None,
            })
            .collect()
    };
    assert_eq!(level(&h.ok(start(Some(2)))), [ActivityLevel::Focus]);
    assert!(level(&h.run(10 * SEC)).is_empty(), "ticks never re-emit");
    assert_eq!(level(&h.ok(Command::Pause)), [ActivityLevel::None]);
    assert_eq!(level(&h.ok(Command::Resume)), [ActivityLevel::Focus]);
    assert_eq!(level(&h.ev(SystemEvent::Suspend)), [ActivityLevel::None]);
    assert!(level(&h.ev(SystemEvent::Wake)).is_empty());
    assert_eq!(level(&h.ok(Command::Resume)), [ActivityLevel::Focus]);
    assert_eq!(
        level(&h.run(2 * MIN)),
        [ActivityLevel::None],
        "completion, exactly once"
    );
    assert_eq!(h.activity, ActivityLevel::None);
    assert_eq!(
        level(&h.ok(Command::StartBreak { minutes: Some(1) })),
        [ActivityLevel::Break]
    );
    assert_eq!(h.engine.activity(), ActivityLevel::Break);
    assert_eq!(level(&h.ok(Command::Toggle)), [ActivityLevel::None]);
    assert_eq!(level(&h.ok(Command::Toggle)), [ActivityLevel::Break]);
    assert_eq!(level(&h.run(MIN)), [ActivityLevel::None]);
    assert!(level(&h.ok(Command::Dismiss)).is_empty());
    assert!(level(&h.ok(Command::Quit)).is_empty());
}

#[test]
fn effect_order_holds_across_a_full_day() {
    let mut h = H::with_settings(|s| s.hide_during_focus = true);
    let mut calls: Vec<Vec<Effect>> = Vec::new();
    let ticks = |h: &mut H, calls: &mut Vec<Vec<Effect>>, secs: u64| {
        for _ in 0..secs {
            h.clock.advance(SEC);
            calls.push(h.tick());
        }
    };
    calls.push(h.ok(start(Some(30))));
    ticks(&mut h, &mut calls, 30 * 60);
    calls.push(h.ok(Command::StartBreak { minutes: None }));
    ticks(&mut h, &mut calls, 5 * 60);
    calls.push(h.ok(start(Some(10))));
    calls.push(h.ok(hide_setting(false)));
    calls.push(h.ok(Command::Pause));
    h.clock.advance(3 * HOUR);
    calls.push(h.ok(Command::Toggle));
    for fx in &calls {
        assert_ordered(fx);
    }
}

#[test]
fn snapshot_is_pure() {
    let mut h = H::new();
    h.ok(start(None));
    h.run(3 * MIN);
    h.clock.advance(400);
    let rev = h.engine.revision();
    let now = h.now();
    let a = h.engine.snapshot(now, &*h.clock);
    let b = h.engine.snapshot(now, &*h.clock);
    assert_eq!(a, b);
    assert_eq!(h.engine.revision(), rev);
    assert_eq!(a.seq, 0);
    assert!(!a.companion_visible);
    // Repeated snapshots (renderer reloads, extra windows) never change the timer.
    for _ in 0..100 {
        let _ = h.snap();
    }
    assert_eq!(h.engine.snapshot(now, &*h.clock), a);
    h.run(MIN);
    assert_eq!(h.snap().session.unwrap().active_ms, 4 * MIN + 400);
}

#[test]
fn reloads_with_a_tick_first_do_not_alter_the_timer() {
    // The shell's get_snapshot runs system(Tick) and then snapshot(). Many reloads in
    // a row behave like ticks: the countdown is unchanged and nothing restarts.
    let mut h = H::new();
    h.ok(start(None));
    h.run(10 * MIN);
    for _ in 0..50 {
        h.tick();
        let s = h.snap();
        assert_eq!(s.session.as_ref().unwrap().remaining_ms, 15 * MIN);
        assert_eq!(s.session.unwrap().id, "s1");
    }
}

#[test]
fn revision_bumps_only_on_observable_changes() {
    let mut h = H::new();
    let r0 = h.engine.revision();
    h.ok(start(None));
    let r1 = h.engine.revision();
    assert_eq!(r1, r0 + 1);
    h.run(MIN);
    assert_eq!(h.engine.revision(), r1, "folding time does not bump");
    h.ok(Command::Pause);
    let r2 = h.engine.revision();
    assert_eq!(r2, r1 + 1);
    h.ok(Command::Pause);
    assert_eq!(h.engine.revision(), r2, "no-op pause");
    let _ = h.cmd(start(None));
    assert_eq!(h.engine.revision(), r2, "Err does not bump");
    h.clock.jump_wall(3 * HOUR as i64);
    h.tick();
    assert_eq!(h.engine.revision(), r2, "an anchor shift is not observable");
    h.ok(Command::Resume);
    assert_eq!(h.engine.revision(), r2 + 1);
    h.ok(Command::UpdateSettings(
        UserSettingsPatch {
            reduced_motion: Some(true),
            ..Default::default()
        }
        .into(),
    ));
    assert_eq!(h.engine.revision(), r2 + 2);
    h.run(24 * MIN);
    assert_eq!(h.engine.revision(), r2 + 3, "completion bumps once");
    h.ok(Command::Dismiss);
    assert_eq!(h.engine.revision(), r2 + 4);
    h.ok(Command::Quit);
    assert_eq!(
        h.engine.revision(),
        r2 + 4,
        "quit while idle changes nothing observable"
    );
    let s = h.snap();
    assert_eq!(s.revision, h.engine.revision());
}

#[test]
fn snapshot_session_view() {
    let mut h = H::new();
    h.ok(Command::StartFocus {
        minutes: Some(25),
        task_label: Some("Review one lecture".into()),
    });
    h.clock.advance(999);
    h.tick();
    let v = h.snap().session.unwrap();
    assert_eq!(v.remaining_ms, 1_499_001);
    assert_eq!(v.remaining_label, "25:00");
    assert_eq!(v.task_label.as_deref(), Some("Review one lecture"));
    h.clock.advance(1);
    h.tick();
    assert_eq!(h.snap().session.unwrap().remaining_label, "24:59");
    h.ok(Command::Pause);
    let v = h.snap().session.unwrap();
    assert_eq!(v.status, RunStatus::Paused);
    assert_eq!(v.pause_reason, Some(focus_core::timer::PauseReason::User));
    assert_eq!(v.remaining_label, "24:59");
}

#[test]
fn friendship_view_orders_by_table_and_appends_unknown_ids() {
    let mut state = PersistedState::default();
    for id in ["zzz_retired", "wave", "base", "aaa_retired"] {
        state.friendship.unlocked.insert(id.into());
    }
    state.friendship.total_points = 30;
    let h = H::with_state(state);
    let f = h.snap().friendship;
    assert_eq!(
        f.unlocked,
        vec!["base", "wave", "aaa_retired", "zzz_retired"]
    );
    assert_eq!(f.total_points, 30);
    let next = f.next.unwrap();
    assert_eq!(
        (next.id.as_str(), next.points, next.remaining),
        ("stretch", 60, 30)
    );
    assert_eq!(next.label, "Big stretch");
}

#[test]
fn friendship_view_at_the_end_of_the_table() {
    let mut state = PersistedState::default();
    state.friendship.total_points = 1_000_000;
    let h = H::with_state(state);
    let f = h.snap().friendship;
    assert!(f.next.is_none());
    assert_eq!(f.unlocked.len(), UnlockTable::builtin().unlocks.len());
}

#[test]
fn today_tracks_the_current_session() {
    let mut h = H::new();
    h.ok(start(None));
    h.run(10 * MIN);
    let t = h.snap().today;
    assert_eq!(t.active_focus_ms, 600_000);
    assert_eq!(t.focus_ms, 0);
    assert_eq!(t.focus_label, "10 min");
    h.ok(Command::Pause);
    assert_eq!(h.snap().today.active_focus_ms, 600_000);
    h.ok(Command::End);
    let t = h.snap().today;
    assert_eq!((t.active_focus_ms, t.focus_ms), (0, 600_000));
    assert_eq!(t.focus_label, "10 min");
    h.ok(Command::StartBreak { minutes: None });
    h.run(MIN);
    assert_eq!(
        h.snap().today.active_focus_ms,
        0,
        "breaks are not focus time"
    );
}

#[test]
fn a_session_across_midnight_counts_on_the_day_it_ended() {
    let clock = FakeClock::new(Utc.with_ymd_and_hms(2026, 3, 2, 23, 50, 0).unwrap(), 0);
    let mut h = H::with(
        EngineConfig::default(),
        UnlockTable::builtin(),
        PersistedState::default(),
        clock,
    );
    // A record from earlier the same evening.
    h.ok(start(Some(1)));
    h.run(MIN);
    h.ok(Command::Dismiss);
    assert_eq!(h.snap().today.date, "2026-03-02");
    assert_eq!(h.snap().today.harvest.len(), 1);
    h.ok(start(Some(25)));
    h.run_step(25 * MIN, 30 * SEC);
    let t = h.snap().today;
    assert_eq!(t.date, "2026-03-03");
    assert_eq!(
        t.harvest,
        vec![HarvestItem::Acorn],
        "the 23:51 record is yesterday's"
    );
    assert_eq!(t.focus_ms, 25 * MIN);
}

#[test]
fn records_carry_the_local_offset_when_written() {
    // UTC+9: 2026-03-02 20:00 UTC is 05:00 on 03-03 locally.
    let clock = FakeClock::new(Utc.with_ymd_and_hms(2026, 3, 2, 19, 59, 0).unwrap(), 9);
    let mut h = H::with(
        EngineConfig::default(),
        UnlockTable::builtin(),
        PersistedState::default(),
        clock,
    );
    h.ok(start(Some(1)));
    h.run(MIN);
    let r = &h.engine.history().records()[0];
    assert_eq!(r.ended_utc_offset_minutes, Some(540));
    assert_eq!(h.snap().today.date, "2026-03-03");
    assert_eq!(h.snap().today.harvest.len(), 1);
    // Travelling west to UTC-5 later the same moment: the record keeps its day (03-03),
    // which is no longer "today" (03-02 locally).
    h.clock.set_offset_hours(-5);
    let t = h.snap().today;
    assert_eq!(t.date, "2026-03-02");
    assert!(t.harvest.is_empty());
}

#[test]
fn next_tick_delay() {
    let mut h = H::new();
    let delay = |h: &H| h.engine.next_tick_delay_ms(h.now(), &*h.clock);
    // Idle at 09:00 UTC: capped at 60 s.
    assert_eq!(delay(&h), 60_000);
    h.ok(start(None));
    assert_eq!(delay(&h), 1000);
    h.clock.advance(600);
    h.tick();
    assert_eq!(delay(&h), 400, "remaining 1_499_400");
    h.clock.advance(400);
    h.tick();
    assert_eq!(delay(&h), 1000, "remaining 1_499_000");
    h.clock.advance(20);
    h.tick();
    assert_eq!(delay(&h), 980, "remaining 1_498_980");
    h.clock.advance(960);
    h.tick();
    assert_eq!(delay(&h), 50, "remaining 1_498_020 → clamped to 50");
    h.ok(Command::Pause);
    assert_eq!(delay(&h), 15_000);
    h.ok(Command::Resume);
    h.run(25 * MIN);
    assert_eq!(h.snap().phase, Phase::AwaitingChoice);
    assert_eq!(delay(&h), 60_000);
}

#[test]
fn next_tick_delay_reaches_local_midnight() {
    let clock = FakeClock::new(Utc.with_ymd_and_hms(2026, 3, 2, 23, 59, 30).unwrap(), 0);
    let h = H::with(
        EngineConfig::default(),
        UnlockTable::builtin(),
        PersistedState::default(),
        clock,
    );
    assert_eq!(h.engine.next_tick_delay_ms(h.now(), &*h.clock), 30_050);
    // With UTC+2, 21:59:30 UTC is 23:59:30 local.
    let clock = FakeClock::new(Utc.with_ymd_and_hms(2026, 3, 2, 21, 59, 30).unwrap(), 2);
    let h = H::with(
        EngineConfig::default(),
        UnlockTable::builtin(),
        PersistedState::default(),
        clock,
    );
    assert_eq!(h.engine.next_tick_delay_ms(h.now(), &*h.clock), 30_050);
}

#[test]
fn next_tick_delay_when_running_and_already_due() {
    let mut h = H::new();
    h.ok(start(Some(1)));
    h.run(59 * SEC);
    h.clock.advance(2 * SEC); // due, not yet observed
    assert_eq!(h.engine.next_tick_delay_ms(h.now(), &*h.clock), 50);
}

#[test]
fn replay_rules() {
    let mut h = H::new();
    assert_eq!(h.engine.check_replay("react"), Ok(()));
    assert_eq!(
        h.engine.check_replay("wave"),
        Err(CommandError::NotUnlocked { id: "wave".into() })
    );
    assert_eq!(
        h.engine.check_replay("bogus"),
        Err(CommandError::NotUnlocked { id: "bogus".into() })
    );
    h.ok(start(None));
    assert_eq!(
        h.engine.check_replay("react"),
        Err(CommandError::FocusRunning)
    );
    h.run(25 * MIN);
    assert_eq!(h.engine.check_replay("wave"), Ok(()));
    assert_eq!(
        h.engine.check_replay("base"),
        Err(CommandError::NotUnlocked { id: "base".into() }),
        "kind states"
    );
    h.ok(Command::StartBreak { minutes: None });
    assert_eq!(
        h.engine.check_replay("wave"),
        Ok(()),
        "breaks allow replays"
    );
    h.ok(Command::End);
    h.ok(start(None));
    h.ok(Command::Pause);
    assert_eq!(
        h.engine.check_replay("wave"),
        Ok(()),
        "a paused focus allows replays"
    );

    // Idle variants are replayable; palettes are not.
    let mut state = PersistedState::default();
    state.friendship.total_points = 200;
    let h = H::with_state(state);
    assert_eq!(h.engine.check_replay("umbrella"), Ok(()));
    assert_eq!(
        h.engine.check_replay("evening"),
        Err(CommandError::NotUnlocked {
            id: "evening".into()
        })
    );
}

#[test]
fn engine_new_uses_the_system_offset_by_default() {
    // `Engine::new` (no injected clock) works and is Send.
    fn assert_send<T: Send>(_: &T) {}
    let clock = FakeClock::default_start();
    use focus_core::Clock;
    let (mut e, fx) = Engine::new(
        EngineConfig::default(),
        UnlockTable::builtin(),
        PersistedState::default(),
        clock.now(),
        focus_core::testkit::counter_ids(),
        // no clock injected
    );
    assert_send(&e);
    assert_eq!(effect_names(&fx), ["Persist"]);
    let fx = e
        .command(
            Command::StartFocus {
                minutes: Some(1),
                task_label: None,
            },
            clock.now(),
        )
        .unwrap();
    assert!(matches!(
        transitions(&fx)[..],
        [Transition::FocusStarted { .. }]
    ));
    clock.advance(30 * SEC);
    let fx = e.command(Command::End, clock.now()).unwrap();
    match &transitions(&fx)[0] {
        Transition::FocusInterrupted { record, .. } => {
            assert!(record.ended_utc_offset_minutes.is_some())
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn engine_is_usable_behind_a_mutex() {
    let h = std::sync::Mutex::new(H::new().engine);
    std::thread::scope(|s| {
        s.spawn(|| {
            let _ = h.lock().unwrap().revision();
        });
    });
}

#[test]
fn transitions_serialize_for_the_renderer() {
    let mut h = H::new();
    let fx = h.ok(start(Some(1)));
    let v = serde_json::to_value(&transitions(&fx)[0]).unwrap();
    assert_eq!(v["type"], "focusStarted");
    assert_eq!(v["sessionId"], "s1");
    assert_eq!(v["plannedMs"], 60_000);
    let fx = h.run(MIN);
    let t = transitions(&fx)
        .into_iter()
        .find(|t| matches!(t, Transition::FocusCompleted { .. }))
        .unwrap();
    let v = serde_json::to_value(&t).unwrap();
    assert_eq!(v["type"], "focusCompleted");
    assert_eq!(v["longSession"], false);
    assert_eq!(v["award"]["totalPoints"], 6);
    assert_eq!(v["record"]["outcome"], "completed");
    let s = serde_json::to_value(h.snap()).unwrap();
    assert_eq!(s["phase"], "awaitingChoice");
    assert_eq!(s["today"]["harvest"][0], "acorn");
}
