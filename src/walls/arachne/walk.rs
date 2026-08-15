//! Skeletal-trapezoidation walk — per-vertex variable-width walls.
//!
//! This is the opt-in [`WallGenerator::ArachneWalk`](crate::settings::params::WallGenerator)
//! path.  Where the offset-loop [`super::generate`] generator lays constant-`d`
//! perimeter loops and fills only the residual, the walk assigns **every** bead
//! a *per-vertex* width taken from the **local** wall thickness, so a wall that
//! varies in thickness around its perimeter (thin sides, thick corners, a
//! taper) gets correctly-sized beads at every point instead of one uniform
//! residual.  This is the "uneven wall → nozzle" and "keep small ribs" benefit
//! the flow plan left unmet.
//!
//! ## Scope and the thin/thick split
//!
//! Medial-axis ribbing is only correct where the medial axis runs *parallel* to
//! the walls — i.e. where the walls from both boundaries meet with no infill
//! core between them.  For a thick body the medial axis is the interior spine
//! (the "X" of a solid square), perpendicular to the walls, and ribbing it would
//! place beads across the part.  So each island is classified:
//!
//! * **Thin** (`2·r_max ≤ wall_count·d + max_bead`): the whole island is walked
//!   → variable-width beads.
//! * **Thick**: constant-width offset loops ([`super::generate::emit_offset_loops`])
//!   for the wall band + variable-width medial gap fill for the residual, exactly
//!   like [`WallGenerator::Arachne`](crate::settings::params::WallGenerator).
//!
//! A Voronoi build failure on any island degrades gracefully to the offset-loop
//! path for that island — the walk never aborts a slice.
//!
//! ## Bead indexing
//!
//! A thin wall has two facing boundaries.  Beads are indexed from *each*
//! boundary inward — `(Side::A, inset)` and `(Side::B, inset)` — because that
//! keying is invariant as the local bead count grows and shrinks along a taper:
//! the two outermost beads `(A,0)` / `(B,0)` persist while inner beads fade in
//! and out in the middle, producing clean, connected, tapering walls.  Inset 0
//! is tagged [`ExtrusionRole::OuterWall`]; deeper insets are
//! [`ExtrusionRole::InnerWall`].
//!
//! ## Not yet done
//!
//! Junctions (Y/T medial nodes) are handled by splitting the medial graph into
//! chains ([`Skeleton::chains`]) and emitting each independently, leaving a
//! sub-tolerance seam at the branch point — the same trade-off CuraEngine makes.
//! A directed half-edge graph with parabola discretisation and true
//! `connectJunctions` stitching is the documented next refinement.

use std::collections::HashSet;

use clipper2::*;

use super::beading::BeadingConfig;
use super::generate::{
    build_voronoi_safe, emit_offset_loops, emit_residual_medial_fill, split_islands,
};
use super::skeleton::{build_skeleton, SkeletonNode};
use crate::core::{ExtrusionRole, SliceLayer};
use crate::walls::{WallParams, WallTimings};

/// Which boundary a bead is indexed from (a thin wall has two facing sides).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Side {
    A,
    B,
}

/// A bead's connection key: the boundary it hugs and how many beads lie between
/// it and that boundary.  Stable as the local bead count varies along a chain.
type BeadKey = (Side, usize);

/// One bead a medial node contributes: its key, its (already normal-offset)
/// point, and its width.
type NodeBead = (BeadKey, (f64, f64), f64);

/// Radius-ratio below which a medial leaf edge is treated as a boundary-ward
/// spur (convex-vertex artifact) and pruned before the walk.  A spine node or a
/// blunt feature tip has a comparable radius at both ends (ratio near 1); a spur
/// drops sharply toward the boundary.
const SPUR_PRUNE_RATIO: f64 = 0.6;

/// Generate variable-width walls for every layer via the skeleton walk.
///
/// Mirrors [`super::generate::generate_arachne_walls`]'s contract: raw
/// `OuterWall` / `InnerWall` contours are replaced with beads and every
/// non-perimeter path is preserved in its original order after the walls.
pub fn generate_arachne_walk_walls(layers: &mut [SliceLayer], params: &WallParams) -> WallTimings {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        layers
            .par_iter_mut()
            .for_each(|layer| generate_walk_for_layer(layer, params));
    }
    #[cfg(target_arch = "wasm32")]
    for layer in layers.iter_mut() {
        generate_walk_for_layer(layer, params);
    }
    WallTimings {
        collapse_depth_ms: 0,
        bead_shrink_ms: 0,
    }
}

/// Accumulates the five parallel per-path arrays a [`SliceLayer`] carries, so
/// the walk and the shared offset helpers can append to one target.
struct WallOut {
    paths: Paths,
    roles: Vec<ExtrusionRole>,
    widths: Vec<Option<f64>>,
    vwidths: Vec<Option<Vec<f64>>>,
    open: Vec<bool>,
}

impl WallOut {
    fn new() -> Self {
        Self {
            paths: Paths::new(vec![]),
            roles: Vec::new(),
            widths: Vec::new(),
            vwidths: Vec::new(),
            open: Vec::new(),
        }
    }

    /// Push a raw path with a scalar width (used for the preserved
    /// non-perimeter paths).
    fn push_raw(
        &mut self,
        path: Path,
        role: ExtrusionRole,
        width: Option<f64>,
        vwidth: Option<Vec<f64>>,
        open: bool,
    ) {
        self.paths.push(path);
        self.roles.push(role);
        self.widths.push(width);
        self.vwidths.push(vwidth);
        self.open.push(open);
    }

    /// Push a walked bead: a polyline with a per-vertex width array.  Returns
    /// `true` when the bead was non-degenerate and emitted.
    fn push_bead(
        &mut self,
        pts: Vec<(f64, f64)>,
        vw: Vec<f64>,
        role: ExtrusionRole,
        closed: bool,
    ) -> bool {
        if pts.len() < 2 || vw.len() != pts.len() {
            return false;
        }
        let mean_w = vw.iter().sum::<f64>() / vw.len() as f64;
        let path: Path = pts.into();
        self.paths.push(path);
        self.roles.push(role);
        self.widths.push(Some(mean_w));
        self.vwidths.push(Some(vw));
        self.open.push(!closed);
        true
    }

    fn assign_to(self, layer: &mut SliceLayer) {
        layer.paths = self.paths;
        layer.path_roles = self.roles;
        layer.path_widths = self.widths;
        layer.path_vertex_widths = self.vwidths;
        layer.path_is_open = self.open;
    }
}

/// Replace the perimeter paths in one layer with walked / offset beads.
fn generate_walk_for_layer(layer: &mut SliceLayer, params: &WallParams) {
    let d = params.nozzle_diameter_mm;
    if d <= 0.0 {
        return;
    }
    let tol = 1e-4 * d.max(0.01);

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

    let normalised = union(
        Paths::new(raw_perimeters),
        Paths::new(vec![]),
        FillRule::EvenOdd,
    )
    .unwrap_or_default();
    if normalised.is_empty() {
        return;
    }

    let mut out = WallOut::new();
    for island in split_islands(&normalised) {
        walk_or_offset_island(&island, params, tol, &mut out);
    }

    for (path, role, width, open) in non_perimeter {
        out.push_raw(path, role, width, None, open);
    }

    out.assign_to(layer);
}

/// Walk one island when it is thin enough, else fall back to constant-width
/// offset loops plus residual medial fill (identical to the `Arachne` mode).
fn walk_or_offset_island(island: &Paths, params: &WallParams, tol: f64, out: &mut WallOut) {
    let d = params.nozzle_diameter_mm;

    // Build the medial skeleton, containing any Voronoi panic, then prune the
    // boundary-ward spurs convex vertices grow so the spine walks as clean
    // chains / cycles instead of a junction-riddled graph.
    let skel = build_voronoi_safe(island)
        .map(|(diagram, off)| build_skeleton(island, &diagram, off))
        .map(|s| s.prune_boundary_spurs(SPUR_PRUNE_RATIO))
        .filter(|s| !s.nodes.is_empty());

    let thin = skel.as_ref().is_some_and(|s| {
        let r_max = s.nodes.iter().map(|n| n.radius).fold(0.0_f64, f64::max);
        2.0 * r_max <= params.wall_count as f64 * d + params.wall_line_width_max_mm
    });

    if let (true, Some(skel)) = (thin, skel.as_ref()) {
        if walk_thin_island(skel, params, out) {
            return;
        }
    }

    // Thick island, degenerate skeleton, or an empty walk → offset loops.
    let loops = emit_offset_loops(
        island,
        params,
        tol,
        &mut out.paths,
        &mut out.roles,
        &mut out.widths,
        &mut out.vwidths,
        &mut out.open,
    );
    emit_residual_medial_fill(
        island,
        &loops,
        params,
        &mut out.paths,
        &mut out.roles,
        &mut out.widths,
        &mut out.vwidths,
        &mut out.open,
    );
}

/// Walk every medial chain of a thin island, emitting variable-width beads.
/// Returns `true` when at least one bead was produced.
fn walk_thin_island(
    skel: &super::skeleton::Skeleton,
    params: &WallParams,
    out: &mut WallOut,
) -> bool {
    let cfg = BeadingConfig::from_wall_params(params);
    let mut emitted = false;
    for chain in skel.chains() {
        emitted |= walk_chain(&chain, &skel.nodes, &cfg, params, out);
    }
    emitted
}

/// Walk one medial chain: sample the beading strategy per node, connect beads
/// of matching key into per-vertex-width polylines.
fn walk_chain(
    chain: &[usize],
    nodes: &[SkeletonNode],
    cfg: &BeadingConfig,
    params: &WallParams,
    out: &mut WallOut,
) -> bool {
    // A chain that returns to its start is a medial cycle (annulus band); its
    // beads close into loops.  Drop the duplicated closing node so geometry is
    // computed once per unique node.
    let cyclic = chain.len() > 2 && chain[0] == *chain.last().unwrap();
    let seq: Vec<usize> = if cyclic {
        chain[..chain.len() - 1].to_vec()
    } else {
        chain.to_vec()
    };
    if seq.len() < 2 {
        return false;
    }

    let pts: Vec<(f64, f64)> = seq.iter().map(|&i| (nodes[i].x, nodes[i].y)).collect();
    let radii: Vec<f64> = seq.iter().map(|&i| nodes[i].radius).collect();

    // Per-node optimal bead count, then de-noise short upward spikes so the
    // count only ever drops (never forced above a node's own optimum → every
    // bead stays ≥ min width).
    let mut counts: Vec<usize> = radii
        .iter()
        .map(|&r| cfg.optimal_bead_count(2.0 * r))
        .collect();
    smooth_counts(
        &mut counts,
        &pts,
        cyclic,
        params.wall_transition_filter_distance_mm,
    );
    if counts.iter().all(|&c| c == 0) {
        return false;
    }

    let normals = chain_normals(&pts, cyclic);

    // For each node, the bead entries it contributes, keyed by side + inset.
    let node_beads: Vec<Vec<NodeBead>> = (0..pts.len())
        .map(|i| {
            let t = 2.0 * radii[i];
            let n = counts[i];
            if n == 0 {
                return Vec::new();
            }
            let beading = cfg.layout(t, n);
            let half = n.div_ceil(2);
            let (nx, ny) = normals[i];
            (0..n)
                .map(|idx| {
                    let key = if idx < half {
                        (Side::A, idx)
                    } else {
                        (Side::B, n - 1 - idx)
                    };
                    // location is across [0, t]; the medial axis sits at t/2.
                    let off = beading.locations[idx] - radii[i];
                    let pt = (pts[i].0 + nx * off, pts[i].1 + ny * off);
                    (key, pt, beading.widths[idx])
                })
                .collect()
        })
        .collect();

    // Distinct keys, ordered so output is deterministic (A insets, then B).
    let mut seen: HashSet<BeadKey> = HashSet::new();
    let mut keys: Vec<BeadKey> = Vec::new();
    for nb in &node_beads {
        for (k, _, _) in nb {
            if seen.insert(*k) {
                keys.push(*k);
            }
        }
    }
    keys.sort_by_key(|(side, inset)| (matches!(side, Side::B), *inset));

    let min_w = params.wall_line_width_min_mm;
    let max_w = params.wall_line_width_max_mm;
    let mut emitted = false;
    for key in keys {
        let role = if key.1 == 0 {
            ExtrusionRole::OuterWall
        } else {
            ExtrusionRole::InnerWall
        };
        // Presence of this key per node: its point and clamped width, or None.
        let present: Vec<Option<((f64, f64), f64)>> = node_beads
            .iter()
            .map(|nb| {
                nb.iter()
                    .find(|(k, _, _)| *k == key)
                    .map(|(_, pt, w)| (*pt, w.clamp(min_w, max_w)))
            })
            .collect();

        if cyclic && present.iter().all(Option::is_some) {
            // Key spans the whole medial cycle → one closed loop.
            let pts_b: Vec<(f64, f64)> = present.iter().map(|p| p.unwrap().0).collect();
            let vw: Vec<f64> = present.iter().map(|p| p.unwrap().1).collect();
            emitted |= out.push_bead(pts_b, vw, role, true);
        } else {
            for (run_pts, run_vw) in contiguous_runs(&present) {
                emitted |= out.push_bead(run_pts, run_vw, role, false);
            }
        }
    }
    emitted
}

/// Split a per-node presence array into maximal runs of consecutive present
/// nodes, each returned as `(points, widths)`.
#[allow(clippy::type_complexity)]
fn contiguous_runs(present: &[Option<((f64, f64), f64)>]) -> Vec<(Vec<(f64, f64)>, Vec<f64>)> {
    let mut runs = Vec::new();
    let mut cur_pts: Vec<(f64, f64)> = Vec::new();
    let mut cur_vw: Vec<f64> = Vec::new();
    for slot in present {
        match slot {
            Some((pt, w)) => {
                cur_pts.push(*pt);
                cur_vw.push(*w);
            }
            None => {
                if cur_pts.len() >= 2 {
                    runs.push((std::mem::take(&mut cur_pts), std::mem::take(&mut cur_vw)));
                } else {
                    cur_pts.clear();
                    cur_vw.clear();
                }
            }
        }
    }
    if cur_pts.len() >= 2 {
        runs.push((cur_pts, cur_vw));
    }
    runs
}

/// De-noise a per-node bead-count array: lower any isolated upward spike (a node
/// whose count exceeds both neighbours) whose run is shorter than
/// `filter_dist`, so the count only ever decreases from a node's own optimum.
/// This keeps every laid-out bead at or above the minimum width and prevents
/// count flip-flop along a faceted boundary.
fn smooth_counts(counts: &mut [usize], pts: &[(f64, f64)], cyclic: bool, filter_dist: f64) {
    let n = counts.len();
    if n < 3 {
        return;
    }
    let seg = |a: usize, b: usize| -> f64 {
        ((pts[b].0 - pts[a].0).powi(2) + (pts[b].1 - pts[a].1).powi(2)).sqrt()
    };
    // Identify maximal equal-count runs; lower a run that is a strict local
    // maximum and shorter than filter_dist to its higher neighbour.
    let mut lowered = true;
    // Iterate to a fixed point so nested spikes collapse.
    while lowered {
        lowered = false;
        let mut i = 0;
        while i < n {
            let mut j = i;
            while j + 1 < n && counts[j + 1] == counts[i] {
                j += 1;
            }
            // Run [i, j]. Neighbours (respecting cyclic wrap).
            let left = if i > 0 {
                Some(counts[i - 1])
            } else if cyclic {
                Some(counts[n - 1])
            } else {
                None
            };
            let right = if j + 1 < n {
                Some(counts[j + 1])
            } else if cyclic {
                Some(counts[0])
            } else {
                None
            };
            if let (Some(l), Some(r)) = (left, right) {
                let run_len: f64 = (i..j).map(|k| seg(k, k + 1)).sum();
                if counts[i] > l && counts[i] > r && run_len < filter_dist {
                    let target = l.max(r);
                    for c in counts.iter_mut().take(j + 1).skip(i) {
                        *c = target;
                    }
                    lowered = true;
                }
            }
            i = j + 1;
        }
    }
}

/// Per-vertex unit normals of a chain (perpendicular to the local tangent).
/// When `cyclic`, the tangent wraps around so the first/last node normals match
/// and a closed bead loop stays smooth.  Zero for a degenerate tangent.
fn chain_normals(pts: &[(f64, f64)], cyclic: bool) -> Vec<(f64, f64)> {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let (pi, qi) = if cyclic {
                ((i + n - 1) % n, (i + 1) % n)
            } else {
                (i.saturating_sub(1), (i + 1).min(n - 1))
            };
            let (px, py) = pts[pi];
            let (qx, qy) = pts[qi];
            let (tx, ty) = (qx - px, qy - py);
            let len = (tx * tx + ty * ty).sqrt();
            if len < 1e-9 {
                (0.0, 0.0)
            } else {
                (-ty / len, tx / len)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::params::SlicingParams;

    fn wall_params() -> WallParams {
        WallParams::from_slicing_params(&SlicingParams::default())
    }

    /// Build a layer holding a single closed `OuterWall` contour.
    fn layer_with(contour: Vec<(f64, f64)>) -> SliceLayer {
        let mut layer = SliceLayer::new(0.2);
        let p: Path = contour.into();
        layer.paths.push(p);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.path_widths.push(None);
        layer.path_vertex_widths.push(None);
        layer.path_is_open.push(false);
        layer
    }

    #[test]
    fn thin_uniform_wall_becomes_one_variable_width_bead() {
        // A 0.6 mm-thick, 20 mm-long strip: the whole island is thin, so the
        // walk should emit a single ~0.6 mm centred bead per side that carries
        // per-vertex widths — not a 0.4 mm loop with a dropped 0.2 mm residual.
        let params = wall_params();
        let mut layer = layer_with(vec![(0.0, 0.0), (20.0, 0.0), (20.0, 0.6), (0.0, 0.6)]);
        generate_walk_for_layer(&mut layer, &params);
        assert!(!layer.paths.is_empty(), "walk must emit beads");
        // Every emitted bead carries a per-vertex width array.
        assert!(
            layer.path_vertex_widths.iter().any(Option::is_some),
            "walk beads must have per-vertex widths"
        );
        // The outer bead should be tagged OuterWall.
        assert!(
            layer.path_roles.contains(&ExtrusionRole::OuterWall),
            "expected an outer wall bead"
        );
        // Widths must be printable (≥ min, ≤ max).
        for vw in layer.path_vertex_widths.iter().flatten() {
            for &w in vw {
                assert!(
                    w >= params.wall_line_width_min_mm - 1e-6
                        && w <= params.wall_line_width_max_mm + 1e-6,
                    "bead width {w} outside [{}, {}]",
                    params.wall_line_width_min_mm,
                    params.wall_line_width_max_mm
                );
            }
        }
    }

    #[test]
    fn thick_square_falls_back_to_offset_loops() {
        // A 20 mm solid square has a deep infill core → classified thick → the
        // walk defers to constant-width offset loops (scalar widths, closed).
        let params = wall_params();
        let mut layer = layer_with(vec![(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)]);
        generate_walk_for_layer(&mut layer, &params);
        assert!(!layer.paths.is_empty(), "must emit walls");
        assert_eq!(
            layer.role_for_path(0),
            ExtrusionRole::OuterWall,
            "first path must be the outer wall"
        );
        // Offset loops are closed constant-width beads.
        assert!(!layer.is_path_open(0), "offset loop must be closed");
    }

    #[test]
    fn smooth_counts_lowers_isolated_spike() {
        // Counts 1,1,3,1,1 with a short spike at the middle → the 3 is lowered.
        let pts: Vec<(f64, f64)> = (0..5).map(|i| (i as f64 * 0.1, 0.0)).collect();
        let mut counts = vec![1usize, 1, 3, 1, 1];
        smooth_counts(&mut counts, &pts, false, 0.5);
        assert_eq!(
            counts,
            vec![1, 1, 1, 1, 1],
            "isolated spike must be lowered"
        );
    }

    #[test]
    fn smooth_counts_keeps_sustained_step() {
        // A sustained 1→2 step over a long run must survive (not a spike).
        let pts: Vec<(f64, f64)> = (0..6).map(|i| (i as f64 * 1.0, 0.0)).collect();
        let mut counts = vec![1usize, 1, 2, 2, 2, 2];
        smooth_counts(&mut counts, &pts, false, 0.5);
        assert_eq!(
            counts,
            vec![1, 1, 2, 2, 2, 2],
            "sustained step must persist"
        );
    }

    #[test]
    fn annulus_band_emits_closed_variable_width_loops() {
        // A near-circular ring (outer r=6, inner r=5.2 → 0.8 mm band) has a
        // clean degree-2 medial cycle, so the walk should close its beads into
        // loops.  (A *square* annulus splits at its four sharp corners into
        // arcs — the documented junction limitation.)
        let params = wall_params();
        let ring = |r: f64| -> Path {
            (0..48)
                .map(|i| {
                    let a = i as f64 / 48.0 * std::f64::consts::TAU;
                    (r * a.cos(), r * a.sin())
                })
                .collect::<Vec<_>>()
                .into()
        };
        let mut layer = SliceLayer::new(0.2);
        for r in [6.0_f64, 5.2] {
            layer.paths.push(ring(r));
            layer.path_roles.push(ExtrusionRole::OuterWall);
            layer.path_widths.push(None);
            layer.path_vertex_widths.push(None);
            layer.path_is_open.push(false);
        }

        generate_walk_for_layer(&mut layer, &params);
        assert!(!layer.paths.is_empty(), "annulus walk must emit beads");
        // At least one closed variable-width loop.
        let closed_vw = layer
            .path_vertex_widths
            .iter()
            .enumerate()
            .any(|(i, vw)| vw.is_some() && !layer.is_path_open(i));
        assert!(closed_vw, "expected a closed variable-width loop");
    }
}
