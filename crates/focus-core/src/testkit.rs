//! Engine test harness (SPEC §11). Available in this crate's tests and, via the
//! `testkit` feature, to other crates' tests.
//!
//! [`H`] owns an [`Engine`] and a shared [`FakeClock`] and plays the shell's role:
//! every call reads `Now` from the fake clock, and the returned effects are applied
//! the way the shell applies them — `Persist` stores `engine.persisted_state(now)`
//! (same `now`) into `last_saved` and then calls `mark_persisted(now)`;
//! Show/HideCompanion and ActivityChanged update `companion_visible` / `activity`.
//! `last_saved` starts as the state the harness was created from (what was loaded from
//! disk) until the first applied `Persist` replaces it. `relaunch` builds a new engine
//! from `last_saved` (round-tripped through JSON, as on disk) with a new fake clock,
//! which is how quit, crash and restore are tested.
//!
//! **Engine tests must tick at least every 60 s of fake time while a session runs**
//! (use [`H::run`] or [`H::run_step`]); a single long `advance` is an observation gap
//! (SPEC §4.5) and pauses the session instead of earning time.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::TimeDelta;

use crate::clock::{Clock, FakeClock, Now};
use crate::engine::{
    ActivityLevel, Command, CommandError, Effect, Engine, EngineConfig, IdSource, SystemEvent,
    Transition,
};
use crate::rewards::UnlockTable;
use crate::settings::Settings;
use crate::snapshot::Snapshot;
use crate::store::PersistedState;

/// One second in ms.
pub const SEC: u64 = 1_000;
/// One minute in ms.
pub const MIN: u64 = 60_000;
/// One hour in ms.
pub const HOUR: u64 = 3_600_000;

/// Counter id source: `"s1"`, `"s2"`, …
pub fn counter_ids() -> IdSource {
    shared_counter_ids(Arc::new(AtomicU64::new(0)))
}

/// Counter id source over a shared counter, so a relaunched engine continues the
/// sequence instead of reusing ids.
pub fn shared_counter_ids(counter: Arc<AtomicU64>) -> IdSource {
    Box::new(move || format!("s{}", counter.fetch_add(1, Ordering::SeqCst) + 1))
}

/// The transitions in `fx`, in order.
pub fn transitions(fx: &[Effect]) -> Vec<Transition> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Transition(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

/// A short name per effect (transitions by their variant), for ordering assertions,
/// e.g. `["Persist", "ActivityChanged", "FocusStarted"]`.
pub fn effect_names(fx: &[Effect]) -> Vec<&'static str> {
    fx.iter().map(effect_name).collect()
}

/// See [`effect_names`].
pub fn effect_name(e: &Effect) -> &'static str {
    match e {
        Effect::Persist => "Persist",
        Effect::SettingsChanged { .. } => "SettingsChanged",
        Effect::ShowCompanion => "ShowCompanion",
        Effect::HideCompanion => "HideCompanion",
        Effect::ActivityChanged { .. } => "ActivityChanged",
        Effect::PlaySound(_) => "PlaySound",
        Effect::Transition(t) => match t {
            Transition::FocusStarted { .. } => "FocusStarted",
            Transition::BreakStarted { .. } => "BreakStarted",
            Transition::Paused { .. } => "Paused",
            Transition::Resumed { .. } => "Resumed",
            Transition::FocusCompleted { .. } => "FocusCompleted",
            Transition::FocusInterrupted { .. } => "FocusInterrupted",
            Transition::BreakFinished { .. } => "BreakFinished",
            Transition::BreakEnded { .. } => "BreakEnded",
            Transition::Dismissed => "Dismissed",
            Transition::Restored { .. } => "Restored",
        },
    }
}

/// Number of `Persist` effects in `fx`.
pub fn persist_count(fx: &[Effect]) -> usize {
    fx.iter().filter(|e| matches!(e, Effect::Persist)).count()
}

/// The engine harness (SPEC §11).
pub struct H {
    pub engine: Engine,
    pub clock: Arc<FakeClock>,
    /// What a crash would leave on disk: the state as of the last applied `Persist`, or
    /// the state the harness was created from if nothing was saved since. Only `None`
    /// if a test clears it (then `relaunch` starts from defaults, like a missing file).
    pub last_saved: Option<PersistedState>,
    /// Every effect applied since creation or the last relaunch / [`H::take_fx`].
    pub fx: Vec<Effect>,
    /// Effects returned by `Engine::new` at the last (re)launch (also in `fx`).
    pub launch_fx: Vec<Effect>,
    /// When false, `Persist` effects are not applied (a failed save, or a crash before
    /// the save): `last_saved` keeps its old value and `mark_persisted` is not called.
    pub saving: bool,
    /// Number of applied saves.
    pub saves: usize,
    /// Driven by Show/HideCompanion effects (the shell shows the companion at launch).
    pub companion_visible: bool,
    /// Driven by ActivityChanged effects.
    pub activity: ActivityLevel,
    pub cfg: EngineConfig,
    pub table: UnlockTable,
    ids: Arc<AtomicU64>,
}

impl Default for H {
    fn default() -> Self {
        Self::new()
    }
}

impl H {
    /// Fresh state, `FakeClock::default_start()` (2026-03-02 09:00 UTC, offset 0),
    /// default config and the built-in unlock table.
    pub fn new() -> Self {
        Self::with_state(PersistedState::default())
    }

    /// Starts from `state` (as if loaded from disk).
    pub fn with_state(state: PersistedState) -> Self {
        Self::with(
            EngineConfig::default(),
            UnlockTable::builtin(),
            state,
            FakeClock::default_start(),
        )
    }

    /// Fresh state with settings changed by `f`.
    pub fn with_settings(f: impl FnOnce(&mut Settings)) -> Self {
        let mut state = PersistedState::default();
        f(&mut state.settings);
        Self::with_state(state)
    }

    pub fn with(
        cfg: EngineConfig,
        table: UnlockTable,
        state: PersistedState,
        clock: FakeClock,
    ) -> Self {
        let clock = Arc::new(clock);
        let ids = Arc::new(AtomicU64::new(0));
        let now = clock.now();
        // The loaded state is what is on disk until the first applied Persist.
        let on_disk = state.clone();
        let (engine, launch_fx) = Engine::new_with_clock(
            cfg.clone(),
            table.clone(),
            state,
            now,
            shared_counter_ids(ids.clone()),
            clock.clone(),
        );
        let mut h = H {
            engine,
            clock,
            last_saved: Some(on_disk),
            fx: Vec::new(),
            launch_fx: launch_fx.clone(),
            saving: true,
            saves: 0,
            companion_visible: true,
            activity: ActivityLevel::None,
            cfg,
            table,
            ids,
        };
        h.apply(&launch_fx, now);
        h
    }

    pub fn now(&self) -> Now {
        self.clock.now()
    }

    /// Run a command at the clock's `now` and apply its effects.
    pub fn cmd(&mut self, c: Command) -> Result<Vec<Effect>, CommandError> {
        let now = self.now();
        let fx = self.engine.command(c, now)?;
        self.apply(&fx, now);
        Ok(fx)
    }

    /// [`H::cmd`], panicking on `Err`.
    pub fn ok(&mut self, c: Command) -> Vec<Effect> {
        match self.cmd(c.clone()) {
            Ok(fx) => fx,
            Err(e) => panic!("{c:?} failed: {e:?}"),
        }
    }

    /// Start a focus session of `minutes` with no label.
    pub fn start(&mut self, minutes: u32) -> Vec<Effect> {
        self.ok(Command::StartFocus {
            minutes: Some(minutes),
            task_label: None,
        })
    }

    /// Run a system event at the clock's `now` and apply its effects.
    pub fn ev(&mut self, e: SystemEvent) -> Vec<Effect> {
        let now = self.now();
        let fx = self.engine.system(e, now);
        self.apply(&fx, now);
        fx
    }

    pub fn tick(&mut self) -> Vec<Effect> {
        self.ev(SystemEvent::Tick)
    }

    /// Advance both clocks by `ms` in 1 s steps, ticking after each step.
    pub fn run(&mut self, ms: u64) -> Vec<Effect> {
        self.run_step(ms, SEC)
    }

    /// Advance both clocks by `ms` in `step` increments (the last may be shorter),
    /// ticking after each. Keep `step` ≤ 60 s while a session runs.
    pub fn run_step(&mut self, ms: u64, step: u64) -> Vec<Effect> {
        assert!(step > 0, "step must be positive");
        let mut out = Vec::new();
        let mut left = ms;
        while left > 0 {
            let d = step.min(left);
            self.clock.advance(d);
            out.extend(self.tick());
            left -= d;
        }
        out
    }

    /// Pure snapshot at the clock's `now` (no tick first).
    pub fn snap(&self) -> Snapshot {
        self.engine.snapshot(self.now(), &*self.clock)
    }

    /// Simulates sleep on macOS (`Instant` stops): only the wall clock moves.
    pub fn sleep_mono_stopped(&self, ms: u64) {
        self.clock.jump_wall(ms as i64);
    }

    /// Simulates sleep on a monotonic clock that counts sleep: both clocks move.
    pub fn sleep_mono_counting(&self, ms: u64) {
        self.clock.advance(ms);
    }

    /// A user show/hide of the companion (the shell clears hide-during-focus).
    pub fn user_show_hide(&mut self, visible: bool) {
        self.companion_visible = visible;
        self.engine.clear_hidden_for_focus();
    }

    /// Effects applied since the last call (and clears them).
    pub fn take_fx(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.fx)
    }

    /// The state on disk (see [`H::last_saved`]); panics if a test cleared it.
    pub fn saved(&self) -> &PersistedState {
        self.last_saved.as_ref().expect("nothing is on disk")
    }

    /// End this "process" and start a new one from `last_saved` (the state on disk:
    /// the last save, else the state the harness was created from; defaults if a test
    /// cleared it), as after a quit or a crash. The new fake clock's wall is `closed_ms`
    /// later. With `mono_counts_sleep` its monotonic clock continues from the old value
    /// plus `closed_ms`; otherwise it restarts at the default origin (a new process). The
    /// saved state is round-tripped through JSON. Returns `Engine::new`'s effects.
    pub fn relaunch(&mut self, closed_ms: u64, mono_counts_sleep: bool) -> Vec<Effect> {
        let old = self.now();
        let offset = self.clock.local_offset(old.wall);
        let wall = old.wall + TimeDelta::milliseconds(closed_ms as i64);
        let clock = FakeClock::new(wall, offset.local_minus_utc() / 3600);
        if mono_counts_sleep {
            let origin = clock.now().mono;
            clock.advance_mono(
                old.mono
                    .0
                    .saturating_add(closed_ms)
                    .saturating_sub(origin.0),
            );
        }
        let state = match &self.last_saved {
            Some(s) => {
                let json = serde_json::to_string(s).expect("state serializes");
                serde_json::from_str(&json).expect("state deserializes")
            }
            None => PersistedState::default(),
        };
        self.clock = Arc::new(clock);
        let now = self.now();
        let (engine, launch_fx) = Engine::new_with_clock(
            self.cfg.clone(),
            self.table.clone(),
            state,
            now,
            shared_counter_ids(self.ids.clone()),
            self.clock.clone(),
        );
        self.engine = engine;
        self.fx.clear();
        self.launch_fx = launch_fx.clone();
        self.companion_visible = true;
        self.activity = ActivityLevel::None;
        self.apply(&launch_fx, now);
        launch_fx
    }

    /// Apply effects as the shell does: save once with the same `now`, then the rest.
    fn apply(&mut self, effects: &[Effect], now: Now) {
        for e in effects {
            match e {
                Effect::Persist => {
                    if self.saving {
                        self.last_saved = Some(self.engine.persisted_state(now));
                        self.engine.mark_persisted(now);
                        self.saves += 1;
                    }
                }
                Effect::ShowCompanion => self.companion_visible = true,
                Effect::HideCompanion => self.companion_visible = false,
                Effect::ActivityChanged { level } => self.activity = *level,
                Effect::SettingsChanged { .. } | Effect::Transition(_) | Effect::PlaySound(_) => {}
            }
        }
        self.fx.extend(effects.iter().cloned());
    }
}
