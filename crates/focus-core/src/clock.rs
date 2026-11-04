//! Clock injection. Durations come only from the monotonic clock; the wall
//! clock stamps records, finds the local calendar day, and ages pauses.

use std::sync::Mutex;
use std::time::Instant;

use chrono::{DateTime, Duration as ChronoDuration, FixedOffset, Local, Offset, TimeZone, Utc};

/// Milliseconds on a monotonic clock with an arbitrary per-process origin.
/// Never persisted and never compared across processes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Mono(pub u64);

impl Mono {
    /// Milliseconds elapsed from `earlier` to `self`, saturating at zero.
    pub fn since(self, earlier: Mono) -> u64 {
        self.0.saturating_sub(earlier.0)
    }

    pub fn plus(self, ms: u64) -> Mono {
        Mono(self.0.saturating_add(ms))
    }
}

/// Wall-clock timestamp (UTC). Only for record stamps, local-day math and pause age.
pub type Wall = DateTime<Utc>;

/// One observation of both clocks, passed into every engine call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Now {
    pub mono: Mono,
    pub wall: Wall,
}

/// Source of time for the engine and the app shell.
pub trait Clock: Send + Sync {
    fn mono(&self) -> Mono;
    fn wall(&self) -> Wall;
    /// The local UTC offset in effect at `at` (used to find the local calendar day).
    fn local_offset(&self, at: Wall) -> FixedOffset;

    fn now(&self) -> Now {
        Now {
            mono: self.mono(),
            wall: self.wall(),
        }
    }
}

/// Real clocks: `std::time::Instant` for monotonic time, the system wall clock,
/// and the system time zone.
#[derive(Debug)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn mono(&self) -> Mono {
        Mono(self.origin.elapsed().as_millis() as u64)
    }

    fn wall(&self) -> Wall {
        Utc::now()
    }

    fn local_offset(&self, at: Wall) -> FixedOffset {
        Local.offset_from_utc_datetime(&at.naive_utc()).fix()
    }
}

#[derive(Debug, Clone)]
struct FakeState {
    mono: Mono,
    wall: Wall,
    offset: FixedOffset,
}

/// Deterministic clock for tests. Both clocks only move when told to.
#[derive(Debug)]
pub struct FakeClock {
    state: Mutex<FakeState>,
}

impl FakeClock {
    /// Starts at `wall` with monotonic time 1_000_000 ms and a UTC offset of `offset_hours`.
    pub fn new(wall: Wall, offset_hours: i32) -> Self {
        Self {
            state: Mutex::new(FakeState {
                mono: Mono(1_000_000),
                wall,
                offset: FixedOffset::east_opt(offset_hours * 3600).expect("valid offset"),
            }),
        }
    }

    /// Convenience: 2026-03-02 09:00:00 UTC, UTC+0.
    pub fn default_start() -> Self {
        Self::new(Utc.with_ymd_and_hms(2026, 3, 2, 9, 0, 0).unwrap(), 0)
    }

    /// Normal passage of time: both clocks advance by `ms`.
    pub fn advance(&self, ms: u64) {
        let mut s = self.state.lock().unwrap();
        s.mono = s.mono.plus(ms);
        s.wall += ChronoDuration::milliseconds(ms as i64);
    }

    /// Only the monotonic clock advances (e.g. a frozen wall clock).
    pub fn advance_mono(&self, ms: u64) {
        let mut s = self.state.lock().unwrap();
        s.mono = s.mono.plus(ms);
    }

    /// Only the wall clock moves (sleep on macOS, or a manual clock change). May be negative.
    pub fn jump_wall(&self, ms: i64) {
        let mut s = self.state.lock().unwrap();
        s.wall += ChronoDuration::milliseconds(ms);
    }

    pub fn set_offset_hours(&self, hours: i32) {
        let mut s = self.state.lock().unwrap();
        s.offset = FixedOffset::east_opt(hours * 3600).expect("valid offset");
    }
}

impl Clock for FakeClock {
    fn mono(&self) -> Mono {
        self.state.lock().unwrap().mono
    }

    fn wall(&self) -> Wall {
        self.state.lock().unwrap().wall
    }

    fn local_offset(&self, _at: Wall) -> FixedOffset {
        self.state.lock().unwrap().offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clock_moves_only_when_told() {
        let c = FakeClock::default_start();
        let a = c.now();
        c.advance(1500);
        let b = c.now();
        assert_eq!(b.mono.since(a.mono), 1500);
        assert_eq!((b.wall - a.wall).num_milliseconds(), 1500);
        c.jump_wall(-10_000);
        assert_eq!(c.now().mono, b.mono);
        c.advance_mono(5);
        assert_eq!(c.now().mono.since(b.mono), 5);
    }

    #[test]
    fn mono_since_saturates() {
        assert_eq!(Mono(5).since(Mono(10)), 0);
    }
}
