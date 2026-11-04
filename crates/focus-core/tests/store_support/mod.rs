//! Shared fixtures for the `store_*` integration tests. Values are built by hand
//! (plain structs only) so these tests never call other modules' logic.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use chrono::{Duration, TimeZone, Utc};
use focus_core::history::{EndReason, Outcome, SessionRecord};
use focus_core::rewards::{AwardRecord, Friendship};
use focus_core::settings::{Settings, WidgetPosition};
use focus_core::store::{PersistedState, SCHEMA_VERSION};
use focus_core::timer::{PauseReason, PersistedSession, RunStatus, SessionKind};
use focus_core::Wall;

/// 2026-03-02 09:00:00 UTC: the stamp in backup names is `20260302T090000Z`.
pub fn t0() -> Wall {
    Utc.with_ymd_and_hms(2026, 3, 2, 9, 0, 0).unwrap()
}

pub const STAMP: &str = "20260302T090000Z";

/// `t0` plus `minutes` and `ms` (millisecond precision must round-trip).
pub fn at(minutes: i64, ms: i64) -> Wall {
    t0() + Duration::minutes(minutes) + Duration::milliseconds(ms)
}

pub fn record(id: &str, outcome: Outcome, end_reason: EndReason) -> SessionRecord {
    SessionRecord {
        id: id.to_string(),
        kind: SessionKind::Focus,
        task_label: None,
        started_at: at(0, 0),
        ended_at: at(25, 0),
        planned_ms: 1_500_000,
        active_ms: 1_500_000,
        outcome,
        end_reason,
        ended_utc_offset_minutes: None,
    }
}

/// Every field set to a non-default value.
pub fn full_settings() -> Settings {
    Settings {
        last_focus_minutes: 37,
        sound: false,
        reduced_motion: true,
        hide_during_focus: true,
        menu_bar_title: false,
        hotkey: Some("Ctrl+Alt+Shift+Super+KeyF".to_string()),
        active_pack: "my_pack-2".to_string(),
        palette: "evening".to_string(),
        widget_position: Some(WidgetPosition {
            x: -1234.5,
            y: 87.25,
        }),
    }
}

pub fn paused_checkpoint() -> PersistedSession {
    PersistedSession {
        id: "s9".to_string(),
        kind: SessionKind::Focus,
        task_label: Some("Draft \"the\" report — ünïcödé 🌰".to_string()),
        planned_ms: 2_700_000,
        started_at: at(100, 123),
        accrued_ms: 600_001,
        status: RunStatus::Paused,
        checkpoint_at: at(115, 456),
        paused_at: Some(at(110, 789)),
        pause_reason: Some(PauseReason::ClockJump),
        age_anchor: Some(at(112, 5)),
    }
}

pub fn running_checkpoint() -> PersistedSession {
    PersistedSession {
        id: "s10".to_string(),
        kind: SessionKind::Break,
        task_label: None,
        planned_ms: 300_000,
        started_at: at(200, 0),
        accrued_ms: 12_345,
        status: RunStatus::Running,
        checkpoint_at: at(200, 12_345),
        paused_at: None,
        pause_reason: None,
        age_anchor: None,
    }
}

pub fn full_history() -> Vec<SessionRecord> {
    let mut completed = record("s1", Outcome::Completed, EndReason::Completed);
    completed.task_label = Some("Write tests".to_string());
    completed.ended_utc_offset_minutes = Some(-420);

    let mut user_ended = record("s2", Outcome::Interrupted, EndReason::UserEnded);
    user_ended.started_at = at(30, 0);
    user_ended.ended_at = at(52, 999);
    user_ended.active_ms = 1_320_999;
    user_ended.ended_utc_offset_minutes = Some(330);

    let mut expired = record("s3", Outcome::Interrupted, EndReason::PauseExpired);
    expired.planned_ms = 10_800_000;
    expired.active_ms = 61_000;
    expired.ended_utc_offset_minutes = Some(0);

    vec![completed, user_ended, expired]
}

pub fn full_friendship() -> Friendship {
    let mut awards = BTreeMap::new();
    awards.insert(
        "s1".to_string(),
        AwardRecord {
            points: 25,
            bonus: 5,
            completed: true,
            awarded_at: at(25, 0),
        },
    );
    awards.insert(
        "s2".to_string(),
        AwardRecord {
            points: 22,
            bonus: 0,
            completed: false,
            awarded_at: at(52, 999),
        },
    );
    awards.insert(
        "s3".to_string(),
        AwardRecord {
            points: 1,
            bonus: 0,
            completed: false,
            awarded_at: at(300, 1),
        },
    );
    Friendship {
        // Larger than the award sum (stored totals may exceed it; never reduced).
        total_points: 1_234,
        unlocked: BTreeSet::from([
            "base".to_string(),
            "wave".to_string(),
            "stretch".to_string(),
        ]),
        awards,
    }
}

/// A fully populated state: non-default settings, a paused checkpoint with every
/// optional field, all three outcomes in history, and an award ledger.
pub fn full_state() -> PersistedState {
    PersistedState {
        schema_version: SCHEMA_VERSION,
        settings: full_settings(),
        active_session: Some(paused_checkpoint()),
        history: full_history(),
        friendship: full_friendship(),
    }
}

/// A different, still valid state (to tell files apart).
pub fn other_state(points: u64) -> PersistedState {
    let mut s = full_state();
    s.friendship.total_points = points;
    s.active_session = Some(running_checkpoint());
    s
}

pub fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

pub fn to_json(state: &PersistedState) -> Vec<u8> {
    serde_json::to_vec_pretty(state).unwrap()
}
