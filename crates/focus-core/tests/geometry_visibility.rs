//! Companion placement and recovery (`geometry::default_position`,
//! `geometry::ensure_visible`). Logical points, origin at the primary display's
//! top-left, y down. Run with `cargo test -p focus-core --test geometry_visibility`.

use focus_core::geometry::{
    default_position, ensure_visible, Rect, DRAG_STRIP_HEIGHT, EDGE_MARGIN,
};

const W: f64 = 240.0;
const H: f64 = 200.0;
const M: f64 = EDGE_MARGIN;

fn win(x: f64, y: f64) -> Rect {
    Rect::new(x, y, W, H)
}

/// 1440×900 primary: 25 pt menu bar, 70 pt Dock.
const PRIMARY: Rect = Rect::new(0.0, 25.0, 1440.0, 805.0);
/// 2560×1440 external to the right of the primary (menu bar 25 pt).
const RIGHT: Rect = Rect::new(1440.0, 25.0, 2560.0, 1415.0);
/// 1920×1080 external to the LEFT of the primary: negative x.
const LEFT: Rect = Rect::new(-1920.0, 25.0, 1920.0, 1055.0);
/// 1512×982 notched laptop (37 pt menu bar, 70 pt Dock) as primary…
const LAPTOP: Rect = Rect::new(0.0, 37.0, 1512.0, 875.0);
/// …with a taller 2560×1440 display above-right of it: negative y.
const TALL: Rect = Rect::new(1512.0, -433.0, 2560.0, 1415.0);

fn primary_default() -> (f64, f64) {
    (PRIMARY.right() - W - M, PRIMARY.bottom() - H - M)
}

// ---- default_position ----------------------------------------------------------

#[test]
fn geometry_default_position_is_bottom_right_inset_by_margin() {
    assert_eq!(default_position((W, H), PRIMARY), (1176.0, 606.0));
    assert_eq!(default_position((W, H), PRIMARY), primary_default());
    assert_eq!(default_position((W, H), LAPTOP), (1248.0, 688.0));
}

#[test]
fn geometry_default_position_on_negative_coordinate_displays() {
    assert_eq!(default_position((W, H), LEFT), (-264.0, 856.0));
    assert_eq!(default_position((W, H), TALL), (3808.0, 758.0));
}

#[test]
fn geometry_default_position_in_a_tiny_work_area() {
    // Room for the window but not both margins: centered on that axis.
    assert_eq!(
        default_position((W, H), Rect::new(100.0, 50.0, 260.0, 210.0)),
        (110.0, 55.0)
    );
    // Smaller than the window: aligned to the top-left so the drag strip stays reachable.
    assert_eq!(
        default_position((W, H), Rect::new(100.0, 50.0, 200.0, 150.0)),
        (100.0, 50.0)
    );
    // Exactly enough room for both margins.
    assert_eq!(
        default_position((W, H), Rect::new(0.0, 0.0, W + 2.0 * M, H + 2.0 * M)),
        (M, M)
    );
}

#[test]
fn geometry_default_position_is_itself_visible() {
    for wa in [PRIMARY, RIGHT, LEFT, LAPTOP, TALL] {
        let (x, y) = default_position((W, H), wa);
        assert_eq!(ensure_visible(win(x, y), &[wa], 0), None, "{wa:?}");
    }
}

// ---- ensure_visible: basics ----------------------------------------------------------

#[test]
fn geometry_empty_work_area_list_returns_none() {
    assert_eq!(ensure_visible(win(100.0, 100.0), &[], 0), None);
    assert_eq!(ensure_visible(win(-9999.0, 9999.0), &[], 3), None);
}

#[test]
fn geometry_single_display_window_inside_is_left_alone() {
    assert_eq!(ensure_visible(win(100.0, 100.0), &[PRIMARY], 0), None);
    // Flush with the work-area edges (inside the margin) is still fine: no nudging.
    assert_eq!(ensure_visible(win(0.0, 25.0), &[PRIMARY], 0), None);
    assert_eq!(
        ensure_visible(
            win(PRIMARY.right() - W, PRIMARY.bottom() - H),
            &[PRIMARY],
            0
        ),
        None
    );
}

#[test]
fn geometry_single_display_window_far_away_goes_to_default() {
    assert_eq!(
        ensure_visible(win(5000.0, 5000.0), &[PRIMARY], 0),
        Some(primary_default())
    );
    assert_eq!(
        ensure_visible(win(-5000.0, -5000.0), &[PRIMARY], 0),
        Some(primary_default())
    );
    // Touching an edge from outside is zero overlap → default as well.
    assert_eq!(
        ensure_visible(win(PRIMARY.right(), 300.0), &[PRIMARY], 0),
        Some(primary_default())
    );
}

// ---- monitor disconnect ---------------------------------------------------------------

#[test]
fn geometry_window_off_screen_after_monitor_disconnect() {
    let on_external = win(3000.0, 600.0);
    assert_eq!(ensure_visible(on_external, &[PRIMARY, RIGHT], 0), None);
    // The external display is unplugged: the window is now nowhere.
    assert_eq!(
        ensure_visible(on_external, &[PRIMARY], 0),
        Some(primary_default())
    );
    // Same for a display that was left of the primary.
    let on_left = win(-1000.0, 500.0);
    assert_eq!(ensure_visible(on_left, &[PRIMARY, LEFT], 0), None);
    assert_eq!(
        ensure_visible(on_left, &[PRIMARY], 0),
        Some(primary_default())
    );
}

#[test]
fn geometry_off_screen_uses_the_primary_not_the_first_area() {
    let far = win(20_000.0, 20_000.0);
    assert_eq!(
        ensure_visible(far, &[RIGHT, PRIMARY], 1),
        Some(primary_default())
    );
    assert_eq!(
        ensure_visible(far, &[PRIMARY, RIGHT], 1),
        Some(default_position((W, H), RIGHT))
    );
}

#[test]
fn geometry_primary_index_out_of_range_falls_back_to_first_area() {
    let far = win(20_000.0, 20_000.0);
    assert_eq!(
        ensure_visible(far, &[LEFT, PRIMARY], 7),
        Some(default_position((W, H), LEFT))
    );
    // Primary naming a degenerate area also falls back to the first valid one.
    let broken = Rect::new(0.0, 0.0, 0.0, 0.0);
    assert_eq!(
        ensure_visible(far, &[broken, RIGHT, PRIMARY], 0),
        Some(default_position((W, H), RIGHT))
    );
}

#[test]
fn geometry_degenerate_work_areas_are_ignored() {
    let junk = [
        Rect::new(0.0, 0.0, 0.0, 900.0),
        Rect::new(0.0, 0.0, 1440.0, -1.0),
        Rect::new(f64::NAN, 0.0, 1440.0, 900.0),
        Rect::new(0.0, 0.0, f64::INFINITY, 900.0),
    ];
    assert_eq!(ensure_visible(win(100.0, 100.0), &junk, 0), None);
    let mut with_valid = junk.to_vec();
    with_valid.push(PRIMARY);
    assert_eq!(
        ensure_visible(win(5000.0, 100.0), &with_valid, 0),
        Some(primary_default())
    );
    assert_eq!(ensure_visible(win(100.0, 100.0), &with_valid, 0), None);
}

#[test]
fn geometry_non_finite_or_empty_window_goes_to_default() {
    for w in [
        Rect::new(f64::NAN, f64::NAN, W, H),
        Rect::new(f64::INFINITY, 100.0, W, H),
        Rect::new(100.0, 100.0, f64::NAN, H),
    ] {
        let got = ensure_visible(w, &[PRIMARY], 0).expect("moved");
        assert!(got.0.is_finite() && got.1.is_finite(), "{w:?} → {got:?}");
        assert!(got.0 >= PRIMARY.x && got.1 >= PRIMARY.y, "{w:?} → {got:?}");
    }
    assert_eq!(
        ensure_visible(win(f64::NAN, 3.0), &[PRIMARY], 0),
        Some(primary_default())
    );
    assert_eq!(
        ensure_visible(Rect::new(100.0, 100.0, 0.0, 0.0), &[PRIMARY], 0),
        Some((PRIMARY.right() - M, PRIMARY.bottom() - M))
    );
}

// ---- partially visible ------------------------------------------------------------------

#[test]
fn geometry_partially_visible_with_reachable_strip_is_left_alone() {
    // Hanging off the bottom: 110 of 200 pt visible (55%).
    let y = PRIMARY.bottom() - 110.0;
    assert_eq!(ensure_visible(win(500.0, y), &[PRIMARY], 0), None);
    // Exactly half visible still counts.
    let y = PRIMARY.bottom() - 100.0;
    assert_eq!(ensure_visible(win(500.0, y), &[PRIMARY], 0), None);
}

#[test]
fn geometry_mostly_hidden_window_is_clamped_keeping_the_good_axis() {
    // 80 of 200 pt visible (40%): y is clamped in; x was fine and is kept.
    let y = PRIMARY.bottom() - 80.0;
    assert_eq!(
        ensure_visible(win(500.0, y), &[PRIMARY], 0),
        Some((500.0, PRIMARY.bottom() - H - M))
    );
    // x off the right edge, y inside but within the margin: y stays exactly.
    assert_eq!(
        ensure_visible(win(1300.0, 30.0), &[PRIMARY], 0),
        Some((PRIMARY.right() - W - M, 30.0))
    );
}

#[test]
fn geometry_strip_off_a_side_edge_is_recovered() {
    // Mostly visible, but the drag strip runs 2 pt past the right edge.
    let x = PRIMARY.right() - W + 2.0;
    assert_eq!(
        ensure_visible(win(x, 300.0), &[PRIMARY], 0),
        Some((PRIMARY.right() - W - M, 300.0))
    );
    // Off the left edge.
    assert_eq!(
        ensure_visible(win(-60.0, 300.0), &[PRIMARY], 0),
        Some((M, 300.0))
    );
}

#[test]
fn geometry_drag_strip_clipped_by_top_edge() {
    // 10 pt under the menu bar: 95% visible, but the strip is not fully reachable.
    let got = ensure_visible(win(600.0, PRIMARY.y - 10.0), &[PRIMARY], 0);
    assert_eq!(got, Some((600.0, PRIMARY.y + M)));
    // Only the strip's height matters at the top: half of it hidden is still bad.
    let got = ensure_visible(
        win(600.0, PRIMARY.y - DRAG_STRIP_HEIGHT / 2.0),
        &[PRIMARY],
        0,
    );
    assert_eq!(got, Some((600.0, PRIMARY.y + M)));
    // Sub-point rounding from scale conversions is tolerated.
    assert_eq!(
        ensure_visible(win(600.0, PRIMARY.y - 0.5), &[PRIMARY], 0),
        None
    );
    assert_eq!(
        ensure_visible(win(600.0, PRIMARY.y - 1.0), &[PRIMARY], 0),
        None
    );
    assert!(ensure_visible(win(600.0, PRIMARY.y - 1.5), &[PRIMARY], 0).is_some());
}

// ---- multiple displays -------------------------------------------------------------------

#[test]
fn geometry_negative_coordinate_secondary_left_of_primary() {
    let areas = [PRIMARY, LEFT];
    // Fully on the left display.
    assert_eq!(ensure_visible(win(-1000.0, 500.0), &areas, 0), None);
    assert_eq!(ensure_visible(win(-1920.0, 25.0), &areas, 0), None);
    // Straddling the seam, more on the primary → into the primary.
    assert_eq!(
        ensure_visible(win(-100.0, 500.0), &areas, 0),
        Some((M, 500.0))
    );
    // Straddling, more on the left display → into the left display.
    assert_eq!(
        ensure_visible(win(-150.0, 500.0), &areas, 0),
        Some((LEFT.right() - W - M, 500.0))
    );
    // Partly past the far left edge of the left display.
    assert_eq!(
        ensure_visible(win(-1980.0, 500.0), &areas, 0),
        Some((LEFT.x + M, 500.0))
    );
    // Entirely past it → primary default.
    assert_eq!(
        ensure_visible(win(-2500.0, 500.0), &areas, 0),
        Some(primary_default())
    );
}

#[test]
fn geometry_mixed_sizes_and_offsets() {
    let areas = [LAPTOP, TALL];
    // On the tall display, above the laptop's top edge (negative y).
    assert_eq!(ensure_visible(win(2000.0, -300.0), &areas, 0), None);
    // Straddling the seam, mostly on the tall display → clamped into it, y kept.
    assert_eq!(
        ensure_visible(win(1400.0, -100.0), &areas, 0),
        Some((TALL.x + M, -100.0))
    );
    // In the dead zone above the laptop and left of the tall display.
    assert_eq!(
        ensure_visible(win(500.0, -300.0), &areas, 0),
        Some(default_position((W, H), LAPTOP))
    );
    // Hanging below the laptop's work area near the seam: the tall display is
    // beside it, not below it, so the hidden part counts as hidden.
    let y = LAPTOP.bottom() - 60.0;
    assert_eq!(
        ensure_visible(win(1000.0, y), &areas, 0),
        Some((1000.0, LAPTOP.bottom() - H - M))
    );
    // The tall display reaches lower (982) than the laptop's work area (912): a
    // window ending at 960 is fine there…
    assert_eq!(ensure_visible(win(2000.0, 760.0), &areas, 0), None);
    // …but one hanging past the tall display's bottom by more than half is not.
    assert_eq!(
        ensure_visible(win(2000.0, 950.0), &areas, 0),
        Some((2000.0, TALL.bottom() - H - M))
    );
}

#[test]
fn geometry_equal_overlap_prefers_the_primary() {
    let a = Rect::new(0.0, 0.0, 1000.0, 1000.0);
    let b = Rect::new(1000.0, 0.0, 1000.0, 1000.0);
    let straddle = win(1000.0 - W / 2.0, 400.0);
    assert_eq!(ensure_visible(straddle, &[a, b], 1), Some((b.x + M, 400.0)));
    assert_eq!(
        ensure_visible(straddle, &[a, b], 0),
        Some((a.right() - W - M, 400.0))
    );
    // No usable primary → the first of the tied areas.
    assert_eq!(
        ensure_visible(straddle, &[a, b], 9),
        Some((a.right() - W - M, 400.0))
    );
}

#[test]
fn geometry_window_larger_than_the_work_area() {
    let small = Rect::new(0.0, 25.0, 200.0, 150.0);
    let moved = ensure_visible(win(500.0, 500.0), &[small], 0);
    assert_eq!(moved, Some((0.0, 25.0)));
    // It can never be fully visible; asking again is not a move.
    assert_eq!(ensure_visible(win(0.0, 25.0), &[small], 0), None);
    // From a partly overlapping spot it is aligned to the top-left as well.
    assert_eq!(
        ensure_visible(win(-30.0, 60.0), &[small], 0),
        Some((0.0, 25.0))
    );
}

// ---- properties over many positions ----------------------------------------------------------

fn inside(w: &Rect, a: &Rect) -> bool {
    w.x >= a.x - 1.0
        && w.y >= a.y - 1.0
        && w.right() <= a.right() + 1.0
        && w.bottom() <= a.bottom() + 1.0
}

/// Independent statement of the acceptance rule from the doc comment.
fn acceptable(w: &Rect, areas: &[Rect]) -> bool {
    let strip = Rect::new(w.x, w.y, w.width, DRAG_STRIP_HEIGHT);
    areas.iter().any(|a| inside(&strip, a))
        && areas
            .iter()
            .any(|a| a.overlap_area(w) >= w.width * w.height / 2.0)
}

#[test]
fn geometry_recovery_is_visible_idempotent_and_minimal() {
    let layouts: Vec<(Vec<Rect>, usize)> = vec![
        (vec![PRIMARY], 0),
        (vec![PRIMARY, RIGHT], 0),
        (vec![PRIMARY, LEFT], 0),
        (vec![RIGHT, PRIMARY, LEFT], 1),
        (vec![LAPTOP, TALL], 0),
        (vec![TALL, LAPTOP], 1),
        (vec![LAPTOP, TALL], 5),
    ];
    let mut moved_count = 0;
    let mut kept_count = 0;
    for (areas, primary) in &layouts {
        let mut x = -4200.0;
        while x < 6500.0 {
            let mut y = -1600.0;
            while y < 2600.0 {
                let w = win(x, y);
                match ensure_visible(w, areas, *primary) {
                    None => {
                        kept_count += 1;
                        assert!(
                            acceptable(&w, areas),
                            "kept unacceptable {w:?} in {areas:?}"
                        );
                    }
                    Some((nx, ny)) => {
                        moved_count += 1;
                        assert!(
                            !acceptable(&w, areas),
                            "moved acceptable {w:?} in {areas:?}"
                        );
                        assert!((nx, ny) != (x, y), "a move must change the position");
                        let moved = win(nx, ny);
                        assert!(
                            areas.iter().any(|a| inside(&moved, a)),
                            "{w:?} → {moved:?} not fully inside any of {areas:?}"
                        );
                        assert_eq!(
                            ensure_visible(moved, areas, *primary),
                            None,
                            "not idempotent: {w:?} → {moved:?} in {areas:?}"
                        );
                    }
                }
                y += 37.0;
            }
            x += 53.0;
        }
    }
    assert!(
        moved_count > 1000 && kept_count > 1000,
        "{moved_count} / {kept_count}"
    );
}
