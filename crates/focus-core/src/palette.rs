//! Palette overrides shared by unlock defaults (`data/unlocks.json`) and packs.

use serde::{Deserialize, Serialize};

/// Colors must be `#rgb`, `#rrggbb`, `#rrggbbaa`, or `rgb()/rgba()/hsl()/hsla()`
/// with numeric arguments. `characterFilter` may only use
/// `brightness|contrast|saturate|sepia|hue-rotate|grayscale` with numeric
/// (`deg`/`%`/plain) arguments. Invalid values are dropped with a warning.
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PaletteSpec {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub background: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub surface: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub muted: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub accent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub ring: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub ring_track: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub character_filter: Option<String>,
}

/// True if `v` is an allowed CSS color (see `PaletteSpec`).
pub fn is_safe_color(v: &str) -> bool {
    let _ = v;
    todo!("palette: is_safe_color")
}

/// True if `v` is an allowed CSS filter list (see `PaletteSpec`).
pub fn is_safe_filter(v: &str) -> bool {
    let _ = v;
    todo!("palette: is_safe_filter")
}

/// Drop unsafe values; returns the cleaned palette and one warning per dropped field.
pub fn sanitize_palette(p: &PaletteSpec) -> (PaletteSpec, Vec<String>) {
    let _ = p;
    todo!("palette: sanitize_palette")
}
