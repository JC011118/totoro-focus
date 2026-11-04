//! The engine: one authoritative state machine for sessions, rewards and
//! history. It performs no I/O. Every call takes `Now`; results are `Effect`s
//! the app shell applies (save, emit, sound, show/hide, hotkey, App Nap).
//! See docs/SPEC.md §4 for the exact rules this implements — in particular the
//! call pipeline (§4.0), `observe` (§4.5), Engine::new steps (§4.7) and effect
//! ordering (§4.9).

use std::sync::Arc;

use chrono::{FixedOffset, TimeDelta, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::clock::{Clock, Mono, Now, SystemClock, Wall};
use crate::history::{offset_minutes, EndReason, History, Outcome, SessionRecord};
use crate::rewards::{AwardOutcome, Friendship, UnlockKind, UnlockTable};
use crate::settings::{Settings, SettingsPatch};
use crate::snapshot::{
    format_countdown, AwaitingView, FriendshipView, NextUnlock, Phase, SessionView, Snapshot,
};
use crate::store::{PersistedState, SCHEMA_VERSION};
use crate::timer::{
    normalize_task_label, ActiveSession, PauseReason, PersistedSession, RunState, RunStatus,
    SessionKind,
};

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
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
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
    ActivityChanged {
        level: ActivityLevel,
    },
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(
    tag = "code",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
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
    /// Supplies local UTC offsets for records the engine writes (never time itself).
    offsets: Arc<dyn Clock>,
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
    ///
    /// Records written by the engine take their `endedUtcOffsetMinutes` from the
    /// system time zone (same as [`SystemClock`]); use [`Engine::new_with_clock`] to
    /// inject another offset source (tests use the fake clock).
    pub fn new(
        cfg: EngineConfig,
        table: UnlockTable,
        state: PersistedState,
        now: Now,
        ids: IdSource,
    ) -> (Self, Vec<Effect>) {
        Self::new_with_clock(cfg, table, state, now, ids, Arc::new(SystemClock::new()))
    }

    /// Like [`Engine::new`], with `clock` supplying the local UTC offset stamped on
    /// records the engine writes (`endedUtcOffsetMinutes`). Only
    /// `Clock::local_offset` is used: time itself always comes from the `Now` passed to
    /// each call.
    pub fn new_with_clock(
        cfg: EngineConfig,
        table: UnlockTable,
        state: PersistedState,
        now: Now,
        ids: IdSource,
        clock: Arc<dyn Clock>,
    ) -> (Self, Vec<Effect>) {
        let PersistedState {
            settings: stored_settings,
            active_session,
            history,
            friendship: stored_friendship,
            ..
        } = state;
        let mut changed = false;

        // 1. Sanitize settings.
        let mut settings = stored_settings
            .clone()
            .sanitized(cfg.min_minutes, cfg.max_minutes);
        // 2. Reconcile friendship (never removes anything).
        let mut friendship = stored_friendship.clone();
        friendship.reconcile(&table);
        changed |= friendship != stored_friendship;
        // The chosen palette must still be selectable (§4.10).
        if !palette_allowed(&table, &friendship, &settings.palette) {
            log::warn!(
                "palette {:?} is not available; using auto",
                settings.palette
            );
            settings.palette = Settings::default().palette;
        }
        changed |= settings != stored_settings;
        // 3. Dedupe history by id, keeping the first.
        let mut history = History::new(history);
        changed |= history.dedupe();

        let mut engine = Engine {
            cfg,
            table,
            settings,
            history,
            friendship,
            active: None,
            awaiting: None,
            baseline: Some(now),
            suspended: false,
            activity: ActivityLevel::None,
            last_persist: None,
            hidden_for_focus: false,
            revision: 0,
            ids,
            offsets: clock,
        };
        let mut fx = Fx::default();

        // 4–5. Validate and restore the checkpoint (paused), or complete it silently.
        if let Some(p) = active_session {
            changed |= engine.restore_checkpoint(p, now);
        }
        // 6. Expire a restored pause that is too old (closed-app time counts).
        engine.expire_if_due(now, &mut fx);
        // 7. Not-persisted fields: set above (baseline = now, not suspended, activity
        //    none, not hidden).

        fx.persist |= changed;
        if let Some(s) = &engine.active {
            fx.transitions.push(Transition::Restored {
                session_id: s.id.clone(),
                kind: s.kind,
            });
        }
        // Never ShowCompanion or ActivityChanged at construction.
        fx.companion.clear();
        (engine, fx.into_effects(None))
    }

    pub fn command(&mut self, cmd: Command, now: Now) -> Result<Vec<Effect>, CommandError> {
        // 1. Resolve and validate against the pre-observation state (Err ⇒ unchanged).
        let intent = self.resolve(cmd)?;
        let before = self.observable();
        let session_before = self.active.as_ref().map(|s| s.id.clone());
        let mut fx = Fx::default();
        // 2. Observe, exactly like a Tick.
        self.observe(now, &mut fx);
        let ended_by_observation = session_before.is_some()
            && self.active.as_ref().map(|s| &s.id) != session_before.as_ref();
        // 3. Apply the intent to the post-observation state (overtaken intents are no-ops).
        self.apply(intent, now, ended_by_observation, &mut fx);
        // 4. A user command ends any suspended interval.
        self.suspended = false;
        self.baseline = Some(max_by_mono(self.baseline, now));
        Ok(self.finish(before, fx))
    }

    pub fn system(&mut self, event: SystemEvent, now: Now) -> Vec<Effect> {
        let before = self.observable();
        let mut fx = Fx::default();
        match event {
            SystemEvent::Tick => {
                self.observe(now, &mut fx);
                let checkpoint_due = self
                    .last_persist
                    .is_none_or(|p| now.mono.since(p) >= self.cfg.checkpoint_interval_ms);
                if self.is_running() && checkpoint_due {
                    fx.persist = true;
                }
            }
            SystemEvent::Suspend => {
                self.observe(now, &mut fx);
                if self.is_running() {
                    self.pause_running(now.mono, now.wall, PauseReason::Suspend, now, &mut fx);
                }
                self.baseline = None;
                self.suspended = true;
            }
            SystemEvent::Wake => {
                if self.is_running() {
                    // Suspend was missed: pause at the last observation, never past it.
                    let b = self.baseline.unwrap_or(now);
                    self.pause_running(b.mono, b.wall, PauseReason::Suspend, now, &mut fx);
                    fx.persist = true;
                }
                self.suspended = false;
                self.expire_if_due(now, &mut fx);
                self.baseline = Some(max_by_mono(self.baseline, now));
            }
            SystemEvent::ClockChanged => self.observe(now, &mut fx),
        }
        self.finish(before, fx)
    }

    /// Full view model at `now` (pure; folds a running session at
    /// `min(now.mono, baseline.mono)`). `clock` supplies local offsets for the today
    /// summary. `seq` is 0 and `companion_visible` false: the shell stamps both.
    pub fn snapshot(&self, now: Now, clock: &dyn Clock) -> Snapshot {
        let at = self.fold_mono(now);
        let session = self.active.as_ref().map(|s| {
            let active_ms = s.active_ms(at);
            let remaining_ms = s.planned_ms.saturating_sub(active_ms);
            SessionView {
                id: s.id.clone(),
                kind: s.kind,
                status: s.status(),
                pause_reason: s.pause_reason(),
                task_label: s.task_label.clone(),
                planned_ms: s.planned_ms,
                active_ms,
                remaining_ms,
                remaining_label: format_countdown(remaining_ms),
                started_at: s.started_at,
            }
        });
        let active_focus_ms = self
            .active
            .as_ref()
            .filter(|s| s.kind == SessionKind::Focus)
            .map_or(0, |s| s.active_ms(at));
        let today = self
            .history
            .today(now.wall, &|w| clock.local_offset(w), active_focus_ms);
        Snapshot {
            seq: 0,
            revision: self.revision,
            phase: self.phase(),
            session,
            awaiting: self.awaiting.as_ref().map(|a| AwaitingView {
                after: a.after,
                record: a.record.clone(),
                award: a.award.clone(),
                long_session: a.long_session,
            }),
            today,
            friendship: self.friendship_view(),
            settings: self.settings.clone(),
            companion_visible: false,
        }
    }

    /// Everything to persist at `now` (pure), with the active session folded into a
    /// checkpoint at `min(now.mono, baseline.mono)` and `checkpointAt = now.wall`.
    pub fn persisted_state(&self, now: Now) -> PersistedState {
        let at = Now {
            mono: self.fold_mono(now),
            wall: now.wall,
        };
        PersistedState {
            schema_version: SCHEMA_VERSION,
            settings: self.settings.clone(),
            active_session: self.active.as_ref().map(|s| s.to_persisted(at)),
            history: self.history.records().to_vec(),
            friendship: self.friendship.clone(),
        }
    }

    /// Record that a save at `now` succeeded (resets the checkpoint interval).
    pub fn mark_persisted(&mut self, now: Now) {
        self.last_persist = Some(now.mono);
    }

    /// Milliseconds until the next useful tick (SPEC §8.2 Ticker): running → the next
    /// whole-second boundary of remaining time (`remaining % 1000`, or 1000 when 0),
    /// clamped to 50..=1000 (matches the ceil countdown format); paused → 15_000;
    /// idle/awaiting → ms until the next local midnight + 50, capped at 60_000.
    ///
    /// A running session with no time left (completion not yet observed) gets the
    /// minimum delay so the completing tick comes promptly.
    pub fn next_tick_delay_ms(&self, now: Now, clock: &dyn Clock) -> u64 {
        match &self.active {
            Some(s) if s.is_running() => {
                let remaining = s.remaining_ms(now.mono);
                if remaining == 0 {
                    return TICK_MIN_MS;
                }
                match remaining % 1000 {
                    0 => 1000,
                    r => r.clamp(TICK_MIN_MS, 1000),
                }
            }
            Some(_) => TICK_PAUSED_MS,
            None => ms_until_local_midnight(now.wall, clock.local_offset(now.wall))
                .map_or(TICK_IDLE_MAX_MS, |ms| {
                    ms.saturating_add(TICK_MIN_MS).min(TICK_IDLE_MAX_MS)
                }),
        }
    }

    /// Validate a replay request (SPEC §10): `FocusRunning` while a focus session is
    /// running; `key` must be `react` or an unlocked id of kind animation|idleVariant
    /// (else `NotUnlocked`).
    pub fn check_replay(&self, key: &str) -> Result<(), CommandError> {
        if self
            .active
            .as_ref()
            .is_some_and(|s| s.kind == SessionKind::Focus && s.is_running())
        {
            return Err(CommandError::FocusRunning);
        }
        if key == REACT_KEY {
            return Ok(());
        }
        let replayable = self.friendship.unlocked.contains(key)
            && self
                .table
                .get(key)
                .is_some_and(|u| matches!(u.kind, UnlockKind::Animation | UnlockKind::IdleVariant));
        if replayable {
            Ok(())
        } else {
            Err(CommandError::NotUnlocked {
                id: key.to_string(),
            })
        }
    }

    /// A user show/hide of the companion clears the hide-during-focus flag (SPEC §4.10).
    pub fn clear_hidden_for_focus(&mut self) {
        self.hidden_for_focus = false;
    }

    /// Whether the companion is currently hidden because of `hideDuringFocus`.
    pub fn hidden_for_focus(&self) -> bool {
        self.hidden_for_focus
    }

    /// The activity level last emitted as `ActivityChanged` (SPEC §4.8).
    pub fn activity(&self) -> ActivityLevel {
        self.activity
    }

    /// Between Suspend and Wake (or the next user command).
    pub fn is_suspended(&self) -> bool {
        self.suspended
    }

    /// The last observation (SPEC §4.5); `None` while suspended.
    pub fn baseline(&self) -> Option<Now> {
        self.baseline
    }

    /// What the companion offers after a session ended (`None` unless awaitingChoice).
    pub fn awaiting(&self) -> Option<&AwaitingChoice> {
        self.awaiting.as_ref()
    }

    /// Whether `id` may be chosen as `settings.palette`: "auto", "forest", or an
    /// unlocked id of kind palette (SPEC §4.10).
    pub fn palette_allowed(&self, id: &str) -> bool {
        palette_allowed(&self.table, &self.friendship, id)
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

    // ----------------------------------------------------------------------------
    // Pipeline pieces
    // ----------------------------------------------------------------------------

    /// SPEC §4.0 step 1: resolve the command against the current (pre-observation)
    /// state. Never mutates.
    fn resolve(&self, cmd: Command) -> Result<Intent, CommandError> {
        match cmd {
            Command::StartFocus {
                minutes,
                task_label,
            } => {
                if self.active.is_some() {
                    return Err(CommandError::SessionActive);
                }
                let minutes = minutes.unwrap_or(self.settings.last_focus_minutes);
                self.check_minutes(minutes)?;
                let label = task_label
                    .as_deref()
                    .and_then(|l| normalize_task_label(l, self.cfg.max_task_label_chars as usize));
                Ok(Intent::StartFocus { minutes, label })
            }
            Command::StartBreak { minutes } => {
                if self.active.is_some() {
                    return Err(CommandError::SessionActive);
                }
                let minutes = minutes.unwrap_or(self.cfg.break_minutes);
                self.check_minutes(minutes)?;
                Ok(Intent::StartBreak { minutes })
            }
            Command::Pause => self.require_session().map(|()| Intent::Pause),
            Command::Resume => self.require_session().map(|()| Intent::Resume),
            Command::End => self.require_session().map(|()| Intent::End),
            Command::Toggle => Ok(match &self.active {
                None => Intent::StartFocus {
                    minutes: self.settings.last_focus_minutes.clamp(
                        self.cfg.min_minutes,
                        self.cfg.max_minutes.max(self.cfg.min_minutes),
                    ),
                    label: None,
                },
                Some(s) if s.is_running() => Intent::Pause,
                Some(_) => Intent::Resume,
            }),
            Command::Dismiss => {
                if self.active.is_none() && self.awaiting.is_some() {
                    Ok(Intent::Dismiss)
                } else {
                    Err(CommandError::NotAwaitingChoice)
                }
            }
            Command::UpdateSettings(patch) => {
                let mut next = self.settings.clone();
                next.apply(&patch, self.cfg.min_minutes, self.cfg.max_minutes)
                    .map_err(|reason| CommandError::InvalidSettings { reason })?;
                if let Some(p) = &patch.palette {
                    if !self.palette_allowed(p) {
                        return Err(CommandError::InvalidSettings {
                            reason: format!("palette {p:?} is not unlocked"),
                        });
                    }
                }
                Ok(Intent::UpdateSettings(patch))
            }
            Command::Quit => Ok(Intent::Quit),
        }
    }

    fn check_minutes(&self, minutes: u32) -> Result<(), CommandError> {
        if (self.cfg.min_minutes..=self.cfg.max_minutes).contains(&minutes) {
            Ok(())
        } else {
            Err(CommandError::InvalidMinutes {
                min: self.cfg.min_minutes,
                max: self.cfg.max_minutes,
            })
        }
    }

    fn require_session(&self) -> Result<(), CommandError> {
        if self.active.is_some() {
            Ok(())
        } else {
            Err(CommandError::NoSession)
        }
    }

    /// SPEC §4.0 step 3: apply the resolved intent after observation. An intent that
    /// observation overtook is a no-op.
    fn apply(&mut self, intent: Intent, now: Now, ended_by_observation: bool, fx: &mut Fx) {
        match intent {
            Intent::StartFocus { minutes, label } => {
                if self.active.is_none() && !ended_by_observation {
                    self.start_focus(minutes, label, now, fx);
                }
            }
            Intent::StartBreak { minutes } => {
                if self.active.is_none() && !ended_by_observation {
                    self.start_break(minutes, now, fx);
                }
            }
            Intent::Pause => {
                if self.is_running() {
                    self.pause_running(now.mono, now.wall, PauseReason::User, now, fx);
                }
            }
            Intent::Resume => {
                if let Some(s) = self.active.as_mut() {
                    if !s.is_running() {
                        s.resume(now);
                        fx.persist = true;
                        fx.transitions.push(Transition::Resumed {
                            session_id: s.id.clone(),
                            kind: s.kind,
                        });
                    }
                }
            }
            Intent::End => self.end_session(now, fx),
            Intent::Dismiss => {
                if self.active.is_none() && self.awaiting.take().is_some() {
                    fx.transitions.push(Transition::Dismissed);
                }
            }
            Intent::UpdateSettings(patch) => self.update_settings(&patch, fx),
            Intent::Quit => {
                if self.is_running() {
                    self.pause_running(now.mono, now.wall, PauseReason::Quit, now, fx);
                }
                fx.persist = true;
            }
        }
    }

    /// SPEC §4.5 `observe(now)`.
    fn observe(&mut self, now: Now, fx: &mut Fx) {
        if !self.suspended {
            let running = self.active.as_ref().map(ActiveSession::is_running);
            match (self.baseline, running) {
                (Some(b), Some(true)) => {
                    let dm = now.mono.since(b.mono);
                    if dm > self.cfg.max_observation_gap_ms {
                        // The gap earns nothing.
                        self.pause_running(b.mono, b.wall, PauseReason::Suspend, now, fx);
                    } else if !self.settle(now.mono, now.wall, now, fx)
                        && skew_ms(b, now) > tolerance_i64(self.cfg.clock_jump_tolerance_ms)
                    {
                        self.pause_running(now.mono, now.wall, PauseReason::ClockJump, now, fx);
                    }
                }
                (Some(b), Some(false)) => {
                    let skew = skew_ms(b, now);
                    if skew.unsigned_abs() > self.cfg.clock_jump_tolerance_ms {
                        if let Some(s) = self.active.as_mut() {
                            let before = s.age_anchor();
                            s.shift_age_anchor(skew);
                            // The anchor is saved state (§4.7: every state change emits
                            // Persist), so a crash keeps the shift and the detected jump
                            // never counts as pause time (§4.6). Not observable: no
                            // revision bump.
                            if s.age_anchor() != before {
                                fx.persist = true;
                            }
                        }
                    }
                }
                (None, Some(true)) => {
                    // Defensive: no baseline to compare against.
                    self.settle(now.mono, now.wall, now, fx);
                }
                _ => {}
            }
        }
        self.expire_if_due(now, fx);
        if !self.suspended {
            self.baseline = Some(max_by_mono(self.baseline, now));
        }
    }

    /// SPEC §4.6: end a paused session whose age is strictly beyond the limit.
    fn expire_if_due(&mut self, now: Now, fx: &mut Fx) {
        let Some(s) = &self.active else {
            return;
        };
        let Some(age) = s.pause_age_ms(now.wall) else {
            return;
        };
        if age <= self.cfg.pause_expiry_ms {
            return;
        }
        let paused_at = s.paused_at().unwrap_or(now.wall);
        match s.kind {
            SessionKind::Focus => self.interrupt_focus(EndReason::PauseExpired, paused_at, now, fx),
            SessionKind::Break => self.end_break(fx),
        }
    }

    /// Fold a running session at `mono`; if that reaches planned, finish it (focus
    /// completion or break finished, `endedAt` back-dated by the overshoot from
    /// `wall`). Returns whether the session ended.
    fn settle(&mut self, mono: Mono, wall: Wall, now: Now, fx: &mut Fx) -> bool {
        let Some(s) = self.active.as_mut() else {
            return false;
        };
        if !s.is_running() {
            return false;
        }
        let overshoot = s.fold_overshoot(mono);
        if !s.is_due(mono) {
            return false;
        }
        let Some(s) = self.active.take() else {
            return false;
        };
        match s.kind {
            SessionKind::Focus => {
                let ended_at = wall_minus_ms(wall, overshoot).max(s.started_at);
                self.complete_focus(s, ended_at, now, fx);
            }
            SessionKind::Break => self.finish_break(s, fx),
        }
        true
    }

    /// Pause a running session at (`mono`, `wall`) — unless the fold completes it.
    fn pause_running(
        &mut self,
        mono: Mono,
        wall: Wall,
        reason: PauseReason,
        now: Now,
        fx: &mut Fx,
    ) {
        if self.settle(mono, wall, now, fx) {
            return;
        }
        let Some(s) = self.active.as_mut() else {
            return;
        };
        if !s.is_running() {
            return;
        }
        s.pause_at(mono, wall, reason);
        fx.persist = true;
        fx.transitions.push(Transition::Paused {
            session_id: s.id.clone(),
            kind: s.kind,
            reason,
        });
    }

    fn start_focus(&mut self, minutes: u32, label: Option<String>, now: Now, fx: &mut Fx) {
        let id = self.next_id();
        if self.settings.last_focus_minutes != minutes {
            let previous = self.settings.clone();
            self.settings.last_focus_minutes = minutes;
            fx.settings_changed(previous, self.settings.clone());
        }
        let planned_ms = u64::from(minutes) * 60_000;
        self.awaiting = None;
        self.active = Some(ActiveSession::new(
            id.clone(),
            SessionKind::Focus,
            label,
            planned_ms,
            now,
        ));
        if self.settings.hide_during_focus {
            self.hidden_for_focus = true;
            fx.companion.push(Effect::HideCompanion);
        }
        fx.persist = true;
        fx.transitions.push(Transition::FocusStarted {
            session_id: id,
            planned_ms,
        });
    }

    fn start_break(&mut self, minutes: u32, now: Now, fx: &mut Fx) {
        let id = self.next_id();
        let planned_ms = u64::from(minutes) * 60_000;
        self.awaiting = None;
        self.active = Some(ActiveSession::new(
            id.clone(),
            SessionKind::Break,
            None,
            planned_ms,
            now,
        ));
        fx.persist = true;
        fx.transitions.push(Transition::BreakStarted {
            session_id: id,
            planned_ms,
        });
    }

    fn end_session(&mut self, now: Now, fx: &mut Fx) {
        let Some(kind) = self.active.as_ref().map(|s| s.kind) else {
            return;
        };
        // A session whose active time has reached planned is always completed.
        if self.settle(now.mono, now.wall, now, fx) {
            return;
        }
        match kind {
            SessionKind::Focus => self.interrupt_focus(EndReason::UserEnded, now.wall, now, fx),
            SessionKind::Break => self.end_break(fx),
        }
    }

    fn update_settings(&mut self, patch: &SettingsPatch, fx: &mut Fx) {
        let previous = self.settings.clone();
        let mut next = previous.clone();
        if let Err(e) = next.apply(patch, self.cfg.min_minutes, self.cfg.max_minutes) {
            // Validated in `resolve`; observation cannot invalidate a patch.
            log::error!("settings patch rejected after validation: {e}");
            return;
        }
        if next == previous {
            return;
        }
        self.settings = next;
        fx.persist = true;
        fx.settings_changed(previous, self.settings.clone());
        if self.hidden_for_focus && !self.settings.hide_during_focus {
            self.show_if_hidden(fx);
        }
    }

    // ----------------------------------------------------------------------------
    // Transactions
    // ----------------------------------------------------------------------------

    /// SPEC §4.2 steps 1–4 (record + award), shared by the live and silent paths.
    fn record_completion(
        &mut self,
        s: &ActiveSession,
        ended_at: Wall,
        now: Now,
    ) -> (SessionRecord, Option<AwardOutcome>, bool) {
        let active_ms = s.accrued_ms.min(s.planned_ms);
        let record = self.make_record(
            s,
            ended_at,
            active_ms,
            Outcome::Completed,
            EndReason::Completed,
        );
        self.history.push_once(record.clone());
        let award = self
            .friendship
            .award(&s.id, active_ms, true, &self.table, now.wall);
        let long_session =
            s.planned_ms >= u64::from(self.table.rules.long_session_minutes) * 60_000;
        (record, award, long_session)
    }

    /// SPEC §4.2: completion transaction with its effects.
    fn complete_focus(&mut self, s: ActiveSession, ended_at: Wall, now: Now, fx: &mut Fx) {
        let (record, award, long_session) = self.record_completion(&s, ended_at, now);
        self.awaiting = Some(AwaitingChoice {
            after: SessionKind::Focus,
            record: Some(record.clone()),
            award: award.clone(),
            long_session,
        });
        fx.persist = true;
        self.show_if_hidden(fx);
        fx.transitions.push(Transition::FocusCompleted {
            record,
            award,
            long_session,
        });
        if self.settings.sound {
            fx.sounds.push(SoundCue::Completion);
        }
    }

    /// SPEC §4.3: interruption transaction (user end or pause expiry). A focus session
    /// that has reached planned is completed instead.
    fn interrupt_focus(&mut self, reason: EndReason, ended_base: Wall, now: Now, fx: &mut Fx) {
        let Some(s) = self.active.take() else {
            return;
        };
        let ended_at = ended_base.max(s.started_at);
        if s.accrued_ms >= s.planned_ms {
            self.complete_focus(s, ended_at, now, fx);
            return;
        }
        let active_ms = s.accrued_ms;
        let record = self.make_record(&s, ended_at, active_ms, Outcome::Interrupted, reason);
        self.history.push_once(record.clone());
        let award = self
            .friendship
            .award(&s.id, active_ms, false, &self.table, now.wall);
        self.awaiting = None;
        fx.persist = true;
        self.show_if_hidden(fx);
        fx.transitions
            .push(Transition::FocusInterrupted { record, award });
    }

    /// SPEC §4.4: a break that ran to zero.
    fn finish_break(&mut self, s: ActiveSession, fx: &mut Fx) {
        self.awaiting = Some(AwaitingChoice {
            after: SessionKind::Break,
            record: None,
            award: None,
            long_session: false,
        });
        fx.persist = true;
        fx.transitions
            .push(Transition::BreakFinished { session_id: s.id });
        if self.settings.sound {
            fx.sounds.push(SoundCue::BreakOver);
        }
    }

    /// SPEC §4.4: a break ended by the user or by pause expiry → idle.
    fn end_break(&mut self, fx: &mut Fx) {
        let Some(s) = self.active.take() else {
            return;
        };
        self.awaiting = None;
        fx.persist = true;
        fx.transitions
            .push(Transition::BreakEnded { session_id: s.id });
    }

    fn make_record(
        &self,
        s: &ActiveSession,
        ended_at: Wall,
        active_ms: u64,
        outcome: Outcome,
        end_reason: EndReason,
    ) -> SessionRecord {
        SessionRecord {
            id: s.id.clone(),
            kind: SessionKind::Focus,
            task_label: s.task_label.clone(),
            started_at: s.started_at,
            ended_at,
            planned_ms: s.planned_ms,
            active_ms,
            outcome,
            end_reason,
            ended_utc_offset_minutes: Some(offset_minutes(self.offsets.local_offset(ended_at))),
        }
    }

    fn show_if_hidden(&mut self, fx: &mut Fx) {
        if self.hidden_for_focus {
            self.hidden_for_focus = false;
            fx.companion.push(Effect::ShowCompanion);
        }
    }

    /// SPEC §4.7 steps 4–5 for one checkpoint. Returns whether the stored state changed.
    fn restore_checkpoint(&mut self, p: PersistedSession, now: Now) -> bool {
        let min_ms = u64::from(self.cfg.min_minutes.max(1)) * 60_000;
        let max_ms = u64::from(self.cfg.max_minutes) * 60_000;
        if p.id.is_empty()
            || self.history.contains(&p.id)
            || self.friendship.awards.contains_key(&p.id)
            || p.planned_ms < min_ms
            || p.planned_ms > max_ms
        {
            log::warn!(
                "dropping checkpoint {:?}: already recorded, already awarded, or invalid planned duration {}",
                p.id,
                p.planned_ms
            );
            return true;
        }
        let checkpoint_at = p.checkpoint_at;
        let original = p.clone();
        let mut s = ActiveSession::from_persisted(p);
        s.clamp_pause_stamps(now.wall);
        if s.kind == SessionKind::Focus && s.accrued_ms >= s.planned_ms {
            // Fully served before the save: complete silently (no sound, no transition).
            let ended_at = checkpoint_at.min(now.wall).max(s.started_at);
            self.record_completion(&s, ended_at, now);
            return true;
        }
        // Restored paused; persist when that differs from what was stored.
        let changed = s.to_persisted(Now {
            mono: now.mono,
            wall: checkpoint_at,
        }) != original;
        self.active = Some(s);
        changed
    }

    // ----------------------------------------------------------------------------
    // Helpers
    // ----------------------------------------------------------------------------

    /// Effects, ActivityChanged and revision at the end of every successful call.
    fn finish(&mut self, before: Observable, fx: Fx) -> Vec<Effect> {
        let level = self.activity_level();
        let activity = if level != self.activity {
            self.activity = level;
            Some(level)
        } else {
            None
        };
        if self.observable() != before {
            self.revision += 1;
        }
        fx.into_effects(activity)
    }

    fn activity_level(&self) -> ActivityLevel {
        match &self.active {
            Some(s) if s.is_running() => match s.kind {
                SessionKind::Focus => ActivityLevel::Focus,
                SessionKind::Break => ActivityLevel::Break,
            },
            _ => ActivityLevel::None,
        }
    }

    fn observable(&self) -> Observable {
        Observable {
            phase: self.phase(),
            session: self
                .active
                .as_ref()
                .map(|s| (s.id.clone(), s.status(), s.pause_reason())),
            awaiting: self
                .awaiting
                .as_ref()
                .map(|a| (a.after, a.record.as_ref().map(|r| r.id.clone()))),
            settings: self.settings.clone(),
            history_len: self.history.len(),
            total_points: self.friendship.total_points,
            unlocked: self.friendship.unlocked.len(),
            awards: self.friendship.awards.len(),
        }
    }

    /// Where a running session may be folded without passing the last observation.
    fn fold_mono(&self, now: Now) -> Mono {
        match (self.baseline, &self.active) {
            (Some(b), _) => b.mono.min(now.mono),
            (
                None,
                Some(ActiveSession {
                    run: RunState::Running { since },
                    ..
                }),
            ) => *since,
            (None, _) => now.mono,
        }
    }

    fn friendship_view(&self) -> FriendshipView {
        let unlocked_set = &self.friendship.unlocked;
        let mut unlocked: Vec<String> = self
            .table
            .unlocks
            .iter()
            .filter(|u| unlocked_set.contains(&u.id))
            .map(|u| u.id.clone())
            .collect();
        unlocked.extend(
            unlocked_set
                .iter()
                .filter(|id| self.table.get(id).is_none())
                .cloned(),
        );
        let total = self.friendship.total_points;
        FriendshipView {
            total_points: total,
            unlocked,
            next: self.table.next_after(total).map(|u| NextUnlock {
                id: u.id.clone(),
                label: u.label.clone(),
                points: u.points,
                remaining: u.points.saturating_sub(total),
            }),
        }
    }

    /// A fresh session id that is not already used by history, awards or the session.
    fn next_id(&mut self) -> String {
        let mut attempt = 0u32;
        loop {
            let raw = (self.ids)();
            let id = if attempt < 8 {
                raw
            } else {
                format!("{raw}-{attempt}")
            };
            if !id.is_empty() && !self.id_in_use(&id) {
                return id;
            }
            attempt += 1;
        }
    }

    fn id_in_use(&self, id: &str) -> bool {
        self.history.contains(id)
            || self.friendship.awards.contains_key(id)
            || self.active.as_ref().is_some_and(|s| s.id == id)
    }
}

/// Key that is always replayable (the click reaction).
pub const REACT_KEY: &str = "react";
/// Palette ids that are always selectable (`auto` chooses by time and season).
pub const ALWAYS_AVAILABLE_PALETTES: [&str; 2] = ["auto", "forest"];
/// Shortest ticker delay.
pub const TICK_MIN_MS: u64 = 50;
/// Ticker delay while a session is paused.
pub const TICK_PAUSED_MS: u64 = 15_000;
/// Longest ticker delay while idle or awaiting a choice.
pub const TICK_IDLE_MAX_MS: u64 = 60_000;

fn palette_allowed(table: &UnlockTable, friendship: &Friendship, id: &str) -> bool {
    ALWAYS_AVAILABLE_PALETTES.contains(&id)
        || (friendship.unlocked.contains(id) && table.is_kind(id, UnlockKind::Palette))
}

/// A command resolved and validated against the pre-observation state (SPEC §4.0).
enum Intent {
    StartFocus { minutes: u32, label: Option<String> },
    StartBreak { minutes: u32 },
    Pause,
    Resume,
    End,
    Dismiss,
    UpdateSettings(SettingsPatch),
    Quit,
}

/// Effects collected during one call, emitted in SPEC §4.9 order.
#[derive(Default)]
struct Fx {
    persist: bool,
    settings: Option<(Settings, Settings)>,
    companion: Vec<Effect>,
    transitions: Vec<Transition>,
    sounds: Vec<SoundCue>,
}

impl Fx {
    fn settings_changed(&mut self, previous: Settings, current: Settings) {
        match &mut self.settings {
            Some((_, cur)) => *cur = current,
            None => self.settings = Some((previous, current)),
        }
    }

    fn into_effects(self, activity: Option<ActivityLevel>) -> Vec<Effect> {
        let mut out = Vec::new();
        if self.persist {
            out.push(Effect::Persist);
        }
        if let Some((previous, current)) = self.settings {
            if previous != current {
                out.push(Effect::SettingsChanged {
                    previous: Box::new(previous),
                    current: Box::new(current),
                });
            }
        }
        out.extend(self.companion);
        if let Some(level) = activity {
            out.push(Effect::ActivityChanged { level });
        }
        out.extend(self.transitions.into_iter().map(Effect::Transition));
        out.extend(self.sounds.into_iter().map(Effect::PlaySound));
        out
    }
}

/// The observable state whose change bumps `revision` (SPEC §4.9).
#[derive(PartialEq)]
struct Observable {
    phase: Phase,
    session: Option<(String, RunStatus, Option<PauseReason>)>,
    awaiting: Option<(SessionKind, Option<String>)>,
    settings: Settings,
    history_len: usize,
    total_points: u64,
    unlocked: usize,
    awards: usize,
}

/// The later of the baseline and `now` by monotonic time (the baseline never moves back).
fn max_by_mono(baseline: Option<Now>, now: Now) -> Now {
    match baseline {
        Some(b) if b.mono > now.mono => b,
        _ => now,
    }
}

/// Wall-minus-monotonic divergence since `b`, in ms (positive = wall ran ahead).
fn skew_ms(b: Now, now: Now) -> i64 {
    let wall = (now.wall - b.wall).num_milliseconds();
    let dm = i64::try_from(now.mono.since(b.mono)).unwrap_or(i64::MAX);
    wall.saturating_sub(dm)
}

fn tolerance_i64(ms: u64) -> i64 {
    i64::try_from(ms).unwrap_or(i64::MAX)
}

fn wall_minus_ms(w: Wall, ms: u64) -> Wall {
    i64::try_from(ms)
        .ok()
        .and_then(TimeDelta::try_milliseconds)
        .and_then(|d| w.checked_sub_signed(d))
        .unwrap_or(w)
}

/// Milliseconds from `now` to the next local midnight under `offset`.
fn ms_until_local_midnight(now: Wall, offset: FixedOffset) -> Option<u64> {
    let local = now.with_timezone(&offset);
    let midnight = local.date_naive().succ_opt()?.and_hms_opt(0, 0, 0)?;
    let midnight = offset
        .from_local_datetime(&midnight)
        .single()?
        .with_timezone(&Utc);
    u64::try_from((midnight - now).num_milliseconds()).ok()
}
