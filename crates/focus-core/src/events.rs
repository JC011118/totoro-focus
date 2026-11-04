//! Event names and payloads exchanged with the renderer (`src/ipc/events.ts` mirrors these).

use serde::{Deserialize, Serialize};

pub const SNAPSHOT: &str = "focus://snapshot";
pub const TRANSITION: &str = "focus://transition";
pub const PACK_CHANGED: &str = "focus://pack-changed";
pub const PLAY: &str = "focus://play";
pub const OPEN_TAB: &str = "focus://open-tab";
pub const OPEN_PICKER: &str = "focus://open-picker";

/// Window labels (must match `src-tauri/capabilities/*.json`).
pub const COMPANION_WINDOW: &str = "companion";
pub const SETTINGS_WINDOW: &str = "settings";

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlayEvent {
    pub key: String,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "camelCase")]
pub enum SettingsTab {
    #[default]
    General,
    Start,
    History,
    Companion,
}

impl SettingsTab {
    pub fn as_str(self) -> &'static str {
        match self {
            SettingsTab::General => "general",
            SettingsTab::Start => "start",
            SettingsTab::History => "history",
            SettingsTab::Companion => "companion",
        }
    }
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenTabEvent {
    pub tab: SettingsTab,
}
