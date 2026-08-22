//! Path simplification using the Ramer-Douglas-Peucker algorithm.
//!
//! Reduces the number of points in a polyline while preserving its overall
//! shape within a configurable tolerance.  Applied during G-code generation so
//! that mesh detail is never discarded during geometry calculations — only the
//! printer-bound output is thinned.
//!
//! # Algorithm
//!
//! The [Ramer-Douglas-Peucker] algorithm recursively divides the input polyline
//! at the point that deviates most from the line connecting the current
//! endpoints.  Points whose perpendicular distance from that chord is less than
//! `tolerance` are discarded.  The process repeats until no point exceeds the
//! threshold.
//!
//! [Ramer-Douglas-Peucker]: https://en.wikipedia.org/wiki/Ramer%E2%80%93Douglas%E2%80%93Peucker_algorithm

/// Simplify a polyline using the Ramer-Douglas-Peucker algorithm.
///
/// # Arguments
/// * `points`    – ordered sequence of 2-D points `(x, y)` in mm.
/// * `tolerance` – maximum allowed perpendicular deviation from the simplified
///   line (mm).  A value of `0.0` returns the original points
///   unchanged; typical printer values are `0.01`–`0.1` mm.
///
/// # Returns
/// A new `Vec` containing only the points required to represent the polyline
/// within `tolerance`.  The first and last points are always preserved.
/// Returns an empty `Vec` when the input is empty.
///
/// # Example
/// ```
/// use slicer_engine::gcode::simplify::douglas_peucker;
///
/// // Five collinear points collapse to just the two endpoints.
/// let pts = vec![(0.0_f64, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 0.0), (4.0, 0.0)];
/// let simplified = douglas_peucker(&pts, 0.01);
/// assert_eq!(simplified, vec![(0.0, 0.0), (4.0, 0.0)]);
/// ```
pub fn douglas_peucker(points: &[(f64, f64)], tolerance: f64) -> Vec<(f64, f64)> {
    if points.len() < 2 {
        return points.to_vec();
    }

    // Shortcut: zero tolerance means no simplification.
    if tolerance <= 0.0 {
        return points.to_vec();
    }

    let mut result = Vec::with_capacity(points.len());
    rdp_recursive(points, tolerance, &mut result);
    // The recursive helper only pushes the *first* point of each segment;
    // append the overall last point to close the polyline.
    // Safety: we checked `points.len() >= 2` above, so `last()` is always Some.
    debug_assert!(
        points.len() >= 2,
        "invariant: douglas_peucker called with len < 2 after early-return guard"
    );
    result.push(*points.last().unwrap());
    result
}

/// Recursive helper that appends simplified points to `out`.
///
/// The first point of the current segment is always appended; the caller is
/// responsible for appending the very last point after the root call.
fn rdp_recursive(points: &[(f64, f64)], tolerance: f64, out: &mut Vec<(f64, f64)>) {
    let n = points.len();
    debug_assert!(n >= 2);

    // Find the point with the greatest perpendicular distance from the chord
    // that connects the first and last points of the current segment.
    let (max_dist, max_idx) = max_perpendicular_distance(points);

    if max_dist > tolerance {
        // Split at the farthest point and recurse on each half.
        rdp_recursive(&points[..=max_idx], tolerance, out);
        rdp_recursive(&points[max_idx..], tolerance, out);
    } else {
        // The whole segment is within tolerance — keep only the first point.
        // The last point will be kept by the parent call or the root caller.
        out.push(points[0]);
    }
}

/// Find the index and perpendicular distance of the point that deviates most
/// from the chord between `points[0]` and `points[last]`.
///
/// Returns `(max_distance, index)`.  The search covers indices `1..len-1`
/// (i.e. neither endpoint is a candidate).  Returns `(0.0, 1)` when there is
/// only one interior point.
fn max_perpendicular_distance(points: &[(f64, f64)]) -> (f64, usize) {
    let n = points.len();
    debug_assert!(n >= 2);

    let (x1, y1) = points[0];
    let (x2, y2) = points[n - 1];

    let dx = x2 - x1;
    let dy = y2 - y1;
    let chord_len_sq = dx * dx + dy * dy;

    let mut max_dist = 0.0_f64;
    let mut max_idx = 1_usize;

    for (i, &(px, py)) in points.iter().enumerate().skip(1).take(n - 2) {
        let dist = if chord_len_sq < 1e-12 {
            // Degenerate chord: both endpoints are essentially the same
            // point (chord length < ~1e-6 mm, i.e. squared < 1e-12 mm²).
            // Fall back to plain Euclidean distance from that point.
            let ex = px - x1;
            let ey = py - y1;
            (ex * ex + ey * ey).sqrt()
        } else {
            // Perpendicular distance from point P to line through A–B:
            // d = ||(A-P) × (A-B)|| / ||A-B||
            //   = |(x1-px)*(y2-y1) - (y1-py)*(x2-x1)| / sqrt(chord_len_sq)
            let cross = (x1 - px) * dy - (y1 - py) * dx;
            cross.abs() / chord_len_sq.sqrt()
        };

        if dist > max_dist {
            max_dist = dist;
            max_idx = i;
        }
    }

    (max_dist, max_idx)
}

/// Width-aware Ramer-Douglas-Peucker for variable-width beads.
///
/// Simplifies a polyline that carries a per-vertex extrusion width, keeping a
/// vertex when **either** its geometry deviates from the chord by more than
/// `tolerance` (mm) **or** its width deviates from the linear width interpolation
/// between the kept endpoints by more than `width_tolerance` (mm).  This lets a
/// straight run that merely *tapers* keep the vertices where the taper starts
/// and ends, so per-vertex-width beads (Arachne walk, gap fill, overlap
/// compensation) can be simplified instead of emitted at full Voronoi/Clipper
/// resolution.
///
/// Returns aligned `(points, widths)`; endpoints are always kept.  Falls back to
/// the unchanged input when lengths disagree, the polyline is trivial, or
/// `tolerance <= 0`.
pub fn douglas_peucker_with_widths(
    points: &[(f64, f64)],
    widths: &[f64],
    tolerance: f64,
    width_tolerance: f64,
) -> (Vec<(f64, f64)>, Vec<f64>) {
    let n = points.len();
    if n < 3 || widths.len() != n || tolerance <= 0.0 {
        return (points.to_vec(), widths.to_vec());
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    rdp_widths(
        points,
        widths,
        0,
        n - 1,
        tolerance,
        width_tolerance.max(1e-6),
        &mut keep,
    );

    let mut op = Vec::with_capacity(n);
    let mut ow = Vec::with_capacity(n);
    for i in 0..n {
        if keep[i] {
            op.push(points[i]);
            ow.push(widths[i]);
        }
    }
    (op, ow)
}

/// Recursive width-aware RDP over the index range `[a, b]`, flagging kept
/// vertices.  Splits at the interior vertex whose combined (geometry vs. width)
/// deviation, each normalised by its own tolerance, is greatest — provided it
/// exceeds tolerance.
fn rdp_widths(
    points: &[(f64, f64)],
    widths: &[f64],
    a: usize,
    b: usize,
    tol: f64,
    wtol: f64,
    keep: &mut [bool],
) {
    if b <= a + 1 {
        return;
    }
    let (xa, ya) = points[a];
    let (xb, yb) = points[b];
    let dx = xb - xa;
    let dy = yb - ya;
    let chord_len_sq = dx * dx + dy * dy;

    // Arc-length parameter along [a, b] for width interpolation.
    let mut cum = 0.0;
    let mut prev = points[a];
    let mut arc = vec![0.0; b - a + 1];
    for k in (a + 1)..=b {
        cum += ((points[k].0 - prev.0).powi(2) + (points[k].1 - prev.1).powi(2)).sqrt();
        arc[k - a] = cum;
        prev = points[k];
    }
    let total = cum;
    let (wa, wb) = (widths[a], widths[b]);

    let mut best_score = 1.0;
    let mut best_idx = 0;
    for i in (a + 1)..b {
        let (px, py) = points[i];
        let gdev = if chord_len_sq < 1e-12 {
            ((px - xa).powi(2) + (py - ya).powi(2)).sqrt()
        } else {
            ((xa - px) * dy - (ya - py) * dx).abs() / chord_len_sq.sqrt()
        };
        let t = if total > 1e-12 {
            arc[i - a] / total
        } else {
            0.0
        };
        let wdev = (widths[i] - (wa + (wb - wa) * t)).abs();
        let score = (gdev / tol).max(wdev / wtol);
        if score > best_score {
            best_score = score;
            best_idx = i;
        }
    }

    if best_idx > 0 {
        keep[best_idx] = true;
        rdp_widths(points, widths, a, best_idx, tol, wtol, keep);
        rdp_widths(points, widths, best_idx, b, tol, wtol, keep);
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_input_returns_empty() {
        assert_eq!(douglas_peucker(&[], 0.05), vec![]);
    }

    #[test]
    fn test_single_point_returned_unchanged() {
        let pts = vec![(1.0_f64, 2.0)];
        assert_eq!(douglas_peucker(&pts, 0.05), pts);
    }

    #[test]
    fn test_two_points_returned_unchanged() {
        let pts = vec![(0.0_f64, 0.0), (5.0, 5.0)];
        assert_eq!(douglas_peucker(&pts, 0.05), pts);
    }

    #[test]
    fn width_aware_collapses_straight_constant_width_run() {
        // Collinear, constant width → collapses to the two endpoints.
        let pts = vec![
            (0.0_f64, 0.0),
            (1.0, 0.0),
            (2.0, 0.0),
            (3.0, 0.0),
            (4.0, 0.0),
        ];
        let w = vec![0.4; 5];
        let (sp, sw) = douglas_peucker_with_widths(&pts, &w, 0.01, 0.02);
        assert_eq!(sp, vec![(0.0, 0.0), (4.0, 0.0)]);
        assert_eq!(sw, vec![0.4, 0.4]);
    }

    #[test]
    fn width_aware_keeps_taper_vertices_on_straight_run() {
        // Geometrically straight, with a multi-vertex width dip (an overlap
        // pocket): the dip must survive while the flat ends collapse.
        let pts: Vec<(f64, f64)> = (0..9).map(|i| (i as f64, 0.0)).collect();
        let w = vec![0.4, 0.4, 0.4, 0.30, 0.30, 0.30, 0.4, 0.4, 0.4];
        let (sp, sw) = douglas_peucker_with_widths(&pts, &w, 0.01, 0.02);
        assert_eq!(sp.len(), sw.len(), "points and widths stay aligned");
        assert!(
            sw.iter().any(|&x| (x - 0.30).abs() < 1e-9),
            "the dip width must be preserved, got {sw:?}"
        );
        assert!(sp.len() < 9, "flat ends should collapse, got {sp:?}");
    }

    #[test]
    fn test_collinear_points_collapse_to_endpoints() {
        // Five points on the X-axis — all intermediate points lie exactly on
        // the chord and should be removed.
        let pts = vec![
            (0.0_f64, 0.0),
            (1.0, 0.0),
            (2.0, 0.0),
            (3.0, 0.0),
            (4.0, 0.0),
        ];
        let simplified = douglas_peucker(&pts, 0.01);
        assert_eq!(simplified, vec![(0.0, 0.0), (4.0, 0.0)]);
    }

    #[test]
    fn test_zero_tolerance_returns_all_points() {
        let pts = vec![(0.0_f64, 0.0), (1.0, 1.0), (2.0, 0.0)];
        assert_eq!(douglas_peucker(&pts, 0.0), pts);
    }

    #[test]
    fn test_significant_deviation_point_is_kept() {
        // An L-shaped path: (0,0) → (0,10) → (10,10)
        // The corner at (0,10) has a perpendicular distance of 10/√2 ≈ 7.07 mm
        // from the chord (0,0)–(10,10) which exceeds any reasonable tolerance.
        let pts = vec![(0.0_f64, 0.0), (0.0, 10.0), (10.0, 10.0)];
        let simplified = douglas_peucker(&pts, 0.05);
        assert_eq!(simplified, pts, "corner must be preserved");
    }

    #[test]
    fn test_near_collinear_point_removed_below_tolerance() {
        // Introduce a tiny wobble (0.01 mm) well within the 0.05 mm tolerance.
        let pts = vec![(0.0_f64, 0.0), (1.0, 0.01), (2.0, 0.0)];
        let simplified = douglas_peucker(&pts, 0.05);
        assert_eq!(
            simplified,
            vec![(0.0, 0.0), (2.0, 0.0)],
            "tiny wobble should be removed at 0.05 mm tolerance"
        );
    }

    #[test]
    fn test_near_collinear_point_kept_above_tolerance() {
        // Wobble of 0.1 mm exceeds the 0.05 mm tolerance — must be kept.
        let pts = vec![(0.0_f64, 0.0), (1.0, 0.1), (2.0, 0.0)];
        let simplified = douglas_peucker(&pts, 0.05);
        assert_eq!(
            simplified, pts,
            "wobble exceeding tolerance must be preserved"
        );
    }

    #[test]
    fn test_square_contour_unchanged() {
        // A perfect square has no collinear/redundant vertices; all four
        // corners must survive simplification at any reasonable tolerance.
        let pts = vec![(0.0_f64, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let simplified = douglas_peucker(&pts, 0.05);
        assert_eq!(simplified, pts, "square corners must all be preserved");
    }

    #[test]
    fn test_degenerate_chord_all_same_point() {
        // All points are identical — none of them deviate from the chord.
        let pts = vec![(3.0_f64, 3.0), (3.0, 3.0), (3.0, 3.0)];
        let simplified = douglas_peucker(&pts, 0.05);
        // Should contain the first and last (which are the same point).
        assert!(!simplified.is_empty());
        assert_eq!(simplified[0], (3.0, 3.0));
    }
}
