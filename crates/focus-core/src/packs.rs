//! Character packs: manifest format, validation, animation resolution with
//! fallbacks, discovery across sources, and safe file access for the
//! `pack://` protocol. See docs/SPEC.md §7.
//!
//! Implementers may split this into `packs/` submodules (e.g. `manifest`,
//! `registry`, `petdex`) but must keep these public items stable.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use crate::palette::PaletteSpec;
use crate::rewards::UnlockTable;

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

/// Parse a native `pack.json`.
pub fn parse_manifest(json: &str) -> Result<PackManifest, PackError> {
    let _ = json;
    todo!("packs: parse_manifest")
}

/// Validate a parsed manifest. Errors make the pack unusable (bad id, no `idle`
/// animation AND no other base state, zero/oversized frame sizes or counts,
/// fps outside (0, 60], unsafe sheet paths, missing sheet files). Warnings are
/// informational (missing optional states, unknown fallback targets, dropped
/// palette values). `file_exists` receives sheet paths relative to the pack root.
pub fn validate_manifest(m: &PackManifest, file_exists: &dyn Fn(&str) -> bool) -> (Vec<String>, Vec<String>) {
    let _ = (m, file_exists);
    todo!("packs: validate_manifest")
}

/// Resolve `key` to `(source_key, spec)`: the pack's own animation, then the
/// pack's `fallbacks` chain, then the built-in chain (focus/paused/break/completed
/// → `idle`; `react`, `blink`, unlock ids → none). Cycles are cut. `None` when nothing applies.
pub fn resolve<'a>(m: &'a PackManifest, key: &str) -> Option<(String, &'a AnimationSpec)> {
    let _ = (m, key);
    todo!("packs: resolve")
}

/// Keys the shell resolves for the renderer (SPEC §7 "Render keys"): BASE_STATES ∪
/// EXTRA_STATES ∪ unlock ids of kind animation|idleVariant ∪ the pack's own animation
/// keys. Deduplicated, stable order (that listing order).
pub fn render_keys(table: &UnlockTable, m: &PackManifest) -> Vec<String> {
    let _ = (table, m);
    todo!("packs: render_keys")
}

/// Normalize a pack-relative path. Rejects empty, absolute, drive-prefixed,
/// backslash, `..`, NUL, and hidden (`.`-prefixed) components.
pub fn safe_relative_path(p: &str) -> Option<PathBuf> {
    let _ = p;
    todo!("packs: safe_relative_path")
}

/// MIME type for a served asset by extension (png, webp, svg, json, gif, jpg);
/// `None` for anything else (the protocol refuses it).
pub fn mime_for(path: &Path) -> Option<&'static str> {
    let _ = path;
    todo!("packs: mime_for")
}

/// Files of the bundled placeholder pack, embedded in the binary by the shell.
#[derive(Clone, Debug, Default)]
pub struct BundledPack {
    /// (path relative to the pack root, bytes). Must include `pack.json`.
    pub files: Vec<(String, &'static [u8])>,
}

/// All known packs. Built once at startup and on `list_packs` refresh.
#[derive(Debug, Default)]
pub struct PackRegistry {
    // Implementation-defined fields.
    _private: (),
}

impl PackRegistry {
    /// Discover packs: the bundled placeholder (always present, id reserved),
    /// then every `<user_dir>/<subdir>/pack.json`, then (when given) every
    /// `<dev_dir>/<subdir>/pack.json`; dev overrides user on id collision; packs
    /// claiming the `placeholder` id from disk are rejected with an error entry.
    /// A subdir with `pet.json` and no `pack.json` is converted as a Petdex pack (SPEC §7.2).
    /// Never panics on bad input; problems become `PackInfo.errors`.
    pub fn load(bundled: BundledPack, user_dir: Option<&Path>, dev_dir: Option<&Path>) -> Self {
        let _ = (bundled, user_dir, dev_dir);
        todo!("packs: PackRegistry::load")
    }

    /// All packs (valid and invalid), bundled first then by name.
    pub fn list(&self) -> Vec<PackInfo> {
        todo!("packs: list")
    }

    pub fn is_valid(&self, id: &str) -> bool {
        let _ = id;
        todo!("packs: is_valid")
    }

    /// Resolve `id` for rendering (falling back to the placeholder if `id` is
    /// unknown or invalid, recording `fallback_from`). Resolves exactly
    /// `render_keys(table, manifest)`; `url_prefix` is e.g. `pack://localhost/` and URLs are
    /// `{url_prefix}{packId}/{sheet}` with each path segment percent-encoded.
    /// Use `render_keys(table, manifest)` for `keys` (the shell does).
    pub fn resolve_pack(&self, id: &str, table: &UnlockTable, url_prefix: &str) -> ResolvedPack {
        let _ = (id, table, url_prefix);
        todo!("packs: resolve_pack")
    }

    /// Bytes and MIME type of a file inside a known pack, or `None` if the pack is
    /// unknown, the path is unsafe, escapes the pack directory (after
    /// canonicalization, so symlinks out are refused), has a disallowed type, or
    /// exceeds `MAX_ASSET_BYTES`.
    pub fn read_file(&self, pack_id: &str, rel_path: &str) -> Option<(Vec<u8>, &'static str)> {
        let _ = (pack_id, rel_path);
        todo!("packs: read_file")
    }
}
