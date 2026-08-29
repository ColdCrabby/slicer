//! Core classic-wall bead computation and helper functions.

use std::sync::atomic::{AtomicU64, Ordering};

use clipper2::*;

use super::types::{Bead, WallParams};

// ── Per-run timing accumulators (CPU time Σ across all worker threads) ────────
pub static CLASSIC_COLLAPSE_NS: AtomicU64 = AtomicU64::new(0);
pub static CLASSIC_BEAD_SHRINK_NS: AtomicU64 = AtomicU64::new(0);

/// Inward-offset helper: shrink `input` by `depth` and simplify.
/// Uses `JoinType::Round` to produce smooth bead centerline corners.
pub fn shrink(input: &Paths, depth: f64, tol: f64) -> Paths {
    simplify(
        inflate(
            input.clone(),
            -depth,
            JoinType::Round,
            EndType::Polygon,
            2.0,
        ),
        tol,
        false,
    )
}

/// Drop bead centerline paths whose enclosed area is below `min_area`.
///
/// Slicing a triangulated mesh with sliver/degenerate triangles (common in
/// hand-modeled assets such as the 3DBenchy `#3DBenchy` engraving on the hull
/// stern) frequently yields multiple coincident or near-coincident contour
/// loops at the same XY location.  After Clipper2's negative offset (`shrink`)
/// these collapse into many tiny "centerline" polygons with sub-mm extent and
/// effectively zero enclosed area.  Treating each as a real outer-wall bead
/// produces hundreds of useless retract/travel/extrude pairs per layer in the
/// G-code, which manifests as missing or fragmented perimeters when the slice
/// is rendered.
///
/// `min_area` is intentionally generous (~0.01 × d²) to drop pure noise while
/// preserving any legitimate small feature whose centerline still encloses a
/// printable area.
pub fn drop_degenerate_beads(paths: Paths, min_area: f64) -> Paths {
    let kept: Vec<Path> = paths
        .iter()
        .filter(|p| p.signed_area().abs() >= min_area)
        .cloned()
        .collect();
    Paths::new(kept)
}

/// Reorder beads for the configured perimeter sequence.
///
/// Beads are computed outermost-first (the `is_outer` bead is at index 0).  When
/// [`WallParams::external_perimeters_first`] is `false` (the default) the outer
/// wall must print **last**, so the whole sequence is reversed to innermost-first
/// — the PrusaSlicer / OrcaSlicer / Cura ordering.  When `true` the outer-first
/// order the generator produces is kept as-is.
pub fn order_beads(mut beads: Vec<Bead>, external_perimeters_first: bool) -> Vec<Bead> {
    if !external_perimeters_first {
        beads.reverse();
    }
    beads
}

/// Effective wall-loop cap once `extra_perimeters` is taken into account.
///
/// Returns [`usize::MAX`] — keep adding full loops until the geometry collapses —
/// when `extra_perimeters` is enabled and the core left after the nominal
/// `wall_count` loops is *uniformly* narrower than `extra_perimeters_max_gap_mm`
/// (a thin gap better filled with perimeters than sparse infill).  Otherwise
/// returns `wall_count`: a core with any part wider than the threshold stays
/// infill's job, so a solid body is never turned into concentric loops.
fn effective_wall_count(input: &Paths, params: &WallParams, tol: f64) -> usize {
    if !params.extra_perimeters || params.wall_count == 0 {
        return params.wall_count;
    }
    let d = params.nozzle_diameter_mm;
    let core = shrink(input, params.wall_count as f64 * d, tol);
    if core.is_empty() {
        return params.wall_count;
    }
    // Erosion empty ⟺ no part of the core is wider than the gap ⟺ uniformly thin.
    let half = params.extra_perimeters_max_gap_mm * 0.5;
    let eroded = inflate(core, -half, JoinType::Miter, EndType::Polygon, 2.0);
    if eroded.is_empty() {
        usize::MAX
    } else {
        params.wall_count
    }
}

/// Cheap collapse probe using Miter join — only checks whether the result is
/// empty.  Much faster than `shrink` for emptiness tests because Miter never
/// inserts arc approximation vertices.
pub fn collapses_at(input: &Paths, depth: f64, tol: f64) -> bool {
    simplify(
        inflate(
            input.clone(),
            -depth,
            JoinType::Miter,
            EndType::Polygon,
            2.0,
        ),
        tol,
        false,
    )
    .is_empty()
}

/// Narrow binary search for the exact collapse depth within `[lo, hi]` where
/// `lo` is known to be non-collapsing and `hi` is known to be collapsing.
///
/// Uses `JoinType::Miter` (no arc vertices) because we only test emptiness.
/// 4 iterations give `(hi - lo) / 16` precision over at most one bead-width
/// (0.4mm), so the worst-case error is 0.025mm — well within FDM tolerance.
pub fn narrow_collapse_search(input: &Paths, mut lo: f64, mut hi: f64, tol: f64) -> f64 {
    for _ in 0..4 {
        let mid = (lo + hi) / 2.0;
        if collapses_at(input, mid, tol) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    lo
}

/// Compute classic beads for the given polygon set.
///
/// Returns beads ordered from the outermost wall inward.
///
/// ## Collapse-depth strategy
///
/// Instead of a standalone binary search to find the inradius `D` before
/// placing any beads, we fold the collapse detection into the bead loop:
/// each bead is tested directly by calling `shrink` at its target depth. If
/// the result is empty the polygon cannot sustain that bead (geometry-limited)
/// and we stop.
///
/// When geometry-limited we already know tight bounds on `D`:
/// - lower bound: last fitting bead centerline depth
/// - upper bound: first non-fitting bead centerline depth
///
/// A 4-iteration Miter narrow search within this narrow `[lo, hi]` interval
/// (at most one bead-width = 0.4 mm) locates `D` to <0.025 mm, which is
/// sufficient for FDM thin-wall residual bead placement.
///
/// **Common case** (all `wall_count` beads fit, count-limited): no narrow
/// search at all.  Total Clipper calls = `wall_count` (vs the old 17+N).
/// **Geometry-limited** case: `wall_count` fit tests + 4 narrow probes + 1
/// residual `shrink` ≈ 8 calls (vs the old 17+N+1).
pub fn compute_classic_beads(input: &Paths, params: &WallParams) -> Vec<Bead> {
    let d = params.nozzle_diameter_mm;
    let min_w = params.wall_line_width_min_mm;
    let max_w = params.wall_line_width_max_mm;
    let tol = 1e-4 * d.max(0.01);
    // Drop bead centerlines whose enclosed area is below ~1% of a bead-square.
    // See [`drop_degenerate_beads`] — filters mesh-noise contours that survive
    // the negative offset as zero-area "back-and-forth" line stubs.
    let min_bead_area = 0.01 * d * d;

    // Degenerate / zero-area polygon: bail immediately.
    if collapses_at(input, 1e-6, tol) {
        return vec![];
    }

    let mut beads = Vec::new();

    // ── Standard full-width beads ─────────────────────────────────────────────
    //
    // Try each bead position directly.  If `shrink` returns empty the polygon
    // cannot sustain that bead and we stop.  This replaces the separate
    // `find_collapse_depth` binary search that ran 17 `inflate` calls before
    // touching any actual bead.
    //
    // The last depth that produced a non-empty result and the first depth that
    // produced an empty result give us tight bounds for `big_d` if we need it
    // later (thin-wall residual case).
    let mut bead_path_counts: Vec<usize> = Vec::with_capacity(params.wall_count);
    let mut last_fit_depth: f64 = 0.0; // lower bound on big_d
    let mut first_miss_depth: f64 = (params.wall_count as f64 + 0.5) * d; // upper bound
    let mut n_fit: usize = 0;

    // `extra_perimeters` may lift the cap so a thin residual core is filled with
    // full loops instead of sparse infill; see [`effective_wall_count`].
    let wall_cap = effective_wall_count(input, params, tol);

    for k in 0..wall_cap {
        let depth = (k as f64 + 0.5) * d;
        #[cfg(not(target_arch = "wasm32"))]
        let t = std::time::Instant::now();
        let paths = shrink(input, depth, tol);
        #[cfg(not(target_arch = "wasm32"))]
        CLASSIC_BEAD_SHRINK_NS.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
        if paths.is_empty() {
            first_miss_depth = depth;
            break;
        }
        // Geometry-collapse detection (`first_miss_depth`) must use the raw
        // shrink result so that mesh noise is not mistaken for surviving
        // material; bead emission, in contrast, must skip the noise paths.
        last_fit_depth = depth;
        let kept = drop_degenerate_beads(paths, min_bead_area);
        bead_path_counts.push(kept.len());
        for p in kept.iter() {
            beads.push(Bead {
                path: p.clone(),
                width_mm: d,
                is_outer: k == 0,
            });
        }
        n_fit += 1;
    }

    // Were we stopped by geometry (polygon collapsed) or by the wall cap?
    let geometry_limited = n_fit < wall_cap;

    // ── Thin-wall residual bead ───────────────────────────────────────────────
    //
    // After the standard beads the remaining inner space extends from depth
    // `n_fit × d` to `D` (on each side of the wall).
    //
    // • remaining_width = 2 × (D − n_fit × d)
    //
    // If remaining_width ≥ min_w we add one variable-width bead whose
    // centerline sits at the midpoint of the remaining space.
    //
    // If remaining_width is positive but < min_w the gap is too thin for a
    // separate bead.  In this case we widen the innermost standard bead(s) by
    // distributing the gap across up to `wall_distribution_count` beads.
    if !geometry_limited {
        return beads; // count-limited: remaining space belongs to infill
    }

    // Thin-wall detection is opt-out: when `thin_walls` is disabled we stop at
    // the standard beads and leave sub-perimeter features unfilled.
    if !params.thin_walls {
        return beads;
    }

    // Narrow search for the exact collapse depth within the tight bracket
    // [last_fit_depth, first_miss_depth] established during the bead loop.
    let lo = if n_fit > 0 { last_fit_depth } else { 0.0 };
    let big_d = narrow_collapse_search(input, lo, first_miss_depth, tol);

    // Reject if the polygon is simply too thin to hold any bead.
    if 2.0 * big_d < min_w {
        return beads;
    }

    let inner_edge_depth = n_fit as f64 * d;
    let remaining_half = big_d - inner_edge_depth;
    let remaining_width = 2.0 * remaining_half;

    if remaining_width >= min_w {
        // Add a variable-width residual bead at the center of the remaining space.
        let center_depth = inner_edge_depth + remaining_half / 2.0;
        let width = remaining_width.min(max_w);
        #[cfg(not(target_arch = "wasm32"))]
        let t = std::time::Instant::now();
        let paths = shrink(input, center_depth, tol);
        #[cfg(not(target_arch = "wasm32"))]
        CLASSIC_BEAD_SHRINK_NS.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
        let kept = drop_degenerate_beads(paths, min_bead_area);
        for p in kept.iter() {
            beads.push(Bead {
                path: p.clone(),
                width_mm: width,
                is_outer: n_fit == 0, // Outer if this is the only bead
            });
        }
    } else if remaining_width > 0.0 && !beads.is_empty() {
        // The gap is too thin for a new bead.  Widen the innermost
        // wall_distribution_count beads by spreading the remaining width
        // evenly among them.
        let n_absorb = params.wall_distribution_count.min(n_fit).max(1);
        let extra_per_bead = remaining_width / n_absorb as f64;

        // Identify the last n_absorb beads in the bead list (they are the
        // innermost standard beads) and re-generate them with a wider profile.
        // We only track beads by their position in the list; for the common
        // case of simple polygons each standard bead produces exactly one
        // path, but complex polygons may produce multiple paths per bead.
        //
        // Strategy: remove the last n_absorb path entries and regenerate.
        // We count from the end, only touching beads with width == d
        // (i.e. standard beads, not a previous residual bead).
        let absorb_start_k = n_fit.saturating_sub(n_absorb);

        // Remove the innermost n_absorb standard beads from the list.
        // Use the already-computed per-bead path counts to avoid re-calling
        // `shrink` for depths we already evaluated during the standard-beads loop.
        let paths_to_remove: usize = bead_path_counts[absorb_start_k..n_fit].iter().sum();
        let keep = beads.len().saturating_sub(paths_to_remove);
        beads.truncate(keep);

        // Re-add the affected beads with adjusted widths.
        for k in absorb_start_k..n_fit {
            let new_width = (d + extra_per_bead).min(max_w);
            // Shift the centerline outward by half the extra width so the
            // bead's inner edge aligns with the polygon center.
            let outer_edge = k as f64 * d;
            let new_depth = outer_edge + new_width / 2.0;
            #[cfg(not(target_arch = "wasm32"))]
            let t = std::time::Instant::now();
            let paths = shrink(input, new_depth, tol);
            #[cfg(not(target_arch = "wasm32"))]
            CLASSIC_BEAD_SHRINK_NS.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
            let kept = drop_degenerate_beads(paths, min_bead_area);
            for p in kept.iter() {
                beads.push(Bead {
                    path: p.clone(),
                    width_mm: new_width,
                    is_outer: k == 0, // First bead is outer wall
                });
            }
        }
    }

    beads
}

/// Debug variant of [`compute_classic_beads`].
///
/// Behaves identically to the production function but additionally records
/// every intermediate `shrink` result into `inflate_steps` so that the
/// caller can convert them into `DebugStage::WallOffsetStep` records.
///
/// Each entry in `inflate_steps` is `(bead_k, paths)` where `bead_k` is the
/// zero-based bead index (outermost = 0) and `paths` is the raw Clipper2
/// output of `shrink` at that depth — including both successful fits and the
/// final geometry-limited miss.
#[cfg(not(target_arch = "wasm32"))]
pub fn compute_classic_beads_debug(
    input: &Paths,
    params: &WallParams,
    inflate_steps: &mut Vec<(usize, Paths)>,
) -> Vec<Bead> {
    let d = params.nozzle_diameter_mm;
    let min_w = params.wall_line_width_min_mm;
    let max_w = params.wall_line_width_max_mm;
    let tol = 1e-4 * d.max(0.01);
    let min_bead_area = 0.01 * d * d;

    if collapses_at(input, 1e-6, tol) {
        return vec![];
    }

    let mut beads = Vec::new();
    let mut bead_path_counts: Vec<usize> = Vec::with_capacity(params.wall_count);
    let mut last_fit_depth: f64 = 0.0;
    let mut first_miss_depth: f64 = (params.wall_count as f64 + 0.5) * d;
    let mut n_fit: usize = 0;

    let wall_cap = effective_wall_count(input, params, tol);

    for k in 0..wall_cap {
        let depth = (k as f64 + 0.5) * d;
        let paths = shrink(input, depth, tol);
        // Capture the intermediate regardless of whether it's empty.
        inflate_steps.push((k, paths.clone()));
        if paths.is_empty() {
            first_miss_depth = depth;
            break;
        }
        last_fit_depth = depth;
        let kept = drop_degenerate_beads(paths, min_bead_area);
        bead_path_counts.push(kept.len());
        for p in kept.iter() {
            beads.push(Bead {
                path: p.clone(),
                width_mm: d,
                is_outer: k == 0,
            });
        }
        n_fit += 1;
    }

    let geometry_limited = n_fit < wall_cap;

    if !geometry_limited {
        return beads;
    }

    if !params.thin_walls {
        return beads;
    }

    let lo = if n_fit > 0 { last_fit_depth } else { 0.0 };
    let big_d = narrow_collapse_search(input, lo, first_miss_depth, tol);

    if 2.0 * big_d < min_w {
        return beads;
    }

    let inner_edge_depth = n_fit as f64 * d;
    let remaining_half = big_d - inner_edge_depth;
    let remaining_width = 2.0 * remaining_half;

    if remaining_width >= min_w {
        let center_depth = inner_edge_depth + remaining_half / 2.0;
        let width = remaining_width.min(max_w);
        let paths = shrink(input, center_depth, tol);
        inflate_steps.push((n_fit, paths.clone()));
        let kept = drop_degenerate_beads(paths, min_bead_area);
        for p in kept.iter() {
            beads.push(Bead {
                path: p.clone(),
                width_mm: width,
                is_outer: n_fit == 0,
            });
        }
    } else if remaining_width > 0.0 && !beads.is_empty() {
        let n_absorb = params.wall_distribution_count.min(n_fit).max(1);
        let extra_per_bead = remaining_width / n_absorb as f64;
        let absorb_start_k = n_fit.saturating_sub(n_absorb);
        let paths_to_remove: usize = bead_path_counts[absorb_start_k..n_fit].iter().sum();
        let keep = beads.len().saturating_sub(paths_to_remove);
        beads.truncate(keep);
        for k in absorb_start_k..n_fit {
            let new_width = (d + extra_per_bead).min(max_w);
            let outer_edge = k as f64 * d;
            let new_depth = outer_edge + new_width / 2.0;
            let paths = shrink(input, new_depth, tol);
            inflate_steps.push((k, paths.clone()));
            let kept = drop_degenerate_beads(paths, min_bead_area);
            for p in kept.iter() {
                beads.push(Bead {
                    path: p.clone(),
                    width_mm: new_width,
                    is_outer: k == 0,
                });
            }
        }
    }

    beads
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::params::SlicingParams;

    fn default_params() -> WallParams {
        WallParams::from_slicing_params(&SlicingParams::default())
    }

    fn square_paths(side: f64) -> Paths {
        let half = side / 2.0;
        let sq: Path = vec![(-half, -half), (half, -half), (half, half), (-half, half)].into();
        Paths::new(vec![sq])
    }

    // Compatibility shim: `find_collapse_depth` was inlined into
    // `compute_classic_beads` (now using `narrow_collapse_search`).  Re-expose
    // the same logic here with a full-range binary search so existing test
    // assertions remain valid.
    fn find_collapse_depth(input: &Paths) -> f64 {
        let max_d = {
            let mut x_min = f64::INFINITY;
            let mut x_max = f64::NEG_INFINITY;
            let mut y_min = f64::INFINITY;
            let mut y_max = f64::NEG_INFINITY;
            for path in input.iter() {
                for pt in path.iter() {
                    x_min = x_min.min(pt.x());
                    x_max = x_max.max(pt.x());
                    y_min = y_min.min(pt.y());
                    y_max = y_max.max(pt.y());
                }
            }
            if x_max <= x_min || y_max <= y_min {
                return 0.0;
            }
            ((x_max - x_min).min(y_max - y_min) / 2.0).max(0.0)
        };
        if max_d <= 0.0 {
            return 0.0;
        }
        let tol = 1e-4_f64;
        if collapses_at(input, 1e-6, tol) {
            return 0.0;
        }
        // 20-iteration binary search gives ≤ max_d/2^20 ≈ 5/1M mm precision
        let mut lo = 0.0_f64;
        let mut hi = max_d;
        for _ in 0..20 {
            let mid = (lo + hi) / 2.0;
            if collapses_at(input, mid, tol) {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        lo
    }

    // ── find_collapse_depth ───────────────────────────────────────────────────

    #[test]
    fn test_collapse_depth_square() {
        // A 10×10 square has inradius = 5 mm.
        let paths = square_paths(10.0);
        let d = find_collapse_depth(&paths);
        assert!(
            (d - 5.0).abs() < 0.02,
            "collapse depth should be ≈5 mm, got {d}"
        );
    }

    #[test]
    fn test_collapse_depth_thin_rectangle() {
        // A 1×100 rectangle has inradius ≈ 0.5 mm.
        let rect: Path = vec![(0.0, 0.0), (100.0, 0.0), (100.0, 1.0), (0.0, 1.0)].into();
        let paths = Paths::new(vec![rect]);
        let d = find_collapse_depth(&paths);
        assert!(
            (d - 0.5).abs() < 0.02,
            "collapse depth for 1mm wide rect should be ≈0.5 mm, got {d}"
        );
    }

    // ── compute_classic_beads ─────────────────────────────────────────────────

    #[test]
    fn test_thick_wall_produces_standard_beads() {
        // 20×20 square, wall_count=3, nozzle=0.4 → exactly 3 full-width beads of 0.4 mm.
        // The polygon interior (D=10mm >> wall_count×d=1.2mm) is count-limited, not
        // geometry-limited, so NO residual bead should be placed inside the polygon.
        let paths = square_paths(20.0);
        let params = default_params();
        let beads = compute_classic_beads(&paths, &params);
        assert_eq!(
            beads.len(),
            3,
            "thick wall (count-limited) should produce exactly 3 beads, got {}",
            beads.len()
        );
        for bead in &beads {
            assert!(
                (bead.width_mm - 0.4).abs() < 1e-6,
                "all beads should be standard width 0.4 mm, got {}",
                bead.width_mm
            );
        }
    }

    #[test]
    fn test_thin_wall_produces_variable_width_bead() {
        // 0.5×10 rectangle → inradius ≈ 0.25 mm.
        // With nozzle=0.4, no standard bead fits (depth 0.2 < 0.25, so actually fits?).
        // Let's use a 0.3×10 rect: inradius ≈ 0.15 mm < d/2=0.2 → no standard bead.
        // 2*D=0.3 mm ≥ min_w=0.34mm? No: 0.3 < 0.34.  So let's use 0.4×10.
        // 0.4×10 rect: inradius ≈ 0.2 mm. Standard bead at depth 0.2 exactly hits D → count=0.
        // remaining_width=2*0.2=0.4 ≥ min_w=0.34 → one variable bead.
        let rect: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 0.4), (0.0, 0.4)].into();
        let paths = Paths::new(vec![rect]);
        let params = default_params();
        let beads = compute_classic_beads(&paths, &params);

        assert!(
            !beads.is_empty(),
            "0.4mm wall should produce at least one bead"
        );
        // All beads should have width ≤ nozzle_diameter (variable-width or standard).
        for bead in &beads {
            assert!(
                bead.width_mm > 0.0 && bead.width_mm <= params.wall_line_width_max_mm,
                "bead width {} should be in (0, max_w={}]",
                bead.width_mm,
                params.wall_line_width_max_mm
            );
        }
    }

    #[test]
    fn test_very_thin_wall_produces_no_beads() {
        // A wall thinner than min_width should yield no beads.
        // min_w = 0.85 * 0.4 = 0.34 mm → wall must be < 0.34 mm.
        let rect: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 0.2), (0.0, 0.2)].into();
        let paths = Paths::new(vec![rect]);
        let params = default_params();
        let beads = compute_classic_beads(&paths, &params);
        assert!(
            beads.is_empty(),
            "0.2mm wall (< min_w=0.34mm) should produce no beads, got {}",
            beads.len()
        );
    }
}
