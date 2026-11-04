//! The ticker: one dedicated thread that refreshes the display (SPEC §8.2).
//!
//! It sleeps `Engine::next_tick_delay_ms` (running: to the next second boundary;
//! paused: 15 s; idle/awaiting: until local midnight, at most 60 s), then runs
//! `system(Tick)`. Any command kicks it so the delay is recomputed at once (for
//! example, a session started while the ticker was in a 60 s idle wait). A tick
//! only refreshes: remaining time always comes from accumulated active time.

use std::sync::{Condvar, Mutex, PoisonError};
use std::time::Duration;

use focus_core::engine::SystemEvent;
use tauri::{AppHandle, Manager};

use crate::effects;
use crate::shell::Shell;

/// Bounds applied to the engine's delay, defensively.
pub const MIN_DELAY_MS: u64 = 50;
pub const MAX_DELAY_MS: u64 = 60_000;

/// A wake-up flag the ticker waits on with a timeout.
#[derive(Debug, Default)]
pub struct Waker {
    kicked: Mutex<bool>,
    cv: Condvar,
}

impl Waker {
    /// Wake the ticker so it recomputes its delay.
    pub fn kick(&self) {
        *self.kicked.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.cv.notify_all();
    }

    /// Wait up to `timeout`. Returns `true` if kicked (and clears the flag),
    /// `false` when the timeout elapsed.
    pub fn wait(&self, timeout: Duration) -> bool {
        let guard = self.kicked.lock().unwrap_or_else(PoisonError::into_inner);
        let (mut kicked, _) = self
            .cv
            .wait_timeout_while(guard, timeout, |k| !*k)
            .unwrap_or_else(PoisonError::into_inner);
        std::mem::replace(&mut *kicked, false)
    }
}

pub fn clamp_delay(ms: u64) -> Duration {
    Duration::from_millis(ms.clamp(MIN_DELAY_MS, MAX_DELAY_MS))
}

/// Start the ticker thread (once, from setup).
pub fn spawn(app: AppHandle) {
    let started = std::thread::Builder::new()
        .name("focus-ticker".into())
        .spawn(move || run(&app));
    if let Err(e) = started {
        log::error!("could not start the ticker thread: {e}");
    }
}

fn run(app: &AppHandle) {
    let Some(shell) = app.try_state::<Shell>() else {
        log::error!("ticker started before the shell state existed");
        return;
    };
    loop {
        if shell.is_shutting_down() {
            break;
        }
        let delay = shell.read(|engine, now, clock| engine.next_tick_delay_ms(now, clock));
        if shell.ticker.wait(clamp_delay(delay)) {
            continue; // kicked: state changed, recompute the delay
        }
        if shell.is_shutting_down() {
            break;
        }
        effects::system_event(app, SystemEvent::Tick);
    }
    log::debug!("ticker stopped");
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Instant;

    use super::*;

    #[test]
    fn wait_times_out_without_a_kick() {
        let w = Waker::default();
        let t = Instant::now();
        assert!(!w.wait(Duration::from_millis(30)));
        assert!(t.elapsed() >= Duration::from_millis(25));
    }

    #[test]
    fn kick_wakes_a_waiting_ticker_early_and_is_consumed() {
        let w = Arc::new(Waker::default());
        let w2 = w.clone();
        let kicker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            w2.kick();
        });
        let t = Instant::now();
        assert!(w.wait(Duration::from_secs(10)));
        assert!(t.elapsed() < Duration::from_secs(5));
        kicker.join().unwrap();
        // The flag was cleared by the wait that saw it.
        assert!(!w.wait(Duration::from_millis(5)));
    }

    #[test]
    fn a_kick_before_the_wait_is_not_lost() {
        let w = Waker::default();
        w.kick();
        assert!(w.wait(Duration::from_secs(10)));
    }

    #[test]
    fn delays_are_clamped() {
        assert_eq!(clamp_delay(0), Duration::from_millis(50));
        assert_eq!(clamp_delay(400), Duration::from_millis(400));
        assert_eq!(clamp_delay(3_600_000), Duration::from_millis(60_000));
    }
}
