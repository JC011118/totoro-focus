//! Keeping the companion on a visible display (pure geometry, logical points).
//!
//! Coordinates are logical AppKit points with the origin at the top-left of the
//! primary display and y growing downward (SPEC §8.2). Work areas exclude the
//! menu bar and the Dock. Displays left of or above the primary have negative
//! coordinates.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    /// Finite coordinates and a positive width and height.
    pub fn is_valid(&self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite())
            && self.width > 0.0
            && self.height > 0.0
    }

    /// Area of the intersection with `other` (0 when they only touch or are apart).
    pub fn overlap_area(&self, other: &Rect) -> f64 {
        let w = self.right().min(other.right()) - self.x.max(other.x);
        let h = self.bottom().min(other.bottom()) - self.y.max(other.y);
        if w > 0.0 && h > 0.0 {
            w * h
        } else {
            0.0
        }
    }
}

/// Inset from the work-area edges used for default and recovered positions.
pub const EDGE_MARGIN: f64 = 24.0;
/// Height of the companion's drag strip that must stay reachable.
pub const DRAG_STRIP_HEIGHT: f64 = 28.0;
/// Slack for containment checks: logical rects converted from per-monitor physical
/// pixels can be off by a fraction of a point.
const CONTAIN_TOLERANCE: f64 = 1.0;

/// Default position: bottom-right of `work_area`, inset by `EDGE_MARGIN`.
///
/// When the work area is too small for the margins on an axis, the window is
/// centered on that axis; when it is smaller than the window, the window is
/// aligned to the work area's left/top edge (so the drag strip stays reachable).
pub fn default_position(size: (f64, f64), work_area: Rect) -> (f64, f64) {
    (
        place_on_axis(f64::INFINITY, size.0, work_area.x, work_area.width),
        place_on_axis(f64::INFINITY, size.1, work_area.y, work_area.height),
    )
}

/// If `window` is acceptably visible (its drag strip is fully inside one work
/// area and at least 50% of its area is on some work area) returns `None`.
/// Otherwise returns a new top-left: clamped into the work area it overlaps
/// most, or `default_position` on `work_areas[primary]` when it overlaps none.
/// Returns `None` if `work_areas` is empty.
///
/// Details:
/// - The drag strip is the window's top `DRAG_STRIP_HEIGHT` points, full width.
///   "At least 50% on some work area" means one work area holds at least half of
///   the window's area. Containment allows `CONTAIN_TOLERANCE` (1 pt) of slack.
/// - Clamping is per axis: an axis on which the window already lies inside the
///   work area is kept; otherwise it is placed `EDGE_MARGIN` inside the nearest
///   edge (centered if the margins do not fit, start-aligned if the window is
///   larger than the work area). Ties in overlap prefer `primary`, then the lower index.
/// - Work areas that are not `Rect::is_valid` are ignored (all invalid → `None`).
///   A `primary` out of range, or naming an invalid area, falls back to the first
///   valid work area. A window with non-finite or non-positive geometry overlaps
///   nothing and goes to the default position.
/// - Returns `None` instead of a position equal to the current one, so the result
///   is always a real move. A recovered position is itself acceptable whenever the
///   window fits in the work area, so applying the result again returns `None`.
pub fn ensure_visible(window: Rect, work_areas: &[Rect], primary: usize) -> Option<(f64, f64)> {
    let areas: Vec<(usize, Rect)> = work_areas
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, r)| r.is_valid())
        .collect();
    let first_valid = areas.first()?.1;

    let target = if window.is_valid() {
        if is_acceptable(&window, &areas) {
            return None;
        }
        best_overlap(&window, &areas, primary).map(|wa| clamp_into(&window, &wa))
    } else {
        None
    };
    let target = target.unwrap_or_else(|| {
        let home = work_areas
            .get(primary)
            .copied()
            .filter(Rect::is_valid)
            .unwrap_or(first_valid);
        default_position((window.width, window.height), home)
    });

    let unchanged = (target.0 - window.x).abs() < 1e-9 && (target.1 - window.y).abs() < 1e-9;
    (!unchanged).then_some(target)
}

fn is_acceptable(window: &Rect, areas: &[(usize, Rect)]) -> bool {
    let strip = Rect::new(
        window.x,
        window.y,
        window.width,
        DRAG_STRIP_HEIGHT.min(window.height),
    );
    let strip_inside = areas.iter().any(|(_, wa)| contains(wa, &strip));
    let half = window.width * window.height / 2.0;
    let mostly_visible = areas.iter().any(|(_, wa)| wa.overlap_area(window) >= half);
    strip_inside && mostly_visible
}

fn contains(outer: &Rect, inner: &Rect) -> bool {
    inner.x >= outer.x - CONTAIN_TOLERANCE
        && inner.y >= outer.y - CONTAIN_TOLERANCE
        && inner.right() <= outer.right() + CONTAIN_TOLERANCE
        && inner.bottom() <= outer.bottom() + CONTAIN_TOLERANCE
}

/// The work area with the largest positive overlap; ties prefer `primary`, then
/// the lower index.
fn best_overlap(window: &Rect, areas: &[(usize, Rect)], primary: usize) -> Option<Rect> {
    let mut best: Option<(f64, bool, Rect)> = None;
    for &(i, wa) in areas {
        let overlap = window.overlap_area(&wa);
        if overlap <= 0.0 {
            continue;
        }
        let is_primary = i == primary;
        let better = match best {
            None => true,
            Some((b, b_primary, _)) => overlap > b || (overlap == b && is_primary && !b_primary),
        };
        if better {
            best = Some((overlap, is_primary, wa));
        }
    }
    best.map(|(_, _, wa)| wa)
}

fn clamp_into(window: &Rect, wa: &Rect) -> (f64, f64) {
    let axis = |pos: f64, size: f64, start: f64, len: f64| {
        let inside =
            pos >= start - CONTAIN_TOLERANCE && pos + size <= start + len + CONTAIN_TOLERANCE;
        if inside {
            pos
        } else {
            place_on_axis(pos, size, start, len)
        }
    };
    (
        axis(window.x, window.width, wa.x, wa.width),
        axis(window.y, window.height, wa.y, wa.height),
    )
}

/// Clamp `pos` into `[start + EDGE_MARGIN, start + len - size - EDGE_MARGIN]`.
/// Too little room for both margins → centered; window larger than the span → `start`.
fn place_on_axis(pos: f64, size: f64, start: f64, len: f64) -> f64 {
    let room = len - size;
    if room >= 2.0 * EDGE_MARGIN {
        pos.clamp(start + EDGE_MARGIN, start + room - EDGE_MARGIN)
    } else if room >= 0.0 {
        start + room / 2.0
    } else {
        start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_area_of_touching_rects_is_zero() {
        let a = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert_eq!(a.overlap_area(&Rect::new(100.0, 0.0, 50.0, 50.0)), 0.0);
        assert_eq!(a.overlap_area(&Rect::new(50.0, 50.0, 100.0, 100.0)), 2500.0);
        assert_eq!(a.overlap_area(&Rect::new(-500.0, 0.0, 10.0, 10.0)), 0.0);
    }

    #[test]
    fn place_on_axis_cases() {
        // Plenty of room: clamped between the margins.
        assert_eq!(place_on_axis(-50.0, 240.0, 0.0, 1000.0), 24.0);
        assert_eq!(place_on_axis(5000.0, 240.0, 0.0, 1000.0), 736.0);
        assert_eq!(place_on_axis(300.0, 240.0, 0.0, 1000.0), 300.0);
        // Room for the window but not both margins: centered.
        assert_eq!(place_on_axis(0.0, 240.0, 100.0, 260.0), 110.0);
        // Window larger than the span: start-aligned.
        assert_eq!(place_on_axis(0.0, 240.0, 100.0, 200.0), 100.0);
        // NaN size never panics.
        assert_eq!(place_on_axis(0.0, f64::NAN, 100.0, 200.0), 100.0);
    }

    #[test]
    fn rect_validity() {
        assert!(Rect::new(-10.0, -10.0, 1.0, 1.0).is_valid());
        assert!(!Rect::new(0.0, 0.0, 0.0, 10.0).is_valid());
        assert!(!Rect::new(0.0, 0.0, 10.0, -1.0).is_valid());
        assert!(!Rect::new(f64::NAN, 0.0, 10.0, 10.0).is_valid());
        assert!(!Rect::new(0.0, 0.0, f64::INFINITY, 10.0).is_valid());
    }
}
