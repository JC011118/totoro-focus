//! History and the derived today summary (SPEC §6).

use chrono::{DateTime, FixedOffset, TimeZone, Utc};
use focus_core::clock::Wall;
use focus_core::history::{
    local_date, offset_minutes, record_local_date, EndReason, HarvestItem, History, Outcome,
    SessionRecord,
};
use focus_core::timer::SessionKind;

const MIN: u64 = 60_000;

fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> Wall {
    Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
}

fn rec(
    id: &str,
    ended_at: Wall,
    active_min: u64,
    outcome: Outcome,
    offset_min: Option<i32>,
) -> SessionRecord {
    SessionRecord {
        id: id.into(),
        kind: SessionKind::Focus,
        task_label: None,
        started_at: ended_at - chrono::TimeDelta::minutes(active_min as i64),
        ended_at,
        planned_ms: 25 * MIN,
        active_ms: active_min * MIN,
        outcome,
        end_reason: match outcome {
            Outcome::Completed => EndReason::Completed,
            Outcome::Interrupted => EndReason::UserEnded,
        },
        ended_utc_offset_minutes: offset_min,
    }
}

fn fixed(hours: i32) -> impl Fn(Wall) -> FixedOffset {
    move |_| FixedOffset::east_opt(hours * 3600).unwrap()
}

#[test]
fn push_once_appends_unique_ids_only() {
    let mut h = History::default();
    assert!(h.is_empty());
    assert!(h.push_once(rec(
        "a",
        utc(2026, 3, 2, 9, 0),
        25,
        Outcome::Completed,
        None
    )));
    assert!(!h.push_once(rec(
        "a",
        utc(2026, 3, 2, 10, 0),
        5,
        Outcome::Interrupted,
        None
    )));
    assert!(h.push_once(rec(
        "b",
        utc(2026, 3, 2, 11, 0),
        5,
        Outcome::Interrupted,
        None
    )));
    assert_eq!(h.len(), 2);
    assert!(h.contains("a") && h.contains("b") && !h.contains("c"));
    assert_eq!(
        h.records()[0].active_ms,
        25 * MIN,
        "the first record for an id wins"
    );
}

#[test]
fn dedupe_keeps_first_occurrence() {
    let mut h = History::new(vec![
        rec("a", utc(2026, 3, 2, 9, 0), 1, Outcome::Completed, None),
        rec("b", utc(2026, 3, 2, 9, 0), 2, Outcome::Completed, None),
        rec("a", utc(2026, 3, 2, 9, 0), 3, Outcome::Interrupted, None),
        rec("b", utc(2026, 3, 2, 9, 0), 4, Outcome::Interrupted, None),
        rec("c", utc(2026, 3, 2, 9, 0), 5, Outcome::Completed, None),
    ]);
    assert!(h.dedupe());
    let ids: Vec<_> = h
        .records()
        .iter()
        .map(|r| (r.id.as_str(), r.active_ms / MIN))
        .collect();
    assert_eq!(ids, vec![("a", 1), ("b", 2), ("c", 5)]);
    assert!(!h.dedupe());
}

#[test]
fn recent_is_reverse_insertion_order_with_limit() {
    let h = History::new(vec![
        rec("a", utc(2026, 3, 2, 12, 0), 1, Outcome::Completed, None),
        // Inserted later but with an earlier timestamp (clock moved back): order is insertion.
        rec("b", utc(2026, 3, 1, 8, 0), 2, Outcome::Completed, None),
        rec("c", utc(2026, 3, 2, 13, 0), 3, Outcome::Completed, None),
    ]);
    let ids = |v: Vec<SessionRecord>| v.into_iter().map(|r| r.id).collect::<Vec<_>>();
    assert_eq!(ids(h.recent(10)), vec!["c", "b", "a"]);
    assert_eq!(ids(h.recent(2)), vec!["c", "b"]);
    assert!(h.recent(0).is_empty());
    assert_eq!(h.clone().into_records().len(), 3);
}

#[test]
fn today_counts_only_todays_records_in_insertion_order() {
    let now = utc(2026, 3, 2, 15, 0);
    let h = History::new(vec![
        rec(
            "y",
            utc(2026, 3, 1, 23, 40),
            25,
            Outcome::Completed,
            Some(0),
        ), // yesterday
        rec("a", utc(2026, 3, 2, 9, 25), 25, Outcome::Completed, Some(0)),
        rec(
            "b",
            utc(2026, 3, 2, 8, 0),
            22,
            Outcome::Interrupted,
            Some(0),
        ), // earlier stamp, later insert
        rec("c", utc(2026, 3, 2, 14, 0), 45, Outcome::Completed, Some(0)),
        rec("t", utc(2026, 3, 3, 0, 5), 10, Outcome::Completed, Some(0)), // "tomorrow" (clock moved)
    ]);
    let s = h.today(now, &fixed(0), 0);
    assert_eq!(s.date, "2026-03-02");
    assert_eq!(
        s.harvest,
        vec![HarvestItem::Acorn, HarvestItem::Leaf, HarvestItem::Acorn]
    );
    assert_eq!((s.completed, s.interrupted), (2, 1));
    assert_eq!(s.focus_ms, (25 + 22 + 45) * MIN);
    assert_eq!(s.active_focus_ms, 0);
    assert_eq!(s.focus_label, "1 h 32 min");
}

#[test]
fn today_label_includes_the_current_session() {
    let now = utc(2026, 3, 2, 15, 0);
    let h = History::new(vec![rec(
        "a",
        utc(2026, 3, 2, 9, 25),
        25,
        Outcome::Completed,
        Some(0),
    )]);
    let s = h.today(now, &fixed(0), 10 * MIN + 59_999);
    assert_eq!(s.focus_ms, 25 * MIN);
    assert_eq!(s.active_focus_ms, 10 * MIN + 59_999);
    assert_eq!(s.focus_label, "35 min");
    let empty = History::default().today(now, &fixed(0), 0);
    assert_eq!(empty.focus_label, "0 min");
    assert!(empty.harvest.is_empty());
}

#[test]
fn harvest_is_uncapped() {
    let now = utc(2026, 3, 2, 23, 0);
    let recs: Vec<_> = (0..15)
        .map(|i| {
            rec(
                &format!("r{i}"),
                utc(2026, 3, 2, 8, i),
                1,
                if i % 3 == 0 {
                    Outcome::Interrupted
                } else {
                    Outcome::Completed
                },
                None,
            )
        })
        .collect();
    let s = History::new(recs).today(now, &fixed(0), 0);
    assert_eq!(s.harvest.len(), 15);
    assert_eq!(s.harvest[0], HarvestItem::Leaf);
    assert_eq!(s.harvest[1], HarvestItem::Acorn);
}

#[test]
fn stored_offset_decides_the_record_day() {
    // Ended 23:30 UTC on Mar 1. Written at UTC+0 → Mar 1. Written at UTC+2 → Mar 2.
    let ended = utc(2026, 3, 1, 23, 30);
    let now = utc(2026, 3, 2, 10, 0); // Mar 2 under both offsets
    let utc0 = History::new(vec![rec("a", ended, 25, Outcome::Completed, Some(0))]);
    assert!(
        utc0.today(now, &fixed(2), 0).harvest.is_empty(),
        "stored offset wins over the current one"
    );
    let plus2 = History::new(vec![rec("a", ended, 25, Outcome::Completed, Some(120))]);
    assert_eq!(
        plus2.today(now, &fixed(0), 0).harvest,
        vec![HarvestItem::Acorn]
    );
}

#[test]
fn missing_offset_uses_offset_at_ended_at() {
    // today_timezone_change: record ended 23:30 UTC (offset 0 → day D). With the current
    // offset +2 and now in D+1 local, the legacy record (no stored offset) counts today.
    let ended = utc(2026, 3, 1, 23, 30);
    let h = History::new(vec![rec("a", ended, 25, Outcome::Completed, None)]);
    let now = utc(2026, 3, 2, 8, 0);
    assert_eq!(h.today(now, &fixed(2), 0).harvest, vec![HarvestItem::Acorn]);
    assert!(h.today(now, &fixed(0), 0).harvest.is_empty());
}

#[test]
fn dst_closure_uses_the_offset_at_each_instant() {
    // A closure that switches from +1 to +2 at 2026-03-29 01:00 UTC (EU DST start).
    let switch: DateTime<Utc> = utc(2026, 3, 29, 1, 0);
    let offset_at =
        move |w: Wall| FixedOffset::east_opt(if w < switch { 3600 } else { 7200 }).unwrap();
    // Ended 22:30 UTC on Mar 28 → 23:30 local (+1) → Mar 28.
    let r_before = rec("a", utc(2026, 3, 28, 22, 30), 25, Outcome::Completed, None);
    // Ended 22:30 UTC on Mar 29 → 00:30 local (+2) → Mar 30.
    let r_after = rec("b", utc(2026, 3, 29, 22, 30), 25, Outcome::Completed, None);
    assert_eq!(
        record_local_date(&r_before, &offset_at).to_string(),
        "2026-03-28"
    );
    assert_eq!(
        record_local_date(&r_after, &offset_at).to_string(),
        "2026-03-30"
    );
    let h = History::new(vec![r_before, r_after]);
    // now = Mar 29 22:45 UTC → 00:45 Mar 30 local (+2).
    let s = h.today(utc(2026, 3, 29, 22, 45), &offset_at, 0);
    assert_eq!(s.date, "2026-03-30");
    assert_eq!(s.harvest, vec![HarvestItem::Acorn]);
}

#[test]
fn invalid_stored_offset_falls_back() {
    let ended = utc(2026, 3, 1, 23, 30);
    let mut r = rec("a", ended, 25, Outcome::Completed, Some(100_000)); // beyond ±24 h
    assert_eq!(record_local_date(&r, &fixed(2)).to_string(), "2026-03-02");
    r.ended_utc_offset_minutes = Some(i32::MAX);
    assert_eq!(record_local_date(&r, &fixed(0)).to_string(), "2026-03-01");
}

#[test]
fn non_focus_records_are_ignored() {
    let mut r = rec("brk", utc(2026, 3, 2, 9, 0), 5, Outcome::Completed, None);
    r.kind = SessionKind::Break;
    let s = History::new(vec![r]).today(utc(2026, 3, 2, 10, 0), &fixed(0), 0);
    assert!(s.harvest.is_empty());
    assert_eq!(s.focus_ms, 0);
}

#[test]
fn helpers() {
    assert_eq!(
        offset_minutes(FixedOffset::east_opt(5 * 3600 + 1800).unwrap()),
        330
    );
    assert_eq!(
        offset_minutes(FixedOffset::west_opt(8 * 3600).unwrap()),
        -480
    );
    let d = local_date(
        utc(2026, 3, 1, 23, 30),
        FixedOffset::east_opt(3600).unwrap(),
    );
    assert_eq!(d.to_string(), "2026-03-02");
}

#[test]
fn record_json_shape() {
    let r = rec(
        "a",
        utc(2026, 3, 2, 9, 25),
        25,
        Outcome::Completed,
        Some(-300),
    );
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["endedUtcOffsetMinutes"], -300);
    assert_eq!(v["outcome"], "completed");
    assert_eq!(v["endReason"], "completed");
    assert!(v.get("taskLabel").is_none());
    let legacy = serde_json::json!({
        "id": "x", "kind": "focus", "startedAt": "2026-03-02T09:00:00Z", "endedAt": "2026-03-02T09:25:00Z",
        "plannedMs": 1500000, "activeMs": 1500000, "outcome": "completed", "endReason": "completed"
    });
    let back: SessionRecord = serde_json::from_value(legacy).unwrap();
    assert_eq!(back.ended_utc_offset_minutes, None);
}
