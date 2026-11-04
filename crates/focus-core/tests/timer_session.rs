//! ActiveSession (SPEC §3) and task-label normalization.

use chrono::{TimeDelta, TimeZone, Utc};
use focus_core::clock::{Mono, Now, Wall};
use focus_core::timer::{
    normalize_task_label, ActiveSession, PauseReason, PersistedSession, RunState, RunStatus,
    SessionKind,
};

const MIN: u64 = 60_000;

fn t0() -> Wall {
    Utc.with_ymd_and_hms(2026, 3, 2, 9, 0, 0).unwrap()
}

fn at(mono_ms: u64, wall_offset_ms: i64) -> Now {
    Now {
        mono: Mono(1_000_000 + mono_ms),
        wall: t0() + TimeDelta::milliseconds(wall_offset_ms),
    }
}

fn mono(ms: u64) -> Mono {
    Mono(1_000_000 + ms)
}

fn focus(planned: u64) -> ActiveSession {
    ActiveSession::new("s1".into(), SessionKind::Focus, None, planned, at(0, 0))
}

#[test]
fn new_is_running_with_zero_accrued() {
    let s = ActiveSession::new(
        "x".into(),
        SessionKind::Focus,
        Some("Read".into()),
        25 * MIN,
        at(0, 0),
    );
    assert!(s.is_running());
    assert_eq!(s.status(), RunStatus::Running);
    assert_eq!(s.accrued_ms, 0);
    assert_eq!(s.started_at, t0());
    assert_eq!(s.run, RunState::Running { since: mono(0) });
    assert_eq!(s.task_label.as_deref(), Some("Read"));
    assert_eq!(s.pause_reason(), None);
    assert_eq!(s.active_ms(mono(0)), 0);
    assert_eq!(s.remaining_ms(mono(0)), 25 * MIN);
}

#[test]
fn active_ms_caps_at_planned() {
    let s = focus(25 * MIN);
    assert_eq!(s.active_ms(mono(30 * MIN)), 25 * MIN);
    assert_eq!(s.remaining_ms(mono(30 * MIN)), 0);
    assert!(s.is_due(mono(30 * MIN)));
    assert!(!s.is_due(mono(25 * MIN - 1)));
    assert!(s.is_due(mono(25 * MIN)));
}

#[test]
fn active_ms_is_pure() {
    let s = focus(25 * MIN);
    let before = s.clone();
    let _ = s.active_ms(mono(10 * MIN));
    assert_eq!(s, before);
}

#[test]
fn fold_accumulates_and_restarts_since() {
    let mut s = focus(25 * MIN);
    s.fold(mono(10_000));
    assert_eq!(s.accrued_ms, 10_000);
    assert_eq!(
        s.run,
        RunState::Running {
            since: mono(10_000)
        }
    );
    s.fold(mono(15_000));
    assert_eq!(s.accrued_ms, 15_000);
    // Earlier than `since` folds nothing and does not move `since` back.
    s.fold(mono(12_000));
    assert_eq!(s.accrued_ms, 15_000);
    assert_eq!(
        s.run,
        RunState::Running {
            since: mono(15_000)
        }
    );
    // Folding twice at the same instant is idempotent.
    s.fold(mono(15_000));
    assert_eq!(s.accrued_ms, 15_000);
}

#[test]
fn fold_caps_and_reports_overshoot() {
    let mut s = focus(MIN);
    assert_eq!(s.fold_overshoot(mono(50_000)), 0);
    assert_eq!(s.fold_overshoot(mono(61_500)), 1_500);
    assert_eq!(s.accrued_ms, MIN);
    assert!(s.is_due(mono(61_500)));
}

#[test]
fn fold_is_noop_while_paused() {
    let mut s = focus(25 * MIN);
    s.pause_at(mono(5_000), at(5_000, 5_000).wall, PauseReason::User);
    s.fold(mono(100_000));
    assert_eq!(s.accrued_ms, 5_000);
    assert_eq!(s.fold_overshoot(mono(100 * MIN)), 0);
}

#[test]
fn pause_at_folds_and_sets_anchor() {
    let mut s = focus(25 * MIN);
    let w = at(7_000, 7_000).wall;
    s.pause_at(mono(7_000), w, PauseReason::User);
    assert_eq!(s.accrued_ms, 7_000);
    assert_eq!(
        s.run,
        RunState::Paused {
            paused_at: w,
            reason: PauseReason::User,
            age_anchor: w
        }
    );
    assert_eq!(s.pause_reason(), Some(PauseReason::User));
    assert_eq!(s.paused_at(), Some(w));
    assert_eq!(s.age_anchor(), Some(w));
    assert_eq!(s.status(), RunStatus::Paused);
    // A second pause is a no-op (reason and stamps unchanged).
    s.pause_at(mono(20_000), at(20_000, 20_000).wall, PauseReason::Suspend);
    assert_eq!(s.accrued_ms, 7_000);
    assert_eq!(s.pause_reason(), Some(PauseReason::User));
    assert_eq!(s.paused_at(), Some(w));
}

#[test]
fn pause_freezes_active_time() {
    let mut s = focus(25 * MIN);
    s.pause_at(mono(10 * MIN), at(10 * MIN, 0).wall, PauseReason::User);
    assert_eq!(s.active_ms(mono(70 * MIN)), 10 * MIN);
    assert_eq!(s.remaining_ms(mono(70 * MIN)), 15 * MIN);
}

#[test]
fn resume_restarts_at_now() {
    let mut s = focus(25 * MIN);
    s.pause_at(mono(10_000), at(10_000, 10_000).wall, PauseReason::User);
    s.resume(at(70_000, 70_000));
    assert!(s.is_running());
    assert_eq!(s.active_ms(mono(75_000)), 15_000);
    // Resume while running is a no-op (does not restart `since`).
    s.resume(at(74_000, 74_000));
    assert_eq!(s.active_ms(mono(75_000)), 15_000);
}

#[test]
fn pause_age() {
    let mut s = focus(25 * MIN);
    assert_eq!(s.pause_age_ms(t0()), None);
    let w = at(0, 60_000).wall;
    s.pause_at(mono(1), w, PauseReason::User);
    assert_eq!(s.pause_age_ms(w), Some(0));
    assert_eq!(
        s.pause_age_ms(w + TimeDelta::milliseconds(1234)),
        Some(1234)
    );
    // Anchor in the future → clamped to zero.
    assert_eq!(s.pause_age_ms(w - TimeDelta::hours(1)), Some(0));
}

#[test]
fn shift_age_anchor_both_ways() {
    let mut s = focus(25 * MIN);
    let w = t0();
    s.pause_at(mono(0), w, PauseReason::User);
    s.shift_age_anchor(3 * 3_600_000);
    assert_eq!(s.age_anchor(), Some(w + TimeDelta::hours(3)));
    assert_eq!(
        s.paused_at(),
        Some(w),
        "paused_at is the true stamp and never shifts"
    );
    // After a matching +3 h wall jump the age is unchanged.
    assert_eq!(
        s.pause_age_ms(w + TimeDelta::hours(3) + TimeDelta::seconds(5)),
        Some(5_000)
    );
    s.shift_age_anchor(-6 * 3_600_000);
    assert_eq!(s.age_anchor(), Some(w - TimeDelta::hours(3)));
    // Running sessions ignore shifts.
    let mut r = focus(25 * MIN);
    r.shift_age_anchor(1000);
    assert!(r.is_running());
    // Extreme values do not panic.
    s.shift_age_anchor(i64::MIN);
    s.shift_age_anchor(i64::MAX);
}

#[test]
fn clamp_pause_stamps() {
    let mut s = focus(25 * MIN);
    let future = t0() + TimeDelta::days(1);
    s.pause_at(mono(0), future, PauseReason::User);
    assert!(s.clamp_pause_stamps(t0()));
    assert_eq!(s.paused_at(), Some(t0()));
    assert_eq!(s.age_anchor(), Some(t0()));
    assert!(!s.clamp_pause_stamps(t0()));
    assert!(!focus(MIN).clamp_pause_stamps(t0()));
}

#[test]
fn to_persisted_running_does_not_mutate() {
    let mut s = focus(25 * MIN);
    s.fold(mono(MIN));
    let before = s.clone();
    let p = s.to_persisted(at(3 * MIN, 3 * MIN as i64));
    assert_eq!(s, before);
    assert_eq!(p.accrued_ms, 3 * MIN);
    assert_eq!(p.status, RunStatus::Running);
    assert_eq!(p.checkpoint_at, at(0, 3 * MIN as i64).wall);
    assert_eq!(p.paused_at, None);
    assert_eq!(p.pause_reason, None);
    assert_eq!(p.age_anchor, None);
    assert_eq!(p.planned_ms, 25 * MIN);
    assert_eq!(p.started_at, t0());
}

#[test]
fn to_persisted_caps_accrued() {
    let s = focus(MIN);
    assert_eq!(s.to_persisted(at(5 * MIN, 0)).accrued_ms, MIN);
}

#[test]
fn to_persisted_paused_keeps_fields() {
    let mut s = focus(25 * MIN);
    let w = at(2 * MIN, 2 * MIN as i64).wall;
    s.pause_at(mono(2 * MIN), w, PauseReason::Quit);
    s.shift_age_anchor(1000);
    let p = s.to_persisted(at(9 * MIN, 9 * MIN as i64));
    assert_eq!(p.accrued_ms, 2 * MIN);
    assert_eq!(p.status, RunStatus::Paused);
    assert_eq!(p.paused_at, Some(w));
    assert_eq!(p.pause_reason, Some(PauseReason::Quit));
    assert_eq!(p.age_anchor, Some(w + TimeDelta::seconds(1)));
}

fn persisted(status: RunStatus) -> PersistedSession {
    PersistedSession {
        id: "s9".into(),
        kind: SessionKind::Focus,
        task_label: Some("Review".into()),
        planned_ms: 25 * MIN,
        started_at: t0(),
        accrued_ms: 10 * MIN,
        status,
        checkpoint_at: t0() + TimeDelta::minutes(10),
        paused_at: None,
        pause_reason: None,
        age_anchor: None,
    }
}

#[test]
fn from_persisted_running_becomes_paused_restored() {
    let p = persisted(RunStatus::Running);
    let s = ActiveSession::from_persisted(p.clone());
    assert_eq!(
        s.run,
        RunState::Paused {
            paused_at: p.checkpoint_at,
            reason: PauseReason::Restored,
            age_anchor: p.checkpoint_at
        }
    );
    assert_eq!(s.accrued_ms, 10 * MIN);
    assert_eq!(s.id, "s9");
    assert_eq!(s.task_label.as_deref(), Some("Review"));
    assert_eq!(s.started_at, t0());
}

#[test]
fn from_persisted_paused_keeps_reason_and_anchor() {
    let mut p = persisted(RunStatus::Paused);
    let paused = t0() + TimeDelta::minutes(5);
    let anchor = paused + TimeDelta::hours(1);
    p.paused_at = Some(paused);
    p.pause_reason = Some(PauseReason::User);
    p.age_anchor = Some(anchor);
    let s = ActiveSession::from_persisted(p.clone());
    assert_eq!(
        s.run,
        RunState::Paused {
            paused_at: paused,
            reason: PauseReason::User,
            age_anchor: anchor
        }
    );
    // Missing anchor → paused_at; missing paused_at → checkpoint_at.
    p.age_anchor = None;
    assert_eq!(
        ActiveSession::from_persisted(p.clone()).age_anchor(),
        Some(paused)
    );
    p.paused_at = None;
    let s = ActiveSession::from_persisted(p.clone());
    assert_eq!(s.paused_at(), Some(p.checkpoint_at));
    assert_eq!(s.age_anchor(), Some(p.checkpoint_at));
    // Missing reason → Restored.
    p.pause_reason = None;
    assert_eq!(
        ActiveSession::from_persisted(p).pause_reason(),
        Some(PauseReason::Restored)
    );
}

#[test]
fn from_persisted_clamps_accrued() {
    let mut p = persisted(RunStatus::Running);
    p.accrued_ms = 99 * MIN;
    assert_eq!(ActiveSession::from_persisted(p).accrued_ms, 25 * MIN);
}

#[test]
fn persisted_roundtrip_through_restore() {
    let mut s = focus(25 * MIN);
    s.pause_at(mono(MIN), at(MIN, MIN as i64).wall, PauseReason::User);
    let p = s.to_persisted(at(2 * MIN, 2 * MIN as i64));
    assert_eq!(ActiveSession::from_persisted(p), s);
}

#[test]
fn persisted_json_is_camel_case_and_omits_none() {
    let p = persisted(RunStatus::Running);
    let v = serde_json::to_value(&p).unwrap();
    let obj = v.as_object().unwrap();
    for key in [
        "id",
        "kind",
        "taskLabel",
        "plannedMs",
        "startedAt",
        "accruedMs",
        "status",
        "checkpointAt",
    ] {
        assert!(obj.contains_key(key), "{key} missing in {v}");
    }
    for key in ["pausedAt", "pauseReason", "ageAnchor"] {
        assert!(!obj.contains_key(key), "{key} should be omitted when None");
    }
    assert_eq!(obj["status"], "running");
    assert_eq!(obj["kind"], "focus");
    let back: PersistedSession = serde_json::from_value(v).unwrap();
    assert_eq!(back, p);

    let mut paused = persisted(RunStatus::Paused);
    paused.task_label = None;
    paused.paused_at = Some(t0());
    paused.pause_reason = Some(PauseReason::ClockJump);
    paused.age_anchor = Some(t0());
    let v = serde_json::to_value(&paused).unwrap();
    assert_eq!(v["pauseReason"], "clockJump");
    assert!(v.get("taskLabel").is_none());
    assert_eq!(
        serde_json::from_value::<PersistedSession>(v).unwrap(),
        paused
    );
}

#[test]
fn label_normalization() {
    let n = |s: &str| normalize_task_label(s, 80);
    assert_eq!(n("  Review  ").as_deref(), Some("Review"));
    assert_eq!(n("   "), None);
    assert_eq!(n(""), None);
    assert_eq!(n("a\nb\tc").as_deref(), Some("a b c"));
    assert_eq!(n("a \u{0}\u{7} b").as_deref(), Some("a b"));
    assert_eq!(
        n("one   two\r\n\r\nthree").as_deref(),
        Some("one two three")
    );
    assert_eq!(n("\u{1b}[31m red").as_deref(), Some("[31m red"));
    assert_eq!(n(&"x".repeat(100)).unwrap(), "x".repeat(80));
    assert_eq!(n(&"x".repeat(80)).unwrap(), "x".repeat(80));
    let emoji = "🌰".repeat(81);
    let got = n(&emoji).unwrap();
    assert_eq!(got.chars().count(), 80);
    assert!(got.chars().all(|c| c == '🌰'));
    // Truncation that ends on a space is trimmed again.
    let s = format!("{} b", "a".repeat(79));
    assert_eq!(n(&s).unwrap(), "a".repeat(79));
    // Unicode whitespace runs collapse too.
    assert_eq!(n("a\u{3000}\u{00a0} b").as_deref(), Some("a b"));
    assert_eq!(
        n("Review one lecture").as_deref(),
        Some("Review one lecture")
    );
}
