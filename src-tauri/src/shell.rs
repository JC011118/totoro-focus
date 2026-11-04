//! Shared shell state and the engine call pipeline (SPEC §1 "Engine lock
//! discipline", §4.7, §4.9).
//!
//! There is exactly one [`Engine`], behind one mutex. The guard is held only
//! around engine calls (`command`, `system`, `snapshot`, `persisted_state`,
//! `mark_persisted`), the `seq` stamp and reading `Now`. Everything a caller
//! needs is copied out into an [`Outcome`] before the guard drops; effects are
//! applied afterwards (`effects::apply`) with no lock held. Nothing in this
//! module calls a Tauri, panel, tray, menu or global-shortcut API, or emits.
//!
//! Effects that set a piece of OS state (companion visibility, the process
//! activity) are applied latest-wins: the wanted value is recorded under the
//! engine lock, so in `seq` order, and the applier reads the newest value
//! instead of trusting its own effect's payload. Two threads that apply their
//! effects in the opposite order of their engine calls still settle on the
//! state of the newer call.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use focus_core::engine::{ActivityLevel, CommandError, Effect, Engine, SystemEvent};
use focus_core::packs::PackRegistry;
use focus_core::rewards::UnlockTable;
use focus_core::snapshot::{Phase, Snapshot};
use focus_core::store::{PersistedState, Store};
use focus_core::{Clock, Now, SystemClock};

use crate::ticker::Waker;

/// Where the shell reads and writes.
#[derive(Clone, Debug)]
pub struct Dirs {
    /// `<appData>`: holds `state.json`.
    pub data: PathBuf,
    /// `<appData>/packs`: user character packs.
    pub user_packs: PathBuf,
    /// Debug builds only: the repo's gitignored `packs-local/`.
    pub dev_packs: Option<PathBuf>,
}

/// Everything one engine call produced, copied out under the engine lock.
#[derive(Debug)]
pub struct Outcome {
    /// The `Now` that was passed to the engine call. Persist is applied with this
    /// same value, and `mark_persisted(now)` follows a successful save (SPEC §4.7).
    pub now: Now,
    pub effects: Vec<Effect>,
    /// `Some(engine.persisted_state(now))` iff `effects` contains `Persist`.
    pub persist: Option<PersistedState>,
    /// The single snapshot for this call, stamped with `seq` and `companion_visible`.
    pub snapshot: Snapshot,
}

struct EngineCell {
    engine: Engine,
    /// Last `seq` stamped on a snapshot. Strictly increasing.
    seq: u64,
}

pub struct Shell {
    engine: Mutex<EngineCell>,
    clock: SystemClock,
    store: Store,
    saves: SaveGate,
    read_only_logged: AtomicBool,
    /// Static unlock table (a copy of the engine's), so pack resolution never needs
    /// the engine lock.
    table: UnlockTable,
    /// The pack registry has its own lock and never takes the engine lock.
    packs: RwLock<PackRegistry>,
    pub dirs: Dirs,
    /// Whether the companion should be shown. Written only under the engine lock,
    /// so `Snapshot.companion_visible` is stamped in `seq` order.
    companion_wanted: AtomicBool,
    /// The process activity (App Nap / idle sleep), applied latest-wins.
    activity: ActivitySync,
    emits: EmitGate,
    pub ticker: Waker,
    shutting_down: AtomicBool,
}

impl Shell {
    pub fn new(
        engine: Engine,
        clock: SystemClock,
        store: Store,
        table: UnlockTable,
        packs: PackRegistry,
        dirs: Dirs,
    ) -> Self {
        Self {
            engine: Mutex::new(EngineCell { engine, seq: 0 }),
            clock,
            store,
            saves: SaveGate::default(),
            read_only_logged: AtomicBool::new(false),
            table,
            packs: RwLock::new(packs),
            dirs,
            companion_wanted: AtomicBool::new(false),
            activity: ActivitySync::default(),
            emits: EmitGate::default(),
            ticker: Waker::default(),
            shutting_down: AtomicBool::new(false),
        }
    }

    fn lock(&self) -> MutexGuard<'_, EngineCell> {
        self.engine.lock().unwrap_or_else(|poisoned| {
            log::error!("engine lock was poisoned by a panic; continuing with its state");
            poisoned.into_inner()
        })
    }

    /// Run one engine call under the lock and copy out its [`Outcome`].
    /// `Now` is read while the lock is held. On `Err` the engine is unchanged
    /// (SPEC §4.0) and nothing is stamped.
    pub fn call(
        &self,
        f: impl FnOnce(&mut Engine, Now) -> Result<Vec<Effect>, CommandError>,
    ) -> Result<Outcome, CommandError> {
        let mut cell = self.lock();
        let now = self.clock.now();
        let effects = f(&mut cell.engine, now)?;
        Ok(self.finish(&mut cell, now, effects))
    }

    /// Like [`Shell::call`], for a call that may decide under the lock that there
    /// is nothing to do: `Ok(None)` from `f` stamps nothing and returns `Ok(None)`
    /// (`f` must then have left the engine unchanged).
    pub fn try_call(
        &self,
        f: impl FnOnce(&mut Engine, Now) -> Result<Option<Vec<Effect>>, CommandError>,
    ) -> Result<Option<Outcome>, CommandError> {
        let mut cell = self.lock();
        let now = self.clock.now();
        let Some(effects) = f(&mut cell.engine, now)? else {
            return Ok(None);
        };
        Ok(Some(self.finish(&mut cell, now, effects)))
    }

    /// `engine.system(event, now)` through the same pipeline.
    pub fn call_system(&self, event: SystemEvent) -> Outcome {
        let mut cell = self.lock();
        let now = self.clock.now();
        let effects = cell.engine.system(event, now);
        self.finish(&mut cell, now, effects)
    }

    /// Build the outcome for effects returned by `Engine::new(…, now, …)` at startup.
    pub fn outcome_for(&self, now: Now, effects: Vec<Effect>) -> Outcome {
        let mut cell = self.lock();
        self.finish(&mut cell, now, effects)
    }

    fn finish(&self, cell: &mut EngineCell, now: Now, effects: Vec<Effect>) -> Outcome {
        if let Some(visible) = visibility_change(&effects) {
            self.companion_wanted.store(visible, Ordering::SeqCst);
        }
        if let Some(level) = activity_change(&effects) {
            self.activity.want(level);
        }
        let persist = effects
            .iter()
            .any(|e| matches!(e, Effect::Persist))
            .then(|| cell.engine.persisted_state(now));
        let snapshot = self.stamp(cell, now);
        Outcome {
            now,
            effects,
            persist,
            snapshot,
        }
    }

    fn stamp(&self, cell: &mut EngineCell, now: Now) -> Snapshot {
        let mut snapshot = cell.engine.snapshot(now, &self.clock);
        cell.seq += 1;
        snapshot.seq = cell.seq;
        snapshot.companion_visible = self.companion_wanted.load(Ordering::SeqCst);
        snapshot
    }

    /// A freshly stamped snapshot without running an engine call (no tick).
    pub fn snapshot_now(&self) -> Snapshot {
        let mut cell = self.lock();
        let now = self.clock.now();
        self.stamp(&mut cell, now)
    }

    /// Read (copy out) something from the engine under the lock. `f` must be pure:
    /// no Tauri calls, no emits.
    pub fn read<T>(&self, f: impl FnOnce(&Engine, Now, &dyn Clock) -> T) -> T {
        let cell = self.lock();
        let now = self.clock.now();
        f(&cell.engine, now, &self.clock)
    }

    /// Mark the companion wanted-visible at launch (under the engine lock, so later
    /// snapshots carry it in `seq` order).
    pub fn want_companion_at_launch(&self) {
        let _cell = self.lock();
        self.companion_wanted.store(true, Ordering::SeqCst);
    }

    pub fn companion_wanted(&self) -> bool {
        self.companion_wanted.load(Ordering::SeqCst)
    }

    /// The process activity the newest engine call asked for.
    #[cfg(test)]
    pub fn activity_wanted(&self) -> ActivityLevel {
        self.activity.wanted()
    }

    /// Hand the newest wanted process activity to `apply` (latest-wins, see
    /// [`ActivitySync`]). Called for every `ActivityChanged` effect instead of
    /// applying that effect's own level. Returns the level applied, if any.
    pub fn sync_activity(&self, apply: impl FnOnce(ActivityLevel)) -> Option<ActivityLevel> {
        self.activity.sync(apply)
    }

    /// Apply a Persist: save `state` (taken at `now`, generation `seq`), then
    /// `mark_persisted(now)` only after a successful save. Saves are serialized and
    /// an older state never overwrites a newer one. Failures are logged; the next
    /// running tick retries (SPEC §4.7).
    pub fn persist(&self, seq: u64, now: Now, state: &PersistedState) {
        if self.store.is_read_only() {
            if !self.read_only_logged.swap(true, Ordering::SeqCst) {
                log::warn!(
                    "state.json was written by a newer version; saving is disabled for this run"
                );
            }
            return;
        }
        match self.saves.save(seq, || self.store.save(state)) {
            SaveOutcome::Saved => self.lock().engine.mark_persisted(now),
            SaveOutcome::Superseded => {
                log::debug!("skipped saving state #{seq}: a newer state was already saved");
            }
            SaveOutcome::Failed(e) => {
                log::error!("saving state failed (retried on the next running tick): {e}");
            }
        }
    }

    pub fn table(&self) -> &UnlockTable {
        &self.table
    }

    pub fn packs(&self) -> RwLockReadGuard<'_, PackRegistry> {
        self.packs.read().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn packs_mut(&self) -> RwLockWriteGuard<'_, PackRegistry> {
        self.packs.write().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether `snapshot` should be emitted now (see [`EmitGate`]).
    pub fn admit_emit(&self, snapshot: &Snapshot, always: bool) -> bool {
        self.emits.admit(snapshot, always)
    }

    pub fn begin_shutdown(&self) -> bool {
        !self.shutting_down.swap(true, Ordering::SeqCst)
    }

    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::SeqCst)
    }
}

/// The last companion visibility requested by `effects`, if any.
pub fn visibility_change(effects: &[Effect]) -> Option<bool> {
    effects.iter().rev().find_map(|e| match e {
        Effect::ShowCompanion => Some(true),
        Effect::HideCompanion => Some(false),
        _ => None,
    })
}

/// The last process activity level requested by `effects`, if any.
pub fn activity_change(effects: &[Effect]) -> Option<ActivityLevel> {
    effects.iter().rev().find_map(|e| match e {
        Effect::ActivityChanged { level } => Some(*level),
        _ => None,
    })
}

/// The process activity (SPEC §4.8), applied latest-wins.
///
/// `want` is called under the engine lock, so `wanted` always holds the level of
/// the newest engine call. `sync` reads `wanted` and applies it while holding
/// `applied`, so reading and applying are one step among appliers: whichever
/// applier runs last read the value after every earlier call's `want`, and the
/// OS ends up with the newest level even when two threads apply their
/// `ActivityChanged` effects in the opposite order of their engine calls.
#[derive(Debug)]
pub struct ActivitySync {
    wanted: Mutex<ActivityLevel>,
    /// The level last handed to the OS. A new process holds no activity.
    applied: Mutex<ActivityLevel>,
}

impl Default for ActivitySync {
    fn default() -> Self {
        Self {
            wanted: Mutex::new(ActivityLevel::None),
            applied: Mutex::new(ActivityLevel::None),
        }
    }
}

impl ActivitySync {
    /// Record the level the newest engine call asked for (under the engine lock).
    pub fn want(&self, level: ActivityLevel) {
        *self.wanted.lock().unwrap_or_else(PoisonError::into_inner) = level;
    }

    pub fn wanted(&self) -> ActivityLevel {
        *self.wanted.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Apply the newest wanted level unless it is already applied. Returns the
    /// level handed to `apply`, if any.
    pub fn sync(&self, apply: impl FnOnce(ActivityLevel)) -> Option<ActivityLevel> {
        let mut applied = self.applied.lock().unwrap_or_else(PoisonError::into_inner);
        let wanted = self.wanted();
        if *applied == wanted {
            return None;
        }
        apply(wanted);
        *applied = wanted;
        Some(wanted)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SaveOutcome<E> {
    Saved,
    /// A state from a later call was already saved; this one was dropped.
    Superseded,
    Failed(E),
}

/// Serializes saves (one writer of `state.json.tmp` at a time) and drops a save
/// whose generation is not newer than the last successful one, so a slow
/// checkpoint can never overwrite a newer state written by a concurrent call.
#[derive(Debug, Default)]
pub struct SaveGate {
    last_saved: Mutex<u64>,
}

impl SaveGate {
    pub fn save<E>(&self, generation: u64, save: impl FnOnce() -> Result<(), E>) -> SaveOutcome<E> {
        let mut last = self
            .last_saved
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if generation <= *last {
            return SaveOutcome::Superseded;
        }
        match save() {
            Ok(()) => {
                *last = generation;
                SaveOutcome::Saved
            }
            Err(e) => SaveOutcome::Failed(e),
        }
    }
}

/// The parts of a snapshot whose change makes a tick worth emitting (SPEC §8.2
/// Ticker): `remainingLabel`, `phase`, `revision` and `today.date`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmitKey {
    remaining_label: Option<String>,
    phase: Phase,
    revision: u64,
    date: String,
}

impl EmitKey {
    pub fn of(s: &Snapshot) -> Self {
        Self {
            remaining_label: s.session.as_ref().map(|v| v.remaining_label.clone()),
            phase: s.phase,
            revision: s.revision,
            date: s.today.date.clone(),
        }
    }
}

/// Remembers the newest emitted snapshot. Commands always emit; ticks emit only
/// when the [`EmitKey`] changed and the snapshot is newer than the last emitted.
#[derive(Debug, Default)]
pub struct EmitGate {
    last: Mutex<Option<(u64, EmitKey)>>,
}

impl EmitGate {
    pub fn admit(&self, snapshot: &Snapshot, always: bool) -> bool {
        let key = EmitKey::of(snapshot);
        let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
        let newer = last.as_ref().is_none_or(|(seq, _)| snapshot.seq > *seq);
        let changed = last.as_ref().is_none_or(|(_, k)| *k != key);
        let emit = always || (newer && changed);
        if emit && newer {
            *last = Some((snapshot.seq, key));
        }
        emit
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Arc;

    use chrono::{TimeZone, Utc};
    use focus_core::history::TodaySummary;
    use focus_core::settings::Settings;
    use focus_core::snapshot::{FriendshipView, SessionView};
    use focus_core::timer::{RunStatus, SessionKind};

    use super::*;

    /// A plain snapshot for shell tests (built by hand: the engine is not needed).
    pub(crate) fn snapshot(seq: u64, phase: Phase, session: Option<SessionView>) -> Snapshot {
        Snapshot {
            seq,
            revision: 1,
            phase,
            session,
            awaiting: None,
            today: TodaySummary {
                date: "2026-03-02".into(),
                focus_ms: 0,
                active_focus_ms: 0,
                completed: 0,
                interrupted: 0,
                harvest: vec![],
                focus_label: "0 min".into(),
            },
            friendship: FriendshipView {
                total_points: 0,
                unlocked: vec!["base".into()],
                next: None,
            },
            settings: Settings::default(),
            companion_visible: true,
        }
    }

    pub(crate) fn session(kind: SessionKind, status: RunStatus, label: &str) -> SessionView {
        SessionView {
            id: "s1".into(),
            kind,
            status,
            pause_reason: None,
            task_label: None,
            planned_ms: 1_500_000,
            active_ms: 0,
            remaining_ms: 1_500_000,
            remaining_label: label.into(),
            started_at: Utc.with_ymd_and_hms(2026, 3, 2, 9, 0, 0).unwrap(),
        }
    }

    #[test]
    fn visibility_change_takes_the_last_show_or_hide() {
        assert_eq!(visibility_change(&[]), None);
        assert_eq!(visibility_change(&[Effect::Persist]), None);
        assert_eq!(
            visibility_change(&[Effect::Persist, Effect::ShowCompanion]),
            Some(true)
        );
        assert_eq!(visibility_change(&[Effect::HideCompanion]), Some(false));
        assert_eq!(
            visibility_change(&[Effect::HideCompanion, Effect::ShowCompanion]),
            Some(true)
        );
    }

    #[test]
    fn activity_change_takes_the_last_level() {
        assert_eq!(activity_change(&[]), None);
        assert_eq!(activity_change(&[Effect::Persist]), None);
        assert_eq!(
            activity_change(&[
                Effect::Persist,
                Effect::ActivityChanged {
                    level: ActivityLevel::Focus
                }
            ]),
            Some(ActivityLevel::Focus)
        );
        assert_eq!(
            activity_change(&[
                Effect::ActivityChanged {
                    level: ActivityLevel::Focus
                },
                Effect::ShowCompanion,
                Effect::ActivityChanged {
                    level: ActivityLevel::None
                },
            ]),
            Some(ActivityLevel::None)
        );
    }

    fn recorder() -> (
        Arc<Mutex<Vec<ActivityLevel>>>,
        impl Fn() -> Vec<ActivityLevel>,
    ) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let read = {
            let log = log.clone();
            move || log.lock().unwrap().clone()
        };
        (log, read)
    }

    #[test]
    fn a_new_process_holds_no_activity_and_syncing_it_does_nothing() {
        let sync = ActivitySync::default();
        assert_eq!(sync.wanted(), ActivityLevel::None);
        assert_eq!(sync.sync(|_| panic!("nothing to apply")), None);
    }

    #[test]
    fn activity_sync_applies_each_new_level_once() {
        let sync = ActivitySync::default();
        let (log, applied) = recorder();
        let record = |l| log.lock().unwrap().push(l);
        sync.want(ActivityLevel::Focus);
        assert_eq!(sync.sync(record), Some(ActivityLevel::Focus));
        assert_eq!(sync.sync(record), None, "already applied");
        sync.want(ActivityLevel::Break);
        assert_eq!(sync.sync(record), Some(ActivityLevel::Break));
        sync.want(ActivityLevel::None);
        assert_eq!(sync.sync(record), Some(ActivityLevel::None));
        assert_eq!(
            applied(),
            vec![
                ActivityLevel::Focus,
                ActivityLevel::Break,
                ActivityLevel::None
            ]
        );
    }

    /// The review finding's interleaving: the ticker's call (seq N) completes a
    /// focus session (→ None) and is preempted before applying; the hotkey's call
    /// (seq N+1) starts focus (→ Focus) and applies first; then the ticker applies.
    /// Focus must stay held.
    #[test]
    fn a_late_apply_of_an_older_call_never_undoes_a_newer_level() {
        let sync = ActivitySync::default();
        let (log, applied) = recorder();
        let record = |l| log.lock().unwrap().push(l);
        sync.want(ActivityLevel::Focus); // the session that is about to complete
        sync.sync(record);

        sync.want(ActivityLevel::None); // ticker, seq N, under the engine lock
        sync.want(ActivityLevel::Focus); // hotkey Toggle, seq N+1, under the engine lock
        assert_eq!(sync.sync(record), None, "hotkey: Focus is already held");
        assert_eq!(sync.sync(record), None, "ticker: reads the newest level");
        assert_eq!(applied(), vec![ActivityLevel::Focus]);

        // The same race with the newer call ending the session.
        sync.want(ActivityLevel::Break); // seq N+2
        sync.want(ActivityLevel::None); // seq N+3
        assert_eq!(sync.sync(record), Some(ActivityLevel::None), "seq N+3");
        assert_eq!(sync.sync(record), None, "seq N+2 applies late: no change");
        assert_eq!(applied(), vec![ActivityLevel::Focus, ActivityLevel::None]);
    }

    #[test]
    fn racing_appliers_settle_on_the_newest_level() {
        const LEVELS: [ActivityLevel; 3] = [
            ActivityLevel::None,
            ActivityLevel::Break,
            ActivityLevel::Focus,
        ];
        for round in 0..20 {
            let sync = Arc::new(ActivitySync::default());
            // Stands in for the engine lock: `want` happens under it, in seq order.
            let engine = Arc::new(Mutex::new(0u64));
            let applied = Arc::new(Mutex::new(Vec::new()));
            let handles: Vec<_> = (0..8usize)
                .map(|t| {
                    let (sync, engine, applied) = (sync.clone(), engine.clone(), applied.clone());
                    std::thread::spawn(move || {
                        for i in 0..50usize {
                            {
                                let mut seq = engine.lock().unwrap();
                                *seq += 1;
                                sync.want(LEVELS[(t * 7 + i + round) % 3]);
                            }
                            if i % 3 == t % 3 {
                                std::thread::yield_now();
                            }
                            sync.sync(|l| applied.lock().unwrap().push(l));
                        }
                    })
                })
                .collect();
            for h in handles {
                h.join().unwrap();
            }
            let newest = sync.wanted();
            let last_applied = applied
                .lock()
                .unwrap()
                .last()
                .copied()
                .unwrap_or(ActivityLevel::None);
            assert_eq!(last_applied, newest, "round {round}");
        }
    }

    #[test]
    fn save_gate_saves_newer_generations_only() {
        let gate = SaveGate::default();
        let saved = Mutex::new(Vec::new());
        let save = |g: u64| {
            gate.save(g, || -> Result<(), ()> {
                saved.lock().unwrap().push(g);
                Ok(())
            })
        };
        assert_eq!(save(2), SaveOutcome::Saved);
        // An older state (computed earlier, saved later) never overwrites a newer one.
        assert_eq!(save(1), SaveOutcome::Superseded);
        assert_eq!(save(2), SaveOutcome::Superseded);
        assert_eq!(save(5), SaveOutcome::Saved);
        assert_eq!(*saved.lock().unwrap(), vec![2, 5]);
    }

    #[test]
    fn save_gate_failure_does_not_advance_the_generation() {
        let gate = SaveGate::default();
        assert_eq!(
            gate.save(3, || Err("disk full")),
            SaveOutcome::Failed("disk full")
        );
        // The retry of an older generation still saves: nothing newer reached disk.
        assert_eq!(gate.save(2, || Ok::<(), &str>(())), SaveOutcome::Saved);
    }

    #[test]
    fn save_gate_serializes_concurrent_saves() {
        let gate = Arc::new(SaveGate::default());
        let inside = Arc::new(AtomicBool::new(false));
        let handles: Vec<_> = (1..=8u64)
            .map(|g| {
                let gate = gate.clone();
                let inside = inside.clone();
                std::thread::spawn(move || {
                    gate.save(g, || -> Result<(), ()> {
                        assert!(!inside.swap(true, Ordering::SeqCst), "two saves overlapped");
                        std::thread::sleep(std::time::Duration::from_millis(2));
                        inside.store(false, Ordering::SeqCst);
                        Ok(())
                    })
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(*gate.last_saved.lock().unwrap(), 8);
    }

    #[test]
    fn ticks_emit_only_when_label_phase_revision_or_date_change() {
        let gate = EmitGate::default();
        let running = |seq, label: &str| {
            snapshot(
                seq,
                Phase::Focus,
                Some(session(SessionKind::Focus, RunStatus::Running, label)),
            )
        };
        assert!(
            gate.admit(&running(1, "25:00"), false),
            "first snapshot is emitted"
        );
        assert!(
            !gate.admit(&running(2, "25:00"), false),
            "same label: no emit"
        );
        assert!(gate.admit(&running(3, "24:59"), false), "label changed");

        let mut bumped = running(4, "24:59");
        bumped.revision = 2;
        assert!(gate.admit(&bumped, false), "revision changed");

        let mut next_day = running(5, "24:59");
        next_day.revision = 2;
        next_day.today.date = "2026-03-03".into();
        assert!(gate.admit(&next_day, false), "date changed");

        let mut awaiting = snapshot(6, Phase::AwaitingChoice, None);
        awaiting.revision = 2;
        awaiting.today.date = "2026-03-03".into();
        assert!(gate.admit(&awaiting, false), "phase changed");
    }

    #[test]
    fn commands_always_emit_and_stale_ticks_never_replace_newer() {
        let gate = EmitGate::default();
        let idle = |seq| snapshot(seq, Phase::Idle, None);
        assert!(gate.admit(&idle(1), true));
        assert!(
            gate.admit(&idle(2), true),
            "commands emit even when unchanged"
        );
        // A tick computed before the command (lower seq) with a different key is not emitted.
        let mut stale = snapshot(
            1,
            Phase::Focus,
            Some(session(SessionKind::Focus, RunStatus::Running, "10:00")),
        );
        stale.revision = 9;
        assert!(!gate.admit(&stale, false));
        // And it did not replace the newest key: an unchanged newer tick stays quiet.
        assert!(!gate.admit(&idle(3), false));
    }
}
