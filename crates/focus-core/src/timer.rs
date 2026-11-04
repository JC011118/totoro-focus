//! The active session: accumulated active time against a planned duration.
//!
//! Remaining time is always `planned - active`, where active time is folded
//! from monotonic deltas while running. Ticks only refresh; wall time never
//! adds active time. See docs/SPEC.md §3–4.

use chrono::TimeDelta;
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
        Self {
            id,
            kind,
            task_label,
            planned_ms,
            started_at: now.wall,
            accrued_ms: 0,
            run: RunState::Running { since: now.mono },
        }
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

    /// The true wall stamp of the current pause (`None` while running).
    pub fn paused_at(&self) -> Option<Wall> {
        match self.run {
            RunState::Paused { paused_at, .. } => Some(paused_at),
            RunState::Running { .. } => None,
        }
    }

    /// The pause-age anchor (`None` while running).
    pub fn age_anchor(&self) -> Option<Wall> {
        match self.run {
            RunState::Paused { age_anchor, .. } => Some(age_anchor),
            RunState::Running { .. } => None,
        }
    }

    /// Active time at monotonic instant `at`, capped at `planned_ms`. Does not mutate.
    pub fn active_ms(&self, at: Mono) -> u64 {
        self.uncapped_active_ms(at).min(self.planned_ms)
    }

    /// Active time at `at` before the `planned_ms` cap (used for overshoot).
    fn uncapped_active_ms(&self, at: Mono) -> u64 {
        match self.run {
            RunState::Running { since } => self.accrued_ms.saturating_add(at.since(since)),
            RunState::Paused { .. } => self.accrued_ms,
        }
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
        self.fold_overshoot(at);
    }

    /// Like [`fold`](Self::fold), and returns the overshoot: the folded active time
    /// beyond `planned_ms` that the cap discarded (0 when not due). The completion
    /// transaction back-dates `endedAt` by it (SPEC §4.2).
    pub fn fold_overshoot(&mut self, at: Mono) -> u64 {
        let RunState::Running { since } = self.run else {
            return 0;
        };
        if at <= since {
            return 0;
        }
        let raw = self.uncapped_active_ms(at);
        self.accrued_ms = raw.min(self.planned_ms);
        self.run = RunState::Running { since: at };
        raw.saturating_sub(self.planned_ms)
    }

    /// Fold at `mono`, then pause with `paused_at = age_anchor = wall`. No-op if already paused.
    pub fn pause_at(&mut self, mono: Mono, wall: Wall, reason: PauseReason) {
        if !self.is_running() {
            return;
        }
        self.fold(mono);
        self.run = RunState::Paused {
            paused_at: wall,
            reason,
            age_anchor: wall,
        };
    }

    /// Resume from pause, accruing from `now.mono`. No-op if running.
    pub fn resume(&mut self, now: Now) {
        if self.is_running() {
            return;
        }
        self.run = RunState::Running { since: now.mono };
    }

    /// Pause age for expiry: `now - age_anchor`, clamped at zero. `None` while running.
    pub fn pause_age_ms(&self, now: Wall) -> Option<u64> {
        let anchor = self.age_anchor()?;
        let age = (now - anchor).num_milliseconds();
        Some(u64::try_from(age).unwrap_or(0))
    }

    /// Shift the pause-age anchor by `skew_ms` (positive = forward) while paused.
    pub fn shift_age_anchor(&mut self, skew_ms: i64) {
        if let RunState::Paused { age_anchor, .. } = &mut self.run {
            if let Some(shifted) =
                TimeDelta::try_milliseconds(skew_ms).and_then(|d| age_anchor.checked_add_signed(d))
            {
                *age_anchor = shifted;
            }
        }
    }

    /// Clamp `paused_at` and `age_anchor` to at most `limit` (used at restore when the
    /// clock moved back while the app was closed). Returns whether anything changed.
    pub fn clamp_pause_stamps(&mut self, limit: Wall) -> bool {
        let RunState::Paused {
            paused_at,
            age_anchor,
            ..
        } = &mut self.run
        else {
            return false;
        };
        let mut changed = false;
        if *paused_at > limit {
            *paused_at = limit;
            changed = true;
        }
        if *age_anchor > limit {
            *age_anchor = limit;
            changed = true;
        }
        changed
    }

    /// Checkpoint view at `now` (running time folded in, without mutating self).
    pub fn to_persisted(&self, now: Now) -> PersistedSession {
        let (paused_at, pause_reason, age_anchor) = match self.run {
            RunState::Running { .. } => (None, None, None),
            RunState::Paused {
                paused_at,
                reason,
                age_anchor,
            } => (Some(paused_at), Some(reason), Some(age_anchor)),
        };
        PersistedSession {
            id: self.id.clone(),
            kind: self.kind,
            task_label: self.task_label.clone(),
            planned_ms: self.planned_ms,
            started_at: self.started_at,
            accrued_ms: self.active_ms(now.mono),
            status: self.status(),
            checkpoint_at: now.wall,
            paused_at,
            pause_reason,
            age_anchor,
        }
    }

    /// Restore from a checkpoint. Always returns a PAUSED session: a running
    /// checkpoint becomes `Paused{reason: Restored, paused_at = age_anchor = checkpoint_at}`;
    /// a paused checkpoint keeps its reason, `paused_at` and `age_anchor`
    /// (falling back to `checkpoint_at` when absent). `accrued_ms` is clamped to `planned_ms`.
    ///
    /// For a paused checkpoint a missing `age_anchor` falls back to `paused_at`, then to
    /// `checkpoint_at`; a missing reason becomes `Restored`.
    pub fn from_persisted(p: PersistedSession) -> Self {
        let run = match p.status {
            RunStatus::Running => RunState::Paused {
                paused_at: p.checkpoint_at,
                reason: PauseReason::Restored,
                age_anchor: p.checkpoint_at,
            },
            RunStatus::Paused => {
                let paused_at = p.paused_at.unwrap_or(p.checkpoint_at);
                RunState::Paused {
                    paused_at,
                    reason: p.pause_reason.unwrap_or(PauseReason::Restored),
                    age_anchor: p.age_anchor.unwrap_or(paused_at),
                }
            }
        };
        Self {
            id: p.id,
            kind: p.kind,
            task_label: p.task_label,
            planned_ms: p.planned_ms,
            started_at: p.started_at,
            accrued_ms: p.accrued_ms.min(p.planned_ms),
            run,
        }
    }
}

/// Task label normalization (SPEC §3), never an error: every `char::is_control` becomes
/// a space, whitespace runs collapse to one space, the result is trimmed; empty → `None`;
/// otherwise the first `max_chars` Unicode scalar values are kept and trimmed again.
pub fn normalize_task_label(raw: &str, max_chars: usize) -> Option<String> {
    let replaced: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let collapsed = replaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let truncated: String = collapsed.chars().take(max_chars).collect();
    let trimmed = truncated.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
