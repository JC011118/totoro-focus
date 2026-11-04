//! App setup (runs once inside `.setup()`, on the main thread, after the
//! single-instance plugin has let this process continue). Store and Engine are
//! created only here, so a second process never touches state (SPEC §8.2).

use std::path::PathBuf;

use focus_core::engine::{uuid_ids, Engine, EngineConfig};
use focus_core::packs::PackRegistry;
use focus_core::rewards::UnlockTable;
use focus_core::store::{LoadNote, Store};
use focus_core::{Clock, SystemClock};
use tauri::{AppHandle, Manager};

use crate::companion::{self, CompanionState, WindowMode};
use crate::effects::{self, Emit};
use crate::hotkey::{self, HotkeyState};
use crate::shell::{Dirs, Shell};
use crate::{pack_protocol, platform, ticker, tray};

pub fn run(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let dirs = resolve_dirs(app)?;
    if let Err(e) = std::fs::create_dir_all(&dirs.user_packs) {
        log::warn!("could not create {}: {e}", dirs.user_packs.display());
    }

    // One clock for the whole process: the engine's monotonic baseline and every
    // later call share its origin.
    let clock = SystemClock::new();
    let store = Store::new(&dirs.data);
    let report = store.load(clock.wall());
    log_load_note(&report.note);
    let table = UnlockTable::builtin();
    let now = clock.now();
    let (engine, startup_effects) = Engine::new(
        EngineConfig::default(),
        table.clone(),
        report.state,
        now,
        uuid_ids(),
    );
    let packs = PackRegistry::load(
        pack_protocol::bundled_pack(),
        Some(&dirs.user_packs),
        dirs.dev_packs.as_deref(),
    );
    log::info!(
        "{} {} starting; data in {}",
        app.package_info().name,
        app.package_info().version,
        dirs.data.display()
    );
    app.manage(Shell::new(engine, clock, store, table, packs, dirs));
    let shell = app.state::<Shell>();

    // Persist if Engine::new changed anything (same `now`); Restored has no
    // listeners yet, the renderer derives the paused state from the snapshot.
    let outcome = shell.outcome_for(now, startup_effects);
    effects::apply(app, &shell, outcome, Emit::Always);

    let mode = WindowMode::detect();
    app.manage(CompanionState::new(app, mode));
    app.manage(HotkeyState::default());
    if let Err(e) = companion::create(app) {
        log::error!("could not create the companion window: {e}");
    }
    // The shell shows the companion at launch (Engine::new never does).
    shell.want_companion_at_launch();
    companion::sync_visibility(app);

    if let Err(e) = tray::build(app, &shell.snapshot_now()) {
        log::error!("could not create the menu-bar item: {e}");
    }
    hotkey::register_saved(app);
    platform::install_observers(app);
    ticker::spawn(app.clone());
    Ok(())
}

fn resolve_dirs(app: &AppHandle) -> tauri::Result<Dirs> {
    let data = app.path().app_data_dir()?;
    Ok(Dirs {
        user_packs: data.join("packs"),
        data,
        dev_packs: dev_packs_dir(),
    })
}

/// Debug builds only: the repo's gitignored `packs-local/`.
fn dev_packs_dir() -> Option<PathBuf> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("packs-local");
    dir.is_dir().then_some(dir)
}

fn log_load_note(note: &LoadNote) {
    match note {
        LoadNote::Missing => log::info!("no state file yet; starting fresh"),
        LoadNote::Loaded => log::info!("state loaded"),
        LoadNote::Migrated { from } => log::info!("state migrated from schema {from}"),
        LoadNote::CorruptBackedUp { backup, error } => {
            log::warn!(
                "state file was unreadable ({error}); moved to {}",
                backup.display()
            );
        }
        LoadNote::RecoveredFromPrevious { backup, error } => log::warn!(
            "state file was unreadable ({error}); moved to {} and recovered the previous copy",
            backup.display()
        ),
        LoadNote::NewerSchemaReadOnly { backup, version } => log::warn!(
            "state file has newer schema {version} (copied to {}); running read-only",
            backup.display()
        ),
    }
}
