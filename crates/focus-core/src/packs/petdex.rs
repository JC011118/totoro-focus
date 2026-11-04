//! Petdex / Codex pet format (`pet.json` + one sprite atlas), converted into a
//! native [`PackManifest`]. SPEC §7.2; the row table and its rationale are in
//! `docs/PACK_FORMAT.md`. This module never touches the network: it only reads
//! what the registry hands it from a local folder.
//!
//! Atlas: 8 columns of 192×208 px cells, 9 rows (v1, 1536×1872) or 11 rows (v2,
//! 1536×2288). Frames run left to right from column 0. Scaled atlases are
//! accepted when the grid and the 192:208 cell aspect still fit exactly.

use std::collections::BTreeMap;
use std::path::Path;

use crate::palette::clean_text;

use super::manifest::is_valid_pack_id;
use super::paths::{is_sheet_type, normalized_key};
use super::{
    AnimationSpec, PackError, PackManifest, Rendering, MAX_ASSET_BYTES, MAX_MANIFEST_BYTES,
    PACK_FORMAT,
};

pub const PETDEX_COLUMNS: u32 = 8;
pub const PETDEX_ROWS_V1: u32 = 9;
pub const PETDEX_ROWS_V2: u32 = 11;
pub const PETDEX_CELL_WIDTH: u32 = 192;
pub const PETDEX_CELL_HEIGHT: u32 = 208;
/// Spritesheet names tried when `spritesheetPath` is absent or unusable.
pub const DEFAULT_SHEETS: [&str; 2] = ["spritesheet.webp", "spritesheet.png"];
/// The warning every converted pet carries (pet.json has no license field).
pub const NO_LICENSE_WARNING: &str = "no license declared in pet.json";
/// Max length of a converted pet's display name / description.
const MAX_NAME_CHARS: usize = 80;
const MAX_DESCRIPTION_CHARS: usize = 500;

/// One row of the Petdex atlas: its index, Petdex state name and per-frame
/// timing (the native Petdex renderer's list; its length is the frame count).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PetdexRow {
    pub row: u32,
    pub state: &'static str,
    pub durations_ms: &'static [u32],
}

impl PetdexRow {
    pub fn frames(&self) -> u32 {
        self.durations_ms.len() as u32
    }
    pub fn total_ms(&self) -> u32 {
        self.durations_ms.iter().sum()
    }
    /// Average rate, `frames × 1000 / total_ms` (used when a renderer ignores
    /// `frameDurationsMs`).
    pub fn fps(&self) -> f32 {
        self.frames() as f32 * 1000.0 / self.total_ms() as f32
    }
}

/// Row 0: neutral breathing and blinking.
pub const ROW_IDLE: PetdexRow = PetdexRow {
    row: 0,
    state: "idle",
    durations_ms: &[280, 110, 110, 140, 140, 320],
};
/// Row 3: greeting.
pub const ROW_WAVING: PetdexRow = PetdexRow {
    row: 3,
    state: "waving",
    durations_ms: &[140, 140, 140, 280],
};
/// Row 4: anticipation, lift, peak, descent, settle.
pub const ROW_JUMPING: PetdexRow = PetdexRow {
    row: 4,
    state: "jumping",
    durations_ms: &[140, 140, 140, 140, 280],
};
/// Row 6: patient idle variant.
pub const ROW_WAITING: PetdexRow = PetdexRow {
    row: 6,
    state: "waiting",
    durations_ms: &[150, 150, 150, 150, 150, 260],
};

/// Our key → (Petdex row, loop). Rows 1, 2 and 7 (locomotion), 5 (`failed`:
/// no failure animations, ever), 8 (`review`, too busy for focus) and the v2
/// look-direction rows 9–10 are deliberately unmapped. Every other key falls
/// through to the built-in chain.
pub const PETDEX_MAPPING: [(&str, PetdexRow, bool); 9] = [
    ("idle", ROW_IDLE, true),
    ("focus", ROW_IDLE, true),
    ("break", ROW_IDLE, true),
    ("paused", ROW_WAITING, true),
    ("completed", ROW_JUMPING, false),
    ("react", ROW_WAVING, false),
    ("wave", ROW_WAVING, false),
    ("celebrate", ROW_JUMPING, false),
    ("long_session", ROW_JUMPING, false),
];

/// What the registry knows about a candidate spritesheet file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SheetInfo {
    /// File size in bytes.
    pub len: u64,
    /// Pixel size read from the header (`image_dims`), when readable.
    pub dims: Option<(u32, u32)>,
}

/// A converted pet: the native manifest plus conversion warnings.
#[derive(Clone, Debug, PartialEq)]
pub struct PetdexConversion {
    pub manifest: PackManifest,
    pub warnings: Vec<String>,
}

/// Width and height from an image header, without decoding: PNG (IHDR), WebP
/// (`VP8X`, `VP8L`, `VP8 `) and GIF. `None` for anything else, truncated data, or
/// a zero dimension. 30 bytes are enough for every supported format.
pub fn image_dims(b: &[u8]) -> Option<(u32, u32)> {
    let u16le = |o: usize| -> Option<u32> {
        Some(u32::from(u16::from_le_bytes(
            b.get(o..o + 2)?.try_into().ok()?,
        )))
    };
    let u24le = |o: usize| -> Option<u32> {
        let s = b.get(o..o + 3)?;
        Some(u32::from(s[0]) | (u32::from(s[1]) << 8) | (u32::from(s[2]) << 16))
    };
    let dims = if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        if b.get(12..16)? != b"IHDR" {
            return None;
        }
        let w = u32::from_be_bytes(b.get(16..20)?.try_into().ok()?);
        let h = u32::from_be_bytes(b.get(20..24)?.try_into().ok()?);
        (w, h)
    } else if b.starts_with(b"RIFF") && b.get(8..12)? == b"WEBP" {
        match b.get(12..16)? {
            b"VP8X" => (u24le(24)? + 1, u24le(27)? + 1),
            b"VP8L" => {
                if *b.get(20)? != 0x2f {
                    return None;
                }
                let bits = u32::from_le_bytes(b.get(21..25)?.try_into().ok()?);
                ((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1)
            }
            b"VP8 " => {
                if b.get(23..26)? != [0x9d, 0x01, 0x2a] {
                    return None;
                }
                (u16le(26)? & 0x3fff, u16le(28)? & 0x3fff)
            }
            _ => return None,
        }
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        (u16le(6)?, u16le(8)?)
    } else {
        return None;
    };
    (dims.0 > 0 && dims.1 > 0).then_some(dims)
}

/// Row count implied by a sheet's size (the native Petdex rule), or an error
/// when the size does not form an 8-column grid of 192:208 cells.
pub fn rows_for_dims(w: u32, h: u32) -> Result<u32, String> {
    let (w64, h64) = (u64::from(w), u64::from(h));
    let rows: u32 = if h64 * 1536 >= w64 * 2288 {
        PETDEX_ROWS_V2
    } else {
        PETDEX_ROWS_V1
    };
    let fits = w > 0
        && h > 0
        && w.is_multiple_of(PETDEX_COLUMNS)
        && h.is_multiple_of(rows)
        && u64::from(w / PETDEX_COLUMNS) * u64::from(PETDEX_CELL_HEIGHT)
            == u64::from(h / rows) * u64::from(PETDEX_CELL_WIDTH);
    if fits {
        Ok(rows)
    } else {
        Err(format!(
            "spritesheet is {w}×{h} px; expected 8 columns of 192×208 cells (1536×1872 for v1, 1536×2288 for v2, or a uniform scale)"
        ))
    }
}

/// Lowercase `s`, map characters outside `[a-z0-9_-]` to `-`, strip leading
/// `-`/`_`, cut to 64 bytes; `None` if nothing valid remains.
fn sanitize_id(s: &str) -> Option<String> {
    let mapped: String = s
        .chars()
        .map(|c| c.to_ascii_lowercase())
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed: String = mapped
        .trim_start_matches(['-', '_'])
        .chars()
        .take(64)
        .collect();
    let trimmed = trimmed.trim_end_matches('-').to_string();
    is_valid_pack_id(&trimmed).then_some(trimmed)
}

/// A flat file name the Petdex native app accepts: `[A-Za-z0-9._-]{1,63}`,
/// additionally not hidden and of an allowed raster type.
fn plain_sheet_name(s: &str) -> Option<String> {
    let flat = !s.is_empty()
        && s.len() <= 63
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    let key = normalized_key(s).filter(|_| flat)?;
    let raster = is_sheet_type(Path::new(&key)) && !key.to_ascii_lowercase().ends_with(".svg");
    raster.then_some(key)
}

fn str_field<'a>(obj: &'a serde_json::Map<String, serde_json::Value>, k: &str) -> Option<&'a str> {
    obj.get(k).and_then(serde_json::Value::as_str)
}

/// Convert a `pet.json` found in folder `dir_name`. `probe(rel)` reports a
/// candidate spritesheet inside the folder (`None` = missing or not servable).
///
/// Hard errors (`Err`): pet.json too large, not JSON or not an object; a
/// `spriteVersionNumber` other than 1 or 2; no usable id (the folder name, else
/// the sanitized pet.json `id`, else the sanitized folder name); no spritesheet
/// (`spritesheetPath`, then `spritesheet.webp`, then `spritesheet.png`); a sheet
/// larger than `MAX_ASSET_BYTES`; readable dimensions that are not an 8-column
/// grid of 192:208 cells. Warnings: no license (always), fallbacks taken, and
/// dimensions that disagree with `spriteVersionNumber` (the dimensions win).
pub fn convert_petdex(
    pet_json: &str,
    dir_name: &str,
    probe: &dyn Fn(&str) -> Option<SheetInfo>,
) -> Result<PetdexConversion, PackError> {
    if pet_json.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(PackError::Invalid(format!(
            "pet.json is larger than {} KiB",
            MAX_MANIFEST_BYTES / 1024
        )));
    }
    let value: serde_json::Value =
        serde_json::from_str(pet_json).map_err(|e| PackError::Json(e.to_string()))?;
    let Some(obj) = value.as_object() else {
        return Err(PackError::Invalid(
            "pet.json must contain a JSON object".into(),
        ));
    };
    let mut warnings = vec![NO_LICENSE_WARNING.to_string()];

    let version = match obj.get("spriteVersionNumber") {
        None | Some(serde_json::Value::Null) => 1,
        Some(v) => match v.as_u64() {
            Some(1) => 1,
            Some(2) => 2,
            _ => {
                return Err(PackError::Invalid(format!(
                    "unsupported spriteVersionNumber {v} (expected 1 or 2)"
                )))
            }
        },
    };

    let id = if is_valid_pack_id(dir_name) {
        dir_name.to_string()
    } else if let Some(id) = str_field(obj, "id")
        .and_then(sanitize_id)
        .or_else(|| sanitize_id(dir_name))
    {
        warnings.push(format!(
            "folder name {dir_name:?} is not a valid pack id; using {id:?}"
        ));
        id
    } else {
        return Err(PackError::InvalidId(dir_name.to_string()));
    };

    let name = str_field(obj, "displayName")
        .and_then(|n| clean_text(n, MAX_NAME_CHARS).0)
        .unwrap_or_else(|| id.clone());
    let description =
        str_field(obj, "description").and_then(|d| clean_text(d, MAX_DESCRIPTION_CHARS).0);

    // Spritesheet: the declared path, then the Petdex defaults.
    let mut candidates: Vec<String> = Vec::new();
    match obj.get("spritesheetPath") {
        None | Some(serde_json::Value::Null) => {}
        Some(serde_json::Value::String(s)) => match plain_sheet_name(s) {
            Some(k) => candidates.push(k),
            None => warnings.push(format!(
                "spritesheetPath {s:?} is not a plain .webp/.png file name in this folder; trying {}",
                DEFAULT_SHEETS.join(", ")
            )),
        },
        Some(other) => warnings.push(format!("spritesheetPath {other} is not a string; trying {}", DEFAULT_SHEETS.join(", "))),
    }
    let declared = candidates.first().cloned();
    for d in DEFAULT_SHEETS {
        if !candidates.iter().any(|c| c == d) {
            candidates.push(d.to_string());
        }
    }
    let Some((sheet, info)) = candidates
        .iter()
        .find_map(|c| probe(c).map(|i| (c.clone(), i)))
    else {
        return Err(PackError::Invalid(format!(
            "spritesheet not found (looked for {})",
            candidates.join(", ")
        )));
    };
    if let Some(d) = declared.filter(|d| *d != sheet) {
        warnings.push(format!(
            "spritesheetPath {d:?} was not found; using {sheet:?}"
        ));
    }
    if info.len > MAX_ASSET_BYTES {
        return Err(PackError::Invalid(format!(
            "spritesheet {sheet:?} is larger than {} MiB",
            MAX_ASSET_BYTES / (1024 * 1024)
        )));
    }

    let version_rows = if version == 2 {
        PETDEX_ROWS_V2
    } else {
        PETDEX_ROWS_V1
    };
    let rows = match info.dims {
        Some((w, h)) => {
            let rows = rows_for_dims(w, h).map_err(PackError::Invalid)?;
            if rows != version_rows {
                warnings.push(format!(
                    "spritesheet has {rows} rows but spriteVersionNumber is {version}; using {rows} rows (from the image size)"
                ));
            }
            rows
        }
        None => {
            warnings.push(format!(
                "could not read the spritesheet size; assuming {version_rows} rows (spriteVersionNumber {version})"
            ));
            version_rows
        }
    };

    let animations: BTreeMap<String, AnimationSpec> = PETDEX_MAPPING
        .iter()
        .map(|(key, r, looping)| {
            let spec = AnimationSpec {
                sheet: sheet.clone(),
                frame_width: PETDEX_CELL_WIDTH,
                frame_height: PETDEX_CELL_HEIGHT,
                frames: r.frames(),
                fps: r.fps(),
                looping: *looping,
                row: r.row,
                column: 0,
                sheet_columns: Some(PETDEX_COLUMNS),
                sheet_rows: Some(rows),
                // A one-shot rests on its last frame, so reduced motion shows that pose too.
                still_frame: Some(if *looping { 0 } else { r.frames() - 1 }),
                frame_durations_ms: Some(r.durations_ms.to_vec()),
            };
            (key.to_string(), spec)
        })
        .collect();

    let manifest = PackManifest {
        format: PACK_FORMAT,
        id,
        name,
        author: None,
        license: None,
        description,
        rendering: Rendering::Smooth,
        animations,
        fallbacks: BTreeMap::new(),
        palettes: BTreeMap::new(),
    };
    Ok(PetdexConversion { manifest, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_table_timing() {
        assert_eq!((ROW_IDLE.frames(), ROW_IDLE.total_ms()), (6, 1100));
        assert_eq!((ROW_WAVING.frames(), ROW_WAVING.total_ms()), (4, 700));
        assert_eq!((ROW_JUMPING.frames(), ROW_JUMPING.total_ms()), (5, 840));
        assert_eq!((ROW_WAITING.frames(), ROW_WAITING.total_ms()), (6, 1010));
        assert!((ROW_IDLE.fps() - 5.4545).abs() < 0.001);
        for (_, r, _) in PETDEX_MAPPING {
            assert!(r.fps() > 0.0 && r.fps() <= 60.0);
            assert!(
                ![1, 2, 5, 7, 8, 9, 10].contains(&r.row),
                "row {} must stay unmapped",
                r.row
            );
        }
    }

    #[test]
    fn sanitize_id_cases() {
        assert_eq!(sanitize_id("My Pet!").as_deref(), Some("my-pet"));
        assert_eq!(sanitize_id("__x").as_deref(), Some("x"));
        assert_eq!(sanitize_id("ÄÖ"), None);
        assert_eq!(sanitize_id("---"), None);
        assert_eq!(sanitize_id(&"a".repeat(80)).map(|s| s.len()), Some(64));
    }

    #[test]
    fn plain_sheet_names() {
        assert_eq!(
            plain_sheet_name("spritesheet.webp").as_deref(),
            Some("spritesheet.webp")
        );
        assert_eq!(
            plain_sheet_name("Sheet-2_x.PNG").as_deref(),
            Some("Sheet-2_x.PNG")
        );
        for bad in [
            "",
            "a/b.png",
            "../x.png",
            ".hidden.png",
            "x.svg",
            "x.json",
            "x y.png",
            &format!("{}.png", "a".repeat(60)),
        ] {
            assert_eq!(plain_sheet_name(bad), None, "{bad}");
        }
    }
}
