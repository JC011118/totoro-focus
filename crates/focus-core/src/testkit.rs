//! Engine test harness (SPEC §11). Available in this crate's tests and, via the
//! `testkit` feature, to other crates' tests.
//!
//! To be implemented by the engine module: `H { engine, clock, last_saved, fx }`
//! with `cmd`, `ev`, `tick`, `run(ms)` (1 s steps, ticking each step),
//! `run_step(ms, step)`, `snap`, `relaunch(closed_ms, mono_counts_sleep)`,
//! a counter id source (`s1`, `s2`, …), `transitions(&fx)`, and
//! `sleep_mono_stopped(ms)` / `sleep_mono_counting(ms)` clock helpers.
