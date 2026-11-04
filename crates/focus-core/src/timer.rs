//! The active session: accumulated active time against a planned duration.
//!
//! Remaining time is always `planned - active`, where active time is folded
//! from monotonic deltas while running. Ticks only refresh; wall time never
//! adds active time. See docs/SPEC.md §3–4.

use serde::{Deserialize, Serialize};

use crate::clock::{Mono, Now, Wall};

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum SessionKind {
    Focus,
    Break,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum PauseReason {
    /// The user pressed pause (widget, tray or hotkey).
    User,
    /// The system went to sleep, or an observation gap implied it.
    Suspend,
    /// The wall clock jumped forward unexplained while running (possible missed sleep).
    ClockJump,
    /// The app quit with a session in progress.
    Quit,
    /// Restored from a running checkpoint after a crash/relaunch.
    Restored,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum RunStatus {
    Running,
    Paused,
}

/// In-memory run state. `Mono` never leaves the process.
#[derive(Clone, Debug, PartialEq)]
pub enum RunState {
    /// Active time accrues from `since`.
    Running { since: Mono },
    /// `paused_at` is the true wall stamp; `age_anchor` starts equal to it and is
    /// shifted by detected wall-clock jumps so they never count toward expiry.
    Paused {
        paused_at: Wall,
        reason: PauseReason,
        age_anchor: Wall,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActiveSession {
    pub id: String,
    pub kind: SessionKind,
    pub task_label: Option<String>,
    pub planned_ms: u64,
    pub started_at: Wall,
    /// Active time folded into the session so far (never exceeds `planned_ms`).
    pub accrued_ms: u64,
    pub run: RunState,
}

/// Checkpoint format of the active session inside `state.json`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PersistedSession {
    pub id: String,
    pub kind: SessionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_label: Option<String>,
    pub planned_ms: u64,
    pub started_at: Wall,
    pub accrued_ms: u64,
    pub status: RunStatus,
    /// Wall time this checkpoint was taken.
    pub checkpoint_at: Wall,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused_at: Option<Wall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<PauseReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age_anchor: Option<Wall>,
}

impl ActiveSession {
    /// A new session, running from `now`.
    pub fn new(
        id: String,
        kind: SessionKind,
        task_label: Option<String>,
        planned_ms: u64,
        now: Now,
    ) -> Self {
        let _ = (id, kind, task_label, planned_ms, now);
        todo!("timer: ActiveSession::new")
    }

    pub fn is_running(&self) -> bool {
        matches!(self.run, RunState::Running { .. })
    }

    pub fn status(&self) -> RunStatus {
        if self.is_running() {
            RunStatus::Running
        } else {
            RunStatus::Paused
        }
    }

    pub fn pause_reason(&self) -> Option<PauseReason> {
        match self.run {
            RunState::Paused { reason, .. } => Some(reason),
            RunState::Running { .. } => None,
        }
    }

    /// Active time at monotonic instant `at`, capped at `planned_ms`. Does not mutate.
    pub fn active_ms(&self, at: Mono) -> u64 {
        let _ = at;
        todo!("timer: active_ms")
    }

    pub fn remaining_ms(&self, at: Mono) -> u64 {
        self.planned_ms.saturating_sub(self.active_ms(at))
    }

    /// True when active time has reached the planned duration.
    pub fn is_due(&self, at: Mono) -> bool {
        self.active_ms(at) >= self.planned_ms
    }

    /// Move running time up to `at` into `accrued_ms` (capped) and restart `since` at `at`.
    /// No-op while paused. `at` earlier than `since` folds nothing.
    pub fn fold(&mut self, at: Mono) {
        let _ = at;
        todo!("timer: fold")
    }

    /// Fold at `mono`, then pause with `paused_at = age_anchor = wall`. No-op if already paused.
    pub fn pause_at(&mut self, mono: Mono, wall: Wall, reason: PauseReason) {
        let _ = (mono, wall, reason);
        todo!("timer: pause_at")
    }

    /// Resume from pause, accruing from `now.mono`. No-op if running.
    pub fn resume(&mut self, now: Now) {
        let _ = now;
        todo!("timer: resume")
    }

    /// Pause age for expiry: `now - age_anchor`, clamped at zero. `None` while running.
    pub fn pause_age_ms(&self, now: Wall) -> Option<u64> {
        let _ = now;
        todo!("timer: pause_age_ms")
    }

    /// Shift the pause-age anchor by `skew_ms` (positive = forward) while paused.
    pub fn shift_age_anchor(&mut self, skew_ms: i64) {
        let _ = skew_ms;
        todo!("timer: shift_age_anchor")
    }

    /// Checkpoint view at `now` (running time folded in, without mutating self).
    pub fn to_persisted(&self, now: Now) -> PersistedSession {
        let _ = now;
        todo!("timer: to_persisted")
    }

    /// Restore from a checkpoint. Always returns a PAUSED session: a running
    /// checkpoint becomes `Paused{reason: Restored, paused_at = age_anchor = checkpoint_at}`;
    /// a paused checkpoint keeps its reason, `paused_at` and `age_anchor`
    /// (falling back to `checkpoint_at` when absent). `accrued_ms` is clamped to `planned_ms`.
    pub fn from_persisted(p: PersistedSession) -> Self {
        let _ = p;
        todo!("timer: from_persisted")
    }
}
