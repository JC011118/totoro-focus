//! macOS integration (SPEC §8.1, §8.2, §4.8). Compiled and type-checked from
//! Linux only (scripts/check-macos.sh); every behavior here is pending manual
//! verification on a Mac.

use std::cell::{OnceCell, RefCell};
use std::ptr::NonNull;
use std::sync::{Mutex, PoisonError};

use focus_core::engine::{ActivityLevel, SystemEvent};
use focus_core::events::COMPANION_WINDOW;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{
    NSApplicationDidChangeScreenParametersNotification, NSFloatingWindowLevel, NSFont,
    NSFontWeightRegular, NSSound, NSWindowCollectionBehavior, NSWindowStyleMask, NSWorkspace,
    NSWorkspaceDidWakeNotification, NSWorkspaceWillSleepNotification,
};
use objc2_foundation::{
    ns_string, NSActivityOptions, NSData, NSNotification, NSNotificationCenter, NSNotificationName,
    NSObjectProtocol, NSProcessInfo, NSSystemClockDidChangeNotification,
};
use tauri::tray::TrayIcon;
use tauri::{AppHandle, WebviewWindow};
use tauri_nspanel::{ManagerExt, WebviewWindowExt};

use super::{activity_for, ActivityKind};
use crate::panel::CompanionPanel;
use crate::{companion, effects};

fn on_main_thread() -> bool {
    MainThreadMarker::new().is_some()
}

// ---------------------------------------------------------------- panel (§8.1)

/// Convert the (hidden) companion window into the non-activating panel and
/// configure it through tauri-nspanel only (rule 3). Main thread (setup).
pub fn make_panel(window: &WebviewWindow) -> tauri::Result<()> {
    let panel = window.to_panel::<CompanionPanel>()?;
    panel
        .add_style_mask(NSWindowStyleMask::NonactivatingPanel)
        .map_err(|e| tauri::Error::Anyhow(e.into()))?;
    panel.set_level(NSFloatingWindowLevel as i64);
    panel.set_hides_on_deactivate(false);
    panel.set_works_when_modal(true);
    panel.set_collection_behavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    panel.set_transparent(true);
    panel.set_has_shadow(false);
    Ok(())
}

/// App-level Hide (Cmd+H while Settings is focused, "Hide Others") must not take
/// the companion off screen: only its own hide control or the tray hides it, so
/// the tray label always matches what is visible (SPEC §8.1 rule 3). Works for
/// the panel (same NSWindow object after class conversion) and native mode.
/// Main thread (setup).
pub fn exempt_from_app_hide(window: &WebviewWindow) {
    match window.ns_window() {
        Ok(ptr) if !ptr.is_null() => {
            // SAFETY: Tauri returns this window's live NSWindow pointer; we are on
            // the main thread during setup and only call a property setter.
            let ns_window: &objc2_app_kit::NSWindow =
                unsafe { &*(ptr as *const objc2_app_kit::NSWindow) };
            ns_window.setCanHide(false);
        }
        _ => log::warn!("could not reach the companion NSWindow to disable app-level hide"),
    }
}

/// Show = `orderFrontRegardless`, hide = `orderOut` (rule 4). Never
/// `set_focus`/`show` on the companion. Main thread only.
pub fn panel_set_visible(app: &AppHandle, visible: bool) {
    if !on_main_thread() {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || panel_set_visible(&handle, visible));
        return;
    }
    match app.get_webview_panel(COMPANION_WINDOW) {
        Ok(panel) if visible => panel.show(),
        Ok(panel) => panel.hide(),
        Err(_) => log::warn!("companion panel not found"),
    }
}

// ---------------------------------------------------------------- tray font

/// Monospaced digits so the menu-bar countdown does not change width every second.
pub fn style_tray(tray: &TrayIcon) {
    let result = tray.with_inner_tray_icon(|inner| {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        if let Some(button) = inner.ns_status_item().and_then(|item| item.button(mtm)) {
            // SAFETY: reading an AppKit extern constant.
            let weight = unsafe { NSFontWeightRegular };
            let font =
                NSFont::monospacedDigitSystemFontOfSize_weight(NSFont::systemFontSize(), weight);
            button.setFont(Some(&font));
        }
    });
    if let Err(e) = result {
        log::warn!("could not set the tray font: {e}");
    }
}

// ---------------------------------------------------------------- observers (§8.2)

type Token = Retained<ProtocolObject<dyn NSObjectProtocol>>;

thread_local! {
    /// Observer tokens, kept for the app's lifetime (main thread).
    static TOKENS: RefCell<Vec<Token>> = const { RefCell::new(Vec::new()) };
}

fn observe(center: &NSNotificationCenter, name: &NSNotificationName, f: impl Fn() + 'static) {
    let block = block2::RcBlock::new(move |_n: NonNull<NSNotification>| f());
    // SAFETY: queue None → the block runs synchronously on the posting thread
    // (the main thread for all four notifications); the block is retained by
    // the center and the token is kept below.
    let token = unsafe {
        center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
    };
    TOKENS.with(|t| t.borrow_mut().push(token));
}

/// Workspace sleep/wake (on NSWorkspace's center), clock changes and screen
/// parameter changes (on the default center). Main thread (setup).
pub fn install_observers(app: &AppHandle) {
    let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
    let default_center = NSNotificationCenter::defaultCenter();

    let a = app.clone();
    // SAFETY: reading AppKit/Foundation extern notification-name constants.
    observe(
        &workspace,
        unsafe { NSWorkspaceWillSleepNotification },
        move || {
            // Suspend and save synchronously before returning: macOS waits only briefly.
            log::info!("system will sleep");
            effects::system_event(&a, SystemEvent::Suspend);
        },
    );
    let a = app.clone();
    observe(
        &workspace,
        unsafe { NSWorkspaceDidWakeNotification },
        move || {
            log::info!("system did wake");
            effects::system_event(&a, SystemEvent::Wake);
        },
    );
    let a = app.clone();
    observe(
        &default_center,
        unsafe { NSSystemClockDidChangeNotification },
        move || {
            log::info!("system clock changed");
            effects::system_event(&a, SystemEvent::ClockChanged);
        },
    );
    let a = app.clone();
    observe(
        &default_center,
        unsafe { NSApplicationDidChangeScreenParametersNotification },
        move || companion::ensure_visible(&a),
    );
    log::info!("sleep/wake, clock and display observers installed");
}

// ---------------------------------------------------------------- App Nap (§4.8)

struct Activity(Token);
// SAFETY: NSProcessInfo activity tokens are opaque objects that may be ended
// from any thread; we only hand them back to `endActivity`.
unsafe impl Send for Activity {}

static ACTIVITY: Mutex<Option<(ActivityKind, Activity)>> = Mutex::new(None);

pub fn set_activity(level: ActivityLevel) {
    let wanted = activity_for(level);
    let mut slot = ACTIVITY.lock().unwrap_or_else(PoisonError::into_inner);
    if slot.as_ref().map(|(kind, _)| *kind) == wanted {
        return;
    }
    let info = NSProcessInfo::processInfo();
    if let Some((_, Activity(token))) = slot.take() {
        // SAFETY: `token` came from beginActivityWithOptions_reason on this process.
        unsafe { info.endActivity(&token) };
    }
    if let Some(kind) = wanted {
        let token = match kind {
            ActivityKind::UserInitiated => info.beginActivityWithOptions_reason(
                NSActivityOptions::UserInitiated,
                ns_string!("Focus session running"),
            ),
            ActivityKind::UserInitiatedAllowingIdleSystemSleep => info
                .beginActivityWithOptions_reason(
                    NSActivityOptions::UserInitiatedAllowingIdleSystemSleep,
                    ns_string!("Break running"),
                ),
        };
        *slot = Some((kind, Activity(token)));
    }
    log::debug!("process activity: {wanted:?}");
}

// ---------------------------------------------------------------- sound (§8.2)

static CHIME: &[u8] = include_bytes!("../../assets/chime.wav");

thread_local! {
    /// Cached on the main thread so the sound outlives playback.
    static SOUND: OnceCell<Option<Retained<NSSound>>> = const { OnceCell::new() };
}

fn load_chime() -> Option<Retained<NSSound>> {
    let sound = NSSound::initWithData(NSSound::alloc(), &NSData::with_bytes(CHIME));
    if sound.is_none() {
        log::warn!("the embedded chime could not be decoded; staying silent");
    }
    sound
}

/// Play the soft chime with NSSound on the main thread.
pub fn play_chime(app: &AppHandle) {
    let scheduled = app.run_on_main_thread(|| {
        SOUND.with(|cell| {
            if let Some(sound) = cell.get_or_init(load_chime) {
                let _ = sound.stop();
                let _ = sound.play();
            }
        });
    });
    if let Err(e) = scheduled {
        log::warn!("could not schedule the chime: {e}");
    }
}
