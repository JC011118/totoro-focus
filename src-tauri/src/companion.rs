//! The companion window (SPEC §8.0, §8.1, §8.2 Position). All companion
//! window code lives here behind create / show-hide / position functions, so a
//! fallback window technology stays contained.
//!
//! Window paths:
//! - `panel` (macOS default): a non-activating `NSPanel` via tauri-nspanel.
//! - `native` (`FOCUS_WINDOW_MODE=native`, macOS): a plain always-on-top Tauri
//!   window, for the spike comparison.
//! - `other` (non-macOS): the same plain undecorated always-on-top window.
//!
//! Positions are logical points with the origin at the top-left of the primary
//! display. A move made by the shell (startup placement, recovery, reset) does
//! not overwrite the saved preferred position: the Moved event it causes is
//! ignored. User moves are debounced (~500 ms) into `settings.widgetPosition`;
//! a drag still waiting in the debouncer at quit is saved by `effects::shutdown`
//! together with the Quit checkpoint.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use focus_core::events::COMPANION_WINDOW;
use focus_core::geometry::{self, Rect};
use tauri::{
    AppHandle, LogicalPosition, Manager, Monitor, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};

use crate::shell::Shell;
use crate::{actions, platform};

/// Logical size of the companion (SPEC §8.1 rule 2).
pub const SIZE: (f64, f64) = (240.0, 200.0);
pub const URL: &str = "companion.html";
/// User moves are saved once the window has been still this long.
pub const MOVE_DEBOUNCE: Duration = Duration::from_millis(500);
/// How long a shell-initiated move's Moved events are recognized and ignored.
const PROGRAMMATIC_MOVE_WINDOW: Duration = Duration::from_secs(2);
/// Logical-point tolerance when matching a Moved event to a shell move.
const MOVE_TOLERANCE: f64 = 1.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowMode {
    /// Non-activating NSPanel (macOS default).
    Panel,
    /// Plain always-on-top window on macOS (`FOCUS_WINDOW_MODE=native`).
    Native,
    /// Plain always-on-top window on other operating systems.
    Other,
}

impl WindowMode {
    /// Read `FOCUS_WINDOW_MODE` at runtime.
    pub fn detect() -> Self {
        let value = std::env::var("FOCUS_WINDOW_MODE").ok();
        Self::from_env(value.as_deref(), cfg!(target_os = "macos"))
    }

    pub fn from_env(value: Option<&str>, macos: bool) -> Self {
        if !macos {
            return WindowMode::Other;
        }
        match value.map(str::trim) {
            Some(v) if v.eq_ignore_ascii_case("native") => WindowMode::Native,
            _ => WindowMode::Panel,
        }
    }

    /// The `AppInfo.windowMode` string.
    pub fn as_str(self) -> &'static str {
        match self {
            WindowMode::Panel => "panel",
            WindowMode::Native => "native",
            WindowMode::Other => "other",
        }
    }
}

/// A move the shell made, whose Moved event(s) must not be saved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProgrammaticMove {
    pub target: (f64, f64),
    pub until: Instant,
}

impl ProgrammaticMove {
    pub fn new(target: (f64, f64), now: Instant) -> Self {
        Self {
            target,
            until: now + PROGRAMMATIC_MOVE_WINDOW,
        }
    }

    pub fn matches(&self, pos: (f64, f64), now: Instant) -> bool {
        now <= self.until && close_to(pos, self.target)
    }
}

fn close_to(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() <= MOVE_TOLERANCE && (a.1 - b.1).abs() <= MOVE_TOLERANCE
}

/// Coalesces bursts of values and calls `commit` with the last one once no new
/// value arrived for `quiet`. Runs on its own thread.
pub struct Debouncer<T: Send + 'static> {
    tx: Sender<T>,
}

impl<T: Send + 'static> Debouncer<T> {
    pub fn spawn(name: &str, quiet: Duration, commit: impl Fn(T) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel::<T>();
        let started = std::thread::Builder::new()
            .name(name.to_string())
            .spawn(move || {
                while let Ok(mut latest) = rx.recv() {
                    loop {
                        match rx.recv_timeout(quiet) {
                            Ok(v) => latest = v,
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => {
                                commit(latest);
                                return;
                            }
                        }
                    }
                    commit(latest);
                }
            });
        if let Err(e) = started {
            log::error!("could not start the {name} thread: {e}");
        }
        Self { tx }
    }

    pub fn push(&self, value: T) {
        let _ = self.tx.send(value);
    }
}

/// The newest user position that has not reached `settings.widgetPosition` yet
/// (it is waiting in the debouncer, or was left for shutdown to save).
#[derive(Debug, Default)]
pub struct PendingPosition(Mutex<Option<(f64, f64)>>);

impl PendingPosition {
    fn slot(&self) -> std::sync::MutexGuard<'_, Option<(f64, f64)>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn get(&self) -> Option<(f64, f64)> {
        *self.slot()
    }

    pub fn set(&self, pos: (f64, f64)) {
        *self.slot() = Some(pos);
    }

    /// Take the pending position (shutdown folds it into its final save).
    pub fn take(&self) -> Option<(f64, f64)> {
        self.slot().take()
    }

    /// Clear the slot iff it still holds `pos` (checked and cleared under one
    /// lock, so a newer drag recorded meanwhile stays pending).
    pub fn clear_if(&self, pos: (f64, f64)) -> bool {
        let mut slot = self.slot();
        let matches = *slot == Some(pos);
        if matches {
            *slot = None;
        }
        matches
    }
}

/// Companion window state (managed by Tauri).
pub struct CompanionState {
    mode: WindowMode,
    programmatic: Mutex<Option<ProgrammaticMove>>,
    pending: PendingPosition,
    moves: Debouncer<(f64, f64)>,
}

impl CompanionState {
    pub fn new(app: &AppHandle, mode: WindowMode) -> Self {
        let app = app.clone();
        let moves = Debouncer::spawn("companion-moves", MOVE_DEBOUNCE, move |pos| {
            commit_user_move(&app, pos);
        });
        Self {
            mode,
            programmatic: Mutex::new(None),
            pending: PendingPosition::default(),
            moves,
        }
    }

    pub fn mode(&self) -> WindowMode {
        self.mode
    }

    fn pending(&self) -> Option<(f64, f64)> {
        self.pending.get()
    }

    /// Take a user drag that has not been saved yet. Called by
    /// `effects::shutdown` after shutdown has begun, so the debouncer can no
    /// longer save it (`actions::save_widget_position` defers to shutdown).
    pub fn take_pending(&self) -> Option<(f64, f64)> {
        self.pending.take()
    }

    fn mark_programmatic(&self, target: (f64, f64)) {
        *self
            .programmatic
            .lock()
            .unwrap_or_else(PoisonError::into_inner) =
            Some(ProgrammaticMove::new(target, Instant::now()));
    }

    fn is_programmatic(&self, pos: (f64, f64)) -> bool {
        self.programmatic
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some_and(|m| m.matches(pos, Instant::now()))
    }
}

/// The debouncer's commit. Once shutdown has begun the position stays pending:
/// `effects::shutdown` takes it and saves it with the Quit checkpoint.
fn commit_user_move(app: &AppHandle, pos: (f64, f64)) {
    if !actions::save_widget_position(app, pos) {
        return;
    }
    if let Some(state) = app.try_state::<CompanionState>() {
        state.pending.clear_if(pos);
    }
}

// ---------------------------------------------------------------- creation

fn builder(app: &AppHandle, mode: WindowMode) -> WebviewWindowBuilder<'_, tauri::Wry, AppHandle> {
    let b = WebviewWindowBuilder::new(app, COMPANION_WINDOW, WebviewUrl::App(URL.into()))
        .title(app.package_info().name.clone())
        .inner_size(SIZE.0, SIZE.1)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .skip_taskbar(true)
        .visible(false)
        .focused(false)
        .accept_first_mouse(true);
    match mode {
        // Panel behavior is configured through tauri-nspanel after conversion.
        WindowMode::Panel => b,
        WindowMode::Native | WindowMode::Other => b
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .focusable(false),
    }
}

/// Create the companion (hidden), place it, and convert it to a panel in panel
/// mode. Showing it is `sync_visibility`'s job.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let mode = app.state::<CompanionState>().mode();
    let window = builder(app, mode).build()?;
    install_window_events(app, &window);
    place_initially(app, &window);
    if mode == WindowMode::Panel {
        platform::make_panel(&window)?;
    }
    platform::exempt_from_app_hide(&window);
    log::info!("companion created ({} mode)", mode.as_str());
    Ok(())
}

fn install_window_events(app: &AppHandle, window: &WebviewWindow) {
    let app = app.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Moved(physical) => {
            let Some(w) = app.get_webview_window(COMPANION_WINDOW) else {
                return;
            };
            let Ok(scale) = w.scale_factor() else {
                return;
            };
            let p = physical.to_logical::<f64>(scale);
            on_moved(&app, (p.x, p.y));
        }
        WindowEvent::CloseRequested { api, .. } => {
            // The companion is hidden, never closed (rule 5); the tray restores it.
            api.prevent_close();
            actions::set_companion_visible(&app, false);
        }
        _ => {}
    });
}

fn on_moved(app: &AppHandle, pos: (f64, f64)) {
    let Some(state) = app.try_state::<CompanionState>() else {
        return;
    };
    if state.is_programmatic(pos) {
        return;
    }
    state.pending.set(pos);
    state.moves.push(pos);
}

// ---------------------------------------------------------------- visibility

/// Make the window match the wanted visibility (set under the engine lock).
/// Runs on the main thread: inline when already there, otherwise dispatched.
/// The main-thread task reads the *current* wanted state, so racing requests
/// always settle on the newest one.
pub fn sync_visibility(app: &AppHandle) {
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || apply_visibility(&handle)) {
        log::warn!("could not schedule a companion show/hide: {e}");
    }
}

fn apply_visibility(app: &AppHandle) {
    let (Some(shell), Some(state)) = (app.try_state::<Shell>(), app.try_state::<CompanionState>())
    else {
        return;
    };
    let wanted = shell.companion_wanted();
    match state.mode() {
        WindowMode::Panel => platform::panel_set_visible(app, wanted),
        WindowMode::Native | WindowMode::Other => {
            let Some(w) = app.get_webview_window(COMPANION_WINDOW) else {
                return;
            };
            if w.is_visible().ok() == Some(wanted) {
                return;
            }
            let result = if wanted { w.show() } else { w.hide() };
            if let Err(e) = result {
                log::warn!("companion show/hide failed: {e}");
            }
        }
    }
}

// ---------------------------------------------------------------- position

/// Identity used to find the primary monitor in `available_monitors()`.
#[derive(Clone, Debug, PartialEq)]
pub struct MonitorKey {
    pub name: Option<String>,
    pub position: (i32, i32),
}

impl MonitorKey {
    fn of(m: &Monitor) -> Self {
        Self {
            name: m.name().cloned(),
            position: (m.position().x, m.position().y),
        }
    }
}

/// Index of `primary` in `monitors` (by name and position), else 0.
pub fn primary_index(monitors: &[MonitorKey], primary: Option<&MonitorKey>) -> usize {
    primary
        .and_then(|p| monitors.iter().position(|m| m == p))
        .unwrap_or(0)
}

/// Each monitor's work area in logical points, converted with that monitor's
/// own scale factor, plus the primary's index.
fn work_areas(app: &AppHandle) -> (Vec<Rect>, usize) {
    let monitors = app.available_monitors().unwrap_or_default();
    let primary = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| MonitorKey::of(&m));
    let keys: Vec<MonitorKey> = monitors.iter().map(MonitorKey::of).collect();
    let areas = monitors
        .iter()
        .map(|m| {
            let scale = m.scale_factor();
            let wa = m.work_area();
            let p = wa.position.to_logical::<f64>(scale);
            let s = wa.size.to_logical::<f64>(scale);
            Rect {
                x: p.x,
                y: p.y,
                width: s.width,
                height: s.height,
            }
        })
        .collect();
    (areas, primary_index(&keys, primary.as_ref()))
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))] // used by the macOS display observer
fn window_position(w: &WebviewWindow) -> Option<(f64, f64)> {
    let scale = w.scale_factor().ok()?;
    let p = w.outer_position().ok()?.to_logical::<f64>(scale);
    Some((p.x, p.y))
}

fn rect_at((x, y): (f64, f64), size: (f64, f64)) -> Rect {
    Rect {
        x,
        y,
        width: size.0,
        height: size.1,
    }
}

type EnsureVisible<'a> = &'a dyn Fn(Rect, &[Rect], usize) -> Option<(f64, f64)>;
type DefaultPosition<'a> = &'a dyn Fn((f64, f64), Rect) -> (f64, f64);

/// Where to put the companion at launch: the saved spot if it is visible, a
/// recovered spot near it if not, or the default spot on the primary display.
/// `None` when nothing is known about the displays and nothing was saved.
pub fn initial_position(
    preferred: Option<(f64, f64)>,
    size: (f64, f64),
    areas: &[Rect],
    primary: usize,
    ensure: EnsureVisible<'_>,
    default: DefaultPosition<'_>,
) -> Option<(f64, f64)> {
    match (preferred, areas.get(primary).or(areas.first())) {
        (Some(p), Some(_)) => Some(ensure(rect_at(p, size), areas, primary).unwrap_or(p)),
        (Some(p), None) => Some(p),
        (None, Some(area)) => Some(default(size, *area)),
        (None, None) => None,
    }
}

/// After a display change: go back to the saved spot when it is visible again,
/// otherwise keep the current spot if it is visible, else recover it. `None`
/// means stay put.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))] // used by the macOS display observer
pub fn reposition(
    preferred: Option<(f64, f64)>,
    current: (f64, f64),
    size: (f64, f64),
    areas: &[Rect],
    primary: usize,
    ensure: EnsureVisible<'_>,
) -> Option<(f64, f64)> {
    if areas.is_empty() {
        return None;
    }
    if let Some(p) = preferred {
        if ensure(rect_at(p, size), areas, primary).is_none() {
            return (!close_to(p, current)).then_some(p);
        }
    }
    ensure(rect_at(current, size), areas, primary)
}

fn saved_position(app: &AppHandle) -> Option<(f64, f64)> {
    let pending = app.try_state::<CompanionState>().and_then(|s| s.pending());
    pending.or_else(|| {
        app.try_state::<Shell>()?
            .read(|engine, _, _| engine.settings().widget_position)
            .map(|p| (p.x, p.y))
    })
}

/// A shell-initiated move: its Moved event is not saved as the preferred spot.
fn move_to(app: &AppHandle, w: &WebviewWindow, target: (f64, f64)) {
    if let Some(state) = app.try_state::<CompanionState>() {
        state.mark_programmatic(target);
    }
    if let Err(e) = w.set_position(LogicalPosition::new(target.0, target.1)) {
        log::warn!("moving the companion failed: {e}");
    }
}

fn place_initially(app: &AppHandle, w: &WebviewWindow) {
    let (areas, primary) = work_areas(app);
    let target = initial_position(
        saved_position(app),
        SIZE,
        &areas,
        primary,
        &geometry::ensure_visible,
        &geometry::default_position,
    );
    if let Some(target) = target {
        move_to(app, w, target);
    }
}

/// Keep the companion on a visible display (startup and
/// `NSApplicationDidChangeScreenParametersNotification`). Main thread.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))] // used by the macOS display observer
pub fn ensure_visible(app: &AppHandle) {
    let Some(w) = app.get_webview_window(COMPANION_WINDOW) else {
        return;
    };
    let Some(current) = window_position(&w) else {
        return;
    };
    let (areas, primary) = work_areas(app);
    let target = reposition(
        saved_position(app),
        current,
        SIZE,
        &areas,
        primary,
        &geometry::ensure_visible,
    );
    if let Some(target) = target {
        log::info!("display change: moving the companion to {target:?}");
        move_to(app, &w, target);
    }
}

/// Move to the default spot on the primary display; returns it so the caller
/// can save it.
pub fn reset_position(app: &AppHandle) -> Option<(f64, f64)> {
    let w = app.get_webview_window(COMPANION_WINDOW)?;
    let (areas, primary) = work_areas(app);
    let area = areas.get(primary).or(areas.first())?;
    let target = geometry::default_position(SIZE, *area);
    move_to(app, &w, target);
    Some(target)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    const AREA: Rect = Rect {
        x: 0.0,
        y: 25.0,
        width: 1440.0,
        height: 875.0,
    };
    const SECOND: Rect = Rect {
        x: 1440.0,
        y: 0.0,
        width: 1920.0,
        height: 1080.0,
    };

    /// Stand-in for `geometry::ensure_visible`: visible iff the top-left lies in
    /// some area; recovery = the area's top-left (enough to test the glue logic).
    fn fake_ensure(r: Rect, areas: &[Rect], primary: usize) -> Option<(f64, f64)> {
        let inside =
            |a: &Rect| r.x >= a.x && r.y >= a.y && r.x < a.x + a.width && r.y < a.y + a.height;
        if areas.iter().any(inside) {
            None
        } else {
            areas.get(primary).map(|a| (a.x, a.y))
        }
    }

    fn fake_default(size: (f64, f64), a: Rect) -> (f64, f64) {
        (
            a.x + a.width - size.0 - 24.0,
            a.y + a.height - size.1 - 24.0,
        )
    }

    #[test]
    fn window_mode_from_env() {
        assert_eq!(WindowMode::from_env(None, true), WindowMode::Panel);
        assert_eq!(
            WindowMode::from_env(Some("native"), true),
            WindowMode::Native
        );
        assert_eq!(
            WindowMode::from_env(Some(" NATIVE "), true),
            WindowMode::Native
        );
        assert_eq!(WindowMode::from_env(Some("panel"), true), WindowMode::Panel);
        assert_eq!(WindowMode::from_env(Some("bogus"), true), WindowMode::Panel);
        assert_eq!(
            WindowMode::from_env(Some("native"), false),
            WindowMode::Other
        );
        assert_eq!(WindowMode::from_env(None, false), WindowMode::Other);
        assert_eq!(WindowMode::Panel.as_str(), "panel");
        assert_eq!(WindowMode::Native.as_str(), "native");
        assert_eq!(WindowMode::Other.as_str(), "other");
    }

    #[test]
    fn programmatic_moves_are_recognized_briefly_and_near_the_target() {
        let t0 = Instant::now();
        let m = ProgrammaticMove::new((100.0, 200.0), t0);
        assert!(m.matches((100.0, 200.0), t0));
        assert!(m.matches((101.0, 199.5), t0 + Duration::from_millis(500)));
        assert!(
            !m.matches((140.0, 200.0), t0),
            "a user drag elsewhere is saved"
        );
        assert!(
            !m.matches((100.0, 200.0), t0 + Duration::from_secs(3)),
            "expires"
        );
    }

    #[test]
    fn primary_index_matches_by_name_and_position() {
        let a = MonitorKey {
            name: Some("Built-in".into()),
            position: (0, 0),
        };
        let b = MonitorKey {
            name: Some("External".into()),
            position: (2880, 0),
        };
        let keys = [a.clone(), b.clone()];
        assert_eq!(primary_index(&keys, Some(&b)), 1);
        assert_eq!(primary_index(&keys, Some(&a)), 0);
        assert_eq!(primary_index(&keys, None), 0);
        let gone = MonitorKey {
            name: Some("Projector".into()),
            position: (0, 0),
        };
        assert_eq!(primary_index(&keys, Some(&gone)), 0);
    }

    #[test]
    fn initial_position_prefers_the_saved_spot() {
        let areas = [AREA, SECOND];
        let p = initial_position(
            Some((300.0, 300.0)),
            SIZE,
            &areas,
            0,
            &fake_ensure,
            &fake_default,
        );
        assert_eq!(p, Some((300.0, 300.0)));
        // A spot on a display that is gone is recovered (the saved value is not changed).
        let p = initial_position(
            Some((5000.0, 300.0)),
            SIZE,
            &areas,
            0,
            &fake_ensure,
            &fake_default,
        );
        assert_eq!(p, Some((0.0, 25.0)));
        // Nothing saved: the default spot on the primary display.
        let p = initial_position(None, SIZE, &areas, 1, &fake_ensure, &fake_default);
        assert_eq!(p, Some(fake_default(SIZE, SECOND)));
        // No display information: keep what was saved, or let the OS decide.
        assert_eq!(
            initial_position(Some((1.0, 2.0)), SIZE, &[], 0, &fake_ensure, &fake_default),
            Some((1.0, 2.0))
        );
        assert_eq!(
            initial_position(None, SIZE, &[], 0, &fake_ensure, &fake_default),
            None
        );
        // A stale primary index falls back to the first area.
        assert_eq!(
            initial_position(None, SIZE, &[AREA], 3, &fake_ensure, &fake_default),
            Some(fake_default(SIZE, AREA))
        );
    }

    #[test]
    fn reposition_recovers_and_returns_to_the_saved_spot() {
        let saved = Some((2000.0, 500.0)); // on the external display
                                           // External display unplugged: recover from the (now invisible) current spot.
        assert_eq!(
            reposition(saved, (2000.0, 500.0), SIZE, &[AREA], 0, &fake_ensure),
            Some((0.0, 25.0))
        );
        // Reconnected: go back to the saved spot.
        assert_eq!(
            reposition(saved, (0.0, 25.0), SIZE, &[AREA, SECOND], 0, &fake_ensure),
            Some((2000.0, 500.0))
        );
        // Already there (within tolerance): stay.
        assert_eq!(
            reposition(
                saved,
                (2000.5, 500.0),
                SIZE,
                &[AREA, SECOND],
                0,
                &fake_ensure
            ),
            None
        );
        // Nothing saved and still visible: stay.
        assert_eq!(
            reposition(None, (10.0, 30.0), SIZE, &[AREA], 0, &fake_ensure),
            None
        );
        // No displays reported: never move.
        assert_eq!(
            reposition(saved, (9999.0, 0.0), SIZE, &[], 0, &fake_ensure),
            None
        );
    }

    #[test]
    fn debouncer_commits_only_the_last_value_of_a_burst() {
        let (tx, rx) = mpsc::channel();
        let d = Debouncer::spawn("test-debounce", Duration::from_millis(60), move |v: u32| {
            tx.send(v).unwrap();
        });
        for v in 1..=5 {
            d.push(v);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(5));
        assert!(
            rx.recv_timeout(Duration::from_millis(150)).is_err(),
            "one commit per burst"
        );
        d.push(9);
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(9));
    }

    #[test]
    fn pending_position_take_and_clear_if() {
        let p = PendingPosition::default();
        assert_eq!(p.get(), None);
        assert_eq!(p.take(), None);
        p.set((10.0, 20.0));
        assert_eq!(p.get(), Some((10.0, 20.0)));
        // A commit of an older drag does not clear a newer one.
        assert!(!p.clear_if((1.0, 2.0)));
        assert_eq!(p.get(), Some((10.0, 20.0)));
        assert!(p.clear_if((10.0, 20.0)));
        assert_eq!(p.get(), None);
        // Shutdown takes the newest drag exactly once.
        p.set((30.0, 40.0));
        p.set((50.0, 60.0));
        assert_eq!(p.take(), Some((50.0, 60.0)));
        assert_eq!(p.take(), None);
        assert!(!p.clear_if((50.0, 60.0)), "already taken by shutdown");
    }

    #[test]
    fn debouncer_flushes_a_pending_value_when_dropped() {
        let got = Arc::new(Mutex::new(Vec::new()));
        let sink = got.clone();
        let d = Debouncer::spawn("test-flush", Duration::from_secs(30), move |v: u32| {
            sink.lock().unwrap().push(v);
        });
        d.push(7);
        std::thread::sleep(Duration::from_millis(20));
        drop(d);
        let deadline = Instant::now() + Duration::from_secs(5);
        while got.lock().unwrap().is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(*got.lock().unwrap(), vec![7]);
    }
}
