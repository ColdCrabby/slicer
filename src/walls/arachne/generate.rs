//! Arachne wall generation — medial-axis variable-width perimeters.
//!
//! This is **Phase 2c**: it turns each shell polygon into printable beads using
//! the [`super::voronoi`] segment Voronoi and [`super::skeleton`] medial axis.
//!
//! ## Method
//!
//! 1. **Concentric full-width beads.** Successive Clipper2 negative offsets at
//!    depths `d/2, 3d/2, …` place up to `wall_count` constant-width (`d`) wall
//!    loops.  Because an offset ring is empty wherever the polygon is thinner
//!    than that depth, the bead *count* already varies locally for free — a
//!    thin spur keeps only the outer loop while a thick body keeps them all.
//!
//! 2. **Medial-axis variable-width gap fill.** The residual material the offset
//!    loops leave behind — the centre of a feature too thin to fit another full
//!    loop — is filled with a bead that follows the polygon's medial axis, its
//!    width set to the *local* residual thickness (clamped to
//!    `[min, max]`).  This is the Arachne benefit: continuous, correctly-sized
//!    fill of thin/tapering features instead of a single central blob.
//!
//! At a medial node whose distance-to-boundary is `r`, the number of full loops
//! that reach it (from each side) is `n = ⌊r/d − ½⌋ + 1`, capped at
//! `wall_count`.  The residual thickness is `2·(r − n·d)`; a gap bead is emitted
//! only when the region is geometry-limited (`n < wall_count`) and that residual
//! is a printable `[min, max]` width.  This shares the offsets' distance field,
//! so loops and gap bead never overlap.
//!
//! ## Not yet done (documented future work)
//!
//! The *outer* loops are constant width `d`; full Arachne also varies their
//! width continuously (skeletal trapezoidation with per-vertex widths, for which
//! [`super::beading`] is the foundation).  Curved Voronoi edges are chord-
//! approximated (see [`super::skeleton`]).  The per-layer Voronoi build is
//! `O(n log n)` and currently unconditional — a spatial-index / skip heuristic
//! is the main performance follow-up.

use clipper2::*;

use boostvoronoi::prelude::Diagram;

use super::skeleton::build_skeleton;
use super::voronoi::build_segment_voronoi;
use crate::core::{ExtrusionRole, SliceLayer};
use crate::walls::{WallParams, WallTimings};

/// Generate Arachne variable-width wall paths for every layer.
///
/// Mirrors [`crate::walls::classic`]'s contract: the raw `OuterWall` /
/// `InnerWall` contours are replaced with beads and every non-perimeter path is
/// preserved in its original order after the walls.
pub fn generate_arachne_walls(layers: &mut [SliceLayer], params: &WallParams) -> WallTimings {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        layers
            .par_iter_mut()
            .for_each(|layer| generate_arachne_walls_for_layer(layer, params));
    }
    #[cfg(target_arch = "wasm32")]
    for layer in layers.iter_mut() {
        generate_arachne_walls_for_layer(layer, params);
    }
    WallTimings {
        collapse_depth_ms: 0,
        bead_shrink_ms: 0,
    }
}

/// Replace the perimeter paths in a single layer with Arachne beads.
fn generate_arachne_walls_for_layer(layer: &mut SliceLayer, params: &WallParams) {
    let d = params.nozzle_diameter_mm;
    if d <= 0.0 {
        return;
    }
    let tol = 1e-4 * d.max(0.01);

    // Collect raw perimeter contours; preserve everything else verbatim.
    let raw_perimeters: Vec<Path> = layer
        .paths
        .iter()
        .enumerate()
        .filter(|(i, _)| {
            matches!(
                layer.role_for_path(*i),
                ExtrusionRole::OuterWall | ExtrusionRole::InnerWall
            )
        })
        .map(|(_, p)| p.clone())
        .collect();
    if raw_perimeters.is_empty() {
        return;
    }
    let non_perimeter: Vec<(Path, ExtrusionRole, Option<f64>, bool)> = layer
        .paths
        .iter()
        .enumerate()
        .filter(|(i, _)| {
            !matches!(
                layer.role_for_path(*i),
                ExtrusionRole::OuterWall | ExtrusionRole::InnerWall
            )
        })
        .map(|(i, p)| {
            (
                p.clone(),
                layer.role_for_path(i),
                layer.width_for_path(i),
                layer.is_path_open(i),
            )
        })
        .collect();

    // Normalise winding/overlaps exactly like the classic generator.
    let normalised = union(
        Paths::new(raw_perimeters),
        Paths::new(vec![]),
        FillRule::EvenOdd,
    )
    .unwrap_or_default();
    if normalised.is_empty() {
        return;
    }

    let mut new_paths = Paths::new(vec![]);
    let mut new_roles: Vec<ExtrusionRole> = Vec::new();
    let mut new_widths: Vec<Option<f64>> = Vec::new();
    let mut new_vwidths: Vec<Option<Vec<f64>>> = Vec::new();
    let mut new_open: Vec<bool> = Vec::new();

    // ── Concentric loops ─────────────────────────────────────────────────────
    // Emit perimeter loops exactly like the classic generator (same offsets,
    // sharp corners preserved), collecting each centerline so we can compute
    // what the loops actually cover.
    let mut loop_centerlines: Vec<Path> = Vec::new();
    let mut last = normalised.clone();
    for _k in 0..params.wall_count {
        let inset = simplify(
            inflate(
                last.clone(),
                -0.5 * d,
                JoinType::Round,
                EndType::Polygon,
                2.0,
            ),
            tol,
            false,
        );
        if inset.is_empty() {
            break;
        }
        let k = loop_centerlines.is_empty();
        for p in inset.iter() {
            new_paths.push(p.clone());
            new_roles.push(if k {
                ExtrusionRole::OuterWall
            } else {
                ExtrusionRole::InnerWall
            });
            new_widths.push(Some(d));
            new_vwidths.push(None);
            new_open.push(false);
            loop_centerlines.push(p.clone());
        }
        last = simplify(
            inflate(last, -d, JoinType::Round, EndType::Polygon, 2.0),
            tol,
            false,
        );
        if last.is_empty() {
            break;
        }
    }

    // ── Residual medial fill from actual loop coverage ───────────────────────
    // The material a loop lays down is a `d`-wide band about its centerline.
    // Whatever the union of those bands does NOT cover is the true residual —
    // an enclosed thin gap between the innermost walls (e.g. the centre of the
    // Benchy cargo-box wall).  Deriving it from coverage (rather than the eroded
    // `last`) avoids the dead zone where the onion-peel emits degenerate sliver
    // loops into a thin band instead of leaving it to be filled.  Evaluated per
    // island so a thick infill core cannot suppress a thin gap elsewhere.
    let mut covered = Paths::new(vec![]);
    for lp in &loop_centerlines {
        let band = inflate(
            Paths::new(vec![lp.clone()]),
            0.5 * d,
            JoinType::Round,
            EndType::Joined,
            2.0,
        );
        covered = union(covered.clone(), band, FillRule::NonZero).unwrap_or(covered);
    }
    let uncovered = difference(normalised, covered, FillRule::NonZero).unwrap_or_default();
    for island in split_islands(&uncovered) {
        medial_fill(
            &island,
            params,
            &mut new_paths,
            &mut new_roles,
            &mut new_widths,
            &mut new_vwidths,
            &mut new_open,
        );
    }

    // ── Non-perimeter paths, unchanged and in order ──────────────────────────
    for (path, role, width, open) in non_perimeter {
        new_paths.push(path);
        new_roles.push(role);
        new_widths.push(width);
        new_vwidths.push(None);
        new_open.push(open);
    }

    layer.paths = new_paths;
    layer.path_roles = new_roles;
    layer.path_widths = new_widths;
    layer.path_vertex_widths = new_vwidths;
    layer.path_is_open = new_open;
}

/// Emit up to `wall_count` concentric constant-width (`d`) perimeter loops for
/// one island (holes included), returning each loop centerline so the caller can
/// derive the residual the loops leave behind.  The first ring is tagged
/// [`ExtrusionRole::OuterWall`]; deeper rings are [`ExtrusionRole::InnerWall`].
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_offset_loops(
    island: &Paths,
    params: &WallParams,
    tol: f64,
    paths: &mut Paths,
    roles: &mut Vec<ExtrusionRole>,
    widths: &mut Vec<Option<f64>>,
    vwidths: &mut Vec<Option<Vec<f64>>>,
    open: &mut Vec<bool>,
) -> Vec<Path> {
    let d = params.nozzle_diameter_mm;
    let mut loop_centerlines: Vec<Path> = Vec::new();
    let mut last = island.clone();
    for _k in 0..params.wall_count {
        let inset = simplify(
            inflate(
                last.clone(),
                -0.5 * d,
                JoinType::Round,
                EndType::Polygon,
                2.0,
            ),
            tol,
            false,
        );
        if inset.is_empty() {
            break;
        }
        let is_outer = loop_centerlines.is_empty();
        for p in inset.iter() {
            paths.push(p.clone());
            roles.push(if is_outer {
                ExtrusionRole::OuterWall
            } else {
                ExtrusionRole::InnerWall
            });
            widths.push(Some(d));
            vwidths.push(None);
            open.push(false);
            loop_centerlines.push(p.clone());
        }
        last = simplify(
            inflate(last, -d, JoinType::Round, EndType::Polygon, 2.0),
            tol,
            false,
        );
        if last.is_empty() {
            break;
        }
    }
    loop_centerlines
}

/// Fill the residual an island's offset `loops` leave uncovered with medial
/// gap-fill beads.
///
/// The material a loop deposits is a `d`-wide band about its centerline; the
/// union of those bands is the covered area, and the island's material minus
/// that union is the true thin residual (an enclosed gap between the innermost
/// walls).  Deriving coverage from the centerlines — rather than the eroded
/// offset polygon — avoids the dead zone where the onion-peel emits degenerate
/// sliver loops into a thin band instead of leaving it to be filled.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_residual_medial_fill(
    island: &Paths,
    loops: &[Path],
    params: &WallParams,
    paths: &mut Paths,
    roles: &mut Vec<ExtrusionRole>,
    widths: &mut Vec<Option<f64>>,
    vwidths: &mut Vec<Option<Vec<f64>>>,
    open: &mut Vec<bool>,
) {
    let d = params.nozzle_diameter_mm;
    let mut covered = Paths::new(vec![]);
    for lp in loops {
        let band = inflate(
            Paths::new(vec![lp.clone()]),
            0.5 * d,
            JoinType::Round,
            EndType::Joined,
            2.0,
        );
        covered = union(covered.clone(), band, FillRule::NonZero).unwrap_or(covered);
    }
    let uncovered = difference(island.clone(), covered, FillRule::NonZero).unwrap_or_default();
    for sub in split_islands(&uncovered) {
        medial_fill(&sub, params, paths, roles, widths, vwidths, open);
    }
}

/// Build the Voronoi diagram, catching both the crate's `Err` results and its
/// occasional numerical panics, so a single degenerate layer can never abort
/// the whole slice.
///
/// On `wasm32` (`panic = abort`) `catch_unwind` cannot intercept a panic; the
/// input sanitisation in [`build_segment_voronoi`] is the defence there.
pub(super) fn build_voronoi_safe(paths: &Paths) -> Option<(Diagram, [f64; 2])> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        build_segment_voronoi(paths)
    }))
    .ok()
    .and_then(Result::ok)
}

/// Split a Clipper2 `Paths` into connected islands, each an outer contour plus
/// the holes it encloses.
///
/// Clipper2 output is a flat list of contours (CCW outers, CW holes); grouping
/// each outer with its contained holes lets the caller reason about one island
/// at a time — essential so a thick infill core does not suppress medial fill
/// of an unrelated thin gap elsewhere on the layer.
pub(super) fn split_islands(paths: &Paths) -> Vec<Paths> {
    let contours: Vec<Path> = paths.iter().cloned().collect();
    let holes: Vec<&Path> = contours.iter().filter(|p| p.signed_area() < 0.0).collect();
    contours
        .iter()
        .filter(|p| p.signed_area() > 0.0)
        .map(|outer| {
            let mut island = vec![outer.clone()];
            for hole in &holes {
                if outer.surrounds_path(hole) {
                    island.push((*hole).clone());
                }
            }
            Paths::new(island)
        })
        .collect()
}

/// Total length (mm) of a polyline.
fn polyline_len(pts: &[(f64, f64)]) -> f64 {
    pts.windows(2)
        .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
        .sum()
}

/// Medial-fill a (thin) region: emit variable-width open beads along its medial
/// axis wherever the local thickness (2·radius) is a printable `[min, max]`
/// width.  Thicker sub-regions get no bead — they are left for infill — and
/// sub-minimum slivers are dropped.
///
/// A Voronoi failure (error or numerical panic) degrades gracefully to no fill
/// for this region; the perimeter loops already placed are unaffected.
#[allow(clippy::too_many_arguments)]
fn medial_fill(
    region: &Paths,
    params: &WallParams,
    paths: &mut Paths,
    roles: &mut Vec<ExtrusionRole>,
    widths: &mut Vec<Option<f64>>,
    vwidths: &mut Vec<Option<Vec<f64>>>,
    open: &mut Vec<bool>,
) {
    if region.is_empty() {
        return;
    }
    // Fill only regions that are thin *everywhere* — a genuine thin feature or a
    // narrow residual gap that a single bead can span.  A thicker region is
    // infill (or needs another wall); medial-filling it would spray a spurious
    // bead along every contour where the thickness lands in range, flooding the
    // layer.  The single-bead ceiling is `gap_max` (2.5× nozzle): a 0.4 mm nozzle
    // can lay a ~1 mm line, so a residual up to that is closed with one
    // appropriately-sized bead rather than left as a void.  If eroding by half
    // `gap_max` leaves anything, the region has a thick core → skip.
    let gap_max = 2.5 * params.nozzle_diameter_mm;
    let thick_core = inflate(
        region.clone(),
        -0.5 * gap_max,
        JoinType::Round,
        EndType::Polygon,
        2.0,
    );
    if !thick_core.is_empty() {
        return;
    }
    let Some((diagram, offset)) = build_voronoi_safe(region) else {
        return;
    };
    let skel = build_skeleton(region, &diagram, offset);
    for chain in skel.chains() {
        emit_medial_beads(
            &chain,
            &skel.nodes,
            params,
            paths,
            roles,
            widths,
            vwidths,
            open,
        );
    }
}

/// Walk one medial chain, emitting each maximal run of printable-thickness nodes
/// as an open [`ExtrusionRole::GapFill`] bead whose **per-vertex** width is the
/// local gap thickness (clamped to `[min, gap_max]`), so the bead tapers with
/// the gap instead of carrying a single averaged width.
#[allow(clippy::too_many_arguments)]
fn emit_medial_beads(
    chain: &[usize],
    nodes: &[super::skeleton::SkeletonNode],
    params: &WallParams,
    paths: &mut Paths,
    roles: &mut Vec<ExtrusionRole>,
    widths: &mut Vec<Option<f64>>,
    vwidths: &mut Vec<Option<Vec<f64>>>,
    open: &mut Vec<bool>,
) {
    // A single bead may span up to 2.5× the nozzle diameter; the gap bead's
    // width is the actual local thickness so the residual is filled exactly.
    // Runs shorter than `min_len` are dropped: a real gap (e.g. a box-wall ring)
    // runs for millimetres, whereas the coverage `difference` leaves sub-mm
    // slivers along faceted boundaries that would otherwise become bead noise.
    let min_w = params.wall_line_width_min_mm;
    let gap_max = 2.5 * params.nozzle_diameter_mm;
    let min_len = 2.0 * params.nozzle_diameter_mm;
    let mut run: Vec<(f64, f64)> = Vec::new();
    let mut run_w: Vec<f64> = Vec::new();

    let flush = |run: &mut Vec<(f64, f64)>,
                 run_w: &mut Vec<f64>,
                 paths: &mut Paths,
                 roles: &mut Vec<ExtrusionRole>,
                 widths: &mut Vec<Option<f64>>,
                 vwidths: &mut Vec<Option<Vec<f64>>>,
                 open: &mut Vec<bool>| {
        if run.len() >= 2 && polyline_len(run) >= min_len {
            // Per-vertex width = local gap thickness, clamped to a printable
            // width; the scalar width is the run mean for callers that ignore
            // the per-vertex array.
            let vw: Vec<f64> = run_w.iter().map(|t| t.clamp(min_w, gap_max)).collect();
            let mean = vw.iter().sum::<f64>() / vw.len() as f64;
            let path: Path = std::mem::take(run).into();
            paths.push(path);
            roles.push(ExtrusionRole::GapFill);
            widths.push(Some(mean));
            vwidths.push(Some(vw));
            open.push(true);
        }
        run.clear();
        run_w.clear();
    };

    for &i in chain {
        let t = 2.0 * nodes[i].radius; // local wall thickness at this node
        if (min_w..=gap_max).contains(&t) {
            run.push((nodes[i].x, nodes[i].y));
            run_w.push(t);
        } else {
            flush(&mut run, &mut run_w, paths, roles, widths, vwidths, open);
        }
    }
    flush(&mut run, &mut run_w, paths, roles, widths, vwidths, open);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::params::SlicingParams;

    fn wall_params() -> WallParams {
        WallParams::from_slicing_params(&SlicingParams::default())
    }

    fn layer_with_square(side: f64) -> SliceLayer {
        let mut layer = SliceLayer::new(0.2);
        let h = side / 2.0;
        let sq: Path = vec![(-h, -h), (h, -h), (h, h), (-h, h)].into();
        layer.paths.push(sq);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.path_widths.push(None);
        layer
    }

    #[test]
    fn hollow_box_wall_center_gap_is_filled() {
        // The Benchy cargo-box case: a hollow box wall ~1.2 mm thick shows two
        // perimeter lines (outer + inner) with a center gap that must be closed
        // by a medial bead rather than left void.
        let mut layer = SliceLayer::new(0.2);
        let outer: Path = vec![(-10.0, -10.0), (10.0, -10.0), (10.0, 10.0), (-10.0, 10.0)].into();
        let hi = 10.0 - 1.2;
        let inner: Path = vec![(-hi, -hi), (-hi, hi), (hi, hi), (hi, -hi)].into();
        layer.paths.push(outer);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.path_widths.push(None);
        layer.paths.push(inner);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.path_widths.push(None);

        generate_arachne_walls_for_layer(&mut layer, &wall_params());

        let center_beads = (0..layer.paths.len())
            .filter(|&i| layer.is_path_open(i))
            .count();
        assert!(
            center_beads >= 1,
            "hollow box wall center gap must be filled with a medial bead, got {center_beads}"
        );
    }

    #[test]
    fn produces_closed_offset_walls_for_thick_square() {
        // 20 mm square, 3 walls → 3 concentric closed loops, no gap bead
        // (centre residual is infill, not a wall).
        let mut layer = layer_with_square(20.0);
        generate_arachne_walls_for_layer(&mut layer, &wall_params());

        assert_eq!(layer.role_for_path(0), ExtrusionRole::OuterWall);
        let loops = (0..layer.paths.len())
            .filter(|&i| !layer.is_path_open(i))
            .count();
        assert!(
            loops >= 3,
            "expected >=3 concentric wall loops, got {loops}"
        );
        // Every bead carries an explicit width.
        assert!(layer.path_widths.iter().all(|w| w.is_some()));
    }

    #[test]
    fn residual_gap_gets_a_variable_width_medial_bead() {
        // A 1.16 mm-thick wall (= 2.9·d): one full 0.4 mm loop fits from each
        // side, leaving a ~0.36 mm central residual the medial bead must fill.
        let mut layer = SliceLayer::new(0.2);
        let bar: Path = vec![(-10.0, -0.58), (10.0, -0.58), (10.0, 0.58), (-10.0, 0.58)].into();
        layer.paths.push(bar);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.path_widths.push(None);

        generate_arachne_walls_for_layer(&mut layer, &wall_params());

        let open_beads = (0..layer.paths.len())
            .filter(|&i| layer.is_path_open(i))
            .count();
        assert!(
            open_beads >= 1,
            "residual gap should yield at least one open medial bead, got {open_beads}"
        );
        // The medial bead width must be within [min, max].
        let p = wall_params();
        for i in 0..layer.paths.len() {
            if layer.is_path_open(i) {
                let w = layer.width_for_path(i).unwrap();
                assert!(
                    w >= p.wall_line_width_min_mm - 1e-6 && w <= p.wall_line_width_max_mm + 1e-6,
                    "medial bead width {w} out of [{}, {}]",
                    p.wall_line_width_min_mm,
                    p.wall_line_width_max_mm
                );
            }
        }
    }

    #[test]
    fn preserves_non_perimeter_paths() {
        let mut layer = layer_with_square(20.0);
        let sq: Path = vec![(-5.0, -5.0), (5.0, -5.0), (5.0, 5.0), (-5.0, 5.0)].into();
        layer.paths.push(sq);
        layer.path_roles.push(ExtrusionRole::TopSurface);
        layer.path_widths.push(None);

        generate_arachne_walls_for_layer(&mut layer, &wall_params());

        let tops = (0..layer.paths.len())
            .filter(|&i| layer.role_for_path(i) == ExtrusionRole::TopSurface)
            .count();
        assert_eq!(tops, 1, "the TopSurface path must survive");
    }

    #[test]
    fn sub_nozzle_rib_becomes_a_single_medial_bead() {
        // A 0.4 mm-thick rib (= nozzle): too thin to host a fixed-width loop
        // pair, so it must become a single variable-width medial bead rather
        // than a degenerate double-line loop (which is what Classic produces).
        let mut layer = SliceLayer::new(0.2);
        let rib: Path = vec![(-8.0, -0.2), (8.0, -0.2), (8.0, 0.2), (-8.0, 0.2)].into();
        layer.paths.push(rib);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.path_widths.push(None);

        generate_arachne_walls_for_layer(&mut layer, &wall_params());

        let open_beads = (0..layer.paths.len())
            .filter(|&i| layer.is_path_open(i))
            .count();
        let closed_loops = (0..layer.paths.len())
            .filter(|&i| !layer.is_path_open(i))
            .count();
        assert!(
            open_beads >= 1,
            "a nozzle-thick rib should yield a medial bead, got {open_beads}"
        );
        assert_eq!(
            closed_loops, 0,
            "a nozzle-thick rib should NOT get a degenerate closed loop"
        );
    }
}
