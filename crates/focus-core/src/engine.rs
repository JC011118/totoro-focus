//! The engine: one authoritative state machine for sessions, rewards and
//! history. It performs no I/O. Every call takes `Now`; results are `Effect`s
//! the app shell applies (save, emit, sound, show/hide, hotkey, App Nap).
//! See docs/SPEC.md §4 for the exact rules this implements — in particular the
//! call pipeline (§4.0), `observe` (§4.5), Engine::new steps (§4.7) and effect
//! ordering (§4.9).

use serde::{Deserialize, Serialize};

use crate::clock::{Clock, Mono, Now};
use crate::history::{History, SessionRecord};
use crate::rewards::{AwardOutcome, Friendship, UnlockTable};
use crate::settings::{Settings, SettingsPatch};
use crate::snapshot::{Phase, Snapshot};
use crate::store::PersistedState;
use crate::timer::{ActiveSession, PauseReason, SessionKind};

#[derive(Clone, Debug, PartialEq)]
pub struct EngineConfig {
    /// A session paused longer than this ends (focus: interrupted, minutes kept).
    pub pause_expiry_ms: u64,
    /// A gap between observations longer than this earns nothing and pauses.
    pub max_observation_gap_ms: u64,
    /// Wall/monotonic divergence beyond this is treated as a clock jump.
    pub clock_jump_tolerance_ms: u64,
    /// While running, checkpoint at least this often.
    pub checkpoint_interval_ms: u64,
    pub break_minutes: u32,
    pub presets: Vec<u32>,
    pub min_minutes: u32,
    pub max_minutes: u32,
    pub max_task_label_chars: u32,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            pause_expiry_ms: 120 * 60_000,
            max_observation_gap_ms: 120_000,
            clock_jump_tolerance_ms: 60_000,
            checkpoint_interval_ms: 15_000,
            break_minutes: 5,
            presets: vec![15, 25, 45],
            min_minutes: 1,
            max_minutes: 180,
            max_task_label_chars: 80,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    StartFocus {
        minutes: Option<u32>,
        task_label: Option<String>,
    },
    StartBreak {
        minutes: Option<u32>,
    },
    Pause,
    Resume,
    /// Global hotkey: idle/awaiting → start last duration; running → pause; paused → resume.
    Toggle,
    End,
    Dismiss,
    UpdateSettings(SettingsPatch),
    /// Pause and checkpoint before the process exits.
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemEvent {
    Tick,
    Suspend,
    Wake,
    /// The OS reported a wall-clock change.
    ClockChanged,
}

/// What the shell should hold to keep the ticker honest (SPEC §4.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ActivityLevel {
    /// No session running: release any activity.
    #[default]
    None,
    /// A break is running: prevent App Nap, allow idle system sleep.
    Break,
    /// A focus session is running: prevent App Nap and idle system sleep.
    Focus,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SoundCue {
    Completion,
    BreakOver,
}

/// Session transitions, forwarded to the renderer as `focus://transition`
/// so it can play transition animations (never during active focus).
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Transition {
    FocusStarted {
        session_id: String,
        planned_ms: u64,
    },
    BreakStarted {
        session_id: String,
        planned_ms: u64,
    },
    Paused {
        session_id: String,
        kind: SessionKind,
        reason: PauseReason,
    },
    Resumed {
        session_id: String,
        kind: SessionKind,
    },
    FocusCompleted {
        record: SessionRecord,
        award: Option<AwardOutcome>,
        long_session: bool,
    },
    FocusInterrupted {
        record: SessionRecord,
        award: Option<AwardOutcome>,
    },
    /// A break ran to zero.
    BreakFinished {
        session_id: String,
    },
    /// A break was ended by the user or expired while paused.
    BreakEnded {
        session_id: String,
    },
    Dismissed,
    /// A checkpointed session was restored (always paused) at startup.
    Restored {
        session_id: String,
        kind: SessionKind,
    },
}

/// Effects, in the order they appear within one call (SPEC §4.9):
/// Persist, SettingsChanged, Show/HideCompanion, ActivityChanged, Transition, PlaySound.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Save `engine.persisted_state(now)` atomically using the SAME `now` passed to
    /// the call that returned it; after success call `engine.mark_persisted(now)`.
    Persist,
    Transition(Transition),
    PlaySound(SoundCue),
    ShowCompanion,
    HideCompanion,
    /// Settings changed; the shell diffs to re-register the hotkey, reload the pack, etc.
    SettingsChanged {
        previous: Box<Settings>,
        current: Box<Settings>,
    },
    /// Change the NSProcessInfo activity (emitted only when the level changes).
    ActivityChanged { level: ActivityLevel },
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(tag = "code", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum CommandError {
    #[error("a session is already in progress")]
    SessionActive,
    #[error("there is no session in progress")]
    NoSession,
    #[error("nothing is waiting for a choice")]
    NotAwaitingChoice,
    #[error("duration must be between {min} and {max} minutes")]
    InvalidMinutes { min: u32, max: u32 },
    #[error("invalid settings: {reason}")]
    InvalidSettings { reason: String },
    #[error("animations can't be replayed during an active focus session")]
    FocusRunning,
    #[error("{id} is not unlocked yet")]
    NotUnlocked { id: String },
    #[error("character pack {id} is unavailable: {reason}")]
    PackUnavailable { id: String, reason: String },
}

/// What the companion offers after a session ends.
#[derive(Clone, Debug, PartialEq)]
pub struct AwaitingChoice {
    pub after: SessionKind,
    pub record: Option<SessionRecord>,
    pub award: Option<AwardOutcome>,
    pub long_session: bool,
}

pub type IdSource = Box<dyn FnMut() -> String + Send>;

/// Default id source: random UUID v4 strings.
pub fn uuid_ids() -> IdSource {
    Box::new(|| uuid::Uuid::new_v4().to_string())
}

pub struct Engine {
    cfg: EngineConfig,
    table: UnlockTable,
    settings: Settings,
    history: History,
    friendship: Friendship,
    active: Option<ActiveSession>,
    awaiting: Option<AwaitingChoice>,
    /// Last observation (SPEC §4.5). Cleared on suspend.
    baseline: Option<Now>,
    /// Between Suspend and Wake (SPEC §4.5).
    suspended: bool,
    /// Last emitted activity level (SPEC §4.8).
    activity: ActivityLevel,
    /// Monotonic time of the last successful persist.
    last_persist: Option<Mono>,
    /// The companion was hidden because of `hide_during_focus`.
    hidden_for_focus: bool,
    revision: u64,
    ids: IdSource,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("phase", &self.phase())
            .field("active", &self.active)
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// Build from loaded state following SPEC §4.7 "Engine::new steps": sanitize,
    /// reconcile, dedupe history, validate/drop the checkpoint, restore PAUSED (or
    /// silently complete a fully-served focus checkpoint), expire if due. Effects:
    /// `Persist` if anything changed, `Transition::Restored` iff a session remains.
    /// Never ShowCompanion or ActivityChanged.
    pub fn new(cfg: EngineConfig, table: UnlockTable, state: PersistedState, now: Now, ids: IdSource) -> (Self, Vec<Effect>) {
        let _ = (cfg, table, state, now, ids);
        todo!("engine: new")
    }

    pub fn command(&mut self, cmd: Command, now: Now) -> Result<Vec<Effect>, CommandError> {
        let _ = (cmd, now);
        todo!("engine: command")
    }

    pub fn system(&mut self, event: SystemEvent, now: Now) -> Vec<Effect> {
        let _ = (event, now);
        todo!("engine: system")
    }

    /// Full view model at `now` (pure; folds a running session at
    /// `min(now.mono, baseline.mono)`). `clock` supplies local offsets for the today
    /// summary. `seq` is 0 and `companion_visible` false: the shell stamps both.
    pub fn snapshot(&self, now: Now, clock: &dyn Clock) -> Snapshot {
        let _ = (now, clock);
        todo!("engine: snapshot")
    }

    /// Everything to persist at `now` (pure), with the active session folded into a
    /// checkpoint at `min(now.mono, baseline.mono)` and `checkpointAt = now.wall`.
    pub fn persisted_state(&self, now: Now) -> PersistedState {
        let _ = now;
        todo!("engine: persisted_state")
    }

    /// Record that a save at `now` succeeded (resets the checkpoint interval).
    pub fn mark_persisted(&mut self, now: Now) {
        self.last_persist = Some(now.mono);
    }

    /// Milliseconds until the next useful tick (SPEC §8.2 Ticker): running → the next
    /// whole-second boundary of remaining time (`remaining % 1000`, or 1000 when 0),
    /// clamped to 50..=1000 (matches the ceil countdown format); paused → 15_000;
    /// idle/awaiting → ms until the next local midnight + 50, capped at 60_000.
    pub fn next_tick_delay_ms(&self, now: Now, clock: &dyn Clock) -> u64 {
        let _ = (now, clock);
        todo!("engine: next_tick_delay_ms")
    }

    /// Validate a replay request (SPEC §10): `FocusRunning` while a focus session is
    /// running; `key` must be `react` or an unlocked id of kind animation|idleVariant
    /// (else `NotUnlocked`).
    pub fn check_replay(&self, key: &str) -> Result<(), CommandError> {
        let _ = key;
        todo!("engine: check_replay")
    }

    /// A user show/hide of the companion clears the hide-during-focus flag (SPEC §4.10).
    pub fn clear_hidden_for_focus(&mut self) {
        self.hidden_for_focus = false;
    }

    /// Static limits (for `get_app_info`).
    pub fn limits(&self) -> crate::snapshot::Limits {
        crate::snapshot::Limits {
            presets: self.cfg.presets.clone(),
            min_minutes: self.cfg.min_minutes,
            max_minutes: self.cfg.max_minutes,
            break_minutes: self.cfg.break_minutes,
            max_task_label_chars: self.cfg.max_task_label_chars,
        }
    }

    pub fn phase(&self) -> Phase {
        match (&self.active, &self.awaiting) {
            (Some(s), _) if s.kind == SessionKind::Focus => Phase::Focus,
            (Some(_), _) => Phase::Break,
            (None, Some(_)) => Phase::AwaitingChoice,
            (None, None) => Phase::Idle,
        }
    }

    pub fn active(&self) -> Option<&ActiveSession> {
        self.active.as_ref()
    }

    pub fn is_running(&self) -> bool {
        self.active.as_ref().is_some_and(|s| s.is_running())
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn friendship(&self) -> &Friendship {
        &self.friendship
    }

    pub fn unlock_table(&self) -> &UnlockTable {
        &self.table
    }

    pub fn config(&self) -> &EngineConfig {
        &self.cfg
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
}
