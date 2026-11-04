//! Completion (§4.2), interruption (§4.3) and breaks (§4.4).

use chrono::TimeDelta;
use focus_core::engine::{ActivityLevel, Command, Effect, SoundCue, Transition};
use focus_core::history::{EndReason, HarvestItem, Outcome, SessionRecord};
use focus_core::rewards::AwardOutcome;
use focus_core::snapshot::Phase;
use focus_core::testkit::{effect_names, transitions, H, MIN, SEC};
use focus_core::timer::SessionKind;

fn start(minutes: Option<u32>) -> Command {
    Command::StartFocus {
        minutes,
        task_label: None,
    }
}

fn completed(fx: &[Effect]) -> Vec<(SessionRecord, Option<AwardOutcome>, bool)> {
    transitions(fx)
        .into_iter()
        .filter_map(|t| match t {
            Transition::FocusCompleted {
                record,
                award,
                long_session,
            } => Some((record, award, long_session)),
            _ => None,
        })
        .collect()
}

fn interrupted(fx: &[Effect]) -> Vec<(SessionRecord, Option<AwardOutcome>)> {
    transitions(fx)
        .into_iter()
        .filter_map(|t| match t {
            Transition::FocusInterrupted { record, award } => Some((record, award)),
            _ => None,
        })
        .collect()
}

#[test]
fn completing_25_minutes_awards_30_once() {
    let mut h = H::new();
    let t0 = h.now().wall;
    h.ok(start(None));
    h.run(25 * MIN - SEC);
    assert_eq!(h.snap().session.unwrap().remaining_label, "00:01");
    h.clock.advance(SEC);
    let fx = h.tick();
    assert_eq!(
        effect_names(&fx),
        ["Persist", "ActivityChanged", "FocusCompleted", "PlaySound"]
    );
    assert_eq!(
        fx[1],
        Effect::ActivityChanged {
            level: ActivityLevel::None
        }
    );
    assert_eq!(fx[3], Effect::PlaySound(SoundCue::Completion));
    let (record, award, long) = completed(&fx).remove(0);
    assert_eq!(record.id, "s1");
    assert_eq!(record.kind, SessionKind::Focus);
    assert_eq!(record.outcome, Outcome::Completed);
    assert_eq!(record.end_reason, EndReason::Completed);
    assert_eq!(record.active_ms, 1_500_000);
    assert_eq!(record.planned_ms, 1_500_000);
    assert_eq!(record.started_at, t0);
    assert_eq!(record.ended_at, t0 + TimeDelta::minutes(25));
    assert_eq!(record.ended_utc_offset_minutes, Some(0));
    let award = award.unwrap();
    assert_eq!((award.points, award.bonus, award.total_points), (25, 5, 30));
    assert_eq!(award.newly_unlocked, vec!["wave"]);
    assert!(!long);

    let s = h.snap();
    assert_eq!(s.phase, Phase::AwaitingChoice);
    let a = s.awaiting.unwrap();
    assert_eq!(a.after, SessionKind::Focus);
    assert_eq!(a.record.as_ref(), Some(&record));
    assert_eq!(a.award.as_ref().unwrap().total_points, 30);
    assert!(!a.long_session);
    assert!(s.session.is_none());
    assert_eq!(s.today.harvest, vec![HarvestItem::Acorn]);
    assert_eq!(s.today.focus_ms, 1_500_000);
    assert_eq!(s.today.focus_label, "25 min");
    assert_eq!(s.friendship.total_points, 30);
    assert_eq!(s.friendship.unlocked, vec!["base", "wave"]);
    assert_eq!(h.engine.history().len(), 1);
    // The completion was saved before anything else in that call.
    assert_eq!(h.saved().history.len(), 1);
    assert_eq!(h.saved().friendship.total_points, 30);
    assert!(h.saved().active_session.is_none());

    // Repeated processing (more ticks, events, a duplicate call at the same now) adds nothing.
    let more = h.run(10 * MIN);
    assert!(transitions(&more).is_empty());
    let now = h.now();
    assert!(h
        .engine
        .system(focus_core::SystemEvent::Tick, now)
        .is_empty());
    assert!(h
        .engine
        .system(focus_core::SystemEvent::Wake, now)
        .is_empty());
    assert_eq!(h.engine.friendship().total_points, 30);
    assert_eq!(h.engine.history().len(), 1);
}

#[test]
fn completion_without_sound() {
    let mut h = H::with_settings(|s| s.sound = false);
    h.ok(start(Some(1)));
    let fx = h.run(MIN);
    assert_eq!(completed(&fx).len(), 1);
    assert!(!fx.iter().any(|e| matches!(e, Effect::PlaySound(_))));
}

#[test]
fn late_tick_clamps_and_backdates() {
    let mut h = H::new();
    let t0 = h.now().wall;
    h.ok(start(None));
    h.run(24 * MIN);
    h.clock.advance(90 * SEC); // late but within the 120 s gap
    let fx = h.tick();
    let (record, award, _) = completed(&fx).remove(0);
    assert_eq!(record.active_ms, 1_500_000);
    assert_eq!(award.unwrap().total_points, 30);
    assert_eq!(
        record.ended_at,
        t0 + TimeDelta::minutes(25),
        "endedAt minus the 30 s overshoot"
    );
}

#[test]
fn end_when_due_completes_instead_of_interrupting() {
    let mut h = H::new();
    h.ok(start(None));
    h.run(25 * MIN - SEC);
    h.clock.advance(1_500);
    let fx = h.ok(Command::End);
    assert_eq!(completed(&fx).len(), 1);
    assert!(interrupted(&fx).is_empty());
    assert_eq!(h.engine.friendship().total_points, 30);
    assert_eq!(h.snap().phase, Phase::AwaitingChoice);
    assert_eq!(h.snap().today.harvest, vec![HarvestItem::Acorn]);
}

#[test]
fn pause_toggle_and_quit_when_due_complete() {
    for cmd in [Command::Pause, Command::Toggle, Command::Quit] {
        let mut h = H::new();
        h.ok(start(None));
        h.run(25 * MIN - SEC);
        h.clock.advance(1_500);
        let fx = h.ok(cmd.clone());
        assert_eq!(completed(&fx).len(), 1, "{cmd:?}");
        let ts = transitions(&fx);
        assert!(
            !ts.iter().any(|t| matches!(
                t,
                Transition::Paused { .. } | Transition::FocusStarted { .. }
            )),
            "{cmd:?}: {ts:?}"
        );
        assert_eq!(h.snap().phase, Phase::AwaitingChoice, "{cmd:?}");
        assert_eq!(h.engine.friendship().total_points, 30);
    }
}

#[test]
fn long_session_flag() {
    for (minutes, long) in [(45, true), (44, false), (46, true)] {
        let mut h = H::new();
        h.ok(start(Some(minutes)));
        let fx = h.run(u64::from(minutes) * MIN);
        let (_, award, l) = completed(&fx).remove(0);
        assert_eq!(l, long, "{minutes}");
        assert_eq!(h.snap().awaiting.unwrap().long_session, long);
        assert_eq!(award.unwrap().total_points, u64::from(minutes) + 5);
    }
}

#[test]
fn maximum_180_minute_session() {
    let mut h = H::new();
    h.ok(start(Some(180)));
    assert_eq!(h.snap().session.unwrap().remaining_label, "180:00");
    h.run_step(MIN, SEC);
    assert_eq!(h.snap().session.unwrap().remaining_label, "179:00");
    let fx = h.run_step(179 * MIN, MIN);
    let (record, award, long) = completed(&fx).remove(0);
    assert!(long);
    assert_eq!(record.active_ms, 180 * MIN);
    let award = award.unwrap();
    assert_eq!(
        (award.points, award.bonus, award.total_points),
        (180, 5, 185)
    );
    assert_eq!(
        award.newly_unlocked,
        vec!["wave", "stretch", "umbrella", "evening"]
    );
    assert_eq!(h.snap().today.focus_label, "3 h 00 min");
}

#[test]
fn duplicate_tick_with_the_same_now() {
    let mut h = H::new();
    h.ok(start(Some(1)));
    h.run(MIN - SEC);
    h.clock.advance(SEC);
    let a = h.tick();
    let b = h.tick();
    assert_eq!(completed(&a).len(), 1);
    assert!(b.is_empty());
}

#[test]
fn interrupted_at_22_minutes_awards_22_and_a_leaf() {
    let mut h = H::new();
    let t0 = h.now().wall;
    h.ok(start(None));
    h.run(22 * MIN + 30 * SEC);
    let fx = h.ok(Command::End);
    assert_eq!(
        effect_names(&fx),
        ["Persist", "ActivityChanged", "FocusInterrupted"]
    );
    let (record, award) = interrupted(&fx).remove(0);
    assert_eq!(record.outcome, Outcome::Interrupted);
    assert_eq!(record.end_reason, EndReason::UserEnded);
    assert_eq!(record.active_ms, 1_350_000);
    assert_eq!(record.ended_at, t0 + TimeDelta::seconds(22 * 60 + 30));
    let award = award.unwrap();
    assert_eq!((award.points, award.bonus, award.total_points), (22, 0, 22));
    // No sound, no celebration, no ShowCompanion (setting off).
    assert!(!fx
        .iter()
        .any(|e| matches!(e, Effect::PlaySound(_) | Effect::ShowCompanion)));
    let s = h.snap();
    assert_eq!(s.phase, Phase::Idle);
    assert!(s.awaiting.is_none());
    assert_eq!(s.today.harvest, vec![HarvestItem::Leaf]);
    assert_eq!((s.today.completed, s.today.interrupted), (0, 1));
    assert_eq!(s.today.focus_label, "22 min");
    assert_eq!(s.friendship.total_points, 22);
}

#[test]
fn interruption_floors_whole_minutes() {
    let mut h = H::new();
    h.ok(start(None));
    h.run(21 * MIN + 59 * SEC);
    h.clock.advance(999);
    h.tick();
    let (_, award) = interrupted(&h.ok(Command::End)).remove(0);
    assert_eq!(award.unwrap().points, 21);

    let mut h = H::new();
    h.ok(start(None));
    h.run(22 * MIN);
    let (record, award) = interrupted(&h.ok(Command::End)).remove(0);
    assert_eq!(record.active_ms, 22 * MIN);
    assert_eq!(award.unwrap().points, 22);
}

#[test]
fn interrupting_a_paused_session_uses_accrued_time() {
    let mut h = H::new();
    h.ok(start(None));
    h.run(10 * MIN);
    h.ok(Command::Pause);
    h.clock.advance(30 * MIN);
    let end_wall = h.now().wall;
    let fx = h.ok(Command::End);
    assert_eq!(
        effect_names(&fx),
        ["Persist", "FocusInterrupted"],
        "no ActivityChanged: nothing was running"
    );
    let (record, award) = interrupted(&fx).remove(0);
    assert_eq!(record.active_ms, 600_000);
    assert_eq!(record.ended_at, end_wall);
    assert_eq!(award.unwrap().points, 10);
}

#[test]
fn interrupting_within_the_first_minute() {
    let mut h = H::new();
    h.ok(start(None));
    h.run(30 * SEC);
    let (record, award) = interrupted(&h.ok(Command::End)).remove(0);
    assert_eq!(record.active_ms, 30 * SEC);
    assert_eq!(award.unwrap().points, 0);
    assert!(h.engine.friendship().awards.contains_key("s1"));
    assert_eq!(h.snap().today.harvest, vec![HarvestItem::Leaf]);
}

#[test]
fn double_end() {
    let mut h = H::new();
    h.ok(start(None));
    h.run(MIN);
    h.ok(Command::End);
    let rev = h.engine.revision();
    assert_eq!(
        h.cmd(Command::End),
        Err(focus_core::CommandError::NoSession)
    );
    assert_eq!(h.engine.revision(), rev);
    assert_eq!(h.engine.history().len(), 1);
    assert_eq!(h.engine.friendship().total_points, 1);
}

#[test]
fn break_runs_to_awaiting_choice() {
    let mut h = H::new();
    h.ok(start(Some(1)));
    h.run(MIN);
    let before = h.engine.friendship().clone();
    let history = h.engine.history().len();
    let fx = h.ok(Command::StartBreak { minutes: None });
    assert!(matches!(
        transitions(&fx)[..],
        [Transition::BreakStarted {
            planned_ms: 300_000,
            ..
        }]
    ));
    assert!(h.engine.awaiting().is_none());
    h.run(5 * MIN - SEC);
    h.clock.advance(SEC);
    let fx = h.tick();
    assert_eq!(
        effect_names(&fx),
        ["Persist", "ActivityChanged", "BreakFinished", "PlaySound"]
    );
    assert_eq!(fx[3], Effect::PlaySound(SoundCue::BreakOver));
    let s = h.snap();
    assert_eq!(s.phase, Phase::AwaitingChoice);
    let a = s.awaiting.unwrap();
    assert_eq!(a.after, SessionKind::Break);
    assert!(a.record.is_none() && a.award.is_none() && !a.long_session);
    assert_eq!(h.engine.friendship(), &before, "breaks earn nothing");
    assert_eq!(h.engine.history().len(), history, "breaks are not recorded");
}

#[test]
fn break_without_sound() {
    let mut h = H::with_settings(|s| s.sound = false);
    h.ok(Command::StartBreak { minutes: Some(1) });
    let fx = h.run(MIN);
    assert!(transitions(&fx)
        .iter()
        .any(|t| matches!(t, Transition::BreakFinished { .. })));
    assert!(!fx.iter().any(|e| matches!(e, Effect::PlaySound(_))));
}

#[test]
fn breaks_and_focus_never_chain_automatically() {
    let mut h = H::new();
    h.ok(Command::StartBreak { minutes: Some(1) });
    h.run(MIN);
    let fx = h.run_step(60 * MIN, MIN);
    assert!(transitions(&fx).is_empty());
    assert_eq!(h.snap().phase, Phase::AwaitingChoice);
    assert!(h.snap().session.is_none());
    // And after focus.
    let mut h = H::new();
    h.ok(start(Some(1)));
    h.run(MIN);
    assert!(transitions(&h.run_step(60 * MIN, MIN)).is_empty());
    assert_eq!(h.snap().phase, Phase::AwaitingChoice);
}

#[test]
fn ending_a_break_goes_idle_without_a_record() {
    let mut h = H::new();
    h.ok(Command::StartBreak { minutes: None });
    h.run(2 * MIN);
    let fx = h.ok(Command::End);
    assert_eq!(
        effect_names(&fx),
        ["Persist", "ActivityChanged", "BreakEnded"]
    );
    assert_eq!(h.snap().phase, Phase::Idle);
    assert_eq!(h.engine.history().len(), 0);
    assert_eq!(h.engine.friendship().total_points, 0);
    assert!(h.engine.friendship().awards.is_empty());
}

#[test]
fn ending_a_due_break_finishes_it() {
    let mut h = H::new();
    h.ok(Command::StartBreak { minutes: Some(1) });
    h.run(MIN - SEC);
    h.clock.advance(1_500);
    let fx = h.ok(Command::End);
    assert!(matches!(
        transitions(&fx)[..],
        [Transition::BreakFinished { .. }]
    ));
    assert_eq!(h.snap().phase, Phase::AwaitingChoice);
}

#[test]
fn start_focus_from_awaiting_after_break() {
    let mut h = H::new();
    h.ok(Command::StartBreak { minutes: Some(1) });
    h.run(MIN);
    let fx = h.ok(start(Some(15)));
    assert!(
        matches!(transitions(&fx)[..], [Transition::FocusStarted { planned_ms, .. }] if planned_ms == 15 * MIN)
    );
    assert!(h.engine.awaiting().is_none());
    assert_eq!(h.snap().phase, Phase::Focus);
}

#[test]
fn start_break_from_awaiting_after_focus_clears_awaiting() {
    let mut h = H::new();
    h.ok(start(Some(1)));
    h.run(MIN);
    h.ok(Command::StartBreak { minutes: None });
    assert!(h.engine.awaiting().is_none());
    assert_eq!(h.snap().phase, Phase::Break);
}

#[test]
fn points_and_unlocks_never_decrease_over_a_day() {
    let mut h = H::new();
    let mut last_total = 0;
    let mut last_unlocked = h.engine.friendship().unlocked.clone();
    type Step = Box<dyn Fn(&mut H)>;
    let steps: Vec<Step> = vec![
        Box::new(|h| {
            h.ok(start(Some(25)));
            h.run(25 * MIN);
        }),
        Box::new(|h| {
            h.ok(Command::StartBreak { minutes: None });
            h.run(5 * MIN);
        }),
        Box::new(|h| {
            h.ok(start(Some(25)));
            h.run(3 * MIN);
            h.ok(Command::End);
        }),
        Box::new(|h| {
            h.ok(start(Some(25)));
            h.run(10 * MIN);
            h.ok(Command::Pause);
            h.clock.advance(121 * MIN);
            h.tick();
        }),
        Box::new(|h| {
            h.ok(Command::StartBreak { minutes: None });
            h.run(MIN);
            h.ok(Command::End);
        }),
        Box::new(|h| {
            h.ok(start(Some(45)));
            h.run(45 * MIN);
        }),
    ];
    for step in steps {
        step(&mut h);
        let f = h.engine.friendship();
        assert!(f.total_points >= last_total);
        assert!(f.unlocked.is_superset(&last_unlocked));
        last_total = f.total_points;
        last_unlocked = f.unlocked.clone();
    }
    // 30 + 0 + 3 + 10 + 0 + 50.
    assert_eq!(last_total, 93);
    assert_eq!(
        h.snap().today.harvest,
        vec![
            HarvestItem::Acorn,
            HarvestItem::Leaf,
            HarvestItem::Leaf,
            HarvestItem::Acorn
        ]
    );
    assert!(last_unlocked.contains("stretch"));
}

#[test]
fn unlock_thresholds_from_the_real_table() {
    // 25-min sessions: 30 points each. Thresholds 25 (wave), 60 (stretch), 100 (umbrella),
    // 160 (evening), 240 (sleepy).
    let mut h = H::new();
    let expect = [
        (30, vec!["wave"]),
        (60, vec!["stretch"]),
        (90, vec![]),
        (120, vec!["umbrella"]),
        (150, vec![]),
        (180, vec!["evening"]),
        (210, vec![]),
        (240, vec!["sleepy"]),
    ];
    for (total, newly) in expect {
        h.ok(start(Some(25)));
        let fx = h.run(25 * MIN);
        let award = completed(&fx).remove(0).1.unwrap();
        assert_eq!(award.total_points, total);
        assert_eq!(award.newly_unlocked, newly, "at {total}");
        let snap = h.snap();
        let next = snap.friendship.next.unwrap();
        assert!(next.points > total);
        assert_eq!(next.remaining, next.points - total);
        h.ok(Command::Dismiss);
    }
    assert_eq!(h.snap().friendship.next.unwrap().id, "celebrate");
}
