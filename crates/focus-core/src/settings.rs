//! User settings and validated partial updates.

use serde::{Deserialize, Deserializer, Serialize};

/// Default global hotkey (⌃⌥T), in the global-shortcut plugin's accelerator syntax.
pub const DEFAULT_HOTKEY: &str = "Ctrl+Alt+T";
pub const DEFAULT_PACK: &str = "placeholder";
pub const DEFAULT_FOCUS_MINUTES: u32 = 25;

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WidgetPosition {
    /// Logical (point) coordinates of the companion's top-left corner.
    pub x: f64,
    pub y: f64,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub last_focus_minutes: u32,
    pub sound: bool,
    pub reduced_motion: bool,
    pub hide_during_focus: bool,
    pub menu_bar_title: bool,
    /// `None` = hotkey disabled.
    pub hotkey: Option<String>,
    pub active_pack: String,
    /// `"auto"` or a palette id (an unlocked palette, or `"forest"`).
    pub palette: String,
    pub widget_position: Option<WidgetPosition>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            last_focus_minutes: DEFAULT_FOCUS_MINUTES,
            sound: true,
            reduced_motion: false,
            hide_during_focus: false,
            menu_bar_title: true,
            hotkey: Some(DEFAULT_HOTKEY.to_string()),
            active_pack: DEFAULT_PACK.to_string(),
            palette: "auto".to_string(),
            widget_position: None,
        }
    }
}

/// Distinguishes "field absent" (`None`) from "set to null" (`Some(None)`).
fn double_option<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::<T>::deserialize(de).map(Some)
}

/// The settings a renderer may change directly (`update_settings`). Hotkey, pack and
/// position have dedicated paths (`set_hotkey`, `set_active_pack`, window moves).
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserSettingsPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub last_focus_minutes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub sound: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub reduced_motion: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub hide_during_focus: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub menu_bar_title: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub palette: Option<String>,
}

impl From<UserSettingsPatch> for SettingsPatch {
    fn from(p: UserSettingsPatch) -> Self {
        SettingsPatch {
            last_focus_minutes: p.last_focus_minutes,
            sound: p.sound,
            reduced_motion: p.reduced_motion,
            hide_during_focus: p.hide_during_focus,
            menu_bar_title: p.menu_bar_title,
            palette: p.palette,
            ..SettingsPatch::default()
        }
    }
}

/// Full partial update, internal to the shell. Absent fields are left unchanged.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_focus_minutes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduced_motion: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hide_during_focus: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub menu_bar_title: Option<bool>,
    /// `Some(None)` disables the hotkey. Set via the `set_hotkey` command, which validates it.
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub hotkey: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_pack: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette: Option<String>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub widget_position: Option<Option<WidgetPosition>>,
}

impl Settings {
    /// Validate the whole patch first, then apply it atomically (all or nothing).
    /// Rules: minutes within `min..=max`; pack id and palette match
    /// `^[a-z0-9][a-z0-9_-]{0,63}$` (palette may be "auto"); hotkey non-empty and
    /// ≤ 64 chars when set; widget position finite. Returns a human-readable error.
    /// (Whether the palette is unlocked is checked by the engine, which knows friendship.)
    pub fn apply(&mut self, patch: &SettingsPatch, min_minutes: u32, max_minutes: u32) -> Result<(), String> {
        let _ = (patch, min_minutes, max_minutes);
        todo!("settings: apply")
    }

    /// Repair a loaded value (e.g. out-of-range minutes, empty pack id) to defaults per field.
    pub fn sanitized(self, min_minutes: u32, max_minutes: u32) -> Self {
        let _ = (min_minutes, max_minutes);
        todo!("settings: sanitized")
    }
}
