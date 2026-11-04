//! Pure core of the focus companion: clock-injected timer state machine,
//! friendship rewards, session history, settings, persistence, character
//! packs and window geometry. No Tauri, no UI, no network.
//!
//! See `docs/SPEC.md` for the behavior contract every module follows.

pub mod clock;
pub mod engine;
pub mod geometry;
pub mod history;
pub mod packs;
pub mod rewards;
pub mod settings;
pub mod snapshot;
pub mod store;
pub mod timer;

pub use clock::{Clock, FakeClock, Mono, Now, SystemClock, Wall};
pub use engine::{Command, CommandError, Effect, Engine, EngineConfig, SoundCue, SystemEvent};
pub use snapshot::Snapshot;
