//! Session history and the derived "today" summary (harvest row, focus time).
//! See docs/SPEC.md §6.

use chrono::{FixedOffset, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::clock::Wall;
use crate::timer::SessionKind;

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Completed,
    Interrupted,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum EndReason {
    Completed,
    UserEnded,
    PauseExpired,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub id: String,
    pub kind: SessionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub task_label: Option<String>,
    pub started_at: Wall,
    pub ended_at: Wall,
    pub planned_ms: u64,
    pub active_ms: u64,
    pub outcome: Outcome,
    pub end_reason: EndReason,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum HarvestItem {
    Acorn,
    Leaf,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TodaySummary {
    /// Local calendar date, `YYYY-MM-DD`.
    pub date: String,
    /// Active time of today's recorded focus sessions.
    pub focus_ms: u64,
    /// Active time of the current (running or paused) focus session, if any.
    pub active_focus_ms: u64,
    pub completed: u32,
    pub interrupted: u32,
    /// One item per recorded focus session today, in end order (uncapped; the UI caps at 12).
    pub harvest: Vec<HarvestItem>,
}

/// Local calendar date of `at` under `offset`.
pub fn local_date(at: Wall, offset: FixedOffset) -> NaiveDate {
    at.with_timezone(&offset).date_naive()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {
    records: Vec<SessionRecord>,
}

impl History {
    pub fn new(records: Vec<SessionRecord>) -> Self {
        Self { records }
    }

    pub fn records(&self) -> &[SessionRecord] {
        &self.records
    }

    pub fn into_records(self) -> Vec<SessionRecord> {
        self.records
    }

    pub fn contains(&self, id: &str) -> bool {
        self.records.iter().any(|r| r.id == id)
    }

    /// Append unless a record with the same id exists. Returns whether it was appended.
    pub fn push_once(&mut self, record: SessionRecord) -> bool {
        let _ = record;
        todo!("history: push_once")
    }

    /// Newest first, at most `limit`.
    pub fn recent(&self, limit: usize) -> Vec<SessionRecord> {
        let _ = limit;
        todo!("history: recent")
    }

    /// Summary for the local day containing `now`. Each record's day is the
    /// local date of its `ended_at`, using `offset_at(ended_at)`; today's date
    /// uses `offset_at(now)`. `active_focus_ms` is supplied by the caller.
    pub fn today(
        &self,
        now: Wall,
        offset_at: &dyn Fn(Wall) -> FixedOffset,
        active_focus_ms: u64,
    ) -> TodaySummary {
        let _ = (now, offset_at, active_focus_ms);
        todo!("history: today")
    }
}
