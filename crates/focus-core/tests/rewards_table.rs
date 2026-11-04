//! Friendship rules and the unlock table (SPEC §5), using the real data/unlocks.json.

use std::collections::BTreeSet;

use chrono::{TimeZone, Utc};
use focus_core::clock::Wall;
use focus_core::rewards::{
    is_valid_unlock_id, minute_points, AwardRecord, Friendship, UnlockKind, UnlockTable,
    BUILTIN_UNLOCKS_JSON,
};

const MIN: u64 = 60_000;

fn t0() -> Wall {
    Utc.with_ymd_and_hms(2026, 3, 2, 9, 0, 0).unwrap()
}

fn table() -> UnlockTable {
    UnlockTable::builtin()
}

#[test]
fn builtin_table_is_valid_and_matches_the_brief() {
    let t = table();
    assert_eq!(t.format, 1);
    assert_eq!(t.rules.points_per_active_minute, 1);
    assert_eq!(t.rules.completion_bonus, 5);
    assert_eq!(t.rules.long_session_minutes, 45);
    assert_eq!(t.unlocks[0].id, "base");
    assert_eq!(t.unlocks[0].points, 0);
    let mut ids = BTreeSet::new();
    let mut prev = 0;
    for u in &t.unlocks {
        assert!(is_valid_unlock_id(&u.id), "{}", u.id);
        assert!(ids.insert(u.id.clone()), "duplicate {}", u.id);
        assert!(u.points >= prev, "{} decreases", u.id);
        prev = u.points;
    }
    // The brief's thresholds.
    for (id, pts) in [
        ("base", 0),
        ("wave", 25),
        ("stretch", 60),
        ("umbrella", 100),
        ("evening", 160),
        ("sleepy", 240),
        ("celebrate", 350),
        ("acorn_gift", 500),
        ("autumn", 700),
        ("long_session", 1000),
    ] {
        let u = t.get(id).unwrap_or_else(|| panic!("{id} missing"));
        assert_eq!(u.points, pts, "{id}");
    }
    assert_eq!(t.get("evening").unwrap().kind, UnlockKind::Palette);
    assert_eq!(t.get("wave").unwrap().kind, UnlockKind::Animation);
    assert_eq!(t.get("umbrella").unwrap().kind, UnlockKind::IdleVariant);
    assert!(t.is_kind("autumn", UnlockKind::Palette));
    assert!(!t.is_kind("wave", UnlockKind::Palette));
    assert!(!t.is_kind("nope", UnlockKind::Palette));
    // Roughly 1.45× growth beyond the brief's table: always a next thing.
    assert!(t.unlocks.len() > 10);
}

#[test]
fn builtin_parse_equals_builtin() {
    assert_eq!(UnlockTable::parse(BUILTIN_UNLOCKS_JSON).unwrap(), table());
}

fn json_with(unlocks: &str, rules: &str, format: u32) -> String {
    format!(r#"{{"format": {format}, "rules": {rules}, "unlocks": [{unlocks}]}}"#)
}

const RULES: &str =
    r#"{"pointsPerActiveMinute": 1, "completionBonus": 5, "longSessionMinutes": 45}"#;

fn entry(id: &str, points: u64) -> String {
    format!(
        r#"{{"id": "{id}", "points": {points}, "kind": "animation", "trigger": "manual", "label": "L", "description": "D"}}"#
    )
}

#[test]
fn parse_accepts_a_minimal_table_and_ignores_unknown_fields() {
    let json = format!(
        r#"{{"format": 1, "extra": true, "rules": {RULES}, "unlocks": [{}, {}]}}"#,
        entry("base", 0),
        entry("a_1", 0)
    );
    let t = UnlockTable::parse(&json).unwrap();
    assert_eq!(t.unlocks.len(), 2);
}

#[test]
fn parse_accepts_palette_objects_on_palette_entries() {
    let json = json_with(
        &format!(
            r##"{}, {{"id": "evening", "points": 160, "kind": "palette", "trigger": "afterSunset", "label": "Evening", "description": "D",
                 "palette": {{"background": "#223344", "accent": "#ffcc00", "characterFilter": "brightness(0.9)"}}}}"##,
            entry("base", 0)
        ),
        RULES,
        1,
    );
    let t = UnlockTable::parse(&json).unwrap();
    let p = t.get("evening").unwrap().palette.as_ref().unwrap();
    assert_eq!(p.background.as_deref(), Some("#223344"));
    assert_eq!(p.accent.as_deref(), Some("#ffcc00"));
    assert_eq!(p.character_filter.as_deref(), Some("brightness(0.9)"));
    assert!(t.get("base").unwrap().palette.is_none());
}

#[test]
fn parse_rejects_invalid_tables() {
    let ok = json_with(
        &format!("{}, {}", entry("base", 0), entry("wave", 25)),
        RULES,
        1,
    );
    assert!(UnlockTable::parse(&ok).is_ok());

    let cases = [
        ("format", json_with(&entry("base", 0), RULES, 2)),
        (
            "duplicate",
            json_with(
                &format!("{}, {}", entry("base", 0), entry("base", 5)),
                RULES,
                1,
            ),
        ),
        (
            "decreasing",
            json_with(
                &format!(
                    "{}, {}, {}",
                    entry("base", 0),
                    entry("a", 25),
                    entry("b", 24)
                ),
                RULES,
                1,
            ),
        ),
        ("first not zero", json_with(&entry("base", 1), RULES, 1)),
        ("empty", json_with("", RULES, 1)),
        (
            "uppercase id",
            json_with(
                &format!("{}, {}", entry("base", 0), entry("Wave", 25)),
                RULES,
                1,
            ),
        ),
        (
            "hyphen id",
            json_with(
                &format!("{}, {}", entry("base", 0), entry("a-b", 25)),
                RULES,
                1,
            ),
        ),
        (
            "leading underscore",
            json_with(
                &format!("{}, {}", entry("base", 0), entry("_a", 25)),
                RULES,
                1,
            ),
        ),
        (
            "long id",
            json_with(
                &format!("{}, {}", entry("base", 0), entry(&"a".repeat(65), 25)),
                RULES,
                1,
            ),
        ),
        (
            "per-minute too large",
            json_with(
                &entry("base", 0),
                r#"{"pointsPerActiveMinute": 1001, "completionBonus": 5, "longSessionMinutes": 45}"#,
                1,
            ),
        ),
        (
            "bonus too large",
            json_with(
                &entry("base", 0),
                r#"{"pointsPerActiveMinute": 1, "completionBonus": 1001, "longSessionMinutes": 45}"#,
                1,
            ),
        ),
        (
            "bad kind",
            json_with(
                r#"{"id": "base", "points": 0, "kind": "sticker", "trigger": "x", "label": "L", "description": "D"}"#,
                RULES,
                1,
            ),
        ),
        ("not json", "{".to_string()),
    ];
    for (name, json) in cases {
        assert!(
            UnlockTable::parse(&json).is_err(),
            "{name} should be rejected"
        );
    }
    // Boundaries that are fine: 64-char id, 1000 points per minute.
    let json = json_with(
        &format!("{}, {}", entry("base", 0), entry(&"a".repeat(64), 25)),
        r#"{"pointsPerActiveMinute": 1000, "completionBonus": 1000, "longSessionMinutes": 45}"#,
        1,
    );
    assert!(UnlockTable::parse(&json).is_ok());
}

#[test]
fn ids_at_and_next_after_follow_table_order() {
    let t = table();
    assert_eq!(t.ids_at(0), vec!["base"]);
    assert_eq!(t.ids_at(24), vec!["base"]);
    assert_eq!(t.ids_at(25), vec!["base", "wave"]);
    assert_eq!(t.ids_at(100), vec!["base", "wave", "stretch", "umbrella"]);
    assert_eq!(t.next_after(0).unwrap().id, "wave");
    assert_eq!(t.next_after(25).unwrap().id, "stretch");
    assert_eq!(t.next_after(24).unwrap().id, "wave");
    let last = t.unlocks.last().unwrap();
    assert!(t.next_after(last.points).is_none());
    assert_eq!(t.ids_at(u64::MAX).len(), t.unlocks.len());
}

#[test]
fn minute_points_floor() {
    let r = &table().rules;
    assert_eq!(minute_points(0, r), 0);
    assert_eq!(minute_points(59_999, r), 0);
    assert_eq!(minute_points(60_000, r), 1);
    assert_eq!(minute_points(1_350_000, r), 22);
    assert_eq!(minute_points(22 * MIN - 1, r), 21);
    assert_eq!(minute_points(25 * MIN, r), 25);
    assert_eq!(minute_points(180 * MIN, r), 180);
    assert_eq!(minute_points(u64::MAX, r), u64::MAX / 60_000);
}

#[test]
fn award_completed_25_is_30_and_unlocks_wave_once() {
    let t = table();
    let mut f = Friendship::default();
    f.reconcile(&t);
    assert!(f.unlocked.contains("base"));
    let out = f.award("x", 25 * MIN, true, &t, t0()).unwrap();
    assert_eq!(out.session_id, "x");
    assert_eq!(out.points, 25);
    assert_eq!(out.bonus, 5);
    assert_eq!(out.total_points, 30);
    assert_eq!(out.newly_unlocked, vec!["wave"]);
    assert_eq!(f.total_points, 30);
    assert_eq!(
        f.awards["x"],
        AwardRecord {
            points: 25,
            bonus: 5,
            completed: true,
            awarded_at: t0()
        }
    );
    // Repeating adds nothing, even with different arguments.
    assert!(f.award("x", 25 * MIN, true, &t, t0()).is_none());
    assert!(f.award("x", 180 * MIN, false, &t, t0()).is_none());
    assert_eq!(f.total_points, 30);
    assert_eq!(f.awards.len(), 1);
}

#[test]
fn award_interrupted_has_no_bonus() {
    let t = table();
    let mut f = Friendship::default();
    let out = f.award("y", 22 * MIN + 30_000, false, &t, t0()).unwrap();
    assert_eq!((out.points, out.bonus, out.total_points), (22, 0, 22));
    assert!(out.newly_unlocked.contains(&"base".to_string()));
    assert!(!f.awards["y"].completed);
    let zero = f.award("z", 30_000, false, &t, t0()).unwrap();
    assert_eq!((zero.points, zero.bonus, zero.total_points), (0, 0, 22));
    assert!(
        f.awards.contains_key("z"),
        "a zero-point award is still recorded"
    );
}

#[test]
fn cumulative_unlocks() {
    let t = table();
    let mut f = Friendship::default();
    f.reconcile(&t);
    assert_eq!(
        f.award("a", 25 * MIN, true, &t, t0())
            .unwrap()
            .newly_unlocked,
        vec!["wave"]
    );
    assert!(f
        .award("b", 22 * MIN, false, &t, t0())
        .unwrap()
        .newly_unlocked
        .is_empty());
    assert_eq!(f.total_points, 52);
    assert_eq!(
        f.award("c", 25 * MIN, true, &t, t0())
            .unwrap()
            .newly_unlocked,
        vec!["stretch"]
    );
    assert_eq!(f.total_points, 82);
    // One big jump unlocks several, in table order.
    let out = f.award("d", 180 * MIN, true, &t, t0()).unwrap();
    assert_eq!(out.total_points, 267);
    assert_eq!(out.newly_unlocked, vec!["umbrella", "evening", "sleepy"]);
}

#[test]
fn reconcile_never_decreases() {
    let t = table();
    let mut f = Friendship {
        total_points: 100,
        unlocked: ["retired_id".to_string()].into_iter().collect(),
        ..Friendship::default()
    };
    f.award("a", 25 * MIN, true, &t, t0());
    // Simulate stored state where the total was higher than the ledger (kept).
    f.total_points = 100;
    let added = f.reconcile(&t);
    assert_eq!(f.total_points, 100);
    assert!(
        added.is_empty(),
        "award already unioned up to 130; got {added:?}"
    );
    assert!(f.unlocked.contains("retired_id"), "unknown ids are kept");
    assert!(f.unlocked.contains("umbrella"));

    // A ledger larger than the stored total raises the total.
    let mut g = Friendship::default();
    g.awards.insert(
        "x".into(),
        AwardRecord {
            points: 340,
            bonus: 10,
            completed: true,
            awarded_at: t0(),
        },
    );
    let added = g.reconcile(&t);
    assert_eq!(g.total_points, 350);
    assert_eq!(
        added,
        vec![
            "base",
            "wave",
            "stretch",
            "umbrella",
            "evening",
            "sleepy",
            "celebrate"
        ]
    );
    assert!(g.reconcile(&t).is_empty(), "reconcile is idempotent");
}

#[test]
fn reconcile_with_changed_thresholds() {
    // A raised threshold never removes an unlock; a lowered one adds it.
    let mut f = Friendship {
        total_points: 30,
        unlocked: ["base".to_string(), "wave".to_string()]
            .into_iter()
            .collect(),
        ..Friendship::default()
    };
    let raised = UnlockTable::parse(&json_with(
        &format!("{}, {}", entry("base", 0), entry("wave", 500)),
        RULES,
        1,
    ))
    .unwrap();
    assert!(f.reconcile(&raised).is_empty());
    assert!(f.unlocked.contains("wave"));
    let lowered = UnlockTable::parse(&json_with(
        &format!(
            "{}, {}, {}",
            entry("base", 0),
            entry("wave", 25),
            entry("stretch", 30)
        ),
        RULES,
        1,
    ))
    .unwrap();
    assert_eq!(f.reconcile(&lowered), vec!["stretch"]);
}

#[test]
fn award_saturates_instead_of_overflowing() {
    let t = table();
    let mut f = Friendship {
        total_points: u64::MAX - 1,
        ..Friendship::default()
    };
    let out = f.award("x", 25 * MIN, true, &t, t0()).unwrap();
    assert_eq!(out.total_points, u64::MAX);
    assert_eq!(f.awarded_sum(), 30);
}

#[test]
fn friendship_json_shape() {
    let t = table();
    let mut f = Friendship::default();
    f.award("x", 25 * MIN, true, &t, t0());
    let v = serde_json::to_value(&f).unwrap();
    assert_eq!(v["totalPoints"], 30);
    assert!(v["unlocked"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x == "wave"));
    assert_eq!(v["awards"]["x"]["points"], 25);
    assert_eq!(v["awards"]["x"]["bonus"], 5);
    assert_eq!(v["awards"]["x"]["completed"], true);
    assert!(v["awards"]["x"]["awardedAt"].is_string());
    let back: Friendship = serde_json::from_value(v).unwrap();
    assert_eq!(back, f);
}
