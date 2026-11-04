//! The engine: one authoritative state machine for sessions, rewards and
//! history. It performs no I/O. Every call takes `Now`; results are `Effect`s
//! the app shell applies (save, emit, sound, show/hide, hotkey, App Nap).
//! See docs/SPEC.md §4 for the exact rules this implements.

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

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Save `engine.persisted_state(now)` atomically, then call `engine.mark_persisted(now)`.
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
    /// Hold (true) or release (false) the App Nap activity assertion.
    ActivityChanged { running: bool },
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(tag = "code", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum CommandError {
    #[error("a session is already in progress")]
    SessionActive,
    #[error("there is no session in progress")]
    NoSession,
    #[error("the session is not running")]
    NotRunning,
    #[error("the session is not paused")]
    NotPaused,
    #[error("nothing is waiting for a choice")]
    NotAwaitingChoice,
    #[error("duration must be between {min} and {max} minutes")]
    InvalidMinutes { min: u32, max: u32 },
    #[error("invalid settings: {reason}")]
    InvalidSettings { reason: String },
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
    /// Last observation (SPEC §4.5). Cleared on suspend/wake.
    baseline: Option<Now>,
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
    /// Build from loaded state. Sanitizes settings, reconciles friendship,
    /// restores any checkpoint as a PAUSED session (SPEC §4.7) and immediately
    /// applies pause expiry (§4.6). Returns startup effects (e.g. `Transition::Restored`,
    /// `Persist` if anything was repaired or expired, `ShowCompanion`).
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

    /// Full view model at `now`. `clock` supplies local offsets for the today summary.
    pub fn snapshot(&self, now: Now, clock: &dyn Clock) -> Snapshot {
        let _ = (now, clock);
        todo!("engine: snapshot")
    }

    /// Everything to persist at `now`, with the active session folded into a checkpoint.
    pub fn persisted_state(&self, now: Now) -> PersistedState {
        let _ = now;
        todo!("engine: persisted_state")
    }

    /// Record that a save at `now` succeeded (resets the checkpoint interval).
    pub fn mark_persisted(&mut self, now: Now) {
        self.last_persist = Some(now.mono);
    }

    /// Milliseconds until the next useful tick: the next whole-second boundary of the
    /// remaining time while running (clamped to 50..=1000), otherwise 1000.
    pub fn next_tick_delay_ms(&self, now: Now) -> u64 {
        let _ = now;
        todo!("engine: next_tick_delay_ms")
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
