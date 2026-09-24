//! Output-stage path simplification.
//!
//! Thins every printer-bound polyline in two passes via [`simplify_path`]:
//! merge segments shorter than [`MIN_SEGMENT_MM`], then Ramer-Douglas-Peucker. Applied during G-code generation so that mesh detail is
//! never discarded during geometry calculations — only the printer-bound output
//! is thinned. Why both passes are needed is in the module README.
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

/// Shortest segment (mm) a printer-bound path should carry.
///
/// Sliced geometry lives on Clipper2's 0.01 mm integer grid, so a round-join
/// offset or a finely tessellated mesh leaves clusters of vertices 0.01–0.03 mm
/// apart whose directions snap to multiples of 45°. Every one of them is a kink
/// the motion planner must slow down for — a smooth arc then stutters and the
/// wall shows the jitter. RDP alone cannot drop them without a tolerance coarse
/// enough to facet the curve, so [`simplify_path`] merges them first.
pub const MIN_SEGMENT_MM: f64 = 0.1;

/// Longest run of consecutive vertices one merge may swallow. Bounds the
/// deviation check to O(n · cap) on long runs of dense, nearly straight points;
/// a forced keep is harmless because RDP follows.
const MERGE_RUN_CAP: usize = 32;

/// Simplify a printer-bound polyline: merge sub-[`MIN_SEGMENT_MM`] segments,
/// then [`douglas_peucker`] at `tolerance`.
///
/// A vertex bounding a short segment is dropped only while every vertex merged
/// into the resulting chord stays within `2 × tolerance` of it, so real corners
/// survive. On a grid-snapped circle this is both smoother and truer to the
/// model than either pass alone. `tolerance <= 0` returns the input unchanged.
///
/// # Example
/// ```
/// use slicer_engine::gcode::simplify::simplify_path;
///
/// // A 0.01 mm grid-step kink in a straight run is merged away.
/// let pts = vec![(0.0_f64, 0.0), (1.0, 0.0), (1.01, 0.01), (2.0, 0.0)];
/// assert_eq!(simplify_path(&pts, 0.0125), vec![(0.0, 0.0), (2.0, 0.0)]);
/// ```
pub fn simplify_path(points: &[(f64, f64)], tolerance: f64) -> Vec<(f64, f64)> {
    if points.len() < 3 || tolerance <= 0.0 {
        return points.to_vec();
    }
    let merged: Vec<(f64, f64)> = merge_short_segments(points, None, 2.0 * tolerance, 0.0)
        .into_iter()
        .map(|i| points[i])
        .collect();
    douglas_peucker(&merged, tolerance)
}

/// Width-aware [`simplify_path`] for variable-width beads: the merge also keeps
/// any vertex whose width departs from the interpolated width by more than
/// `width_tolerance`, then [`douglas_peucker_with_widths`] runs. Returns aligned
/// `(points, widths)`; falls back to the input on mismatched lengths.
pub fn simplify_path_with_widths(
    points: &[(f64, f64)],
    widths: &[f64],
    tolerance: f64,
    width_tolerance: f64,
) -> (Vec<(f64, f64)>, Vec<f64>) {
    if points.len() < 3 || widths.len() != points.len() || tolerance <= 0.0 {
        return (points.to_vec(), widths.to_vec());
    }
    let (p, w): (Vec<(f64, f64)>, Vec<f64>) = merge_short_segments(
        points,
        Some(widths),
        2.0 * tolerance,
        width_tolerance.max(1e-6),
    )
    .into_iter()
    .map(|i| (points[i], widths[i]))
    .unzip();
    douglas_peucker_with_widths(&p, &w, tolerance, width_tolerance)
}

/// Indices of the vertices that survive merging segments shorter than
/// [`MIN_SEGMENT_MM`]. Endpoints are always kept.
fn merge_short_segments(
    points: &[(f64, f64)],
    widths: Option<&[f64]>,
    max_dev: f64,
    width_tol: f64,
) -> Vec<usize> {
    let n = points.len();
    let mut keep = Vec::with_capacity(n);
    keep.push(0);
    for i in 1..n - 1 {
        let a = *keep.last().expect("keep starts with index 0");
        let short = dist(points[a], points[i]) < MIN_SEGMENT_MM
            || dist(points[i], points[i + 1]) < MIN_SEGMENT_MM;
        let mergeable = short
            && i - a < MERGE_RUN_CAP
            && run_within(points, widths, a, i + 1, max_dev, width_tol);
        if !mergeable {
            keep.push(i);
        }
    }
    keep.push(n - 1);
    keep
}

/// Whether every vertex strictly between `a` and `b` lies within `max_dev` of
/// the chord `a → b` and, with widths, within `width_tol` of the width
/// interpolated along it by arc length.
fn run_within(
    points: &[(f64, f64)],
    widths: Option<&[f64]>,
    a: usize,
    b: usize,
    max_dev: f64,
    width_tol: f64,
) -> bool {
    if (a + 1..b).any(|k| perpendicular_distance(points[k], points[a], points[b]) > max_dev) {
        return false;
    }
    let Some(w) = widths else {
        return true;
    };
    let total: f64 = (a..b).map(|k| dist(points[k], points[k + 1])).sum();
    let mut along = 0.0;
    (a + 1..b).all(|k| {
        along += dist(points[k - 1], points[k]);
        let t = if total > 1e-12 { along / total } else { 0.0 };
        (w[k] - (w[a] + (w[b] - w[a]) * t)).abs() <= width_tol
    })
}

fn dist(p: (f64, f64), q: (f64, f64)) -> f64 {
    (q.0 - p.0).hypot(q.1 - p.1)
}

/// Distance from `p` to the line through `a` and `b` (to `a` itself when the
/// chord is degenerate).
fn perpendicular_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx.hypot(dy);
    if len < 1e-6 {
        dist(p, a)
    } else {
        ((a.0 - p.0) * dy - (a.1 - p.1) * dx).abs() / len
    }
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

    /// A radius-`r` circle as the slicer hands it over: a 720-gon whose every
    /// vertex is followed by a round-join micro vertex, all snapped to the
    /// 0.01 mm Clipper2 grid. Closed (last point repeats the first).
    fn grid_snapped_circle(r: f64) -> Vec<(f64, f64)> {
        let snap = |v: f64| (v * 100.0).round() / 100.0;
        let mut pts = Vec::new();
        for i in 0..720 {
            let a = std::f64::consts::TAU * i as f64 / 720.0;
            for da in [0.0, 0.0004] {
                pts.push((snap(r * (a + da).cos()), snap(r * (a + da).sin())));
            }
        }
        pts.push(pts[0]);
        pts
    }

    fn turns_deg(pts: &[(f64, f64)]) -> Vec<f64> {
        pts.windows(3)
            .map(|w| {
                let a1 = (w[1].1 - w[0].1).atan2(w[1].0 - w[0].0);
                let a2 = (w[2].1 - w[1].1).atan2(w[2].0 - w[1].0);
                let t = (a2 - a1 + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
                    - std::f64::consts::PI;
                t.to_degrees()
            })
            .collect()
    }

    #[test]
    fn simplify_path_turns_a_grid_snapped_circle_into_a_smooth_arc() {
        let r = 40.0;
        let raw = grid_snapped_circle(r);
        assert!(
            turns_deg(&raw).iter().any(|&t| t < -20.0),
            "fixture must carry the zig-zag it models"
        );

        let out = simplify_path(&raw, 0.0125);
        let turns = turns_deg(&out);
        // A counter-clockwise circle only ever turns left, gently.
        assert!(
            turns.iter().all(|&t| (0.0..10.0).contains(&t)),
            "every turn must be a small left turn, got {turns:?}"
        );
        for w in out.windows(2) {
            assert!(
                dist(w[0], w[1]) >= MIN_SEGMENT_MM,
                "micro-segment survived: {:?} -> {:?}",
                w[0],
                w[1]
            );
        }
        // Still true to the model: chord midpoints stay on the circle.
        for w in out.windows(2) {
            let mid = ((w[0].0 + w[1].0) / 2.0, (w[0].1 + w[1].1) / 2.0);
            let dev = (mid.0.hypot(mid.1) - r).abs();
            assert!(dev < 0.025, "chord sags {dev} mm off the circle");
        }
    }

    #[test]
    fn simplify_path_keeps_a_real_short_step() {
        // A 0.05 mm step is a feature, not noise: it deviates past 2 × tolerance.
        let pts = vec![(0.0_f64, 0.0), (10.0, 0.0), (10.0, 0.05), (20.0, 0.05)];
        assert_eq!(simplify_path(&pts, 0.0125), pts);
    }

    #[test]
    fn simplify_path_zero_tolerance_is_a_noop() {
        let raw = grid_snapped_circle(10.0);
        assert_eq!(simplify_path(&raw, 0.0), raw);
    }

    #[test]
    fn simplify_path_with_widths_keeps_a_width_step_on_a_short_segment() {
        let pts = vec![(0.0_f64, 0.0), (5.0, 0.0), (5.05, 0.0), (10.0, 0.0)];
        let w = vec![0.4, 0.4, 0.3, 0.3];
        let (sp, sw) = simplify_path_with_widths(&pts, &w, 0.0125, 0.02);
        assert_eq!(sp.len(), sw.len());
        assert!(
            sw.iter().any(|&x| (x - 0.3).abs() < 1e-9)
                && sw.iter().any(|&x| (x - 0.4).abs() < 1e-9),
            "the width step must survive, got {sw:?}"
        );
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
