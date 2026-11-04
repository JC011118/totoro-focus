//! `pack.json` parsing, validation, animation resolution and render keys.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::palette::sanitize_palette;
use crate::rewards::{UnlockKind, UnlockTable};

use super::paths::{is_sheet_type, normalized_key};
use super::{
    AnimationSpec, PackError, PackManifest, BASE_STATES, EXTRA_STATES, MAX_FPS, MAX_FRAMES,
    MAX_FRAME_DURATION_MS, MAX_FRAME_PX, MAX_MANIFEST_BYTES, MAX_SHEET_PX, PACK_FORMAT,
};

/// True if `id` matches `^[a-z0-9][a-z0-9_-]{0,63}$` (pack ids, animation keys,
/// fallback keys and palette ids).
pub fn is_valid_pack_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
}

/// The built-in chain: `focus`, `paused`, `break`, `completed` → `idle`;
/// everything else (idle, react, blink, unlock ids) → none.
pub(crate) fn builtin_fallback(key: &str) -> Option<&'static str> {
    match key {
        "focus" | "paused" | "break" | "completed" => Some("idle"),
        _ => None,
    }
}

/// Parse a native `pack.json`.
///
/// Checks, in order: size (≤ `MAX_MANIFEST_BYTES`), JSON syntax and a top-level
/// object (`Json` / `Invalid`), `format == PACK_FORMAT` (`UnsupportedFormat`,
/// checked before the rest of the shape so future formats get a clear error),
/// the field shapes (`Json`), and the id pattern (`InvalidId`). Unknown fields are
/// ignored. Semantic checks live in [`validate_manifest`].
pub fn parse_manifest(json: &str) -> Result<PackManifest, PackError> {
    if json.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(PackError::Invalid(format!(
            "manifest is larger than {} KiB",
            MAX_MANIFEST_BYTES / 1024
        )));
    }
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| PackError::Json(e.to_string()))?;
    let Some(obj) = value.as_object() else {
        return Err(PackError::Invalid(
            "pack.json must contain a JSON object".into(),
        ));
    };
    if let Some(format) = obj.get("format").and_then(serde_json::Value::as_u64) {
        if format != u64::from(PACK_FORMAT) {
            return Err(PackError::UnsupportedFormat(
                u32::try_from(format).unwrap_or(u32::MAX),
            ));
        }
    }
    let m: PackManifest =
        serde_json::from_value(value).map_err(|e| PackError::Json(e.to_string()))?;
    if !is_valid_pack_id(&m.id) {
        return Err(PackError::InvalidId(m.id));
    }
    Ok(m)
}

/// Validate a parsed manifest. Errors make the pack unusable (bad id, no `idle`
/// animation AND no other base state, zero/oversized frame sizes or counts,
/// fps outside (0, 60], unsafe sheet paths, missing sheet files). Warnings are
/// informational (missing optional states, unknown fallback targets, dropped
/// palette values). `file_exists` receives sheet paths relative to the pack root.
///
/// Precisely, errors are: `format != 1`; an id not matching
/// `^[a-z0-9][a-z0-9_-]{0,63}$`; no base state resolving to any animation (via
/// `resolve`); and, per animation: an invalid key, a sheet path rejected by
/// `safe_relative_path`, a sheet that is not png/webp/gif/jpg/svg, a sheet for
/// which `file_exists` is false, `frameWidth`/`frameHeight` 0 or above
/// `MAX_FRAME_PX`, `frames` 0 or above `MAX_FRAMES`, fps not in (0, 60],
/// `column + frames > sheetColumns`, `row >= sheetRows`, sheet counts above
/// `MAX_FRAMES` or an implied sheet larger than `MAX_SHEET_PX`, `stillFrame >=
/// frames`, and `frameDurationsMs` whose length differs from `frames` or with an
/// entry outside 1..=`MAX_FRAME_DURATION_MS`.
///
/// Warnings are: an empty name; each base state other than the one
/// providing art that has no animation of its own (with where it falls back);
/// fallbacks with invalid keys, unused entries (the key has its own animation),
/// unknown targets, or cycles (which `resolve` cuts); invalid palette ids (the
/// palette is ignored); and every palette value `sanitize_palette` drops. (The
/// registry adds source-specific warnings such as a missing license.)
pub fn validate_manifest(
    m: &PackManifest,
    file_exists: &dyn Fn(&str) -> bool,
) -> (Vec<String>, Vec<String>) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if m.format != PACK_FORMAT {
        errors.push(format!(
            "unsupported pack format {} (expected {PACK_FORMAT})",
            m.format
        ));
    }
    if !is_valid_pack_id(&m.id) {
        errors.push(format!(
            "invalid pack id {:?}: use 1–64 of a-z, 0-9, _ and -, starting with a letter or digit",
            m.id
        ));
    }
    if m.name.trim().is_empty() {
        warnings.push("name is empty; the id is shown instead".into());
    }
    for (key, spec) in &m.animations {
        validate_animation(key, spec, file_exists, &mut errors);
    }

    // Base states: at least one must resolve; report the ones that borrow art.
    let resolved: Vec<(&str, Option<String>)> = BASE_STATES
        .iter()
        .map(|&k| (k, resolve(m, k).map(|(src, _)| src)))
        .collect();
    if resolved.iter().all(|(_, r)| r.is_none()) {
        errors.push(format!(
            "no base state animation: add at least an \"idle\" animation (base states: {})",
            BASE_STATES.join(", ")
        ));
    } else {
        for (key, r) in &resolved {
            match r {
                Some(src) if src == key => {}
                Some(src) => warnings.push(format!("no \"{key}\" animation; it uses \"{src}\"")),
                None => warnings.push(format!(
                    "no \"{key}\" animation; the built-in companion is shown instead"
                )),
            }
        }
    }

    validate_fallbacks(m, &mut warnings);

    for (id, palette) in &m.palettes {
        if !is_valid_pack_id(id) {
            warnings.push(format!(
                "palette id {id:?} is not a valid id; the palette is ignored"
            ));
            continue;
        }
        let (_, dropped) = sanitize_palette(palette);
        warnings.extend(dropped.into_iter().map(|w| format!("palettes.{id}.{w}")));
    }

    (errors, warnings)
}

fn validate_animation(
    key: &str,
    a: &AnimationSpec,
    file_exists: &dyn Fn(&str) -> bool,
    errors: &mut Vec<String>,
) {
    let mut err = |msg: String| errors.push(format!("animations.{key}: {msg}"));
    if !is_valid_pack_id(key) {
        // Report with the raw key escaped so control characters stay visible.
        errors.push(format!(
            "animation key {key:?} is not valid: use 1–64 of a-z, 0-9, _ and -"
        ));
        return;
    }
    match normalized_key(&a.sheet) {
        None => err(format!("unsafe sheet path {:?} (use a relative path without \"..\", \"\\\", \":\" or hidden names)", a.sheet)),
        Some(rel) => {
            if !is_sheet_type(Path::new(&rel)) {
                err(format!("sheet {rel:?} must be a .png, .webp, .gif, .jpg or .svg file"));
            } else if !file_exists(&rel) {
                err(format!("sheet file {rel:?} is missing (or too large)"));
            }
        }
    }
    if a.frame_width == 0 || a.frame_height == 0 {
        err(format!(
            "frame size {}×{} must be at least 1×1",
            a.frame_width, a.frame_height
        ));
    } else if a.frame_width > MAX_FRAME_PX || a.frame_height > MAX_FRAME_PX {
        err(format!(
            "frame size {}×{} exceeds {MAX_FRAME_PX}×{MAX_FRAME_PX}",
            a.frame_width, a.frame_height
        ));
    }
    if a.frames == 0 {
        err("frames must be at least 1".into());
    } else if a.frames > MAX_FRAMES {
        err(format!("frames {} exceeds {MAX_FRAMES}", a.frames));
    }
    if !(a.fps.is_finite() && a.fps > 0.0 && a.fps <= MAX_FPS) {
        err(format!(
            "fps {} must be greater than 0 and at most {MAX_FPS}",
            a.fps
        ));
    }
    let columns = u64::from(a.sheet_columns.unwrap_or(a.column.saturating_add(a.frames)));
    let rows = u64::from(a.sheet_rows.unwrap_or(a.row.saturating_add(1)));
    if columns > u64::from(MAX_FRAMES) || rows > u64::from(MAX_FRAMES) {
        err(format!(
            "sheet grid {columns}×{rows} exceeds {MAX_FRAMES}×{MAX_FRAMES} cells"
        ));
    } else {
        if u64::from(a.column) + u64::from(a.frames) > columns {
            err(format!(
                "frames run past the sheet: column {} + frames {} > sheetColumns {columns}",
                a.column, a.frames
            ));
        }
        if u64::from(a.row) >= rows {
            err(format!(
                "row {} is outside the sheet (sheetRows {rows})",
                a.row
            ));
        }
        if u64::from(a.frame_width) * columns > MAX_SHEET_PX
            || u64::from(a.frame_height) * rows > MAX_SHEET_PX
        {
            err(format!(
                "implied sheet size exceeds {MAX_SHEET_PX}×{MAX_SHEET_PX} px"
            ));
        }
    }
    if let Some(still) = a.still_frame {
        if a.frames > 0 && still >= a.frames {
            err(format!(
                "stillFrame {still} is out of range (frames {})",
                a.frames
            ));
        }
    }
    if let Some(d) = &a.frame_durations_ms {
        if d.len() != a.frames as usize {
            err(format!(
                "frameDurationsMs has {} entries; expected {} (one per frame)",
                d.len(),
                a.frames
            ));
        }
        if d.iter().any(|&ms| ms == 0 || ms > MAX_FRAME_DURATION_MS) {
            err(format!(
                "frameDurationsMs entries must be between 1 and {MAX_FRAME_DURATION_MS} ms"
            ));
        }
    }
}

fn validate_fallbacks(m: &PackManifest, warnings: &mut Vec<String>) {
    for (from, to) in &m.fallbacks {
        if !is_valid_pack_id(from) || !is_valid_pack_id(to) {
            warnings.push(format!(
                "fallback {from:?} → {to:?} uses an invalid key and never applies"
            ));
            continue;
        }
        if m.animations.contains_key(from) {
            warnings.push(format!(
                "fallback for \"{from}\" is unused: the pack has a \"{from}\" animation"
            ));
        }
        let known = m.animations.contains_key(to)
            || m.fallbacks.contains_key(to)
            || builtin_fallback(to).is_some();
        if !known {
            warnings.push(format!(
                "fallback target \"{to}\" (for \"{from}\") is not an animation in this pack"
            ));
        }
    }
    for c in fallback_cycles(&m.fallbacks) {
        warnings.push(format!(
            "fallback cycle among {}; the cycle is cut",
            describe_cycle(&c)
        ));
    }
}

/// Keys of a fallback cycle listed in a warning; longer cycles are elided.
const MAX_CYCLE_KEYS_SHOWN: usize = 8;

/// Every cycle among the pack's own fallback edges, each exactly once, in edge
/// order starting from its smallest key; the list is sorted.
///
/// Each key has at most one outgoing edge (the map is a functional graph), so
/// one walk per unvisited key finds every cycle: a walk stops at a key it has
/// already put on its own path (a new cycle), at a key an earlier walk finished
/// (nothing new beyond it), or at a key without a fallback. Every key is walked
/// once, so a manifest with thousands of entries costs O(n log n), not the
/// quadratic-or-worse of restarting the walk from every key.
fn fallback_cycles(fallbacks: &BTreeMap<String, String>) -> Vec<Vec<&str>> {
    enum Mark {
        /// On the current walk's path, at this index.
        OnPath(usize),
        /// Finished by an earlier walk.
        Done,
    }
    let mut marks: BTreeMap<&str, Mark> = BTreeMap::new();
    let mut cycles: Vec<Vec<&str>> = Vec::new();
    let mut path: Vec<&str> = Vec::new();
    for start in fallbacks.keys() {
        if marks.contains_key(start.as_str()) {
            continue;
        }
        let mut cur = start.as_str();
        loop {
            match marks.get(cur) {
                Some(Mark::Done) => break,
                Some(Mark::OnPath(pos)) => {
                    let mut members = path[*pos..].to_vec();
                    let first = members
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, k)| **k)
                        .map_or(0, |(i, _)| i);
                    members.rotate_left(first);
                    cycles.push(members);
                    break;
                }
                None => {}
            }
            let Some(next) = fallbacks.get(cur) else {
                break;
            };
            marks.insert(cur, Mark::OnPath(path.len()));
            path.push(cur);
            cur = next;
        }
        for k in path.drain(..) {
            marks.insert(k, Mark::Done);
        }
    }
    cycles.sort_unstable();
    cycles
}

/// `a → b → c`, or the first `MAX_CYCLE_KEYS_SHOWN` keys and a count.
fn describe_cycle(members: &[&str]) -> String {
    if members.len() <= MAX_CYCLE_KEYS_SHOWN {
        return members.join(" → ");
    }
    format!(
        "{} → … ({} keys)",
        members[..MAX_CYCLE_KEYS_SHOWN].join(" → "),
        members.len()
    )
}

/// Resolve `key` to `(source_key, spec)`: the pack's own animation, then the
/// pack's `fallbacks` chain, then the built-in chain (focus/paused/break/completed
/// → `idle`; `react`, `blink`, unlock ids → none). Cycles are cut. `None` when nothing applies.
///
/// At every key its own animation wins. Otherwise the pack's fallback is followed
/// first and the built-in one second, depth first: if the pack's chain dead-ends
/// (an unknown target), the built-in chain of the keys already passed is still
/// tried, so a base state always reaches `idle` when the pack has one. A key is
/// visited at most once, which cuts cycles: a pack cycle `paused → focus →
/// paused` still reaches `idle` through the built-in chain.
pub fn resolve<'a>(m: &'a PackManifest, key: &str) -> Option<(String, &'a AnimationSpec)> {
    let mut visited: BTreeSet<&str> = BTreeSet::new();
    let mut stack: Vec<&str> = vec![key];
    while let Some(cur) = stack.pop() {
        if !visited.insert(cur) {
            continue;
        }
        if let Some((k, spec)) = m.animations.get_key_value(cur) {
            return Some((k.clone(), spec));
        }
        // Pushed in reverse preference order: the pack's fallback is popped first.
        if let Some(b) = builtin_fallback(cur).filter(|b| !visited.contains(b)) {
            stack.push(b);
        }
        if let Some(p) = m
            .fallbacks
            .get(cur)
            .map(String::as_str)
            .filter(|p| !visited.contains(p))
        {
            stack.push(p);
        }
    }
    None
}

/// Keys the shell resolves for the renderer (SPEC §7 "Render keys"): BASE_STATES ∪
/// EXTRA_STATES ∪ unlock ids of kind animation|idleVariant ∪ the pack's own animation
/// keys. Deduplicated, stable order (that listing order).
pub fn render_keys(table: &UnlockTable, m: &PackManifest) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let unlock_keys = table
        .unlocks
        .iter()
        .filter(|u| matches!(u.kind, UnlockKind::Animation | UnlockKind::IdleVariant))
        .map(|u| u.id.as_str());
    BASE_STATES
        .iter()
        .copied()
        .chain(EXTRA_STATES.iter().copied())
        .chain(unlock_keys)
        .chain(m.animations.keys().map(String::as_str))
        .filter(|k| seen.insert(*k))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_id_pattern() {
        for ok in ["a", "0", "placeholder", "my-pack_2", &"a".repeat(64)] {
            assert!(is_valid_pack_id(ok), "{ok}");
        }
        for bad in [
            "",
            "-a",
            "_a",
            "A",
            "a b",
            "a/b",
            "a.b",
            "é",
            &"a".repeat(65),
        ] {
            assert!(!is_valid_pack_id(bad), "{bad}");
        }
    }

    fn fallbacks(edges: &[(&str, &str)]) -> BTreeMap<String, String> {
        edges
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn fallback_cycles_are_found_once_in_edge_order() {
        // Two tails (x → y → b, z → c) lead into the cycle a → c → b → a; a
        // self-loop; a chain into a dead end; an edge into the built-in chain.
        let fb = fallbacks(&[
            ("x", "y"),
            ("y", "b"),
            ("z", "c"),
            ("c", "b"),
            ("b", "a"),
            ("a", "c"),
            ("wave", "wave"),
            ("m", "n"),
            ("n", "nowhere"),
            ("paused", "focus"),
        ]);
        assert_eq!(
            fallback_cycles(&fb),
            vec![vec!["a", "c", "b"], vec!["wave"]],
            "rotated to the smallest key, edges kept in order"
        );
        assert!(fallback_cycles(&BTreeMap::new()).is_empty());
        assert!(fallback_cycles(&fallbacks(&[("a", "b"), ("b", "c")])).is_empty());
        // The walk from the tail key "a" enters the cycle at q; it still reads
        // from its smallest key.
        let fb = fallbacks(&[("a", "q"), ("q", "p"), ("p", "r"), ("r", "q")]);
        assert_eq!(fallback_cycles(&fb), vec![vec!["p", "r", "q"]]);
    }

    #[test]
    fn many_cycles_and_long_cycles() {
        // 1,000 two-key cycles, each with a tail, and one 3,000-key cycle.
        let mut fb = BTreeMap::new();
        for i in 0..1000 {
            fb.insert(format!("a{i:04}"), format!("b{i:04}"));
            fb.insert(format!("b{i:04}"), format!("a{i:04}"));
            fb.insert(format!("t{i:04}"), format!("b{i:04}"));
        }
        for i in 0..3000 {
            fb.insert(format!("r{i:04}"), format!("r{:04}", (i + 1) % 3000));
        }
        let cycles = fallback_cycles(&fb);
        assert_eq!(cycles.len(), 1001);
        assert_eq!(cycles[0], vec!["a0000", "b0000"]);
        let long = cycles.iter().find(|c| c.len() == 3000).unwrap();
        assert_eq!(long[..3], ["r0000", "r0001", "r0002"]);
        assert_eq!(long[2999], "r2999");
    }

    #[test]
    fn long_cycles_are_elided_in_warnings() {
        assert_eq!(describe_cycle(&["a"]), "a");
        let eight: Vec<String> = (0..8).map(|i| format!("k{i}")).collect();
        let eight: Vec<&str> = eight.iter().map(String::as_str).collect();
        assert_eq!(describe_cycle(&eight), eight.join(" → "));
        let many: Vec<String> = (0..500).map(|i| format!("k{i:03}")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        assert_eq!(
            describe_cycle(&many),
            "k000 → k001 → k002 → k003 → k004 → k005 → k006 → k007 → … (500 keys)"
        );
    }

    #[test]
    fn builtin_chain() {
        for k in ["focus", "paused", "break", "completed"] {
            assert_eq!(builtin_fallback(k), Some("idle"));
        }
        for k in ["idle", "react", "blink", "wave", "sleepy"] {
            assert_eq!(builtin_fallback(k), None);
        }
    }
}
