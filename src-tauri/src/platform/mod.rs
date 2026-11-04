//! OS integration behind one interface. macOS: NSPanel conversion, workspace
//! sleep/wake + clock + screen observers, NSProcessInfo activity (App Nap),
//! NSSound chime, tray font. Elsewhere these are no-ops so the shell builds and
//! runs (the companion uses a plain always-on-top window).
//!
//! Functions here are called with no engine lock held.

use focus_core::engine::ActivityLevel;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{
    install_observers, make_panel, panel_set_visible, play_chime, set_activity, style_tray,
};

#[cfg(not(target_os = "macos"))]
mod other;
#[cfg(not(target_os = "macos"))]
pub use other::{
    install_observers, make_panel, panel_set_visible, play_chime, set_activity, style_tray,
};

/// Which `NSProcessInfo` activity a level holds (SPEC §4.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityKind {
    /// Focus running: prevents App Nap and idle system sleep (display sleep and
    /// closing the lid still work), so reading quietly never pauses the timer.
    UserInitiated,
    /// Break running: prevents App Nap, allows idle system sleep.
    UserInitiatedAllowingIdleSystemSleep,
}

/// `None` → end any activity.
pub fn activity_for(level: ActivityLevel) -> Option<ActivityKind> {
    match level {
        ActivityLevel::Focus => Some(ActivityKind::UserInitiated),
        ActivityLevel::Break => Some(ActivityKind::UserInitiatedAllowingIdleSystemSleep),
        ActivityLevel::None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_levels_map_to_process_info_options() {
        assert_eq!(
            activity_for(ActivityLevel::Focus),
            Some(ActivityKind::UserInitiated)
        );
        assert_eq!(
            activity_for(ActivityLevel::Break),
            Some(ActivityKind::UserInitiatedAllowingIdleSystemSleep)
        );
        assert_eq!(activity_for(ActivityLevel::None), None);
    }
}
