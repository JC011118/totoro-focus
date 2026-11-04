//! One schema-versioned JSON file, replaced atomically (temp file + fsync +
//! rename + dir fsync). Holds settings, the active-session checkpoint, history
//! and friendship together so they stay consistent. See docs/SPEC.md §9.
//!
//! Files in the app data directory:
//! - `state.json`: the state (pretty-printed JSON, `schemaVersion` first).
//! - `state.json.tmp`: scratch file for the atomic write. A leftover from a
//!   crash before the rename is ignored by `load` and overwritten by the next save.
//! - `state.prev.json`: copy of `state.json` taken on the first save of each
//!   process (`state.prev.json.tmp` is its scratch file).
//! - `state.corrupt-<stamp>[-N].json`: a corrupt `state.json`, moved aside.
//! - `state.newer-<stamp>[-N].json`: copy of a file written by a newer app version.
//!   Written once per distinct content: a later load of the same bytes reuses it.
//!
//! Nothing here ever deletes one of these files.

use std::borrow::Cow;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::clock::Wall;
use crate::history::SessionRecord;
use crate::rewards::Friendship;
use crate::settings::Settings;
use crate::timer::PersistedSession;

pub const SCHEMA_VERSION: u32 = 1;
pub const STATE_FILE: &str = "state.json";
/// Scratch file of the atomic write; renamed over `state.json`.
pub const TMP_FILE: &str = "state.json.tmp";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PersistedState {
    pub schema_version: u32,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub active_session: Option<PersistedSession>,
    #[serde(default)]
    pub history: Vec<SessionRecord>,
    #[serde(default)]
    pub friendship: Friendship,
}

impl Default for PersistedState {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            settings: Settings::default(),
            active_session: None,
            history: Vec::new(),
            friendship: Friendship::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LoadNote {
    /// No file yet: defaults.
    Missing,
    Loaded,
    Migrated {
        from: u32,
    },
    /// Unreadable or invalid JSON: moved aside to `backup`, defaults used.
    /// (If it could not be moved, `backup` is the untouched `state.json`, `error`
    /// says so, and the store is read-only for this run.)
    CorruptBackedUp {
        backup: PathBuf,
        error: String,
    },
    /// Written by a newer app version: copied aside to `backup`, defaults used, and
    /// the store is read-only for this run (never overwrite newer data with lower totals).
    /// If an earlier run already left a byte-identical `state.newer-*.json`, no new
    /// copy is written and `backup` is that file (so repeated launches of an older
    /// build do not pile up copies). If the copy failed, `backup` is the untouched
    /// original file.
    NewerSchemaReadOnly {
        backup: PathBuf,
        version: u32,
    },
    /// `state.json` was corrupt; `state.prev.json` was loaded instead and `state` holds it.
    ///
    /// Normally the corrupt file was moved to `backup` and `state.json` was restored
    /// from `state.prev.json`; the store stays writable. If the corrupt file could not
    /// be moved aside, `backup` is the untouched `state.json`, nothing is restored,
    /// `error` says so, and **the store is read-only for this run** (every save fails
    /// with `StoreError::ReadOnly`), so that file is never overwritten. The note alone
    /// does not tell these apart: check `Store::is_read_only()` after every load.
    RecoveredFromPrevious {
        backup: PathBuf,
        error: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadReport {
    pub state: PersistedState,
    pub note: LoadNote,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid state file: {0}")]
    Invalid(String),
    #[error("state file schema {0} is newer than this app supports")]
    NewerSchema(u32),
    /// See `Store::is_read_only`: a newer-schema file, or a corrupt file that could
    /// not be moved aside, is protected from being overwritten for this run.
    #[error(
        "saving is disabled for this run: the state file was written by a newer version \
         or could not be moved aside"
    )]
    ReadOnly,
}

pub const PREV_FILE: &str = "state.prev.json";
/// Scratch file used while copying `state.json` to `state.prev.json`.
const PREV_TMP_FILE: &str = "state.prev.json.tmp";
const CORRUPT_PREFIX: &str = "state.corrupt-";
const NEWER_PREFIX: &str = "state.newer-";
/// Upper bound for the numeric suffix of backup names (`…-1.json` … `…-N.json`).
const MAX_BACKUP_SUFFIX: u32 = 9_999;
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

#[derive(Debug)]
pub struct Store {
    dir: PathBuf,
    /// Set after the first save of this process copied `state.json` to `state.prev.json`.
    prev_copied: AtomicBool,
    /// Set by `load` when the file came from a newer schema (or a corrupt file
    /// could not be moved aside). Never cleared within a run.
    read_only: AtomicBool,
    /// Serializes saves: they share `state.json.tmp`.
    save_lock: Mutex<()>,
}

/// The result of reading and parsing one state file.
enum FileRead {
    Missing,
    Parsed {
        bytes: Vec<u8>,
        state: Box<PersistedState>,
        from: u32,
    },
    Newer {
        bytes: Vec<u8>,
        version: u32,
    },
    /// Unreadable, invalid JSON, or the wrong shape.
    Bad {
        error: String,
    },
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            prev_copied: AtomicBool::new(false),
            read_only: AtomicBool::new(false),
            save_lock: Mutex::new(()),
        }
    }

    /// True once a `load` this run found a newer-schema file, or a corrupt
    /// `state.json` it could not move aside. Never cleared within a run; every
    /// `save` then fails with `StoreError::ReadOnly`. Check it after every load.
    pub fn is_read_only(&self) -> bool {
        self.read_only.load(Ordering::SeqCst)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(STATE_FILE)
    }

    /// Path of the copy taken on the first save of each process.
    pub fn prev_path(&self) -> PathBuf {
        self.dir.join(PREV_FILE)
    }

    /// Load the state file. A corrupt file is renamed to
    /// `state.corrupt-<YYYYMMDDTHHMMSSZ>.json` (numeric suffix if taken), then
    /// `state.prev.json` is tried, then defaults. A newer-schema file is copied to
    /// `state.newer-<stamp>.json` (unless a byte-identical copy already exists),
    /// defaults are returned and the store becomes read-only. Older schemas are
    /// migrated. Never deletes anything.
    ///
    /// `state.json.tmp` is never read. A missing file creates nothing (not even the
    /// directory). The only file `load` writes besides backups: after moving a corrupt
    /// `state.json` aside and finding a usable `state.prev.json`, it restores
    /// `state.json` from that copy (atomically), so a crash before the next save
    /// cannot lose the recovered data. A `state.prev.json` with a newer schema is
    /// restored the same way and then handled like a newer `state.json` (read-only).
    ///
    /// **Callers must check `is_read_only()` after every load**, not only on
    /// `NewerSchemaReadOnly`: a corrupt file that cannot be moved aside also makes the
    /// store read-only (so that file is never overwritten), and its note is
    /// `CorruptBackedUp` or `RecoveredFromPrevious` with `backup` = `state.json`.
    pub fn load(&self, now: Wall) -> LoadReport {
        let path = self.path();
        let (state, note) = match read_state_file(&path) {
            FileRead::Missing => (PersistedState::default(), LoadNote::Missing),
            FileRead::Parsed { state, from, .. } if from == SCHEMA_VERSION => {
                (*state, LoadNote::Loaded)
            }
            FileRead::Parsed { state, from, .. } => {
                log::info!(
                    "store: migrated {} from schema {from} to {SCHEMA_VERSION}",
                    path.display()
                );
                (*state, LoadNote::Migrated { from })
            }
            FileRead::Newer { bytes, version } => self.back_up_newer(&path, &bytes, version, now),
            FileRead::Bad { error } => self.recover_from_corrupt(&path, error, now),
        };
        LoadReport { state, note }
    }

    /// Atomic save: create dir if needed; on the first save of this process copy an
    /// existing `state.json` to `state.prev.json`; write `state.json.tmp`, fsync,
    /// rename over `state.json`, fsync the directory (best effort). `ReadOnly` error
    /// when read-only.
    ///
    /// Always writes `schemaVersion = SCHEMA_VERSION`. A non-finite widget position
    /// (which JSON cannot represent) is written as `null` (unset) rather than producing
    /// a file `load` would reject. Concurrent saves are serialized; the last one wins.
    pub fn save(&self, state: &PersistedState) -> Result<(), StoreError> {
        let _guard = self
            .save_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.is_read_only() {
            return Err(StoreError::ReadOnly);
        }
        let bytes = encode(state)?;
        fs::create_dir_all(&self.dir)?;
        if !self.prev_copied.swap(true, Ordering::SeqCst) {
            self.copy_current_to_prev();
        }
        atomic_write(&self.dir, STATE_FILE, TMP_FILE, &bytes)?;
        Ok(())
    }

    /// First save of the process: keep the state as the previous run left it. A
    /// failure is logged and does not block the save (it is not retried this run).
    fn copy_current_to_prev(&self) {
        let current = self.path();
        match fs::read(&current) {
            Ok(bytes) => {
                if let Err(e) = atomic_write(&self.dir, PREV_FILE, PREV_TMP_FILE, &bytes) {
                    log::warn!(
                        "store: could not copy {} to {PREV_FILE}: {e}",
                        current.display()
                    );
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => log::warn!(
                "store: could not read {} to copy it to {PREV_FILE}: {e}",
                current.display()
            ),
        }
    }

    fn back_up_newer(
        &self,
        source: &Path,
        bytes: &[u8],
        version: u32,
        now: Wall,
    ) -> (PersistedState, LoadNote) {
        self.read_only.store(true, Ordering::SeqCst);
        // An older build launched repeatedly against the same newer file must not
        // pile up identical copies (nothing here ever deletes them).
        if let Some(existing) = find_identical_backup(&self.dir, NEWER_PREFIX, bytes) {
            log::warn!(
                "store: {} has schema {version} (newer than {SCHEMA_VERSION}); already \
                 backed up as {}; using defaults and saving is disabled for this run",
                source.display(),
                existing.display()
            );
            return (
                PersistedState::default(),
                LoadNote::NewerSchemaReadOnly {
                    backup: existing,
                    version,
                },
            );
        }
        let backup = match write_new_backup(&self.dir, NEWER_PREFIX, now, bytes) {
            Ok(p) => p,
            Err(e) => {
                log::warn!(
                    "store: could not copy {} aside: {e}; it is left untouched",
                    source.display()
                );
                source.to_path_buf()
            }
        };
        log::warn!(
            "store: {} has schema {version} (newer than {SCHEMA_VERSION}); backed up to {}; \
             using defaults and saving is disabled for this run",
            source.display(),
            backup.display()
        );
        (
            PersistedState::default(),
            LoadNote::NewerSchemaReadOnly { backup, version },
        )
    }

    fn recover_from_corrupt(
        &self,
        path: &Path,
        mut error: String,
        now: Wall,
    ) -> (PersistedState, LoadNote) {
        let (backup, moved) = match move_aside(path, &self.dir, CORRUPT_PREFIX, now) {
            Ok(backup) => (backup, true),
            Err(e) => {
                self.read_only.store(true, Ordering::SeqCst);
                error = format!(
                    "{error}; could not move it aside ({e}), so saving is disabled for this run"
                );
                (path.to_path_buf(), false)
            }
        };
        log::warn!(
            "store: {} is unusable ({error}); kept as {}",
            path.display(),
            backup.display()
        );

        let prev = self.prev_path();
        match read_state_file(&prev) {
            FileRead::Parsed { bytes, state, .. } => {
                if moved {
                    self.restore_state_file(&bytes);
                }
                log::warn!("store: recovered from {}", prev.display());
                (*state, LoadNote::RecoveredFromPrevious { backup, error })
            }
            FileRead::Newer { bytes, version } => {
                // Never let an older app replace newer data: same as a newer state.json.
                if moved {
                    self.restore_state_file(&bytes);
                }
                self.back_up_newer(&prev, &bytes, version, now)
            }
            FileRead::Missing => (
                PersistedState::default(),
                LoadNote::CorruptBackedUp { backup, error },
            ),
            FileRead::Bad { error: prev_error } => {
                log::warn!(
                    "store: {} is unusable too ({prev_error}); using defaults",
                    prev.display()
                );
                (
                    PersistedState::default(),
                    LoadNote::CorruptBackedUp { backup, error },
                )
            }
        }
    }

    /// Put the recovered previous copy back as `state.json` (the corrupt one was moved).
    fn restore_state_file(&self, bytes: &[u8]) {
        if let Err(e) = atomic_write(&self.dir, STATE_FILE, TMP_FILE, bytes) {
            log::warn!("store: could not restore {STATE_FILE} from {PREV_FILE}: {e}");
        }
    }
}

/// Upgrade a parsed JSON document of any known schema to the current one.
/// `schemaVersion` missing (or `null`) → treated as 1. Newer → `StoreError::NewerSchema`.
/// Not an object, a `schemaVersion` that is not a whole number in `1..=u32::MAX`, or
/// a document that does not match the schema → `StoreError::Invalid`. Unknown fields
/// are ignored. Returns the state and the version it was read as.
pub fn migrate(mut value: serde_json::Value) -> Result<(PersistedState, u32), StoreError> {
    let obj = value
        .as_object_mut()
        .ok_or_else(|| StoreError::Invalid("the state file is not a JSON object".into()))?;
    let from = match obj.get("schemaVersion") {
        None | Some(serde_json::Value::Null) => 1,
        Some(v) => v
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n >= 1)
            .ok_or_else(|| StoreError::Invalid(format!("unsupported schemaVersion {v}")))?,
    };
    if from > SCHEMA_VERSION {
        return Err(StoreError::NewerSchema(from));
    }
    // Upgrade steps go here, one per version, e.g. `if from < 2 { v1_to_v2(obj)?; }`.
    obj.insert("schemaVersion".into(), SCHEMA_VERSION.into());
    let state = serde_json::from_value::<PersistedState>(value)
        .map_err(|e| StoreError::Invalid(e.to_string()))?;
    Ok((state, from))
}

fn read_state_file(path: &Path) -> FileRead {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return FileRead::Missing,
        Err(e) => {
            return FileRead::Bad {
                error: format!("unreadable: {e}"),
            }
        }
    };
    match parse(&bytes) {
        Ok((state, from)) => FileRead::Parsed {
            bytes,
            state: Box::new(state),
            from,
        },
        Err(StoreError::NewerSchema(version)) => FileRead::Newer { bytes, version },
        Err(e) => FileRead::Bad {
            error: e.to_string(),
        },
    }
}

fn parse(bytes: &[u8]) -> Result<(PersistedState, u32), StoreError> {
    let bytes = bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes);
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| StoreError::Invalid(e.to_string()))?;
    migrate(value)
}

fn encode(state: &PersistedState) -> Result<Vec<u8>, StoreError> {
    let mut state = Cow::Borrowed(state);
    if state.schema_version != SCHEMA_VERSION {
        state.to_mut().schema_version = SCHEMA_VERSION;
    }
    if let Some(p) = state.settings.widget_position {
        if !(p.x.is_finite() && p.y.is_finite()) {
            log::warn!("store: dropping non-finite widget position {p:?}");
            state.to_mut().settings.widget_position = None;
        }
    }
    let mut bytes = serde_json::to_vec_pretty(state.as_ref())
        .map_err(|e| StoreError::Invalid(e.to_string()))?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Write `tmp_name`, fsync it, rename it over `file_name`, fsync the directory.
fn atomic_write(dir: &Path, file_name: &str, tmp_name: &str, bytes: &[u8]) -> io::Result<()> {
    let tmp = dir.join(tmp_name);
    {
        // `create` truncates a leftover scratch file from an earlier crash.
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, dir.join(file_name))?;
    sync_dir(dir);
    Ok(())
}

/// Best effort: makes the rename durable where the filesystem needs it.
fn sync_dir(dir: &Path) {
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
}

/// `YYYYMMDDTHHMMSSZ` (UTC, whole seconds), used in backup file names.
pub fn backup_stamp(now: Wall) -> String {
    now.format("%Y%m%dT%H%M%SZ").to_string()
}

/// `<prefix><stamp>.json`, then `<prefix><stamp>-1.json`, `-2`, … .
fn backup_candidates(dir: &Path, prefix: &str, now: Wall) -> impl Iterator<Item = PathBuf> {
    let dir = dir.to_path_buf();
    let stamp = backup_stamp(now);
    let prefix = prefix.to_string();
    std::iter::once(None)
        .chain((1..=MAX_BACKUP_SUFFIX).map(Some))
        .map(move |n| match n {
            None => dir.join(format!("{prefix}{stamp}.json")),
            Some(n) => dir.join(format!("{prefix}{stamp}-{n}.json")),
        })
}

fn no_free_name() -> io::Error {
    io::Error::new(io::ErrorKind::AlreadyExists, "no free backup file name")
}

/// Anything at `path`, including a dangling symlink, or a path we cannot inspect.
fn is_taken(path: &Path) -> bool {
    !matches!(fs::symlink_metadata(path), Err(e) if e.kind() == io::ErrorKind::NotFound)
}

/// Rename `path` to the first free backup name. Returns the new path.
fn move_aside(path: &Path, dir: &Path, prefix: &str, now: Wall) -> io::Result<PathBuf> {
    let target = backup_candidates(dir, prefix, now)
        .find(|p| !is_taken(p))
        .ok_or_else(no_free_name)?;
    fs::rename(path, &target)?;
    sync_dir(dir);
    Ok(target)
}

/// An existing backup `<prefix>*.json` in `dir` whose content is exactly `bytes`:
/// the first by file name, so the answer is stable across runs. Only regular files
/// count (not symlinks or directories); entries that cannot be inspected or read
/// are skipped, and an unreadable directory finds nothing. Sizes are compared
/// before any content is read.
fn find_identical_backup(dir: &Path, prefix: &str, bytes: &[u8]) -> Option<PathBuf> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            log::debug!("store: could not list {} for backups: {e}", dir.display());
            return None;
        }
    };
    let len = u64::try_from(bytes.len()).ok()?;
    let mut candidates: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(prefix) && name.ends_with(".json")
        })
        .map(|entry| entry.path())
        .filter(|path| fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && m.len() == len))
        .collect();
    candidates.sort();
    candidates
        .into_iter()
        .find(|path| fs::read(path).is_ok_and(|content| content == bytes))
}

/// Write `bytes` to the first free backup name (never replacing an existing file).
fn write_new_backup(dir: &Path, prefix: &str, now: Wall, bytes: &[u8]) -> io::Result<PathBuf> {
    for candidate in backup_candidates(dir, prefix, now) {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut file) => {
                file.write_all(bytes)?;
                file.sync_all()?;
                sync_dir(dir);
                return Ok(candidate);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {} // taken: try the next name
            Err(e) => return Err(e),
        }
    }
    Err(no_free_name())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn backup_stamp_is_utc_whole_seconds() {
        let at = Utc.with_ymd_and_hms(2026, 3, 2, 9, 5, 7).unwrap()
            + chrono::Duration::milliseconds(987);
        assert_eq!(backup_stamp(at), "20260302T090507Z");
    }

    #[test]
    fn backup_candidates_start_plain_then_number() {
        let at = Utc.with_ymd_and_hms(2026, 3, 2, 9, 0, 0).unwrap();
        let names: Vec<String> = backup_candidates(Path::new("/d"), CORRUPT_PREFIX, at)
            .take(3)
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "state.corrupt-20260302T090000Z.json",
                "state.corrupt-20260302T090000Z-1.json",
                "state.corrupt-20260302T090000Z-2.json",
            ]
        );
        assert_eq!(
            backup_candidates(Path::new("/d"), NEWER_PREFIX, at).count(),
            MAX_BACKUP_SUFFIX as usize + 1
        );
    }

    #[test]
    fn encode_forces_current_schema_and_trailing_newline() {
        let state = PersistedState {
            schema_version: 0,
            ..PersistedState::default()
        };
        let bytes = encode(&state).unwrap();
        assert_eq!(bytes.last(), Some(&b'\n'));
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["schemaVersion"], SCHEMA_VERSION);
    }

    #[test]
    fn parse_strips_a_utf8_bom() {
        let mut bytes = UTF8_BOM.to_vec();
        bytes.extend_from_slice(br#"{"schemaVersion":1}"#);
        let (state, from) = parse(&bytes).unwrap();
        assert_eq!(from, 1);
        assert_eq!(state, PersistedState::default());
    }

    #[test]
    fn store_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Store>();
    }
}
