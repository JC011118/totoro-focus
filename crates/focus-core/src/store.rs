//! One schema-versioned JSON file, replaced atomically (temp file + fsync +
//! rename + dir fsync). Holds settings, the active-session checkpoint, history
//! and friendship together so they stay consistent. See docs/SPEC.md §9.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::clock::Wall;
use crate::history::SessionRecord;
use crate::rewards::Friendship;
use crate::settings::Settings;
use crate::timer::PersistedSession;

pub const SCHEMA_VERSION: u32 = 1;
pub const STATE_FILE: &str = "state.json";

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
    Migrated { from: u32 },
    /// Unreadable or invalid JSON: moved aside to `backup`, defaults used.
    CorruptBackedUp { backup: PathBuf, error: String },
    /// Written by a newer app version: moved aside to `backup`, defaults used.
    NewerSchemaBackedUp { backup: PathBuf, version: u32 },
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
}

#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(STATE_FILE)
    }

    /// Load the state file (creating nothing). Corrupt or newer-schema files are
    /// renamed to `state.corrupt-<YYYYMMDDTHHMMSSZ>.json` next to it (with a
    /// numeric suffix if that exists) and defaults are returned; never deleted.
    pub fn load(&self, now: Wall) -> LoadReport {
        let _ = now;
        todo!("store: load")
    }

    /// Atomic save: create dir if needed, write `state.json.tmp`, fsync, rename
    /// over `state.json`, fsync the directory (best effort on platforms without it).
    pub fn save(&self, state: &PersistedState) -> Result<(), StoreError> {
        let _ = state;
        todo!("store: save")
    }
}

/// Upgrade a parsed JSON document of any known schema to the current one.
/// `schemaVersion` missing → treated as 1. Newer → `StoreError::NewerSchema`.
pub fn migrate(value: serde_json::Value) -> Result<(PersistedState, u32), StoreError> {
    let _ = value;
    todo!("store: migrate")
}
