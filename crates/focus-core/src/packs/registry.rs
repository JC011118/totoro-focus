//! Pack discovery across sources, collision rules, resolution for rendering and
//! file serving for the `pack://` protocol.
//!
//! Listing ids are unique. A disk pack that cannot own its id (it claims the
//! reserved `placeholder` id, or loses an id collision) is listed under
//! `"<source>:<folder>"`, which can never match the id pattern, so it can never
//! be activated or served. Within one source the winner of an id is a parsed
//! native pack, then a converted Petdex pet, then a broken pack whose id was
//! guessed from its folder name; ties go to the first folder name. A dev pack
//! replaces a user pack with the same id (the user pack is not listed).

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use crate::palette::{clean_text, sanitize_palette};
use crate::rewards::UnlockTable;

use super::manifest::{is_valid_pack_id, parse_manifest, render_keys, resolve, validate_manifest};
use super::paths::{encode_url_path, mime_for, normalized_key, safe_relative_path};
use super::petdex::{convert_petdex, image_dims, SheetInfo};
use super::{
    BundledPack, PackFormatKind, PackInfo, PackManifest, PackSource, Rendering, ResolvedAnimation,
    ResolvedPack, MAX_ASSET_BYTES, MAX_MANIFEST_BYTES, MAX_PACK_DIRS, PACK_FORMAT, PLACEHOLDER_ID,
};

const MAX_NAME_CHARS: usize = 80;
const MAX_AUTHOR_CHARS: usize = 120;
const MAX_LICENSE_CHARS: usize = 64;
const MAX_DESCRIPTION_CHARS: usize = 500;
/// Bytes read from a spritesheet to find its dimensions.
const HEADER_BYTES: u64 = 64;

/// All known packs. Built once at startup and on `list_packs` refresh.
#[derive(Debug, Default)]
pub struct PackRegistry {
    /// List order: bundled first, then by name.
    entries: Vec<Entry>,
}

#[derive(Debug)]
struct Entry {
    info: PackInfo,
    /// Present iff the pack is valid.
    manifest: Option<PackManifest>,
    store: Store,
}

enum Store {
    /// Bundled files keyed by normalized relative path.
    Memory(BTreeMap<String, &'static [u8]>),
    /// Canonicalized pack folder.
    Dir(PathBuf),
    None,
}

impl fmt::Debug for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Store::Memory(m) => write!(f, "Memory({} files)", m.len()),
            Store::Dir(p) => write!(f, "Dir({})", p.display()),
            Store::None => f.write_str("None"),
        }
    }
}

/// A disk pack during discovery, before collisions are settled.
struct Candidate {
    dir_name: String,
    /// 0 = parsed native, 1 = converted Petdex, 2 = id guessed from a broken pack.
    rank: u8,
    entry: Entry,
}

fn source_label(s: PackSource) -> &'static str {
    match s {
        PackSource::Bundled => "bundled",
        PackSource::User => "user",
        PackSource::Dev => "dev",
    }
}

fn clean(s: Option<&str>, max: usize, field: &str, warnings: &mut Vec<String>) -> Option<String> {
    let (out, truncated) = clean_text(s?, max);
    if truncated {
        warnings.push(format!("{field} truncated to {max} characters"));
    }
    out
}

fn make_info(
    m: &PackManifest,
    id: &str,
    source: PackSource,
    format_kind: PackFormatKind,
    errors: Vec<String>,
    mut warnings: Vec<String>,
) -> PackInfo {
    let name = clean(Some(&m.name), MAX_NAME_CHARS, "name", &mut warnings)
        .unwrap_or_else(|| id.to_string());
    let author = clean(
        m.author.as_deref(),
        MAX_AUTHOR_CHARS,
        "author",
        &mut warnings,
    );
    let license = clean(
        m.license.as_deref(),
        MAX_LICENSE_CHARS,
        "license",
        &mut warnings,
    );
    let description = clean(
        m.description.as_deref(),
        MAX_DESCRIPTION_CHARS,
        "description",
        &mut warnings,
    );
    PackInfo {
        id: id.to_string(),
        name,
        author,
        license,
        description,
        source,
        format_kind,
        valid: errors.is_empty(),
        errors,
        warnings,
    }
}

fn broken_info(
    id: &str,
    name: &str,
    source: PackSource,
    format_kind: PackFormatKind,
    error: String,
) -> PackInfo {
    PackInfo {
        id: id.to_string(),
        name: clean_text(name, MAX_NAME_CHARS)
            .0
            .unwrap_or_else(|| id.to_string()),
        author: None,
        license: None,
        description: None,
        source,
        format_kind,
        valid: false,
        errors: vec![error],
        warnings: Vec::new(),
    }
}

/// `rel` inside `root` (canonical), after `safe_relative_path` and
/// canonicalization: `None` if unsafe, missing, the root itself, outside the
/// root (symlinks are resolved first) or reaching a hidden name.
fn resolve_in_root(root: &Path, rel: &str) -> Option<PathBuf> {
    let full = root.join(safe_relative_path(rel)?);
    let canon = fs::canonicalize(full).ok()?;
    let inside = canon.strip_prefix(root).ok()?;
    let mut comps = inside.components().peekable();
    comps.peek()?;
    let plain = comps.all(|c| match c {
        Component::Normal(n) => !n.to_string_lossy().starts_with('.'),
        _ => false,
    });
    plain.then_some(canon)
}

/// Size of a regular file inside `root`, if servable by size.
fn file_len_in_root(root: &Path, rel: &str) -> Option<u64> {
    let canon = resolve_in_root(root, rel)?;
    let meta = fs::metadata(canon).ok()?;
    meta.is_file().then_some(meta.len())
}

fn disk_file_ok(root: &Path, rel: &str) -> bool {
    file_len_in_root(root, rel).is_some_and(|len| len <= MAX_ASSET_BYTES)
}

fn disk_probe(root: &Path, rel: &str) -> Option<SheetInfo> {
    let canon = resolve_in_root(root, rel)?;
    let f = fs::File::open(canon).ok()?;
    let meta = f.metadata().ok()?;
    if !meta.is_file() {
        return None;
    }
    let mut header = Vec::with_capacity(HEADER_BYTES as usize);
    f.take(HEADER_BYTES).read_to_end(&mut header).ok()?;
    Some(SheetInfo {
        len: meta.len(),
        dims: image_dims(&header),
    })
}

/// Read a small UTF-8 text file (a manifest) from inside `root`.
fn read_text(root: &Path, rel: &str, max: u64) -> Result<String, String> {
    let canon = resolve_in_root(root, rel)
        .ok_or_else(|| format!("{rel} cannot be read inside the pack folder"))?;
    let f = fs::File::open(&canon).map_err(|e| format!("cannot read {rel}: {e}"))?;
    let mut buf = Vec::new();
    f.take(max + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("cannot read {rel}: {e}"))?;
    if buf.len() as u64 > max {
        return Err(format!("{rel} is larger than {} KiB", max / 1024));
    }
    let text = String::from_utf8(buf).map_err(|_| format!("{rel} is not valid UTF-8"))?;
    Ok(text
        .strip_prefix('\u{feff}')
        .map(str::to_string)
        .unwrap_or(text))
}

fn is_file(p: &Path) -> bool {
    fs::metadata(p).is_ok_and(|m| m.is_file())
}

/// Load one disk pack folder (`dir_name` inside a source directory).
fn load_disk_pack(path: &Path, dir_name: &str, source: PackSource) -> Candidate {
    let guessed_id = if is_valid_pack_id(dir_name) {
        dir_name.to_string()
    } else {
        format!("{}:{dir_name}", source_label(source))
    };
    let broken = |kind: PackFormatKind, error: String| Candidate {
        dir_name: dir_name.to_string(),
        rank: 2,
        entry: Entry {
            info: broken_info(&guessed_id, dir_name, source, kind, error),
            manifest: None,
            store: Store::None,
        },
    };
    let root = match fs::canonicalize(path) {
        Ok(r) => r,
        Err(e) => {
            return broken(
                PackFormatKind::Native,
                format!("cannot open the pack folder: {e}"),
            )
        }
    };
    let file_ok = |rel: &str| disk_file_ok(&root, rel);

    if is_file(&root.join("pack.json")) {
        let text = match read_text(&root, "pack.json", MAX_MANIFEST_BYTES) {
            Ok(t) => t,
            Err(e) => return broken(PackFormatKind::Native, e),
        };
        let m = match parse_manifest(&text) {
            Ok(m) => m,
            Err(e) => return broken(PackFormatKind::Native, format!("pack.json: {e}")),
        };
        let (errors, mut warnings) = validate_manifest(&m, &file_ok);
        if m.license.as_deref().is_none_or(|l| l.trim().is_empty()) {
            warnings.push("no license declared in pack.json".into());
        }
        let info = make_info(&m, &m.id, source, PackFormatKind::Native, errors, warnings);
        let manifest = info.valid.then_some(m);
        return Candidate {
            dir_name: dir_name.to_string(),
            rank: 0,
            entry: Entry {
                info,
                manifest,
                store: Store::Dir(root),
            },
        };
    }

    if is_file(&root.join("pet.json")) {
        let text = match read_text(&root, "pet.json", MAX_MANIFEST_BYTES) {
            Ok(t) => t,
            Err(e) => return broken(PackFormatKind::Petdex, e),
        };
        let probe = |rel: &str| disk_probe(&root, rel);
        let conv = match convert_petdex(&text, dir_name, &probe) {
            Ok(c) => c,
            Err(e) => return broken(PackFormatKind::Petdex, format!("pet.json: {e}")),
        };
        let (errors, warnings) = validate_manifest(&conv.manifest, &file_ok);
        let warnings = conv.warnings.into_iter().chain(warnings).collect();
        let m = conv.manifest;
        let info = make_info(&m, &m.id, source, PackFormatKind::Petdex, errors, warnings);
        let manifest = info.valid.then_some(m);
        return Candidate {
            dir_name: dir_name.to_string(),
            rank: 1,
            entry: Entry {
                info,
                manifest,
                store: Store::Dir(root),
            },
        };
    }

    broken(
        PackFormatKind::Native,
        "no pack.json (or Petdex pet.json) in this folder".into(),
    )
}

/// Every pack folder directly inside `dir` (hidden names skipped, sorted, capped).
fn scan_dir(dir: &Path, source: PackSource) -> Vec<Candidate> {
    let read = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!("packs: cannot read {}: {e}", dir.display());
            }
            return Vec::new();
        }
    };
    let mut dirs: Vec<(String, PathBuf)> = read
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            (!name.starts_with('.')).then(|| (name, e.path()))
        })
        .filter(|(_, p)| fs::metadata(p).is_ok_and(|m| m.is_dir()))
        .collect();
    dirs.sort();
    if dirs.len() > MAX_PACK_DIRS {
        log::warn!(
            "packs: {} has more than {MAX_PACK_DIRS} folders; the rest are ignored",
            dir.display()
        );
        dirs.truncate(MAX_PACK_DIRS);
    }
    dirs.iter()
        .map(|(name, path)| load_disk_pack(path, name, source))
        .collect()
}

/// Demote `c` to its unaddressable listing id with an error.
fn demote(c: &mut Candidate, source: PackSource, error: String) {
    c.entry.info.id = format!("{}:{}", source_label(source), c.dir_name);
    c.entry.info.valid = false;
    c.entry.info.errors.push(error);
    c.entry.manifest = None;
    c.entry.store = Store::None;
}

/// Reserved-id and duplicate-id rules within one source.
fn settle_source(mut cands: Vec<Candidate>, source: PackSource) -> Vec<Candidate> {
    for c in cands
        .iter_mut()
        .filter(|c| c.entry.info.id == PLACEHOLDER_ID)
    {
        demote(
            c,
            source,
            format!("the id \"{PLACEHOLDER_ID}\" is reserved for the bundled pack; give this pack another id (or folder name)"),
        );
    }
    let mut order: Vec<usize> = (0..cands.len()).collect();
    order.sort_by(|&a, &b| {
        (cands[a].rank, &cands[a].dir_name).cmp(&(cands[b].rank, &cands[b].dir_name))
    });
    let mut owner: BTreeMap<String, String> = BTreeMap::new();
    for i in order {
        let id = cands[i].entry.info.id.clone();
        if !is_valid_pack_id(&id) {
            continue; // already a listing id
        }
        match owner.get(&id) {
            None => {
                owner.insert(id, cands[i].dir_name.clone());
            }
            Some(winner) => {
                let msg = format!(
                    "duplicate pack id \"{id}\": the pack in folder \"{winner}\" already uses it"
                );
                demote(&mut cands[i], source, msg);
            }
        }
    }
    cands
}

fn load_bundled(bundled: BundledPack) -> Entry {
    let files: BTreeMap<String, &'static [u8]> = bundled
        .files
        .into_iter()
        .filter_map(|(path, bytes)| {
            let p = path.trim_start_matches("./").trim_start_matches('/');
            Some((normalized_key(p)?, bytes))
        })
        .collect();
    let broken = |error: String| Entry {
        info: broken_info(
            PLACEHOLDER_ID,
            "Placeholder",
            PackSource::Bundled,
            PackFormatKind::Native,
            error,
        ),
        manifest: None,
        store: Store::None,
    };
    let Some(bytes) = files.get("pack.json") else {
        return broken("the bundled pack has no pack.json".into());
    };
    let Ok(text) = std::str::from_utf8(bytes) else {
        return broken("the bundled pack.json is not valid UTF-8".into());
    };
    let m = match parse_manifest(text.strip_prefix('\u{feff}').unwrap_or(text)) {
        Ok(m) => m,
        Err(e) => return broken(format!("pack.json: {e}")),
    };
    let file_ok = |rel: &str| {
        files
            .get(rel)
            .is_some_and(|b| b.len() as u64 <= MAX_ASSET_BYTES)
    };
    let (mut errors, mut warnings) = validate_manifest(&m, &file_ok);
    if m.id != PLACEHOLDER_ID {
        errors.push(format!(
            "the bundled pack must use the id \"{PLACEHOLDER_ID}\" (found {:?})",
            m.id
        ));
    }
    if m.license.as_deref().is_none_or(|l| l.trim().is_empty()) {
        warnings.push("no license declared in pack.json".into());
    }
    let info = make_info(
        &m,
        PLACEHOLDER_ID,
        PackSource::Bundled,
        PackFormatKind::Native,
        errors,
        warnings,
    );
    let manifest = info.valid.then_some(m);
    Entry {
        info,
        manifest,
        store: Store::Memory(files),
    }
}

fn empty_manifest() -> PackManifest {
    PackManifest {
        format: PACK_FORMAT,
        id: PLACEHOLDER_ID.to_string(),
        name: String::new(),
        author: None,
        license: None,
        description: None,
        rendering: Rendering::Smooth,
        animations: BTreeMap::new(),
        fallbacks: BTreeMap::new(),
        palettes: BTreeMap::new(),
    }
}

impl PackRegistry {
    /// Discover packs: the bundled placeholder (always present, id reserved),
    /// then every `<user_dir>/<subdir>/pack.json`, then (when given) every
    /// `<dev_dir>/<subdir>/pack.json`; dev overrides user on id collision; packs
    /// claiming the `placeholder` id from disk are rejected with an error entry.
    /// A subdir with `pet.json` and no `pack.json` is converted as a Petdex pack (SPEC §7.2).
    /// Never panics on bad input; problems become `PackInfo.errors`.
    ///
    /// Missing directories are not errors. Hidden folders are skipped; a visible
    /// folder with neither manifest is listed with an error. At most
    /// `MAX_PACK_DIRS` folders are read per directory. See the module docs for
    /// how listing ids stay unique.
    pub fn load(bundled: BundledPack, user_dir: Option<&Path>, dev_dir: Option<&Path>) -> Self {
        let bundled = load_bundled(bundled);
        let user = user_dir
            .map(|d| settle_source(scan_dir(d, PackSource::User), PackSource::User))
            .unwrap_or_default();
        let mut dev = dev_dir
            .map(|d| settle_source(scan_dir(d, PackSource::Dev), PackSource::Dev))
            .unwrap_or_default();

        let mut disk: Vec<Entry> = Vec::new();
        for u in user {
            let id = &u.entry.info.id;
            let shadow = is_valid_pack_id(id)
                .then(|| dev.iter_mut().find(|d| d.entry.info.id == *id))
                .flatten();
            match shadow {
                Some(d) => d.entry.info.warnings.push(format!(
                    "overrides the user pack with the same id (folder \"{}\")",
                    u.dir_name
                )),
                None => disk.push(u.entry),
            }
        }
        disk.extend(dev.into_iter().map(|d| d.entry));
        disk.sort_by(|a, b| {
            (a.info.name.to_lowercase(), &a.info.id).cmp(&(b.info.name.to_lowercase(), &b.info.id))
        });

        let mut entries = Vec::with_capacity(disk.len() + 1);
        entries.push(bundled);
        entries.extend(disk);
        for e in entries.iter().filter(|e| !e.info.valid) {
            log::warn!(
                "packs: {} is not usable: {}",
                e.info.id,
                e.info.errors.join("; ")
            );
        }
        PackRegistry { entries }
    }

    /// All packs (valid and invalid), bundled first then by name.
    pub fn list(&self) -> Vec<PackInfo> {
        self.entries.iter().map(|e| e.info.clone()).collect()
    }

    pub fn is_valid(&self, id: &str) -> bool {
        self.find_valid(id).is_some()
    }

    /// The listing entry with this id, valid or not.
    pub fn info(&self, id: &str) -> Option<&PackInfo> {
        self.entries
            .iter()
            .find(|e| e.info.id == id)
            .map(|e| &e.info)
    }

    /// Why `id` cannot be activated (for `PackUnavailable`), or `None` if it can.
    pub fn unavailable_reason(&self, id: &str) -> Option<String> {
        match self.info(id) {
            None => Some("no pack with this id is installed".into()),
            Some(i) if i.valid => None,
            Some(i) => Some(
                i.errors
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "the pack is not valid".into()),
            ),
        }
    }

    /// The manifest of a valid pack.
    pub fn manifest(&self, id: &str) -> Option<&PackManifest> {
        self.find_valid(id).and_then(|e| e.manifest.as_ref())
    }

    fn find_valid(&self, id: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| e.info.id == id && e.info.valid && e.manifest.is_some())
    }

    /// Resolve `id` for rendering (falling back to the placeholder if `id` is
    /// unknown or invalid, recording `fallback_from`). Resolves exactly
    /// `render_keys(table, manifest)`; `url_prefix` is e.g. `pack://localhost/` and URLs are
    /// `{url_prefix}{packId}/{sheet}` with each path segment percent-encoded.
    ///
    /// Palettes are passed through `sanitize_palette` (invalid palette ids are
    /// dropped). If even the placeholder is unusable (never the case for a
    /// shipped build), every key resolves to `null`, so the renderer draws its
    /// built-in static companion. A missing trailing `/` on `url_prefix` is added.
    pub fn resolve_pack(&self, id: &str, table: &UnlockTable, url_prefix: &str) -> ResolvedPack {
        let (entry, fallback_from) = match self.find_valid(id) {
            Some(e) => (Some(e), None),
            None => {
                let from = (id != PLACEHOLDER_ID).then(|| id.to_string());
                (self.find_valid(PLACEHOLDER_ID), from)
            }
        };
        let Some(entry) = entry else {
            let info = self.info(PLACEHOLDER_ID).cloned().unwrap_or_else(|| {
                broken_info(
                    PLACEHOLDER_ID,
                    "Placeholder",
                    PackSource::Bundled,
                    PackFormatKind::Native,
                    "the bundled pack is missing".into(),
                )
            });
            let animations = render_keys(table, &empty_manifest())
                .into_iter()
                .map(|k| (k, None))
                .collect();
            return ResolvedPack {
                info,
                rendering: Rendering::Smooth,
                animations,
                palettes: BTreeMap::new(),
                fallback_from,
            };
        };
        let m = entry
            .manifest
            .as_ref()
            .expect("find_valid returns entries with a manifest");
        let mut prefix = url_prefix.to_string();
        if !prefix.is_empty() && !prefix.ends_with('/') {
            prefix.push('/');
        }
        let pack_id = &entry.info.id;
        let animations = render_keys(table, m)
            .into_iter()
            .map(|key| {
                let resolved = resolve(m, &key).and_then(|(source_key, a)| {
                    let rel = normalized_key(&a.sheet)?;
                    Some(ResolvedAnimation {
                        key: key.clone(),
                        source_key,
                        url: format!(
                            "{prefix}{}/{}",
                            encode_url_path(pack_id),
                            encode_url_path(&rel)
                        ),
                        frame_width: a.frame_width,
                        frame_height: a.frame_height,
                        frames: a.frames,
                        fps: a.fps,
                        looping: a.looping,
                        row: a.row,
                        column: a.column,
                        sheet_columns: a.sheet_columns.unwrap_or(a.column.saturating_add(a.frames)),
                        sheet_rows: a.sheet_rows.unwrap_or(a.row.saturating_add(1)),
                        still_frame: a.still_frame.unwrap_or(0),
                        frame_durations_ms: a.frame_durations_ms.clone(),
                    })
                });
                (key, resolved)
            })
            .collect();
        let palettes = m
            .palettes
            .iter()
            .filter(|(pid, _)| is_valid_pack_id(pid))
            .map(|(pid, p)| (pid.clone(), sanitize_palette(p).0))
            .collect();
        ResolvedPack {
            info: entry.info.clone(),
            rendering: m.rendering,
            animations,
            palettes,
            fallback_from,
        }
    }

    /// Bytes and MIME type of a file inside a known pack, or `None` if the pack is
    /// unknown, the path is unsafe, escapes the pack directory (after
    /// canonicalization, so symlinks out are refused), has a disallowed type, or
    /// exceeds `MAX_ASSET_BYTES`.
    ///
    /// Only valid packs are served. Bundled files come from memory. For disk
    /// packs the canonical target must also stay clear of hidden names and have
    /// an allowed type itself.
    pub fn read_file(&self, pack_id: &str, rel_path: &str) -> Option<(Vec<u8>, &'static str)> {
        let entry = self.find_valid(pack_id)?;
        let key = normalized_key(rel_path)?;
        let mime = mime_for(Path::new(&key))?;
        match &entry.store {
            Store::Memory(files) => {
                let bytes = files.get(&key)?;
                (bytes.len() as u64 <= MAX_ASSET_BYTES).then(|| (bytes.to_vec(), mime))
            }
            Store::Dir(root) => {
                let canon = resolve_in_root(root, &key)?;
                mime_for(&canon)?;
                let f = fs::File::open(&canon).ok()?;
                let meta = f.metadata().ok()?;
                if !meta.is_file() || meta.len() > MAX_ASSET_BYTES {
                    return None;
                }
                let mut buf = Vec::with_capacity(meta.len() as usize);
                f.take(MAX_ASSET_BYTES + 1).read_to_end(&mut buf).ok()?;
                (buf.len() as u64 <= MAX_ASSET_BYTES).then_some((buf, mime))
            }
            Store::None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PackRegistry>();
    }

    #[test]
    fn default_registry_never_panics() {
        let r = PackRegistry::default();
        assert!(r.list().is_empty());
        assert!(!r.is_valid(PLACEHOLDER_ID));
        assert_eq!(r.read_file(PLACEHOLDER_ID, "pack.json"), None);
        let table = UnlockTable {
            format: 1,
            rules: crate::rewards::Rules {
                points_per_active_minute: 1,
                completion_bonus: 5,
                long_session_minutes: 45,
            },
            unlocks: vec![],
        };
        let rp = r.resolve_pack("x", &table, "pack://localhost/");
        assert_eq!(rp.fallback_from.as_deref(), Some("x"));
        assert_eq!(rp.info.id, PLACEHOLDER_ID);
        assert!(!rp.info.valid);
        assert!(rp.animations.values().all(Option::is_none));
        assert!(rp.animations.contains_key("idle"));
    }

    #[test]
    fn store_debug_is_compact() {
        let mut m = BTreeMap::new();
        m.insert("a.png".to_string(), &b"xyz"[..]);
        assert_eq!(format!("{:?}", Store::Memory(m)), "Memory(1 files)");
    }
}
