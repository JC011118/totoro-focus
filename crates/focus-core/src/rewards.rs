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
        let table: UnlockTable = serde_json::from_str(json)
            .map_err(|e| format!("unlock table is not valid JSON: {e}"))?;
        table.validate()?;
        Ok(table)
    }

    /// The validation `parse` applies (also usable on a table built in code).
    pub fn validate(&self) -> Result<(), String> {
        if self.format != 1 {
            return Err(format!("unsupported unlock table format {}", self.format));
        }
        if self.rules.points_per_active_minute > MAX_RULE_POINTS {
            return Err(format!(
                "pointsPerActiveMinute {} exceeds {MAX_RULE_POINTS}",
                self.rules.points_per_active_minute
            ));
        }
        if self.rules.completion_bonus > MAX_RULE_POINTS {
            return Err(format!(
                "completionBonus {} exceeds {MAX_RULE_POINTS}",
                self.rules.completion_bonus
            ));
        }
        let first = self
            .unlocks
            .first()
            .ok_or_else(|| "unlock table has no entries".to_string())?;
        if first.points != 0 {
            return Err(format!(
                "the first unlock ({}) must have 0 points, not {}",
                first.id, first.points
            ));
        }
        let mut seen = BTreeSet::new();
        let mut previous = 0u64;
        for u in &self.unlocks {
            if !is_valid_unlock_id(&u.id) {
                return Err(format!("invalid unlock id {:?}", u.id));
            }
            // Default palettes reach the renderer as CSS custom properties: only
            // allow-listed color/filter values (palette::sanitize_palette) pass.
            if let Some(p) = &u.palette {
                let (_, dropped) = crate::palette::sanitize_palette(p);
                if !dropped.is_empty() {
                    return Err(format!(
                        "unlock {}: unsafe palette values: {dropped:?}",
                        u.id
                    ));
                }
            }
            if !seen.insert(u.id.as_str()) {
                return Err(format!("duplicate unlock id {:?}", u.id));
            }
            if u.points < previous {
                return Err(format!(
                    "unlock {} has {} points, below the previous threshold {previous}",
                    u.id, u.points
                ));
            }
            previous = u.points;
        }
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&UnlockDef> {
        self.unlocks.iter().find(|u| u.id == id)
    }

    /// Ids whose threshold is `<= points`.
    pub fn ids_at(&self, points: u64) -> Vec<String> {
        self.unlocks
            .iter()
            .filter(|u| u.points <= points)
            .map(|u| u.id.clone())
            .collect()
    }

    /// The first unlock with a threshold above `points`.
    pub fn next_after(&self, points: u64) -> Option<&UnlockDef> {
        self.unlocks.iter().find(|u| u.points > points)
    }

    /// True if `id` is in the table with the given kind.
    pub fn is_kind(&self, id: &str, kind: UnlockKind) -> bool {
        self.get(id).is_some_and(|u| u.kind == kind)
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
    (active_ms / 60_000).saturating_mul(rules.points_per_active_minute)
}

/// Upper bound for `pointsPerActiveMinute` and `completionBonus`.
pub const MAX_RULE_POINTS: u64 = 1000;

/// Unlock ids: `^[a-z0-9][a-z0-9_]{0,63}$`.
pub fn is_valid_unlock_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
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
        if self.awards.contains_key(session_id) {
            return None;
        }
        let points = minute_points(active_ms, &table.rules);
        let bonus = if completed {
            table.rules.completion_bonus
        } else {
            0
        };
        self.awards.insert(
            session_id.to_string(),
            AwardRecord {
                points,
                bonus,
                completed,
                awarded_at: at,
            },
        );
        self.total_points = self
            .total_points
            .saturating_add(points)
            .saturating_add(bonus);
        let newly_unlocked = self.union_reached(table);
        Some(AwardOutcome {
            session_id: session_id.to_string(),
            points,
            bonus,
            total_points: self.total_points,
            newly_unlocked,
        })
    }

    /// Sum of every award record (points plus bonus).
    pub fn awarded_sum(&self) -> u64 {
        self.awards.values().fold(0u64, |acc, a| {
            acc.saturating_add(a.points).saturating_add(a.bonus)
        })
    }

    /// Add every id whose threshold is reached; returns the added ids in table order.
    fn union_reached(&mut self, table: &UnlockTable) -> Vec<String> {
        let mut added = Vec::new();
        for id in table.ids_at(self.total_points) {
            if self.unlocked.insert(id.clone()) {
                added.push(id);
            }
        }
        added
    }

    /// Repair invariants after load: `total_points = max(stored, sum of awards)`;
    /// `unlocked ∪= thresholds <= total`. Never removes anything. Returns ids added.
    pub fn reconcile(&mut self, table: &UnlockTable) -> Vec<String> {
        self.total_points = self.total_points.max(self.awarded_sum());
        self.union_reached(table)
    }
}
