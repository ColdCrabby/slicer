//! Wall overlap flow compensation.
//!
//! Where two wall beads (or the two legs of one bead at a ~180° hairpin, or the
//! insides of an acute concave corner) run **closer than their combined width**,
//! a constant-flow extrusion deposits material into space an adjacent bead has
//! already filled — visible as over-extrusion, blobs, and dimensional bulge.
//! This pass scales that extrusion **down** across the overlap so the total
//! deposited volume matches the area actually covered, exactly like CuraEngine's
//! `WallOverlapComputation` and PrusaSlicer's perimeter-overlap handling.
//!
//! ## How it works
//!
//! Extrusion flow is proportional to bead width, and the G-code generator already
//! derives per-segment E from [`SliceLayer::path_vertex_widths`]
//! (`E ∝ ½(w[j] + w[j+1])`).  So compensation is a pure **width-reduction** pass:
//! it never touches geometry, only writes a reduced per-vertex width array for
//! the beads that overlap.  Nothing downstream changes.
//!
//! At a bead vertex `p` of width `w_p`, let `d` be the distance to the nearest
//! *non-adjacent* wall segment of width `w_s`.  The two footprints overlap by
//!
//! ```text
//! o = ½(w_p + w_s) − d      (clamped to ≥ 0)
//! ```
//!
//! The excess area is `o` per unit length; split evenly between the two beads
//! (both are processed, each sees the other) each sheds `o/2` of width, so their
//! summed deposit equals the union — no over-extrusion, no net material loss.
//! `wall_overlap_compensation` scales the shed amount (`0` = off, `1` = full).
//!
//! ## Why "non-adjacent"
//!
//! Segments meeting at a shared vertex (a normal corner, a Clipper round join)
//! are trivially within a bead width of each other and must **not** count as
//! overlap.  Only the two edges *incident* to the query vertex are skipped; any
//! other segment of the same bead — a hair-pin fold-back, the far side of a thin
//! slot — is still compared, so even short fold-backs are caught.  Different
//! beads are always compared (two concentric wall loops sit exactly `d` apart,
//! giving `o = 0`, so proper spacing self-cancels).
//!
//! ## Scope & non-goals
//!
//! * Compensates **wall** roles against wall roles (outer/inner/overhang/gap
//!   fill) — the reported hairpin/slot case.  Cross-group (wall↔infill) overlap
//!   and inner-corner pile-up are separate, lower-impact passes left for later.
//! * Deterministic: computed from the original widths, applied at the end, no
//!   dependence on print order.

use std::collections::HashMap;

use crate::core::{ExtrusionRole, SliceLayer};
use crate::settings::params::{SlicingParams, WallGenerator};

/// Never shed more than this fraction of a bead's width, so compensation can
/// thin a grossly-overlapping bead but never starve it to zero flow.
const MAX_SHED_FRACTION: f64 = 0.75;

/// Minimum width to shed (mm) before a bead is marked for compensation.  Below
/// this the overlap is negligible over-extrusion not worth disabling the
/// G-code's path simplification for (which per-vertex widths force off); it also
/// rejects the sub-µm noise that nominal wall spacing produces at rounded joins.
const MIN_SHED_MM: f64 = 0.04;

/// Wall roles whose beads participate in overlap compensation.
fn is_wall_role(role: ExtrusionRole) -> bool {
    matches!(
        role,
        ExtrusionRole::OuterWall
            | ExtrusionRole::InnerWall
            | ExtrusionRole::OverhangPerimeter
            | ExtrusionRole::GapFill
    )
}

/// Reduce wall extrusion where beads overlap, across every layer.
///
/// No-op when `wall_overlap_compensation ≤ 0`.  Runs after path ordering so the
/// per-vertex widths it writes align with the final printed vertex order.
pub fn compensate(layers: &mut [SliceLayer], params: &SlicingParams) {
    let strength = params.wall_overlap_compensation;
    if strength <= 0.0 || params.nozzle_diameter_mm <= 0.0 {
        return;
    }
    // Classic places beads at exact spacing and fills thin residual via bead
    // width distribution; shedding that deliberately-added width would undo it.
    // Overlap compensation is an Arachne-family concern — its variable-width
    // beads and gap fill are what run close in slots and hairpins.
    if matches!(params.wall_generator, WallGenerator::Classic) {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        layers
            .par_iter_mut()
            .for_each(|layer| compensate_layer(layer, params, strength));
    }
    #[cfg(target_arch = "wasm32")]
    for layer in layers.iter_mut() {
        compensate_layer(layer, params, strength);
    }
}

/// One wall bead reduced to the data the overlap search needs.
struct Bead {
    /// Index into the layer's parallel path arrays.
    path_idx: usize,
    pts: Vec<(f64, f64)>,
    /// Base per-vertex width (from per-vertex widths, else the scalar/role width).
    w: Vec<f64>,
    closed: bool,
}

/// A (possibly split) wall segment placed in the spatial grid.
struct Seg {
    a: (f64, f64),
    b: (f64, f64),
    w: f64,
    bead: usize,
    /// Source edge index within its bead (vertex `edge` → next); lets a query
    /// vertex skip only the two edges incident to it.
    edge: usize,
}

fn compensate_layer(layer: &mut SliceLayer, params: &SlicingParams, strength: f64) {
    let d = params.nozzle_diameter_mm;

    // ── 1. Gather wall beads with per-vertex widths and arc lengths ──────────
    let beads: Vec<Bead> = (0..layer.paths.len())
        .filter(|&i| is_wall_role(layer.role_for_path(i)))
        .filter_map(|i| build_bead(layer, i))
        .collect();
    if beads.is_empty() {
        return;
    }

    let max_w = beads
        .iter()
        .flat_map(|b| b.w.iter().copied())
        .fold(0.0_f64, f64::max)
        .max(d);
    // Cell ≥ the max search radius (max bead width) so a 3×3 query covers every
    // segment that could overlap the query point.
    let cell = 1.5 * max_w;

    // ── 2. Build split segments + uniform grid ───────────────────────────────
    let mut segs: Vec<Seg> = Vec::new();
    for (bi, b) in beads.iter().enumerate() {
        let n = b.pts.len();
        let last = if b.closed { n } else { n - 1 };
        for j in 0..last {
            let k = (j + 1) % n;
            split_segment(b, j, k, cell, bi, &mut segs);
        }
    }
    if segs.is_empty() {
        return;
    }
    let grid = build_grid(&segs, cell);

    // ── 3. Per-vertex overlap → width reduction ──────────────────────────────
    if layer.path_vertex_widths.len() < layer.paths.len() {
        layer.path_vertex_widths.resize(layer.paths.len(), None);
    }
    for (bi, b) in beads.iter().enumerate() {
        let n = b.pts.len();
        let mut new_w = b.w.clone();
        let mut changed = false;
        for (vi, nw) in new_w.iter_mut().enumerate() {
            let p = b.pts[vi];
            let w_p = *nw;
            // Edges incident to this vertex trivially touch it and are skipped;
            // every other segment — including a hair-pin fold-back that runs
            // close in space but far in topology — is compared.
            let out_edge = vi;
            let in_edge = if b.closed {
                (vi + n - 1) % n
            } else {
                vi.wrapping_sub(1)
            };
            let mut o_max = 0.0_f64;
            for si in nearby(&grid, p, cell) {
                let s = &segs[si];
                if s.bead == bi && (s.edge == out_edge || s.edge == in_edge) {
                    continue;
                }
                let dist = point_seg_dist(p, s.a, s.b);
                let o = 0.5 * (w_p + s.w) - dist;
                if o > o_max {
                    o_max = o;
                }
            }
            if o_max > 1e-6 {
                let shed = (strength * 0.5 * o_max).min(MAX_SHED_FRACTION * w_p);
                if shed >= MIN_SHED_MM {
                    *nw = w_p - shed;
                    changed = true;
                }
            }
        }
        if changed {
            layer.path_vertex_widths[b.path_idx] = Some(new_w);
        }
    }
}

/// Assemble a [`Bead`] from a layer path, resolving its per-vertex widths.
fn build_bead(layer: &SliceLayer, i: usize) -> Option<Bead> {
    let pts: Vec<(f64, f64)> = layer.paths.get(i)?.iter().map(|p| (p.x(), p.y())).collect();
    if pts.len() < 2 {
        return None;
    }
    let role = layer.role_for_path(i);
    let scalar_w = layer
        .width_for_path(i)
        .unwrap_or_else(|| role.default_width_mm());
    let w: Vec<f64> = match layer.vertex_widths_for_path(i) {
        Some(vw) if vw.len() == pts.len() => vw,
        _ => vec![scalar_w; pts.len()],
    };
    let closed = !layer.is_path_open(i);

    Some(Bead {
        path_idx: i,
        pts,
        w,
        closed,
    })
}

/// Split one bead edge into sub-segments no longer than `cell` (so each lands in
/// only a handful of grid cells), interpolating width and arc length.
fn split_segment(b: &Bead, j: usize, k: usize, cell: f64, bead: usize, out: &mut Vec<Seg>) {
    let (pa, pb) = (b.pts[j], b.pts[k]);
    let len = dist(pa, pb);
    if len < 1e-9 {
        return;
    }
    let (wa, wb) = (b.w[j], b.w[k]);
    let pieces = (len / cell).ceil().max(1.0) as usize;
    for s in 0..pieces {
        let t0 = s as f64 / pieces as f64;
        let t1 = (s + 1) as f64 / pieces as f64;
        let a = lerp_pt(pa, pb, t0);
        let c = lerp_pt(pa, pb, t1);
        let tm = 0.5 * (t0 + t1);
        out.push(Seg {
            a,
            b: c,
            w: wa + (wb - wa) * tm,
            bead,
            edge: j,
        });
    }
}

/// Bucket every segment into the grid cells its endpoints occupy (plus the
/// cell of its midpoint), keyed by integer cell coordinate.
fn build_grid(segs: &[Seg], cell: f64) -> HashMap<(i32, i32), Vec<usize>> {
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, s) in segs.iter().enumerate() {
        let mid = (0.5 * (s.a.0 + s.b.0), 0.5 * (s.a.1 + s.b.1));
        for &pt in &[s.a, s.b, mid] {
            grid.entry(cell_of(pt, cell)).or_default().push(i);
        }
    }
    grid
}

/// Segment indices in the 3×3 cell block around `p` (deduplicated).
fn nearby(grid: &HashMap<(i32, i32), Vec<usize>>, p: (f64, f64), cell: f64) -> Vec<usize> {
    let (cx, cy) = cell_of(p, cell);
    let mut out = Vec::new();
    for dx in -1..=1 {
        for dy in -1..=1 {
            if let Some(v) = grid.get(&(cx + dx, cy + dy)) {
                out.extend_from_slice(v);
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

fn cell_of(p: (f64, f64), cell: f64) -> (i32, i32) {
    ((p.0 / cell).floor() as i32, (p.1 / cell).floor() as i32)
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

fn lerp_pt(a: (f64, f64), b: (f64, f64), t: f64) -> (f64, f64) {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

/// Distance from point `p` to segment `a`–`b`.
fn point_seg_dist(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let len_sq = dx * dx + dy * dy;
    let t = if len_sq <= f64::EPSILON {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len_sq).clamp(0.0, 1.0)
    };
    let cx = a.0 + t * dx;
    let cy = a.1 + t * dy;
    ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clipper2::Path;

    fn params() -> SlicingParams {
        SlicingParams::default()
    }

    /// Push an open wall bead (polyline) with a scalar width into a layer.
    fn push_open_wall(layer: &mut SliceLayer, pts: Vec<(f64, f64)>, role: ExtrusionRole) {
        let p: Path = pts.into();
        layer.paths.push(p);
        layer.path_roles.push(role);
        layer.path_widths.push(None); // use role default (0.4)
        layer.path_vertex_widths.push(None);
        layer.path_is_open.push(true);
    }

    #[test]
    fn parallel_beads_in_a_tight_slot_are_thinned() {
        // Two 0.4 mm beads 0.2 mm apart → overlap o = 0.4 − 0.2 = 0.2, each sheds
        // 0.1 → widths drop from 0.4 to ~0.3.
        let mut layer = SliceLayer::new(0.2);
        push_open_wall(
            &mut layer,
            vec![(0.0, 0.0), (10.0, 0.0)],
            ExtrusionRole::InnerWall,
        );
        push_open_wall(
            &mut layer,
            vec![(0.0, 0.2), (10.0, 0.2)],
            ExtrusionRole::InnerWall,
        );
        compensate_layer(&mut layer, &params(), 1.0);

        for vw in layer.path_vertex_widths.iter().flatten() {
            for &w in vw {
                assert!(
                    (0.28..=0.32).contains(&w),
                    "overlapping bead width {w} should be ~0.30"
                );
            }
        }
        assert!(
            layer.path_vertex_widths.iter().all(Option::is_some),
            "both overlapping beads must be compensated"
        );
    }

    #[test]
    fn beads_at_nominal_spacing_are_untouched() {
        // Two 0.4 mm beads exactly 0.4 mm apart (normal wall spacing) → o = 0,
        // no compensation, widths left as the scalar default (None per-vertex).
        let mut layer = SliceLayer::new(0.2);
        push_open_wall(
            &mut layer,
            vec![(0.0, 0.0), (10.0, 0.0)],
            ExtrusionRole::InnerWall,
        );
        push_open_wall(
            &mut layer,
            vec![(0.0, 0.4), (10.0, 0.4)],
            ExtrusionRole::InnerWall,
        );
        compensate_layer(&mut layer, &params(), 1.0);
        assert!(
            layer.path_vertex_widths.iter().all(Option::is_none),
            "properly-spaced walls must not be compensated"
        );
    }

    #[test]
    fn hairpin_legs_overlap_but_the_turn_is_skipped() {
        // A single bead: down to x=10, a tight 0.2 mm-wide U-turn, back to x=0.
        // The two legs (y=0 and y=0.2) overlap and must be thinned; the turn's
        // own adjacent segments must not self-trigger.
        let mut layer = SliceLayer::new(0.2);
        push_open_wall(
            &mut layer,
            vec![(0.0, 0.0), (10.0, 0.0), (10.0, 0.2), (0.0, 0.2)],
            ExtrusionRole::InnerWall,
        );
        compensate_layer(&mut layer, &params(), 1.0);
        let vw = layer.path_vertex_widths[0]
            .as_ref()
            .expect("hairpin bead must be compensated");
        // The leg endpoints (x=0 and x=10 ends) see the opposite leg and thin.
        assert!(
            vw.iter().any(|&w| w < 0.35),
            "hairpin legs should be thinned, got {vw:?}"
        );
    }

    #[test]
    fn disabled_when_strength_zero() {
        let mut layer = SliceLayer::new(0.2);
        push_open_wall(
            &mut layer,
            vec![(0.0, 0.0), (10.0, 0.0)],
            ExtrusionRole::InnerWall,
        );
        push_open_wall(
            &mut layer,
            vec![(0.0, 0.1), (10.0, 0.1)],
            ExtrusionRole::InnerWall,
        );
        let mut params = params();
        params.wall_overlap_compensation = 0.0;
        compensate(std::slice::from_mut(&mut layer), &params);
        assert!(
            layer.path_vertex_widths.iter().all(Option::is_none),
            "strength 0 must be a no-op"
        );
    }
}
