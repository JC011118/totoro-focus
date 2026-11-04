//! The view model sent to the renderer (`focus://snapshot`) and used by the tray.

use serde::{Deserialize, Serialize};

use crate::clock::Wall;
use crate::history::{SessionRecord, TodaySummary};
use crate::rewards::{AwardOutcome, Rules, UnlockDef};
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
    /// `format_countdown(remaining_ms)`; the tray title and the widget both show this.
    pub remaining_label: String,
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
    /// Strictly increasing per emitted/returned snapshot (stamped by the shell under the
    /// engine lock). The renderer applies a snapshot iff `seq > lastSeq`.
    pub seq: u64,
    /// Bumped when observable state changes (not on pure time passage). SPEC §4.9.
    pub revision: u64,
    pub phase: Phase,
    pub session: Option<SessionView>,
    pub awaiting: Option<AwaitingView>,
    pub today: TodaySummary,
    pub friendship: FriendshipView,
    pub settings: Settings,
    /// Whether the companion window is currently shown (stamped by the shell).
    pub companion_visible: bool,
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
    /// Why the saved hotkey could not be registered, if it couldn't.
    pub hotkey_error: Option<String>,
    pub limits: Limits,
    pub rules: Rules,
    /// The full unlock table, in order (static; not repeated in every snapshot).
    pub unlocks: Vec<UnlockDef>,
}

/// `mm:ss` countdown, ceil to the second, no hour wrap (SPEC §3).
pub fn format_countdown(remaining_ms: u64) -> String {
    let s = remaining_ms.div_ceil(1000);
    format!("{:02}:{:02}", s / 60, s % 60)
}

/// Whole minutes as "1 h 05 min" / "25 min" / "0 min" (SPEC §6).
pub fn format_focus_label(ms: u64) -> String {
    let minutes = ms / 60_000;
    let (h, m) = (minutes / 60, minutes % 60);
    if h > 0 {
        format!("{h} h {m:02} min")
    } else {
        format!("{m} min")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdown_format_matches_spec_examples() {
        for (ms, want) in [
            (1_500_000, "25:00"),
            (1_499_001, "25:00"),
            (1_499_000, "24:59"),
            (999, "00:01"),
            (0, "00:00"),
            (300_000, "05:00"),
            (10_800_000, "180:00"),
        ] {
            assert_eq!(format_countdown(ms), want, "{ms}");
        }
    }

    #[test]
    fn focus_label_format() {
        assert_eq!(format_focus_label(0), "0 min");
        assert_eq!(format_focus_label(59_999), "0 min");
        assert_eq!(format_focus_label(25 * 60_000), "25 min");
        assert_eq!(format_focus_label(65 * 60_000 + 59_000), "1 h 05 min");
        assert_eq!(format_focus_label(600 * 60_000), "10 h 00 min");
    }
}
