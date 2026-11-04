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
    pub fn apply(
        &mut self,
        patch: &SettingsPatch,
        min_minutes: u32,
        max_minutes: u32,
    ) -> Result<(), String> {
        validate_patch(patch, min_minutes, max_minutes)?;
        if let Some(m) = patch.last_focus_minutes {
            self.last_focus_minutes = m;
        }
        if let Some(v) = patch.sound {
            self.sound = v;
        }
        if let Some(v) = patch.reduced_motion {
            self.reduced_motion = v;
        }
        if let Some(v) = patch.hide_during_focus {
            self.hide_during_focus = v;
        }
        if let Some(v) = patch.menu_bar_title {
            self.menu_bar_title = v;
        }
        if let Some(h) = &patch.hotkey {
            self.hotkey = h.clone();
        }
        if let Some(p) = &patch.active_pack {
            self.active_pack = p.clone();
        }
        if let Some(p) = &patch.palette {
            self.palette = p.clone();
        }
        if let Some(w) = patch.widget_position {
            self.widget_position = w;
        }
        Ok(())
    }

    /// Repair a loaded value (e.g. out-of-range minutes, empty pack id) to defaults per field.
    pub fn sanitized(self, min_minutes: u32, max_minutes: u32) -> Self {
        let defaults = Settings::default();
        let mut s = self;
        if !(min_minutes..=max_minutes).contains(&s.last_focus_minutes) {
            s.last_focus_minutes = clamp_minutes(DEFAULT_FOCUS_MINUTES, min_minutes, max_minutes);
        }
        if !is_valid_id(&s.active_pack) {
            s.active_pack = defaults.active_pack;
        }
        if !is_valid_palette_id(&s.palette) {
            s.palette = defaults.palette;
        }
        if let Some(h) = &s.hotkey {
            if !is_valid_hotkey_text(h) {
                s.hotkey = defaults.hotkey;
            }
        }
        if let Some(p) = s.widget_position {
            if !(p.x.is_finite() && p.y.is_finite()) {
                s.widget_position = None;
            }
        }
        s
    }
}

/// Maximum length (Unicode scalar values) of a stored hotkey accelerator.
pub const MAX_HOTKEY_CHARS: usize = 64;

/// Pack and palette ids: `^[a-z0-9][a-z0-9_-]{0,63}$`.
pub fn is_valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_' || *b == b'-')
}

/// `"auto"` or a syntactically valid id. Whether it is unlocked is the engine's check.
pub fn is_valid_palette_id(id: &str) -> bool {
    id == "auto" || is_valid_id(id)
}

/// Non-empty (after trimming) and at most [`MAX_HOTKEY_CHARS`]. Whether it parses and
/// registers is the shell's check (`set_hotkey`).
pub fn is_valid_hotkey_text(h: &str) -> bool {
    !h.trim().is_empty() && h.chars().count() <= MAX_HOTKEY_CHARS
}

fn clamp_minutes(m: u32, min_minutes: u32, max_minutes: u32) -> u32 {
    m.max(min_minutes).min(max_minutes.max(min_minutes))
}

/// Validate every present field of `patch` (no partial application on error).
pub fn validate_patch(
    patch: &SettingsPatch,
    min_minutes: u32,
    max_minutes: u32,
) -> Result<(), String> {
    if let Some(m) = patch.last_focus_minutes {
        if !(min_minutes..=max_minutes).contains(&m) {
            return Err(format!(
                "last focus duration must be between {min_minutes} and {max_minutes} minutes, not {m}"
            ));
        }
    }
    if let Some(Some(h)) = &patch.hotkey {
        if !is_valid_hotkey_text(h) {
            return Err(format!(
                "hotkey must be non-empty and at most {MAX_HOTKEY_CHARS} characters"
            ));
        }
    }
    if let Some(p) = &patch.active_pack {
        if !is_valid_id(p) {
            return Err(format!("invalid pack id {p:?}"));
        }
    }
    if let Some(p) = &patch.palette {
        if !is_valid_palette_id(p) {
            return Err(format!("invalid palette id {p:?}"));
        }
    }
    if let Some(Some(w)) = &patch.widget_position {
        if !(w.x.is_finite() && w.y.is_finite()) {
            return Err("widget position must be finite".to_string());
        }
    }
    Ok(())
}
