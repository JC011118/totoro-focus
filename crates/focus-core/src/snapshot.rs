//! The view model sent to the renderer (`focus://snapshot`) and used by the tray.

use serde::{Deserialize, Serialize};

use crate::clock::Wall;
use crate::history::{SessionRecord, TodaySummary};
use crate::rewards::{AwardOutcome, UnlockDef};
use crate::settings::Settings;
use crate::timer::{PauseReason, RunStatus, SessionKind};

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Idle,
    Focus,
    Break,
    AwaitingChoice,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub id: String,
    pub kind: SessionKind,
    pub status: RunStatus,
    pub pause_reason: Option<PauseReason>,
    pub task_label: Option<String>,
    pub planned_ms: u64,
    pub active_ms: u64,
    pub remaining_ms: u64,
    pub started_at: Wall,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AwaitingView {
    /// What just finished: a focus session (offer break / another / dismiss)
    /// or a break (offer the focus presets again).
    pub after: SessionKind,
    /// The focus record that just completed (None after a break).
    pub record: Option<SessionRecord>,
    pub award: Option<AwardOutcome>,
    pub long_session: bool,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NextUnlock {
    pub id: String,
    pub label: String,
    pub points: u64,
    /// Points still needed.
    pub remaining: u64,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FriendshipView {
    pub total_points: u64,
    /// Unlocked ids in table order (unknown ids from older tables are appended).
    pub unlocked: Vec<String>,
    pub next: Option<NextUnlock>,
    pub unlocks: Vec<UnlockDef>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    pub presets: Vec<u32>,
    pub min_minutes: u32,
    pub max_minutes: u32,
    pub break_minutes: u32,
    pub max_task_label_chars: u32,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// Increments on every engine change; the renderer may ignore older revisions.
    pub revision: u64,
    pub phase: Phase,
    pub session: Option<SessionView>,
    pub awaiting: Option<AwaitingView>,
    pub today: TodaySummary,
    pub friendship: FriendshipView,
    pub settings: Settings,
    pub limits: Limits,
}

/// Static facts about the running app (`get_app_info`).
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// Display name from configuration (never hard-coded in product logic).
    pub name: String,
    pub version: String,
    /// Where user packs are discovered (`<appData>/packs`).
    pub user_packs_dir: String,
    pub data_dir: String,
    /// `panel` (non-activating NSPanel), `native` (plain window), or `other` (non-macOS).
    pub window_mode: String,
    pub debug: bool,
}
