//! The brief's engine-level acceptance checks, one test each (fake clock). Window,
//! focus, sound, tray and real sleep behavior need manual verification on a Mac.

use focus_core::engine::{Command, Effect, SystemEvent, Transition};
use focus_core::history::{EndReason, HarvestItem, Outcome};
use focus_core::snapshot::Phase;
use focus_core::testkit::{transitions, H, HOUR, MIN, SEC};
use focus_core::timer::{PauseReason, RunStatus};

fn start_last() -> Command {
    Command::StartFocus {
        minutes: None,
        task_label: None,
    }
}

fn kinds(fx: &[Effect]) -> Vec<&'static str> {
    focus_core::testkit::effect_names(fx)
}

/// "The last-used duration can be started from the menu bar, the hotkey, and the widget
/// without entering a task." (Tray and widget call start_focus{}; the hotkey is Toggle.)
#[test]
fn last_duration_starts_from_every_entry_point_without_a_task() {
    let mut h = H::new();
    h.ok(Command::StartFocus {
        minutes: Some(45),
        task_label: None,
    });
    h.ok(Command::End);
    for entry in [start_last(), Command::Toggle] {
        let fx = h.ok(entry.clone());
        assert!(
            matches!(transitions(&fx)[..], [Transition::FocusStarted { planned_ms, .. }] if planned_ms == 45 * MIN),
            "{entry:?}"
        );
        assert_eq!(h.snap().session.unwrap().task_label, None);
        h.ok(Command::End);
    }
}

/// "Pause freezes remaining time; resume continues from that point."
#[test]
fn pause_freezes_and_resume_continues() {
    let mut h = H::new();
    h.ok(start_last());
    h.run(7 * MIN + 30 * SEC);
    h.ok(Command::Pause);
    let frozen = h.snap().session.unwrap().remaining_label;
    assert_eq!(frozen, "17:30");
    h.run_step(90 * MIN, 15 * SEC);
    assert_eq!(h.snap().session.unwrap().remaining_label, frozen);
    h.ok(Command::Resume);
    h.run(SEC);
    assert_eq!(h.snap().session.unwrap().remaining_label, "17:29");
}

/// "Hiding the widget or reloading the frontend does not alter the timer or create a
/// second one." (Hide is a shell window op; a reload is get_snapshot = Tick + snapshot.)
#[test]
fn hiding_or_reloading_does_not_alter_the_timer() {
    let mut h = H::new();
    h.ok(start_last());
    h.run(5 * MIN);
    h.user_show_hide(false);
    for _ in 0..20 {
        h.tick();
        let _ = h.snap();
    }
    h.user_show_hide(true);
    let v = h.snap().session.unwrap();
    assert_eq!(v.id, "s1");
    assert_eq!(v.remaining_label, "20:00");
    assert_eq!(v.status, RunStatus::Running);
    assert_eq!(
        h.cmd(start_last()),
        Err(focus_core::CommandError::SessionActive),
        "no second timer"
    );
}

/// "Sleeping the Mac pauses the session; waking cannot earn time for the sleep period."
#[test]
fn sleep_pauses_and_wake_earns_nothing() {
    // (notifications delivered?, monotonic clock counts sleep?)
    for (notified, counting) in [(true, false), (true, true), (false, false), (false, true)] {
        let mut h = H::new();
        h.ok(start_last());
        h.run(10 * MIN);
        if notified {
            h.ev(SystemEvent::Suspend);
        }
        if counting {
            h.sleep_mono_counting(45 * MIN);
        } else {
            h.sleep_mono_stopped(45 * MIN);
        }
        h.ev(SystemEvent::Wake);
        h.run(MIN);
        let v = h.snap().session.unwrap();
        assert_eq!(
            v.status,
            RunStatus::Paused,
            "notified={notified} counting={counting}"
        );
        assert!(
            v.active_ms <= 10 * MIN + SEC,
            "notified={notified} counting={counting}: {}",
            v.active_ms
        );
        assert_eq!(h.engine.friendship().total_points, 0);
    }
}

/// "Relaunch restores a paused checkpoint; closed-app time earns nothing."
#[test]
fn relaunch_restores_paused_and_closed_time_earns_nothing() {
    for quit_first in [true, false] {
        let mut h = H::new();
        h.ok(start_last());
        h.run(12 * MIN);
        if quit_first {
            h.ok(Command::Quit);
        }
        h.relaunch(90 * MIN, true);
        let v = h.snap().session.unwrap();
        assert_eq!(v.status, RunStatus::Paused);
        assert_eq!(v.active_ms, 12 * MIN, "quit_first={quit_first}");
        let expected = if quit_first {
            PauseReason::Quit
        } else {
            PauseReason::Restored
        };
        assert_eq!(v.pause_reason, Some(expected));
    }
}

/// "A completed 25-minute session adds 30 points once; repeating completion processing
/// adds none."
#[test]
fn completed_25_minutes_adds_30_points_once() {
    let mut h = H::new();
    h.ok(start_last());
    h.run(25 * MIN);
    assert_eq!(h.engine.friendship().total_points, 30);
    // Repeat everything that could re-process the completion.
    h.run(5 * MIN);
    h.ev(SystemEvent::Wake);
    h.ev(SystemEvent::ClockChanged);
    h.relaunch(MIN, false);
    h.relaunch(MIN, true);
    h.run(MIN);
    assert_eq!(h.engine.friendship().total_points, 30);
    assert_eq!(h.engine.history().len(), 1);
    assert_eq!(h.engine.friendship().awards.len(), 1);
}

/// "A session interrupted at 22 active minutes adds 22 points, no bonus, a leaf in the
/// harvest row, no scolding."
#[test]
fn interrupted_at_22_minutes() {
    let mut h = H::new();
    h.ok(start_last());
    h.run(22 * MIN);
    let fx = h.ok(Command::End);
    assert_eq!(
        kinds(&fx),
        ["Persist", "ActivityChanged", "FocusInterrupted"],
        "no sound, no celebration"
    );
    let award = match &transitions(&fx)[0] {
        Transition::FocusInterrupted { award, .. } => award.clone().unwrap(),
        other => panic!("{other:?}"),
    };
    assert_eq!((award.points, award.bonus, award.total_points), (22, 0, 22));
    assert_eq!(h.snap().today.harvest, vec![HarvestItem::Leaf]);
    assert_eq!(h.snap().phase, Phase::Idle);
}

/// "A session paused more than 120 minutes ends as interrupted with its earned minutes kept."
#[test]
fn paused_more_than_120_minutes_ends_interrupted_with_minutes_kept() {
    let mut h = H::new();
    h.ok(start_last());
    h.run(18 * MIN);
    h.ok(Command::Pause);
    h.run_step(120 * MIN, 15 * SEC);
    assert!(
        h.engine.active().is_some(),
        "exactly 120 min is not more than 120"
    );
    h.clock.advance(1);
    let fx = h.tick();
    match &transitions(&fx)[..] {
        [Transition::FocusInterrupted { record, award }] => {
            assert_eq!(record.outcome, Outcome::Interrupted);
            assert_eq!(record.end_reason, EndReason::PauseExpired);
            assert_eq!(record.active_ms, 18 * MIN);
            assert_eq!(award.as_ref().unwrap().points, 18);
        }
        other => panic!("{other:?}"),
    }
}

/// "Breaks add no points; points and unlocks never decrease."
#[test]
fn breaks_add_no_points() {
    let mut h = H::new();
    h.ok(start_last());
    h.run(25 * MIN);
    let before = h.engine.friendship().clone();
    for _ in 0..3 {
        h.ok(Command::StartBreak { minutes: None });
        h.run(5 * MIN);
        h.ok(Command::StartBreak { minutes: Some(10) });
        h.run(3 * MIN);
        h.ok(Command::End);
        h.ok(Command::StartBreak { minutes: None });
        h.ok(Command::Pause);
        h.clock.advance(3 * HOUR);
        h.tick();
    }
    assert_eq!(h.engine.friendship(), &before);
    assert_eq!(h.engine.history().len(), 1);
}

/// "Wall-clock changes never finish sessions or grant points."
#[test]
fn wall_clock_changes_never_finish_sessions_or_grant_points() {
    let mut h = H::new();
    h.ok(start_last());
    h.run(20 * MIN);
    for jump in [
        HOUR as i64,
        -(HOUR as i64),
        26 * HOUR as i64,
        -(3 * HOUR as i64),
        90_000,
    ] {
        h.clock.jump_wall(jump);
        for ev in [SystemEvent::ClockChanged, SystemEvent::Tick] {
            let fx = h.ev(ev);
            assert!(!transitions(&fx).iter().any(|t| matches!(
                t,
                Transition::FocusCompleted { .. } | Transition::FocusInterrupted { .. }
            )));
        }
        assert_eq!(h.engine.friendship().total_points, 0);
        assert!(h.snap().session.unwrap().active_ms <= 20 * MIN + 2 * SEC);
        if !h.engine.is_running() {
            h.ok(Command::Resume);
        }
    }
    assert!(h.engine.history().is_empty());
}

/// "The menu-bar title shows mm:ss while running" — the shared countdown label.
#[test]
fn countdown_label_is_mm_ss() {
    let mut h = H::new();
    h.ok(Command::StartFocus {
        minutes: Some(5),
        task_label: None,
    });
    assert_eq!(h.snap().session.unwrap().remaining_label, "05:00");
    h.run(SEC);
    assert_eq!(h.snap().session.unwrap().remaining_label, "04:59");
    h.run(4 * MIN + 58 * SEC);
    assert_eq!(h.snap().session.unwrap().remaining_label, "00:01");
}

/// "Settings, friendship, and history persist across restarts."
#[test]
fn settings_friendship_and_history_persist() {
    let mut h = H::new();
    h.ok(Command::UpdateSettings(
        focus_core::settings::UserSettingsPatch {
            reduced_motion: Some(true),
            ..Default::default()
        }
        .into(),
    ));
    h.ok(start_last());
    h.run(25 * MIN);
    h.ok(Command::Quit);
    h.relaunch(12 * HOUR, false);
    assert!(h.engine.settings().reduced_motion);
    assert_eq!(h.engine.friendship().total_points, 30);
    assert!(h.engine.friendship().unlocked.contains("wave"));
    assert_eq!(h.engine.history().len(), 1);
}
