//! Suspend, wake, observation gaps and wall-clock changes (SPEC §4.5).

use focus_core::engine::{ActivityLevel, Command, Effect, SystemEvent, Transition};
use focus_core::snapshot::Phase;
use focus_core::testkit::{effect_names, transitions, H, HOUR, MIN, SEC};
use focus_core::timer::{PauseReason, RunStatus, SessionKind};

fn running(minutes: u32, run_ms: u64) -> H {
    let mut h = H::new();
    h.ok(Command::StartFocus {
        minutes: Some(minutes),
        task_label: None,
    });
    h.run(run_ms);
    h
}

fn active_ms(h: &H) -> u64 {
    h.snap().session.map_or(0, |s| s.active_ms)
}

fn reason(h: &H) -> Option<PauseReason> {
    h.engine.active().and_then(|s| s.pause_reason())
}

fn completed_count(fx: &[Effect]) -> usize {
    transitions(fx)
        .iter()
        .filter(|t| matches!(t, Transition::FocusCompleted { .. }))
        .count()
}

#[test]
fn suspend_pauses_and_persists() {
    let mut h = running(25, 10 * MIN);
    let fx = h.ev(SystemEvent::Suspend);
    assert_eq!(effect_names(&fx), ["Persist", "ActivityChanged", "Paused"]);
    assert_eq!(
        fx[1],
        Effect::ActivityChanged {
            level: ActivityLevel::None
        }
    );
    assert_eq!(
        transitions(&fx),
        [Transition::Paused {
            session_id: "s1".into(),
            kind: SessionKind::Focus,
            reason: PauseReason::Suspend
        }]
    );
    assert_eq!(active_ms(&h), 600_000);
    assert!(h.engine.is_suspended());
    assert!(h.engine.baseline().is_none());
    let cp = h.saved().active_session.clone().unwrap();
    assert_eq!(cp.status, RunStatus::Paused);
    assert_eq!(cp.pause_reason, Some(PauseReason::Suspend));
    assert_eq!(cp.accrued_ms, 600_000);
}

#[test]
fn suspend_folds_up_to_now() {
    let mut h = running(25, 10 * MIN);
    h.clock.advance(700);
    h.ev(SystemEvent::Suspend);
    assert_eq!(active_ms(&h), 600_700);
}

#[test]
fn wake_never_resumes() {
    for counting in [false, true] {
        let mut h = running(25, 10 * MIN);
        h.ev(SystemEvent::Suspend);
        if counting {
            h.sleep_mono_counting(30 * MIN);
        } else {
            h.sleep_mono_stopped(30 * MIN);
        }
        let fx = h.ev(SystemEvent::Wake);
        assert!(fx.is_empty(), "{fx:?}");
        assert!(!h.engine.is_suspended());
        assert!(h.engine.baseline().is_some());
        let fx = h.run(10 * MIN);
        assert!(!transitions(&fx)
            .iter()
            .any(|t| matches!(t, Transition::Resumed { .. })));
        assert_eq!(reason(&h), Some(PauseReason::Suspend));
        assert_eq!(
            active_ms(&h),
            600_000,
            "sleep earned nothing (counting={counting})"
        );
        // The user resumes; time continues from where it was.
        h.ok(Command::Resume);
        h.run(15 * MIN);
        assert_eq!(h.engine.friendship().total_points, 30);
    }
}

#[test]
fn missed_suspend_with_a_stopped_monotonic_clock() {
    // macOS: Instant stops during sleep, so only the wall clock moved.
    let mut h = running(25, 5 * MIN);
    h.sleep_mono_stopped(60 * MIN);
    h.clock.advance(SEC);
    let fx = h.tick();
    assert!(matches!(
        transitions(&fx)[..],
        [Transition::Paused {
            reason: PauseReason::ClockJump,
            ..
        }]
    ));
    assert_eq!(
        active_ms(&h),
        301_000,
        "only the 1 s of monotonic time is earned"
    );
    assert_eq!(h.engine.friendship().total_points, 0);
}

#[test]
fn missed_suspend_with_a_counting_monotonic_clock() {
    let mut h = running(25, 5 * MIN);
    let last_wall = h.now().wall;
    h.sleep_mono_counting(60 * MIN);
    let fx = h.tick();
    assert!(matches!(
        transitions(&fx)[..],
        [Transition::Paused {
            reason: PauseReason::Suspend,
            ..
        }]
    ));
    assert_eq!(active_ms(&h), 300_000, "the gap earns nothing");
    assert_eq!(h.engine.active().unwrap().paused_at(), Some(last_wall));
}

#[test]
fn wake_without_suspend_pauses_and_earns_nothing() {
    for counting in [false, true] {
        let mut h = running(25, 5 * MIN);
        let last_wall = h.now().wall;
        if counting {
            h.sleep_mono_counting(60 * MIN);
        } else {
            h.sleep_mono_stopped(60 * MIN);
        }
        let fx = h.ev(SystemEvent::Wake);
        assert_eq!(
            effect_names(&fx),
            ["Persist", "ActivityChanged", "Paused"],
            "counting={counting}"
        );
        assert_eq!(reason(&h), Some(PauseReason::Suspend));
        assert_eq!(
            active_ms(&h),
            300_000,
            "never folds past the last observation"
        );
        assert_eq!(h.engine.active().unwrap().paused_at(), Some(last_wall));
        // Paused at the last observation, so the 60 min of sleep counts toward pause age.
        assert_eq!(
            h.engine.active().unwrap().pause_age_ms(h.now().wall),
            Some(60 * MIN)
        );
    }
}

#[test]
fn clock_changed_after_a_missed_sleep() {
    // Counting clock: the gap pauses with reason suspend.
    let mut h = running(25, 5 * MIN);
    h.sleep_mono_counting(60 * MIN);
    let fx = h.ev(SystemEvent::ClockChanged);
    assert!(matches!(
        transitions(&fx)[..],
        [Transition::Paused {
            reason: PauseReason::Suspend,
            ..
        }]
    ));
    assert_eq!(active_ms(&h), 300_000);
    assert!(h.ev(SystemEvent::Wake).is_empty());
    assert_eq!(active_ms(&h), 300_000);
    // Stopped clock: the skew pauses with reason clockJump.
    let mut h = running(25, 5 * MIN);
    h.sleep_mono_stopped(60 * MIN);
    h.clock.advance(500);
    let fx = h.ev(SystemEvent::ClockChanged);
    assert!(matches!(
        transitions(&fx)[..],
        [Transition::Paused {
            reason: PauseReason::ClockJump,
            ..
        }]
    ));
    assert_eq!(active_ms(&h), 300_500);
}

#[test]
fn manual_clock_forward_while_running_is_deterministic() {
    for ev in [SystemEvent::Tick, SystemEvent::ClockChanged] {
        let mut h = running(25, 10 * MIN);
        h.clock.jump_wall(2 * HOUR as i64);
        let fx = h.ev(ev);
        assert!(
            matches!(
                transitions(&fx)[..],
                [Transition::Paused {
                    reason: PauseReason::ClockJump,
                    ..
                }]
            ),
            "{ev:?}"
        );
        assert_eq!(active_ms(&h), 600_000);
        assert_eq!(completed_count(&fx), 0);
        assert_eq!(h.engine.friendship().total_points, 0);
        // ClockChanged never applies the checkpoint cadence; the pause itself persists.
        assert_eq!(fx[0], Effect::Persist);
    }
}

#[test]
fn clock_forward_never_completes() {
    let mut h = running(25, 24 * MIN);
    h.clock.jump_wall(HOUR as i64);
    let fx = h.tick();
    assert_eq!(completed_count(&fx), 0);
    assert_eq!(reason(&h), Some(PauseReason::ClockJump));
    // Even a jump past the end does not.
    h.clock.jump_wall(10 * HOUR as i64);
    assert_eq!(completed_count(&h.tick()), 0);
    h.ok(Command::Resume);
    let fx = h.run(MIN);
    assert_eq!(completed_count(&fx), 1);
    assert_eq!(h.engine.friendship().total_points, 30);
}

#[test]
fn clock_back_while_running_keeps_running_and_clamps_ended_at() {
    let mut h = running(25, 10 * MIN);
    h.clock.jump_wall(-2 * HOUR as i64);
    let fx = h.tick();
    assert!(transitions(&fx).is_empty());
    assert!(h.engine.is_running());
    let fx = h.run(15 * MIN);
    let record = transitions(&fx)
        .into_iter()
        .find_map(|t| match t {
            Transition::FocusCompleted { record, .. } => Some(record),
            _ => None,
        })
        .unwrap();
    assert_eq!(record.active_ms, 25 * MIN);
    assert!(record.ended_at >= record.started_at);
    assert_eq!(
        record.ended_at, record.started_at,
        "clamped: the wall clock is now before the start"
    );
    assert_eq!(h.engine.friendship().total_points, 30);
}

#[test]
fn skew_tolerance_boundary_while_running() {
    let mut h = running(25, MIN);
    h.clock.jump_wall(60_000);
    h.tick();
    assert!(h.engine.is_running(), "exactly the tolerance keeps running");
    let mut h = running(25, MIN);
    h.clock.jump_wall(60_001);
    h.tick();
    assert_eq!(reason(&h), Some(PauseReason::ClockJump));
}

#[test]
fn observation_gap_boundary() {
    let mut h = running(25, MIN);
    h.clock.advance(120_000);
    h.tick();
    assert!(h.engine.is_running());
    assert_eq!(active_ms(&h), 180_000);
    h.clock.advance(120_001);
    let fx = h.tick();
    assert!(matches!(
        transitions(&fx)[..],
        [Transition::Paused {
            reason: PauseReason::Suspend,
            ..
        }]
    ));
    assert_eq!(active_ms(&h), 180_000);
}

#[test]
fn late_tick_within_the_gap_counts() {
    let mut h = running(25, MIN);
    h.clock.advance(90_000);
    h.tick();
    assert_eq!(active_ms(&h), 150_000);
}

#[test]
fn ticker_stall_longer_than_the_gap_pauses_without_earning() {
    let mut h = running(25, 3 * MIN);
    // Both clocks agree 5 minutes passed (e.g. App Nap ignored the activity).
    h.clock.advance(5 * MIN);
    h.tick();
    assert_eq!(reason(&h), Some(PauseReason::Suspend));
    assert_eq!(active_ms(&h), 3 * MIN);
}

#[test]
fn nothing_resumes_automatically() {
    let mut h = running(25, 2 * MIN);
    h.ok(Command::Pause);
    let mut all = Vec::new();
    for ev in [
        SystemEvent::Tick,
        SystemEvent::Suspend,
        SystemEvent::Tick,
        SystemEvent::Wake,
        SystemEvent::ClockChanged,
        SystemEvent::Wake,
        SystemEvent::Suspend,
        SystemEvent::Wake,
    ] {
        h.clock.advance(5 * MIN);
        h.clock.jump_wall(2 * MIN as i64);
        all.extend(h.ev(ev));
        assert!(!h.engine.is_running(), "{ev:?}");
    }
    assert!(!transitions(&all)
        .iter()
        .any(|t| matches!(t, Transition::Resumed { .. })));
    assert_eq!(active_ms(&h), 2 * MIN);
    assert_eq!(
        reason(&h),
        Some(PauseReason::User),
        "the original reason is kept"
    );
}

#[test]
fn suspend_and_wake_in_idle_and_awaiting_do_nothing() {
    let mut h = H::new();
    let rev = h.engine.revision();
    assert!(h.ev(SystemEvent::Suspend).is_empty());
    h.sleep_mono_stopped(HOUR);
    assert!(h.ev(SystemEvent::Wake).is_empty());
    assert_eq!(h.snap().phase, Phase::Idle);
    assert_eq!(h.engine.revision(), rev);

    let mut h = running(1, MIN);
    assert_eq!(h.snap().phase, Phase::AwaitingChoice);
    assert!(h.ev(SystemEvent::Suspend).is_empty());
    h.sleep_mono_counting(3 * HOUR);
    assert!(h.ev(SystemEvent::Wake).is_empty());
    assert_eq!(h.snap().phase, Phase::AwaitingChoice);
}

#[test]
fn double_suspend_and_double_wake() {
    let mut h = running(25, 10 * MIN);
    h.ev(SystemEvent::Suspend);
    assert!(h.ev(SystemEvent::Suspend).is_empty());
    h.sleep_mono_stopped(10 * MIN);
    assert!(h.ev(SystemEvent::Wake).is_empty());
    assert!(h.ev(SystemEvent::Wake).is_empty());
    assert_eq!(active_ms(&h), 600_000);
}

#[test]
fn suspend_during_a_break() {
    let mut h = H::new();
    h.ok(Command::StartBreak { minutes: None });
    h.run(MIN);
    let fx = h.ev(SystemEvent::Suspend);
    assert_eq!(
        fx[1],
        Effect::ActivityChanged {
            level: ActivityLevel::None
        }
    );
    assert!(matches!(
        transitions(&fx)[..],
        [Transition::Paused {
            kind: SessionKind::Break,
            reason: PauseReason::Suspend,
            ..
        }]
    ));
    h.sleep_mono_stopped(10 * MIN);
    h.ev(SystemEvent::Wake);
    assert_eq!(h.snap().phase, Phase::Break);
    assert_eq!(active_ms(&h), MIN);
}

#[test]
fn a_user_command_ends_the_suspended_interval() {
    let mut h = running(25, 5 * MIN);
    h.ev(SystemEvent::Suspend);
    assert!(h.engine.is_suspended());
    h.ok(Command::Resume);
    assert!(!h.engine.is_suspended());
    assert!(h.engine.baseline().is_some());
    h.run(MIN);
    assert_eq!(active_ms(&h), 6 * MIN);
    // Gap detection works again right away.
    h.sleep_mono_counting(10 * MIN);
    h.tick();
    assert_eq!(reason(&h), Some(PauseReason::Suspend));
    assert_eq!(active_ms(&h), 6 * MIN);
}

#[test]
fn ticks_while_suspended_do_not_earn_or_set_a_baseline() {
    let mut h = running(25, 5 * MIN);
    h.ev(SystemEvent::Suspend);
    for _ in 0..5 {
        h.clock.advance(SEC);
        assert!(h.tick().is_empty());
    }
    assert!(h.engine.baseline().is_none());
    assert_eq!(active_ms(&h), 5 * MIN);
}

#[test]
fn snapshot_never_shows_unobserved_time() {
    // After a missed sleep on a counting clock, the snapshot taken before the next tick
    // must not show inflated progress.
    let h = running(25, 5 * MIN);
    h.sleep_mono_counting(60 * MIN);
    let s = h.snap();
    assert_eq!(s.session.unwrap().active_ms, 300_000);
    assert_eq!(
        h.engine
            .persisted_state(h.now())
            .active_session
            .unwrap()
            .accrued_ms,
        300_000
    );
}
