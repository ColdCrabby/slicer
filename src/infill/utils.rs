//! Utility functions for infill generation.
//!
//! Provides common helper functions used across different infill pattern
//! implementations.

use clipper2::*;

/// Calculate the infill region by offsetting perimeters inward.
///
/// Creates a small gap (typically 0.1-0.2mm) between the perimeter wall and
/// infill to ensure good layer adhesion and prevent gaps.
///
/// NOTE: This function is **not** used in the main pipeline any more.  The
/// pipeline uses `calculate_interior_region` (in `core/infill.rs`) to compute
/// the correctly-bounded interior region before calling `generate_infill`,
/// so an additional inward shrink here would double-inset and collapse the
/// infill region for narrow features.  Kept for potential future use.
#[allow(dead_code)]
pub fn calculate_infill_region(perimeters: &Paths) -> Paths {
    // Offset inward by 0.15mm to create gap between perimeter and infill
    // Negative offset = inward (deflate)
    let offset_delta = -0.15;

    // Use Clipper2 inflate operation (inflate with negative delta = deflate)
    inflate(
        perimeters.clone(),
        offset_delta,
        JoinType::Miter,
        EndType::Polygon,
        2.0, // miter limit
    )
}

/// Calculate the axis-aligned bounding box of a set of paths.
///
/// Returns (min_x, min_y, max_x, max_y) or None if paths are empty.
pub fn calculate_bounds(paths: &Paths) -> Option<(f64, f64, f64, f64)> {
    if paths.is_empty() {
        return None;
    }

    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for path in paths.iter() {
        for point in path.iter() {
            let x = point.x();
            let y = point.y();
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }

    if min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite() {
        Some((min_x, min_y, max_x, max_y))
    } else {
        None
    }
}

/// Two parametric t-values within this tolerance are considered identical.
const T_VALUE_EPSILON: f64 = 1e-9;

/// Determinant magnitude below this is treated as parallel (lines don't cross).
const PARALLEL_EPSILON: f64 = 1e-12;

/// Squared length below which a clipped sub-segment is considered degenerate
/// and is discarded (avoids emitting zero-length paths).
const MIN_SEGMENT_LENGTH_SQ: f64 = 1e-12;

/// Clip a multi-point polyline to region boundaries, maintaining connectivity.
///
/// This function processes each segment of the polyline, clips it to the region,
/// and chains together consecutive segments that remain inside the region.
fn clip_polyline_to_region(pts: &[(f64, f64)], region: &Paths, result: &mut Paths) {
    let mut current_path: Vec<(f64, f64)> = Vec::new();

    for i in 0..pts.len() - 1 {
        let (x0, y0) = pts[i];
        let (x1, y1) = pts[i + 1];

        // Collect all t values where this segment intersects a polygon edge
        let mut t_values: Vec<f64> = vec![0.0, 1.0];

        for poly in region.iter() {
            let poly_pts: Vec<(f64, f64)> = poly.iter().map(|p| (p.x(), p.y())).collect();
            let n = poly_pts.len();
            if n < 2 {
                continue;
            }

            for k in 0..n {
                if let Some(t) =
                    segment_edge_t(x0, y0, x1, y1, (poly_pts[k], poly_pts[(k + 1) % n]))
                {
                    if t > T_VALUE_EPSILON && t < 1.0 - T_VALUE_EPSILON {
                        t_values.push(t);
                    }
                }
            }
        }

        t_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        t_values.dedup_by(|a, b| (*a - *b).abs() < T_VALUE_EPSILON);

        // Process each sub-segment
        for window in t_values.windows(2) {
            let ta = window[0];
            let tb = window[1];
            let t_mid = (ta + tb) * 0.5;

            let mx = x0 + t_mid * (x1 - x0);
            let my = y0 + t_mid * (y1 - y0);

            if point_in_region_even_odd(mx, my, region) {
                let seg_start = (x0 + ta * (x1 - x0), y0 + ta * (y1 - y0));
                let seg_end = (x0 + tb * (x1 - x0), y0 + tb * (y1 - y0));

                // Check if segment is long enough
                let dx = seg_end.0 - seg_start.0;
                let dy = seg_end.1 - seg_start.1;
                if dx * dx + dy * dy <= MIN_SEGMENT_LENGTH_SQ {
                    continue;
                }

                // If current_path is empty or the new segment connects, add to current path
                if current_path.is_empty() {
                    current_path.push(seg_start);
                    current_path.push(seg_end);
                } else {
                    let last_pt = *current_path.last().unwrap();
                    let dist_sq =
                        (seg_start.0 - last_pt.0).powi(2) + (seg_start.1 - last_pt.1).powi(2);

                    // If segments connect (within tolerance), continue current path
                    if dist_sq < MIN_SEGMENT_LENGTH_SQ * 100.0 {
                        current_path.push(seg_end);
                    } else {
                        // Segments don't connect, flush current path and start new one
                        if current_path.len() >= 2 {
                            let path: Path = current_path.clone().into();
                            result.push(path);
                        }
                        current_path.clear();
                        current_path.push(seg_start);
                        current_path.push(seg_end);
                    }
                }
            } else {
                // Segment is outside, flush current path if any
                if current_path.len() >= 2 {
                    let path: Path = current_path.clone().into();
                    result.push(path);
                }
                current_path.clear();
            }
        }
    }

    // Flush any remaining path
    if current_path.len() >= 2 {
        let path: Path = current_path.into();
        result.push(path);
    }
}

/// Clip generated line segments to the infill region boundaries.
///
/// Each line path in `lines` is clipped against the closed polygon paths in
/// `region` using a segment–polygon intersection algorithm with even-odd fill
/// rule (consistent with Clipper2 boolean operations used elsewhere).
///
/// For multi-point paths (like gyroid), each segment is clipped individually
/// and connected segments that remain inside the region are combined into
/// continuous paths.
///
/// # Algorithm
/// For every line segment:
/// 1. Collect all parametric `t` values (0..=1) where the segment crosses a
///    polygon edge in `region`.
/// 2. Sort and deduplicate those values, bracketed by 0.0 and 1.0.
/// 3. For each consecutive pair `[t_a, t_b]`, evaluate the midpoint and check
///    whether it lies inside `region` using an even-odd ray-casting test across
///    all polygon paths.
/// 4. Keep segments whose midpoint is inside the region.
pub fn clip_lines_to_region(lines: &Paths, region: &Paths) -> Paths {
    if lines.is_empty() || region.is_empty() {
        return Paths::default();
    }

    let mut result = Paths::default();

    for line in lines.iter() {
        let pts: Vec<(f64, f64)> = line.iter().map(|p| (p.x(), p.y())).collect();
        if pts.len() < 2 {
            continue;
        }

        // For multi-point paths (gyroid), clip each segment and maintain connectivity
        if pts.len() > 2 {
            clip_polyline_to_region(&pts, region, &mut result);
            continue;
        }

        // For simple 2-point lines, use the optimized single-segment clipping
        let (x0, y0) = pts[0];
        let (x1, y1) = pts[1];

        // Collect all t values where this segment intersects a polygon edge.
        let mut t_values: Vec<f64> = vec![0.0, 1.0];

        for poly in region.iter() {
            let poly_pts: Vec<(f64, f64)> = poly.iter().map(|p| (p.x(), p.y())).collect();
            let n = poly_pts.len();
            if n < 2 {
                continue;
            }

            for k in 0..n {
                if let Some(t) =
                    segment_edge_t(x0, y0, x1, y1, (poly_pts[k], poly_pts[(k + 1) % n]))
                {
                    // Only keep t values strictly inside [0, 1] (not at
                    // endpoints) to avoid duplicate splits at shared vertices.
                    if t > T_VALUE_EPSILON && t < 1.0 - T_VALUE_EPSILON {
                        t_values.push(t);
                    }
                }
            }
        }

        // NaN coordinates in polygon geometry would indicate a serious upstream
        // error; treat them as equal (push to the back) so they don't corrupt
        // the sort order.
        t_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        // Deduplicate t values that are nearly equal.
        t_values.dedup_by(|a, b| (*a - *b).abs() < T_VALUE_EPSILON);

        // Emit the sub-segments that lie inside the region.
        for window in t_values.windows(2) {
            let ta = window[0];
            let tb = window[1];
            let t_mid = (ta + tb) * 0.5;

            let mx = x0 + t_mid * (x1 - x0);
            let my = y0 + t_mid * (y1 - y0);

            if point_in_region_even_odd(mx, my, region) {
                let start = (x0 + ta * (x1 - x0), y0 + ta * (y1 - y0));
                let end = (x0 + tb * (x1 - x0), y0 + tb * (y1 - y0));
                // Discard degenerate (zero-length) sub-segments.
                let dx = end.0 - start.0;
                let dy = end.1 - start.1;
                if dx * dx + dy * dy > MIN_SEGMENT_LENGTH_SQ {
                    let path: Path = vec![start, end].into();
                    result.push(path);
                }
            }
        }
    }

    result
}

/// Return the parametric `t` value (in [0, 1]) at which the line segment
/// `(lx0, ly0) → (lx1, ly1)` intersects the edge `(ex0, ey0) → (ex1, ey1)`.
///
/// Returns `None` if the segments are parallel or the intersection falls
/// outside the edge's parameter range `[0, 1]`.
fn segment_edge_t(
    lx0: f64,
    ly0: f64,
    lx1: f64,
    ly1: f64,
    edge: ((f64, f64), (f64, f64)),
) -> Option<f64> {
    let (ex0, ey0) = edge.0;
    let (ex1, ey1) = edge.1;

    let dx = lx1 - lx0;
    let dy = ly1 - ly0;
    let edx = ex1 - ex0;
    let edy = ey1 - ey0;

    let denom = dx * edy - dy * edx;
    if denom.abs() < PARALLEL_EPSILON {
        return None; // Parallel (or coincident) — no single intersection
    }

    // t: parametric position along the line segment (0 = lx0/ly0, 1 = lx1/ly1)
    let t = ((ex0 - lx0) * edy - (ey0 - ly0) * edx) / denom;
    // u: parametric position along the edge (0 = ex0/ey0, 1 = ex1/ey1)
    let u = ((ex0 - lx0) * dy - (ey0 - ly0) * dx) / denom;

    // Intersection is valid only when it lies within the edge's extent.
    if (0.0..=1.0).contains(&u) {
        Some(t)
    } else {
        None
    }
}

/// Even-odd point-in-polygon test across all paths in `region`.
///
/// Uses a horizontal ray cast from `(x, y)` to the right, counting edge
/// crossings over all polygon paths in `region` and returning `true` when the
/// count is odd (i.e. the point is "inside" under even-odd fill rule).
fn point_in_region_even_odd(x: f64, y: f64, region: &Paths) -> bool {
    let mut crossings: u32 = 0;

    for poly in region.iter() {
        let pts: Vec<(f64, f64)> = poly.iter().map(|p| (p.x(), p.y())).collect();
        let n = pts.len();
        if n < 2 {
            continue;
        }

        for k in 0..n {
            let (x0, y0) = pts[k];
            let (x1, y1) = pts[(k + 1) % n];

            // Standard even-odd crossing test: both endpoints must be strictly
            // on opposite sides of the horizontal ray (y axis).
            if (y0 < y) != (y1 < y) {
                let x_intersect = (x1 - x0) * (y - y0) / (y1 - y0) + x0;
                if x < x_intersect {
                    crossings += 1;
                }
            }
        }
    }

    crossings % 2 == 1
}

/// Join loose 2-point segments into continuous polylines.
///
/// A marching-squares contour tracer emits one short segment per grid cell it
/// crosses. Left as separate paths those segments are printed as isolated dabs —
/// each one costing a retract, travel and un-retract — and most are shorter than
/// `min_infill_extrusion_mm`, so the splat filter simply deletes them and the
/// pattern all but disappears. Chaining them by shared endpoints recovers the
/// curve the tracer actually found.
///
/// `tolerance_mm` is how close two endpoints must be to count as the same point;
/// it should be well under the tracer's cell size.
pub fn chain_segments_into_polylines(segments: &Paths, tolerance_mm: f64) -> Paths {
    if segments.is_empty() {
        return Paths::default();
    }

    // Quantising endpoints to a grid turns "is there a segment starting here?"
    // into a hash lookup instead of an O(n²) scan.
    let cell = tolerance_mm.max(1e-6);
    let key = |p: (f64, f64)| -> (i64, i64) {
        ((p.0 / cell).round() as i64, (p.1 / cell).round() as i64)
    };

    let mut segs: Vec<Vec<(f64, f64)>> = Vec::with_capacity(segments.len());
    for path in segments.iter() {
        let pts: Vec<(f64, f64)> = path.iter().map(|p| (p.x(), p.y())).collect();
        if pts.len() >= 2 {
            segs.push(pts);
        }
    }

    let mut ends: std::collections::HashMap<(i64, i64), Vec<usize>> =
        std::collections::HashMap::new();
    for (i, seg) in segs.iter().enumerate() {
        ends.entry(key(seg[0])).or_default().push(i);
        ends.entry(key(*seg.last().expect("len >= 2")))
            .or_default()
            .push(i);
    }

    let mut used = vec![false; segs.len()];
    let mut out = Paths::default();

    for start in 0..segs.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut chain = segs[start].clone();

        // Extend from both ends until nothing more connects.
        for at_front in [false, true] {
            loop {
                let tip = if at_front {
                    chain[0]
                } else {
                    *chain.last().expect("chain is never empty")
                };
                let Some(candidates) = ends.get(&key(tip)) else {
                    break;
                };
                let Some(&next) = candidates.iter().find(|&&i| !used[i]) else {
                    break;
                };
                used[next] = true;

                let mut seg = segs[next].clone();
                let touches_head = dist2(seg[0], tip) <= cell * cell;
                if at_front {
                    // Prepend, so the segment must end at the current head.
                    if touches_head {
                        seg.reverse();
                    }
                    seg.pop();
                    seg.extend(chain);
                    chain = seg;
                } else {
                    if !touches_head {
                        seg.reverse();
                    }
                    chain.extend(seg.into_iter().skip(1));
                }
            }
        }

        if chain.len() >= 2 {
            let path: Path = chain.into();
            out.push(path);
        }
    }

    out
}

fn dist2(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_bounds_empty() {
        let paths = Paths::default();
        assert!(calculate_bounds(&paths).is_none());
    }

    #[test]
    fn test_calculate_bounds_square() {
        let mut paths = Paths::default();
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        paths.push(square);

        let bounds = calculate_bounds(&paths);
        assert!(bounds.is_some());
        let (min_x, min_y, max_x, max_y) = bounds.unwrap();
        assert_eq!(min_x, 0.0);
        assert_eq!(min_y, 0.0);
        assert_eq!(max_x, 10.0);
        assert_eq!(max_y, 10.0);
    }

    #[test]
    fn test_calculate_infill_region_offsets_inward() {
        let mut perimeters = Paths::default();
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        perimeters.push(square);

        let infill_region = calculate_infill_region(&perimeters);
        // Should produce a smaller region (offset inward)
        assert!(!infill_region.is_empty(), "Expected offset region to exist");
    }

    #[test]
    fn test_clip_lines_to_region_empty_lines() {
        let lines = Paths::default();
        let mut region = Paths::default();
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        region.push(square);

        let result = clip_lines_to_region(&lines, &region);
        assert!(result.is_empty());
    }

    #[test]
    fn test_clip_lines_to_region_empty_region() {
        let mut lines = Paths::default();
        let line: Path = vec![(0.0, 5.0), (10.0, 5.0)].into();
        lines.push(line);
        let region = Paths::default();

        let result = clip_lines_to_region(&lines, &region);
        assert!(result.is_empty());
    }

    #[test]
    fn test_clip_lines_to_region_fully_inside() {
        // Line fully inside a 10×10 square — should be returned unchanged.
        let mut lines = Paths::default();
        let line: Path = vec![(2.0, 5.0), (8.0, 5.0)].into();
        lines.push(line);

        let mut region = Paths::default();
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        region.push(square);

        let result = clip_lines_to_region(&lines, &region);
        assert_eq!(
            result.len(),
            1,
            "Fully inside line should be kept as one segment"
        );
    }

    #[test]
    fn test_clip_lines_to_region_fully_outside() {
        // Line entirely outside the square — should produce no output.
        let mut lines = Paths::default();
        let line: Path = vec![(20.0, 5.0), (30.0, 5.0)].into();
        lines.push(line);

        let mut region = Paths::default();
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        region.push(square);

        let result = clip_lines_to_region(&lines, &region);
        assert!(result.is_empty(), "Fully outside line should be discarded");
    }

    #[test]
    fn test_clip_lines_to_region_crossing() {
        // Line that crosses the boundary: only the inside portion is kept.
        let mut lines = Paths::default();
        // Goes from x=-5 to x=15, crossing both sides of [0..10] square at y=5.
        let line: Path = vec![(-5.0, 5.0), (15.0, 5.0)].into();
        lines.push(line);

        let mut region = Paths::default();
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        region.push(square);

        let result = clip_lines_to_region(&lines, &region);
        assert_eq!(
            result.len(),
            1,
            "Should produce exactly one clipped segment"
        );

        let clipped: Vec<(f64, f64)> = result
            .iter()
            .next()
            .unwrap()
            .iter()
            .map(|p: &clipper2::Point<clipper2::Centi>| (p.x(), p.y()))
            .collect();
        assert_eq!(clipped.len(), 2);
        // Clipped segment should be approximately from (0,5) to (10,5).
        assert!((clipped[0].0 - 0.0).abs() < 1e-6, "Start x should be ~0");
        assert!((clipped[1].0 - 10.0).abs() < 1e-6, "End x should be ~10");
    }
}
