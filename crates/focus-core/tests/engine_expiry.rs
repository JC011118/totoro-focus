//! Pause expiry (SPEC §4.6): strictly more than 120 minutes of pause age, measured
//! from the age anchor, which detected wall-clock jumps shift.

use chrono::TimeDelta;
use focus_core::clock::Wall;
use focus_core::engine::{Command, Effect, SystemEvent, Transition};
use focus_core::history::{EndReason, HarvestItem, Outcome, SessionRecord};
use focus_core::rewards::AwardOutcome;
use focus_core::snapshot::Phase;
use focus_core::testkit::{effect_names, transitions, H, HOUR, MIN, SEC};
use focus_core::timer::PauseReason;

const EXPIRY: u64 = 120 * MIN;

fn interrupted(fx: &[Effect]) -> Vec<(SessionRecord, Option<AwardOutcome>)> {
    transitions(fx)
        .into_iter()
        .filter_map(|t| match t {
            Transition::FocusInterrupted { record, award } => Some((record, award)),
            _ => None,
        })
        .collect()
}

/// Focus 25, run 10 min, pause. Returns the harness and the pause wall stamp.
fn paused_after_10() -> (H, Wall) {
    let mut h = H::new();
    h.ok(Command::StartFocus {
        minutes: None,
        task_label: None,
    });
    h.run(10 * MIN);
    h.ok(Command::Pause);
    let at = h.now().wall;
    (h, at)
}

fn is_paused(h: &H) -> bool {
    h.engine.active().is_some_and(|s| !s.is_running())
}

#[test]
fn expiry_needs_strictly_more_than_120_minutes() {
    let (mut h, paused_at) = paused_after_10();
    h.clock.advance(EXPIRY);
    assert!(h.tick().is_empty());
    assert!(is_paused(&h));
    h.clock.advance(1);
    let fx = h.tick();
    assert_eq!(effect_names(&fx), ["Persist", "FocusInterrupted"]);
    let (record, award) = interrupted(&fx).remove(0);
    assert_eq!(record.outcome, Outcome::Interrupted);
    assert_eq!(record.end_reason, EndReason::PauseExpired);
    assert_eq!(record.active_ms, 600_000);
    assert_eq!(record.ended_at, paused_at);
    let award = award.unwrap();
    assert_eq!((award.points, award.bonus), (10, 0));
    let s = h.snap();
    assert_eq!(s.phase, Phase::Idle);
    assert_eq!(s.today.harvest, vec![HarvestItem::Leaf]);
    assert_eq!(s.friendship.total_points, 10);
}

#[test]
fn paused_ticks_on_the_paused_cadence_still_expire() {
    let (mut h, _) = paused_after_10();
    // The ticker runs every 15 s while paused.
    let fx = h.run_step(EXPIRY + 15 * SEC, 15 * SEC);
    assert_eq!(interrupted(&fx).len(), 1);
    assert_eq!(h.snap().phase, Phase::Idle);
}

#[test]
fn resume_after_the_deadline_without_a_tick_is_overtaken() {
    let (mut h, _) = paused_after_10();
    h.clock.advance(121 * MIN);
    let fx = h.ok(Command::Resume);
    let (record, _) = interrupted(&fx).remove(0);
    assert_eq!(record.end_reason, EndReason::PauseExpired);
    assert!(!transitions(&fx)
        .iter()
        .any(|t| matches!(t, Transition::Resumed { .. })));
    assert_eq!(h.snap().phase, Phase::Idle);
}

#[test]
fn toggle_after_the_deadline_starts_nothing() {
    let (mut h, _) = paused_after_10();
    h.clock.advance(121 * MIN);
    let fx = h.ok(Command::Toggle);
    assert_eq!(interrupted(&fx).len(), 1);
    assert!(!transitions(&fx).iter().any(|t| matches!(
        t,
        Transition::FocusStarted { .. } | Transition::Resumed { .. }
    )));
    assert_eq!(h.snap().phase, Phase::Idle);
    assert_eq!(h.engine.history().len(), 1);
}

#[test]
fn end_and_pause_after_the_deadline_record_expiry() {
    for cmd in [Command::End, Command::Pause, Command::Quit] {
        let (mut h, paused_at) = paused_after_10();
        h.clock.advance(121 * MIN);
        let fx = h.ok(cmd.clone());
        let found = interrupted(&fx);
        assert_eq!(found.len(), 1, "{cmd:?}");
        assert_eq!(found[0].0.end_reason, EndReason::PauseExpired, "{cmd:?}");
        assert_eq!(found[0].0.ended_at, paused_at);
        assert_eq!(h.engine.friendship().total_points, 10);
    }
}

#[test]
fn clock_forward_while_paused_does_not_expire() {
    let (mut h, _) = paused_after_10();
    h.clock.advance(SEC);
    h.clock.jump_wall(3 * HOUR as i64);
    assert!(h.tick().is_empty());
    assert!(is_paused(&h));
    h.clock.advance(119 * MIN);
    h.tick();
    assert!(is_paused(&h), "age is 119:01, not 3 h");
    h.clock.advance(2 * MIN);
    assert_eq!(interrupted(&h.tick()).len(), 1);
}

#[test]
fn clock_back_while_paused_does_not_extend_the_pause() {
    let (mut h, _) = paused_after_10();
    h.clock.advance(60 * MIN);
    h.tick();
    h.clock.jump_wall(-3 * HOUR as i64);
    h.tick();
    assert!(is_paused(&h));
    h.clock.advance(60 * MIN);
    h.tick();
    assert!(is_paused(&h), "age is exactly 120 min");
    h.clock.advance(MIN);
    assert_eq!(interrupted(&h.tick()).len(), 1);
}

#[test]
fn clock_changed_event_while_paused_shifts_the_anchor() {
    let (mut h, paused_at) = paused_after_10();
    h.clock.jump_wall(3 * HOUR as i64);
    assert!(h.ev(SystemEvent::ClockChanged).is_empty());
    assert!(is_paused(&h));
    assert_eq!(
        h.engine.active().unwrap().age_anchor(),
        Some(paused_at + TimeDelta::hours(3))
    );
    assert_eq!(h.engine.active().unwrap().paused_at(), Some(paused_at));
    h.clock.advance(119 * MIN);
    h.tick();
    assert!(is_paused(&h));
    // The expiry record still uses the true pause stamp.
    h.clock.advance(2 * MIN);
    let (record, _) = interrupted(&h.tick()).remove(0);
    assert_eq!(record.ended_at, paused_at);
}

#[test]
fn skew_within_tolerance_counts_toward_age() {
    let (mut h, paused_at) = paused_after_10();
    h.clock.jump_wall(59 * SEC as i64);
    h.tick();
    assert_eq!(
        h.engine.active().unwrap().age_anchor(),
        Some(paused_at),
        "not shifted"
    );
    h.clock.advance(EXPIRY - 59 * SEC);
    h.tick();
    assert!(is_paused(&h), "age exactly 120 min");
    h.clock.advance(1);
    assert_eq!(interrupted(&h.tick()).len(), 1);
}

#[test]
fn skew_just_over_tolerance_shifts() {
    let (mut h, paused_at) = paused_after_10();
    h.clock.jump_wall(60 * SEC as i64 + 1);
    h.tick();
    assert_eq!(
        h.engine.active().unwrap().age_anchor(),
        Some(paused_at + TimeDelta::milliseconds(60_001))
    );
}

#[test]
fn sleep_while_paused_counts_toward_expiry() {
    let (mut h, paused_at) = paused_after_10();
    h.ev(SystemEvent::Suspend);
    h.sleep_mono_stopped(150 * MIN);
    let fx = h.ev(SystemEvent::Wake);
    let (record, award) = interrupted(&fx).remove(0);
    assert_eq!(record.end_reason, EndReason::PauseExpired);
    assert_eq!(record.ended_at, paused_at);
    assert_eq!(award.unwrap().points, 10);
    assert_eq!(h.snap().phase, Phase::Idle);
}

#[test]
fn sleep_while_paused_with_a_counting_clock_counts_too() {
    let (mut h, _) = paused_after_10();
    h.ev(SystemEvent::Suspend);
    h.sleep_mono_counting(150 * MIN);
    assert_eq!(interrupted(&h.ev(SystemEvent::Wake)).len(), 1);
}

#[test]
fn short_sleep_while_paused_stays_paused() {
    let (mut h, _) = paused_after_10();
    h.ev(SystemEvent::Suspend);
    h.sleep_mono_stopped(90 * MIN);
    assert!(h.ev(SystemEvent::Wake).is_empty());
    assert!(is_paused(&h));
    // Sleep counted: 30 more minutes (+1 ms) expire it.
    h.clock.advance(30 * MIN);
    h.tick();
    assert!(is_paused(&h));
    h.clock.advance(1);
    assert_eq!(interrupted(&h.tick()).len(), 1);
}

#[test]
fn ticks_between_suspend_and_wake_never_shift_the_anchor() {
    let (mut h, paused_at) = paused_after_10();
    h.ev(SystemEvent::Suspend);
    assert!(h.tick().is_empty());
    h.sleep_mono_stopped(100 * MIN);
    // A DarkWake / pre-DidWake tick: suspended, so no skew handling.
    assert!(h.tick().is_empty());
    assert!(h.ev(SystemEvent::ClockChanged).is_empty());
    assert_eq!(h.engine.active().unwrap().age_anchor(), Some(paused_at));
    assert!(
        h.engine.baseline().is_none(),
        "baseline stays unset while suspended"
    );
    h.sleep_mono_stopped(50 * MIN);
    let fx = h.ev(SystemEvent::Wake);
    assert_eq!(interrupted(&fx).len(), 1);
}

#[test]
fn expiry_is_still_evaluated_while_suspended() {
    let (mut h, paused_at) = paused_after_10();
    h.ev(SystemEvent::Suspend);
    h.sleep_mono_stopped(150 * MIN);
    let fx = h.tick();
    let (record, _) = interrupted(&fx).remove(0);
    assert_eq!(record.ended_at, paused_at);
    assert!(h.ev(SystemEvent::Wake).is_empty());
}

#[test]
fn long_sleep_while_running_expires_on_wake() {
    let mut h = H::new();
    h.ok(Command::StartFocus {
        minutes: None,
        task_label: None,
    });
    h.run(10 * MIN);
    let suspend_wall = h.now().wall;
    h.ev(SystemEvent::Suspend);
    h.sleep_mono_stopped(3 * HOUR);
    let fx = h.ev(SystemEvent::Wake);
    let (record, award) = interrupted(&fx).remove(0);
    assert_eq!(record.end_reason, EndReason::PauseExpired);
    assert_eq!(record.active_ms, 600_000);
    assert_eq!(record.ended_at, suspend_wall);
    assert_eq!(award.unwrap().points, 10);
    assert_eq!(h.snap().phase, Phase::Idle);
}

#[test]
fn missed_long_sleep_on_a_counting_clock_expires_at_the_next_tick() {
    let mut h = H::new();
    h.ok(Command::StartFocus {
        minutes: None,
        task_label: None,
    });
    h.run(10 * MIN);
    let last_tick_wall = h.now().wall;
    h.sleep_mono_counting(3 * HOUR);
    let fx = h.tick();
    // Gap → paused at the last observation, then immediately expired (both clocks agree).
    assert_eq!(
        effect_names(&fx),
        ["Persist", "ActivityChanged", "Paused", "FocusInterrupted"]
    );
    assert!(matches!(
        transitions(&fx)[0],
        Transition::Paused {
            reason: PauseReason::Suspend,
            ..
        }
    ));
    let (record, _) = interrupted(&fx).remove(0);
    assert_eq!(record.ended_at, last_tick_wall);
    assert_eq!(record.active_ms, 600_000);
}

#[test]
fn break_pause_expiry_is_silent() {
    let mut h = H::new();
    h.ok(Command::StartBreak { minutes: None });
    h.run(MIN);
    h.ok(Command::Pause);
    h.clock.advance(121 * MIN);
    let fx = h.tick();
    assert_eq!(effect_names(&fx), ["Persist", "BreakEnded"]);
    assert_eq!(h.snap().phase, Phase::Idle);
    assert_eq!(h.engine.history().len(), 0);
    assert_eq!(h.engine.friendship().total_points, 0);
}

#[test]
fn a_running_session_never_expires() {
    let mut h = H::new();
    h.ok(Command::StartFocus {
        minutes: Some(180),
        task_label: None,
    });
    let fx = h.run_step(179 * MIN, MIN);
    assert!(interrupted(&fx).is_empty());
    assert!(h.engine.is_running());
}

#[test]
fn resume_resets_the_pause_age() {
    let (mut h, _) = paused_after_10();
    h.clock.advance(100 * MIN);
    h.ok(Command::Resume);
    h.run(MIN);
    h.ok(Command::Pause);
    h.clock.advance(100 * MIN);
    h.tick();
    assert!(is_paused(&h), "a new pause starts a new age");
}
