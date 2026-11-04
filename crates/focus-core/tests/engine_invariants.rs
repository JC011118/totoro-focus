//! Deterministic randomized walks over every command, system event, clock jump, sleep
//! and relaunch, checking the engine's global invariants after every call:
//! - points, unlocks, awards and history only grow (history is append-only);
//! - every record is awarded exactly once, with whole active minutes (+ bonus if completed);
//! - completed ⇔ active == planned; records never end before they start;
//! - a session never earns more active time than monotonic time passed (wall-clock
//!   jumps and sleep never add time), and active time never goes backward in a process;
//! - phase, session and awaiting agree; no paused session has already reached planned;
//! - `Err` leaves the engine unchanged; effects are ordered with at most one Persist;
//! - ActivityChanged tracks what is running; revision never decreases;
//! - every state change is saved: the state on disk equals the engine's, except a
//!   running session's active time (checkpoint cadence);
//! - a crash loses at most one checkpoint interval of active time.

use std::collections::HashMap;

use focus_core::engine::{ActivityLevel, Command, Effect, SystemEvent, Transition};
use focus_core::history::Outcome;
use focus_core::settings::UserSettingsPatch;
use focus_core::snapshot::{format_countdown, Phase};
use focus_core::testkit::{transitions, H, HOUR, MIN, SEC};
use focus_core::timer::SessionKind;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

fn rank(e: &Effect) -> usize {
    match e {
        Effect::Persist => 0,
        Effect::SettingsChanged { .. } => 1,
        Effect::ShowCompanion | Effect::HideCompanion => 2,
        Effect::ActivityChanged { .. } => 3,
        Effect::Transition(_) => 4,
        Effect::PlaySound(_) => 5,
    }
}

#[derive(Default)]
struct Track {
    total: u64,
    unlocked: std::collections::BTreeSet<String>,
    history: Vec<focus_core::history::SessionRecord>,
    ids: std::collections::HashSet<String>,
    revision: u64,
    /// Per session id (this process only): monotonic start and last seen active time.
    started_mono: HashMap<String, u64>,
    last_active: HashMap<String, u64>,
    steps: u64,
}

/// Every state change emits Persist (SPEC §4.7): what is on disk equals the engine's
/// state, except the checkpoint stamp and a running session's active time (saved on
/// the checkpoint cadence).
fn check_disk(h: &H) {
    let disk = h.saved();
    let mut live = h.engine.persisted_state(h.now());
    match (&disk.active_session, &mut live.active_session) {
        (Some(d), Some(l)) => {
            l.checkpoint_at = d.checkpoint_at;
            if h.engine.active().is_some_and(|s| s.is_running()) {
                assert!(d.accrued_ms <= l.accrued_ms, "saved {d:?}, live {l:?}");
                l.accrued_ms = d.accrued_ms;
            }
        }
        (None, None) => {}
        (d, l) => panic!("session on disk {d:?}, live {l:?}"),
    }
    assert_eq!(disk, &live, "a state change was not saved");
}

fn check_call(h: &H, fx: &[Effect], t: &mut Track) {
    assert!(
        fx.windows(2).all(|w| rank(&w[0]) <= rank(&w[1])),
        "order: {fx:?}"
    );
    assert!(fx.iter().filter(|e| matches!(e, Effect::Persist)).count() <= 1);
    check_disk(h);
    for tr in transitions(fx) {
        match tr {
            Transition::FocusStarted { session_id, .. }
            | Transition::BreakStarted { session_id, .. } => {
                t.started_mono
                    .insert(session_id, h.engine.baseline().unwrap().mono.0);
            }
            Transition::FocusCompleted { record, .. }
            | Transition::FocusInterrupted { record, .. } => {
                if let Some(start) = t.started_mono.get(&record.id) {
                    let elapsed = h.now().mono.0 - start;
                    assert!(
                        record.active_ms <= elapsed,
                        "{record:?} earned more than {elapsed} ms of mono time"
                    );
                }
            }
            _ => {}
        }
    }
}

fn check_state(h: &H, t: &mut Track, same_process: bool) {
    let e = &h.engine;
    let f = e.friendship();
    // Never decreases.
    assert!(
        f.total_points >= t.total,
        "points decreased: {} < {}",
        f.total_points,
        t.total
    );
    assert!(f.unlocked.is_superset(&t.unlocked), "unlocks decreased");
    let records = e.history().records();
    assert!(records.len() >= t.history.len());
    // Append-only: the known prefix is unchanged (checked fully every 64 steps, else its tail).
    let known = t.history.len();
    if t.steps.is_multiple_of(64) {
        assert_eq!(&records[..known], &t.history[..], "history is append-only");
    } else if known > 0 {
        assert_eq!(
            records[known - 1],
            t.history[known - 1],
            "history is append-only"
        );
    }
    // Ledger consistency.
    assert_eq!(
        f.total_points,
        f.awarded_sum(),
        "total equals the ledger (fresh start)"
    );
    assert_eq!(f.awards.len(), records.len(), "one award per record");
    // Newly appended records (older ones are immutable, checked above).
    for r in &records[known..] {
        assert!(t.ids.insert(r.id.clone()), "duplicate record id {}", r.id);
        assert_eq!(r.kind, SessionKind::Focus);
        assert!(r.ended_at >= r.started_at, "{r:?}");
        assert!(r.active_ms <= r.planned_ms);
        let a = &f.awards[&r.id];
        assert_eq!(a.points, r.active_ms / 60_000, "{r:?}");
        match r.outcome {
            Outcome::Completed => {
                assert_eq!(r.active_ms, r.planned_ms);
                assert_eq!(a.bonus, 5);
                assert!(a.completed);
            }
            Outcome::Interrupted => {
                assert!(r.active_ms < r.planned_ms);
                assert_eq!(a.bonus, 0);
            }
        }
        assert!(r.ended_utc_offset_minutes.is_some());
    }
    // Phase consistency.
    let s = h.snap();
    match s.phase {
        Phase::Idle => assert!(s.session.is_none() && s.awaiting.is_none()),
        Phase::AwaitingChoice => assert!(s.session.is_none() && s.awaiting.is_some()),
        Phase::Focus => assert_eq!(s.session.as_ref().unwrap().kind, SessionKind::Focus),
        Phase::Break => assert_eq!(s.session.as_ref().unwrap().kind, SessionKind::Break),
    }
    if let Some(v) = &s.session {
        assert!(s.awaiting.is_none());
        assert!(v.active_ms <= v.planned_ms);
        assert_eq!(v.remaining_ms, v.planned_ms - v.active_ms);
        assert_eq!(v.remaining_label, format_countdown(v.remaining_ms));
        let a = e.active().unwrap();
        assert!(a.accrued_ms <= a.planned_ms);
        if !a.is_running() {
            assert!(
                a.accrued_ms < a.planned_ms,
                "a paused session has already reached planned: {a:?}"
            );
        }
        if same_process {
            if let Some(prev) = t.last_active.get(&v.id) {
                assert!(v.active_ms >= *prev, "active time went backward");
            }
            if let Some(start) = t.started_mono.get(&v.id) {
                let observed = e
                    .baseline()
                    .map_or(h.now().mono.0, |b| b.mono.0.min(h.now().mono.0));
                assert!(
                    v.active_ms <= observed.saturating_sub(*start),
                    "earned more than mono time"
                );
            }
        }
        t.last_active.insert(v.id.clone(), v.active_ms);
    }
    // Activity follows what is running.
    let expected = match e.active() {
        Some(a) if a.is_running() && a.kind == SessionKind::Focus => ActivityLevel::Focus,
        Some(a) if a.is_running() => ActivityLevel::Break,
        _ => ActivityLevel::None,
    };
    assert_eq!(h.activity, expected);
    assert_eq!(e.activity(), expected);
    if e.hidden_for_focus() {
        assert_eq!(
            e.phase(),
            Phase::Focus,
            "hidden-for-focus outside a focus session"
        );
    }
    if same_process {
        assert!(e.revision() >= t.revision);
    }
    t.revision = e.revision();
    t.total = f.total_points;
    t.unlocked = f.unlocked.clone();
    t.history.extend_from_slice(&records[known..]);
}

fn try_cmd(h: &mut H, cmd: Command, t: &mut Track) {
    let now = h.now();
    let snap = h.engine.snapshot(now, &*h.clock);
    let saved = h.engine.persisted_state(now);
    let rev = h.engine.revision();
    let baseline = h.engine.baseline();
    let suspended = h.engine.is_suspended();
    match h.cmd(cmd.clone()) {
        Ok(fx) => check_call(h, &fx, t),
        Err(_) => {
            assert_eq!(h.engine.snapshot(now, &*h.clock), snap, "{cmd:?}");
            assert_eq!(h.engine.persisted_state(now), saved, "{cmd:?}");
            assert_eq!(h.engine.revision(), rev);
            assert_eq!(h.engine.baseline(), baseline);
            assert_eq!(h.engine.is_suspended(), suspended);
        }
    }
}

fn ev(h: &mut H, e: SystemEvent, t: &mut Track) {
    let fx = h.ev(e);
    check_call(h, &fx, t);
}

fn tick(h: &mut H, t: &mut Track) {
    ev(h, SystemEvent::Tick, t);
}

fn walk(seed: u64, steps: u64) -> Track {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut h = H::new();
    let mut t = Track::default();
    check_state(&h, &mut t, true);
    for _ in 0..steps {
        let mut same_process = true;
        match rng.below(100) {
            0..=44 => {
                h.clock.advance(1 + rng.below(2000));
                tick(&mut h, &mut t);
            }
            45..=49 => {
                for _ in 0..rng.below(240) {
                    h.clock.advance(SEC);
                    tick(&mut h, &mut t);
                }
            }
            50..=55 => {
                let minutes = if rng.chance(30) {
                    None
                } else {
                    Some(1 + rng.below(4) as u32)
                };
                let label = if rng.chance(30) {
                    Some(" a\nlabel ".to_string())
                } else {
                    None
                };
                try_cmd(
                    &mut h,
                    Command::StartFocus {
                        minutes,
                        task_label: label,
                    },
                    &mut t,
                );
            }
            56..=58 => try_cmd(
                &mut h,
                Command::StartBreak {
                    minutes: Some(1 + rng.below(2) as u32),
                },
                &mut t,
            ),
            59..=62 => try_cmd(&mut h, Command::Pause, &mut t),
            63..=66 => try_cmd(&mut h, Command::Resume, &mut t),
            67..=70 => try_cmd(&mut h, Command::Toggle, &mut t),
            71..=73 => try_cmd(&mut h, Command::End, &mut t),
            74..=75 => try_cmd(&mut h, Command::Dismiss, &mut t),
            76 => try_cmd(&mut h, Command::Quit, &mut t),
            77..=78 => {
                let palettes = ["auto", "forest", "evening", "wave", "Nope"];
                let patch = UserSettingsPatch {
                    sound: rng.chance(50).then(|| rng.chance(50)),
                    hide_during_focus: rng.chance(50).then(|| rng.chance(50)),
                    last_focus_minutes: rng.chance(30).then(|| rng.below(200) as u32),
                    palette: rng
                        .chance(30)
                        .then(|| palettes[rng.below(palettes.len() as u64) as usize].to_string()),
                    ..Default::default()
                };
                try_cmd(&mut h, Command::UpdateSettings(patch.into()), &mut t);
            }
            79..=80 => ev(&mut h, SystemEvent::Suspend, &mut t),
            81..=82 => ev(&mut h, SystemEvent::Wake, &mut t),
            83..=84 => ev(&mut h, SystemEvent::ClockChanged, &mut t),
            85..=86 => {
                let ms = rng.below(3 * HOUR) as i64;
                h.clock.jump_wall(if rng.chance(50) { ms } else { -ms });
            }
            87..=88 => h.clock.advance(rng.below(4 * HOUR)),
            89 => h.clock.advance_mono(rng.below(5 * SEC)),
            90..=91 => {
                // A full sleep with or without notifications, either clock behavior.
                let notify = rng.chance(70);
                if notify {
                    ev(&mut h, SystemEvent::Suspend, &mut t);
                }
                let ms = rng.below(4 * HOUR);
                if rng.chance(50) {
                    h.sleep_mono_stopped(ms);
                } else {
                    h.sleep_mono_counting(ms);
                }
                if notify || rng.chance(50) {
                    ev(&mut h, SystemEvent::Wake, &mut t);
                }
            }
            92..=93 => {
                h.user_show_hide(rng.chance(50));
            }
            94..=96 => {
                // Relaunch after a quit or a crash.
                if rng.chance(50) {
                    try_cmd(&mut h, Command::Quit, &mut t);
                }
                let before = h.snap().session.map(|v| (v.id, v.active_ms));
                let closed = if rng.chance(50) {
                    rng.below(10 * MIN)
                } else {
                    rng.below(4 * HOUR)
                };
                h.relaunch(closed, rng.chance(50));
                check_disk(&h);
                if let (Some((id, old)), Some(v)) = (before, h.snap().session) {
                    if v.id == id {
                        assert!(v.active_ms <= old);
                        assert!(
                            old - v.active_ms <= 15 * SEC,
                            "crash lost {} ms",
                            old - v.active_ms
                        );
                    }
                }
                t.started_mono.clear();
                t.last_active.clear();
                same_process = false;
            }
            _ => {
                // A late tick (App Nap) that may exceed the gap.
                h.clock.advance(rng.below(200 * SEC));
                tick(&mut h, &mut t);
            }
        }
        check_state(&h, &mut t, same_process);
        t.steps += 1;
    }
    t
}

#[test]
fn randomized_walks_keep_every_invariant() {
    let mut total_records = 0;
    let mut completed = 0;
    for seed in 1..=24 {
        let t = walk(seed, 2_000);
        total_records += t.history.len();
        completed += t
            .history
            .iter()
            .filter(|r| r.outcome == Outcome::Completed)
            .count();
    }
    // The walks must actually exercise completions and interruptions.
    assert!(completed > 50, "only {completed} completions");
    assert!(
        total_records - completed > 50,
        "only {} interruptions",
        total_records - completed
    );
}

#[test]
fn long_walk() {
    let t = walk(0xDEC0DE, 10_000);
    assert_eq!(t.steps, 10_000);
}
