//! Segment-Voronoi foundation for the Arachne wall generator.
//!
//! Arachne derives its variable-width toolpaths from the **medial axis** of
//! each shell polygon.  The medial axis is a subset of the Voronoi diagram of
//! the polygon's boundary *segments* (not merely its vertices), so we build a
//! segment Voronoi diagram with the BSL-1.0 [`boostvoronoi`] crate — a 100 %
//! Rust port of `boost::polygon::voronoi`, the exact construction CuraEngine
//! and PrusaSlicer use.  BSL-1.0 is permissive, so it is compatible with both
//! the AGPL public tier and a commercial enterprise licence.
//!
//! This module is the **Phase-1 foundation**: it converts a Clipper2 `Paths`
//! (already EvenOdd-normalised by the caller) into integer segment sites and
//! returns the raw Voronoi [`Diagram`].  Skeleton filtering, the bead-count
//! function, transition placement, and centerline extraction build on top of
//! this in Phase 2.
//!
//! ## Why integer input
//!
//! `boost::polygon::voronoi` guarantees a topologically correct, deterministic
//! diagram only for **integer** input coordinates, on which it runs exact
//! predicates (internally promoting to extended integer / fp types).  We scale
//! millimetres by [`VORONOI_SCALE`] and round.  Determinism is important: the
//! same mesh must slice identically on native and wasm.

use boostvoronoi::prelude::{Builder, BvError, Diagram};
use clipper2::{simplify, Paths};

/// Fixed-point scale: millimetres → integer Voronoi input units.
///
/// 1000 units/mm = 1 µm resolution — far finer than any FDM feature.  The input
/// is translated to a local origin (see [`build_segment_voronoi`]) before
/// scaling, so even a part positioned far out on the bed stays comfortably
/// inside `i32` range after the algorithm's internal coordinate squaring.
pub const VORONOI_SCALE: f64 = 1000.0;

/// Contour simplification tolerance (mm) applied before building the diagram.
///
/// `boost::polygon::voronoi` is numerically fragile on near-collinear or
/// near-coincident vertices (curved slices produce many of them).  Collapsing
/// points within 5 µm — an order of magnitude below the nozzle — sheds most
/// degenerate sites without visibly moving the walls.
const SIMPLIFY_EPS_MM: f64 = 0.005;

/// Bounding-box minimum corner of every point in `paths` (mm), or `[0, 0]` when
/// empty.  Used to translate the polygon to a local origin.
fn bbox_min(paths: &Paths) -> [f64; 2] {
    let mut min = [f64::INFINITY, f64::INFINITY];
    for path in paths.iter() {
        for p in path.iter() {
            min[0] = min[0].min(p.x());
            min[1] = min[1].min(p.y());
        }
    }
    if min[0].is_finite() && min[1].is_finite() {
        min
    } else {
        [0.0, 0.0]
    }
}

/// Convert every closed contour in `paths` to integer segment sites, first
/// translated by `-offset` (mm) then scaled.
///
/// Consecutive vertices — including the closing edge back to the first vertex —
/// become one segment each.  Segments whose endpoints round to the same integer
/// point (sub-µm slivers) are dropped: `boost::polygon::voronoi` rejects
/// zero-length segments and coincident points.
fn to_segments(paths: &Paths, scale: f64, offset: [f64; 2]) -> Vec<[i32; 4]> {
    let mut segments = Vec::new();
    for path in paths.iter() {
        let pts: Vec<[i32; 2]> = path
            .iter()
            .map(|p| {
                [
                    ((p.x() - offset[0]) * scale).round() as i32,
                    ((p.y() - offset[1]) * scale).round() as i32,
                ]
            })
            .collect();
        let n = pts.len();
        if n < 2 {
            continue;
        }
        for i in 0..n {
            let a = pts[i];
            let b = pts[(i + 1) % n];
            if a != b {
                segments.push([a[0], a[1], b[0], b[1]]);
            }
        }
    }
    segments
}

/// Build the segment Voronoi diagram of the (already normalised) polygon set.
///
/// # Contract
///
/// The caller MUST pass Clipper2 output with canonical winding (CCW outer
/// rings, CW holes) and no self-intersections — segments may only meet at
/// shared endpoints, which `boost::polygon::voronoi` requires.  Reuse the same
/// `union(…, EvenOdd)` normalisation the classic generator applies.
///
/// The polygon is simplified ([`SIMPLIFY_EPS_MM`]) and translated to a local
/// origin before scaling, both for numerical robustness.  The returned offset
/// (the pre-translation bounding-box minimum, mm) must be added back to vertex
/// coordinates — after dividing by [`VORONOI_SCALE`] — to recover world
/// millimetres; [`super::skeleton::build_skeleton`] does exactly that.
///
/// # Errors
///
/// Returns [`BvError`] if the builder rejects the input.  A deeper numerical
/// panic *inside* the crate is not an `Err`; the caller in [`super::generate`]
/// contains it with [`std::panic::catch_unwind`] and falls back to the plain
/// offset loops.
pub fn build_segment_voronoi(paths: &Paths) -> Result<(Diagram, [f64; 2]), BvError> {
    let cleaned = simplify(paths.clone(), SIMPLIFY_EPS_MM, false);
    let src = if cleaned.iter().next().is_some() {
        cleaned
    } else {
        paths.clone()
    };
    let offset = bbox_min(&src);
    let segments = to_segments(&src, VORONOI_SCALE, offset);
    let diagram = Builder::<i32>::default()
        .with_segments(segments.iter())?
        .build()?;
    Ok((diagram, offset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clipper2::{Path, Paths};

    fn square(side: f64) -> Paths {
        let h = side / 2.0;
        let sq: Path = vec![(-h, -h), (h, -h), (h, h), (-h, h)].into();
        Paths::new(vec![sq])
    }

    #[test]
    fn to_segments_closes_the_contour() {
        // A 4-vertex square must yield 4 segments (the closing edge included).
        let segs = to_segments(&square(10.0), VORONOI_SCALE, [0.0, 0.0]);
        assert_eq!(segs.len(), 4, "square should produce 4 closed segments");
    }

    #[test]
    fn to_segments_drops_zero_length_edges() {
        // Duplicate consecutive vertex must not create a zero-length segment.
        let p: Path = vec![(0.0, 0.0), (0.0, 0.0), (10.0, 0.0), (10.0, 10.0)].into();
        let segs = to_segments(&Paths::new(vec![p]), VORONOI_SCALE, [0.0, 0.0]);
        for s in &segs {
            assert!(
                s[0] != s[2] || s[1] != s[3],
                "no zero-length segment expected, got {s:?}"
            );
        }
    }

    #[test]
    fn builds_segment_voronoi_of_square() {
        let (diagram, off) = build_segment_voronoi(&square(20.0)).expect("voronoi build");
        assert!(diagram.num_vertices() > 0, "expected Voronoi vertices");
        assert!(diagram.num_edges() > 0, "expected Voronoi edges");

        // The medial axis of a square passes through its centre (0,0): a
        // Voronoi vertex equidistant from all four edges must exist there.
        // Vertices are in the translated frame; add the offset back.
        let has_centre = diagram.vertices().iter().any(|v| {
            let x = v.x() / VORONOI_SCALE + off[0];
            let y = v.y() / VORONOI_SCALE + off[1];
            x.abs() < 0.5 && y.abs() < 0.5
        });
        assert!(
            has_centre,
            "expected a medial-axis vertex near the square centre"
        );
    }

    #[test]
    fn builds_medial_axis_for_square_with_hole() {
        // Annulus: 40 mm outer square, 20 mm inner hole (CW winding for the
        // hole, as Clipper2 produces).  The medial axis of the wall band must
        // include vertices strictly inside the band (|coord| between 10 and 20).
        let outer: Path = vec![(-20.0, -20.0), (20.0, -20.0), (20.0, 20.0), (-20.0, 20.0)].into();
        let hole: Path = vec![(-10.0, -10.0), (-10.0, 10.0), (10.0, 10.0), (10.0, -10.0)].into();
        let paths = Paths::new(vec![outer, hole]);

        let (diagram, off) = build_segment_voronoi(&paths).expect("voronoi build");
        let band_vertices = diagram
            .vertices()
            .iter()
            .filter(|v| {
                let x = (v.x() / VORONOI_SCALE + off[0]).abs();
                let y = (v.y() / VORONOI_SCALE + off[1]).abs();
                // Inside the wall band on at least one axis, outside the hole.
                (10.5..19.5).contains(&x.max(y)) && x.min(y) < 19.5
            })
            .count();
        assert!(
            band_vertices > 0,
            "expected medial-axis vertices within the wall band"
        );
    }
}
