//! The engine test harness itself (SPEC §11): `relaunch` rebuilds from what is on
//! disk, and until the first applied Persist that is the state the harness loaded.

use chrono::TimeDelta;
use focus_core::engine::{Command, SystemEvent};
use focus_core::history::{EndReason, Outcome, SessionRecord};
use focus_core::rewards::AwardRecord;
use focus_core::store::PersistedState;
use focus_core::testkit::{effect_names, H, MIN, SEC};
use focus_core::timer::SessionKind;
use focus_core::{Clock, FakeClock};

/// A state `Engine::new` leaves unchanged (so launch emits no Persist): one completed
/// 25-minute session worth 30 points, plus non-default settings.
fn loaded_state() -> PersistedState {
    let w = FakeClock::default_start().now().wall - TimeDelta::hours(1);
    let mut s = PersistedState::default();
    s.settings.sound = false;
    s.settings.last_focus_minutes = 40;
    s.friendship.unlocked.insert("base".into());
    s.friendship.unlocked.insert("wave".into());
    s.friendship.total_points = 30;
    s.friendship.awards.insert(
        "old".into(),
        AwardRecord {
            points: 25,
            bonus: 5,
            completed: true,
            awarded_at: w,
        },
    );
    s.history.push(SessionRecord {
        id: "old".into(),
        kind: SessionKind::Focus,
        task_label: None,
        started_at: w - TimeDelta::minutes(25),
        ended_at: w,
        planned_ms: 25 * MIN,
        active_ms: 25 * MIN,
        outcome: Outcome::Completed,
        end_reason: EndReason::Completed,
        ended_utc_offset_minutes: Some(0),
    });
    s
}

fn assert_loaded(h: &H) {
    assert_eq!(h.engine.history().len(), 1);
    assert_eq!(h.engine.history().records()[0].id, "old");
    assert_eq!(h.engine.friendship().total_points, 30);
    assert!(!h.engine.settings().sound);
    assert_eq!(h.engine.settings().last_focus_minutes, 40);
}

#[test]
fn the_loaded_state_counts_as_on_disk_before_the_first_save() {
    let h = H::with_state(loaded_state());
    assert!(h.launch_fx.is_empty(), "{:?}", effect_names(&h.launch_fx));
    assert_eq!(h.saves, 0);
    assert_eq!(h.saved(), &loaded_state());
    assert_loaded(&h);
}

#[test]
fn relaunch_without_any_save_keeps_the_loaded_state() {
    let mut h = H::with_state(loaded_state());
    let fx = h.relaunch(MIN, false);
    assert!(fx.is_empty(), "{:?}", effect_names(&fx));
    assert_loaded(&h);
    assert_eq!(h.saves, 0);
    // And again.
    h.relaunch(MIN, true);
    assert_loaded(&h);
}

#[test]
fn a_crash_after_unsaved_changes_keeps_the_loaded_state() {
    let mut h = H::with_state(loaded_state());
    h.saving = false;
    h.ok(Command::StartFocus {
        minutes: Some(5),
        task_label: None,
    });
    h.run(5 * MIN);
    assert_eq!(h.engine.history().len(), 2);
    assert!(h.engine.friendship().total_points > 30);
    // Crash: none of that reached the disk.
    h.saving = true;
    h.relaunch(MIN, false);
    assert_loaded(&h);
    assert!(h.engine.active().is_none());
}

#[test]
fn points_and_history_never_decrease_across_a_crash() {
    let mut h = H::with_state(loaded_state());
    h.ok(Command::StartFocus {
        minutes: None,
        task_label: None,
    });
    h.run(5 * MIN);
    h.relaunch(SEC, false);
    assert_eq!(h.engine.friendship().total_points, 30);
    assert_eq!(h.engine.history().len(), 1);
    // The running checkpoint was saved too, so the session comes back paused.
    assert!(h.engine.active().is_some_and(|s| !s.is_running()));
}

#[test]
fn a_fresh_harness_relaunches_from_its_launch_save() {
    let mut h = H::new();
    // A fresh install reconciles and saves at launch.
    assert_eq!(effect_names(&h.launch_fx), ["Persist"]);
    assert_eq!(h.saves, 1);
    assert!(h.saved().friendship.unlocked.contains("base"));
    h.relaunch(MIN, false);
    assert!(h.launch_fx.is_empty(), "{:?}", effect_names(&h.launch_fx));
    assert!(h.engine.friendship().unlocked.contains("base"));
}

#[test]
fn a_cleared_disk_relaunches_from_defaults() {
    let mut h = H::with_state(loaded_state());
    h.last_saved = None;
    let fx = h.relaunch(MIN, false);
    // Like a missing file: a fresh install reconciles and saves.
    assert_eq!(effect_names(&fx), ["Persist"]);
    assert_eq!(h.engine.history().len(), 0);
    assert_eq!(h.engine.friendship().total_points, 0);
    assert!(h.saved().friendship.unlocked.contains("base"));
}

#[test]
fn saving_off_from_the_start_relaunches_from_the_loaded_state() {
    let mut h = H::with_state(loaded_state());
    h.saving = false;
    h.ev(SystemEvent::Tick);
    h.ok(Command::Quit);
    h.saving = true;
    h.relaunch(MIN, false);
    assert_loaded(&h);
}
