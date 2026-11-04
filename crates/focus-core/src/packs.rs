//! Character packs: manifest format, validation, animation resolution with
//! fallbacks, discovery across sources, and safe file access for the
//! `pack://` protocol. See docs/SPEC.md §7.
//!
//! Layout: `manifest` (parse, validate, resolve, render keys), `paths` (path
//! safety, MIME types, URL encoding), `petdex` (Petdex/Codex pet conversion,
//! SPEC §7.2) and `registry` (discovery and file serving). The public items
//! below are re-exported here and are the stable API.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use crate::palette::PaletteSpec;

mod manifest;
mod paths;
pub mod petdex;
mod registry;

pub use manifest::{is_valid_pack_id, parse_manifest, render_keys, resolve, validate_manifest};
pub use paths::{encode_url_path, is_sheet_type, mime_for, safe_relative_path};
pub use registry::PackRegistry;

pub const PACK_FORMAT: u32 = 1;
pub const PLACEHOLDER_ID: &str = "placeholder";
/// States every pack should provide; each falls back to `idle`, then to the
/// renderer's built-in static companion. Built-in chain: focus/paused/break/completed → idle;
/// `react`, `blink` and every unlock id → none.
pub const BASE_STATES: [&str; 5] = ["idle", "focus", "paused", "completed", "break"];
/// Optional extra states: `react` (click reaction), `blink` (idle/focus blink overlay).
pub const EXTRA_STATES: [&str; 2] = ["react", "blink"];
/// Max manifest size accepted from disk.
pub const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
/// Max single asset size served by the protocol.
pub const MAX_ASSET_BYTES: u64 = 16 * 1024 * 1024;
/// Max `frameWidth` / `frameHeight`, in pixels.
pub const MAX_FRAME_PX: u32 = 4096;
/// Max implied sheet width/height (`frameWidth × sheetColumns`, `frameHeight × sheetRows`), in pixels.
pub const MAX_SHEET_PX: u64 = 16_384;
/// Max `frames` per animation (also the max `sheetColumns` / `sheetRows`).
pub const MAX_FRAMES: u32 = 256;
/// Max `fps` (exclusive lower bound 0).
pub const MAX_FPS: f32 = 60.0;
/// Max single entry of `frameDurationsMs` (entries must be ≥ 1).
pub const MAX_FRAME_DURATION_MS: u32 = 60_000;
/// Max pack folders scanned per source directory.
pub const MAX_PACK_DIRS: usize = 256;
/// Max path length (bytes) and depth accepted by `safe_relative_path`.
pub const MAX_PATH_LEN: usize = 512;
pub const MAX_PATH_DEPTH: usize = 16;

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum Rendering {
    #[default]
    Smooth,
    Pixelated,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnimationSpec {
    pub sheet: String,
    pub frame_width: u32,
    pub frame_height: u32,
    pub frames: u32,
    pub fps: f32,
    #[serde(rename = "loop", default = "default_true")]
    pub looping: bool,
    #[serde(default)]
    pub row: u32,
    #[serde(default)]
    pub column: u32,
    /// Defaults to `column + frames`.
    #[serde(default)]
    pub sheet_columns: Option<u32>,
    /// Defaults to `row + 1`.
    #[serde(default)]
    pub sheet_rows: Option<u32>,
    /// Frame shown under reduced motion (default 0).
    #[serde(default)]
    pub still_frame: Option<u32>,
    /// Per-frame durations; when present (length == frames) they override `fps`.
    #[serde(default)]
    pub frame_durations_ms: Option<Vec<u32>>,
}

fn default_true() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PackManifest {
    pub format: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub rendering: Rendering,
    #[serde(default)]
    pub animations: BTreeMap<String, AnimationSpec>,
    #[serde(default)]
    pub fallbacks: BTreeMap<String, String>,
    #[serde(default)]
    pub palettes: BTreeMap<String, PaletteSpec>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum PackSource {
    Bundled,
    User,
    Dev,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum PackFormatKind {
    Native,
    Petdex,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PackInfo {
    pub id: String,
    pub name: String,
    pub author: Option<String>,
    pub license: Option<String>,
    pub description: Option<String>,
    pub source: PackSource,
    pub format_kind: PackFormatKind,
    /// Invalid packs are listed but cannot be activated.
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedAnimation {
    /// The key that was requested (e.g. `paused`).
    pub key: String,
    /// The pack animation actually used after fallbacks (e.g. `idle`).
    pub source_key: String,
    /// Absolute URL of the sheet, e.g. `pack://localhost/placeholder/idle.svg`.
    pub url: String,
    pub frame_width: u32,
    pub frame_height: u32,
    pub frames: u32,
    pub fps: f32,
    #[serde(rename = "loop")]
    #[cfg_attr(feature = "ts", ts(rename = "loop"))]
    pub looping: bool,
    pub row: u32,
    pub column: u32,
    pub sheet_columns: u32,
    pub sheet_rows: u32,
    pub still_frame: u32,
    pub frame_durations_ms: Option<Vec<u32>>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPack {
    pub info: PackInfo,
    pub rendering: Rendering,
    /// Every requested key → its resolved animation, or `null` (renderer uses its
    /// built-in static companion for base states and skips flourishes otherwise).
    pub animations: BTreeMap<String, Option<ResolvedAnimation>>,
    pub palettes: BTreeMap<String, PaletteSpec>,
    /// Set when the requested pack was unavailable and the placeholder is shown instead.
    pub fallback_from: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PackError {
    #[error("manifest is not valid JSON: {0}")]
    Json(String),
    #[error("unsupported pack format {0}")]
    UnsupportedFormat(u32),
    #[error("invalid pack id {0:?}")]
    InvalidId(String),
    #[error("{0}")]
    Invalid(String),
}

/// Files of the bundled placeholder pack, embedded in the binary by the shell.
#[derive(Clone, Debug, Default)]
pub struct BundledPack {
    /// (path relative to the pack root, bytes). Must include `pack.json`.
    pub files: Vec<(String, &'static [u8])>,
}
