//! Keeping the companion on a visible display (pure geometry, logical points).

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Inset from the work-area edges used for default and recovered positions.
pub const EDGE_MARGIN: f64 = 24.0;
/// Height of the companion's drag strip that must stay reachable.
pub const DRAG_STRIP_HEIGHT: f64 = 28.0;

/// Default position: bottom-right of `work_area`, inset by `EDGE_MARGIN`.
pub fn default_position(size: (f64, f64), work_area: Rect) -> (f64, f64) {
    let _ = (size, work_area);
    todo!("geometry: default_position")
}

/// If `window` is acceptably visible (its drag strip is fully inside one work
/// area and at least 50% of its area is on some work area) returns `None`.
/// Otherwise returns a new top-left: clamped into the work area it overlaps
/// most, or `default_position` on `work_areas[primary]` when it overlaps none.
/// Returns `None` if `work_areas` is empty.
pub fn ensure_visible(window: Rect, work_areas: &[Rect], primary: usize) -> Option<(f64, f64)> {
    let _ = (window, work_areas, primary);
    todo!("geometry: ensure_visible")
}
