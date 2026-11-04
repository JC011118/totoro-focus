//! Friendship points and the data-driven unlock table (`data/unlocks.json`).
//!
//! Points: one per whole active minute of a focus session (completed or
//! interrupted) plus a completion bonus. Awards are keyed by session id and
//! idempotent. Totals and unlocks never decrease. See docs/SPEC.md §5.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::clock::Wall;
use crate::palette::PaletteSpec;

/// The unlock manifest shipped with the app.
pub const BUILTIN_UNLOCKS_JSON: &str = include_str!("../../../data/unlocks.json");

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Rules {
    pub points_per_active_minute: u64,
    pub completion_bonus: u64,
    pub long_session_minutes: u32,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum UnlockKind {
    States,
    Animation,
    IdleVariant,
    Palette,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UnlockDef {
    pub id: String,
    pub points: u64,
    pub kind: UnlockKind,
    /// Presentation hint for the frontend (e.g. `focusStart`, `afterSunset`).
    pub trigger: String,
    pub label: String,
    pub description: String,
    /// Default colors for `kind: palette` unlocks (packs may override per field).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub palette: Option<PaletteSpec>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UnlockTable {
    pub format: u32,
    pub rules: Rules,
    pub unlocks: Vec<UnlockDef>,
}

impl UnlockTable {
    /// The embedded table. Panics only if the shipped JSON is invalid (covered by tests).
    pub fn builtin() -> Self {
        Self::parse(BUILTIN_UNLOCKS_JSON).expect("data/unlocks.json must be valid")
    }

    /// Parse and validate: format 1; ids unique and `^[a-z0-9][a-z0-9_]{0,63}$`;
    /// thresholds non-decreasing in list order; first entry has 0 points;
    /// completion bonus and per-minute points are sane (≤ 1000).
    pub fn parse(json: &str) -> Result<Self, String> {
        let _ = json;
        todo!("rewards: UnlockTable::parse")
    }

    pub fn get(&self, id: &str) -> Option<&UnlockDef> {
        self.unlocks.iter().find(|u| u.id == id)
    }

    /// Ids whose threshold is `<= points`.
    pub fn ids_at(&self, points: u64) -> Vec<String> {
        let _ = points;
        todo!("rewards: ids_at")
    }

    /// The first unlock with a threshold above `points`.
    pub fn next_after(&self, points: u64) -> Option<&UnlockDef> {
        let _ = points;
        todo!("rewards: next_after")
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AwardRecord {
    /// Points for whole active minutes.
    pub points: u64,
    /// Completion bonus (0 for interrupted sessions).
    pub bonus: u64,
    pub completed: bool,
    pub awarded_at: Wall,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AwardOutcome {
    pub session_id: String,
    pub points: u64,
    pub bonus: u64,
    pub total_points: u64,
    pub newly_unlocked: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Friendship {
    pub total_points: u64,
    pub unlocked: BTreeSet<String>,
    /// Award records keyed by session id: the idempotency ledger.
    pub awards: BTreeMap<String, AwardRecord>,
}

/// Whole active minutes times the per-minute rate.
pub fn minute_points(active_ms: u64, rules: &Rules) -> u64 {
    let _ = (active_ms, rules);
    todo!("rewards: minute_points")
}

impl Friendship {
    /// Award a focus session once. Returns `None` if `session_id` was already
    /// awarded (nothing changes). Otherwise records the award, adds points and
    /// bonus (bonus only when `completed`), unions newly reached unlocks, and
    /// returns what changed.
    pub fn award(
        &mut self,
        session_id: &str,
        active_ms: u64,
        completed: bool,
        table: &UnlockTable,
        at: Wall,
    ) -> Option<AwardOutcome> {
        let _ = (session_id, active_ms, completed, table, at);
        todo!("rewards: award")
    }

    /// Repair invariants after load: `total_points = max(stored, sum of awards)`;
    /// `unlocked ∪= thresholds <= total`. Never removes anything. Returns ids added.
    pub fn reconcile(&mut self, table: &UnlockTable) -> Vec<String> {
        let _ = table;
        todo!("rewards: reconcile")
    }
}
