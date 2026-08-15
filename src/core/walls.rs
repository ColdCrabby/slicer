use clipper2::*;

use crate::settings::params::SlicingParams;

use super::surfaces::perimeter_paths_of;
use super::types::{ExtrusionRole, SliceLayer};

/// Apply single-wall restrictions to specific islands and layers based on parameters.
///
/// This function modifies layers to use only a single outer wall in two cases:
///
/// 1. **First layer** (`only_one_wall_first_layer`): all islands on layer 0
///    have their inner walls stripped unconditionally.
/// 2. **Last layer of each per-island top-surface run** (`only_one_wall_top`):
///    an island's inner walls are stripped only when that specific island is
///    at the end of its top-surface exposure run (the island's footprint
///    disappears or the solid ends above it).  Islands on the same layer that
///    continue upward keep all their walls.
///
/// The per-island approach fixes the previous layer-wide bug where a small
/// sub-feature ending mid-model (e.g. a raised ledge on the side of a cube)
/// would strip inner walls from every island on that layer — including the
/// main body — causing the infill boundary to over-expand into the wall zone.
pub(crate) fn apply_single_wall_restrictions(layers: &mut [SliceLayer], params: &SlicingParams) {
    if layers.is_empty() {
        return;
    }

    // First layer: strip all inner walls regardless of island (layer-wide is
    // correct here because the first layer is a single flat extrusion zone).
    if params.only_one_wall_first_layer {
        remove_inner_walls_from_layer(&mut layers[0]);
    }

    // Top surface: compute a per-island strip mask and selectively remove.
    if params.only_one_wall_top {
        let perimeters: Vec<Paths> = layers.iter().map(perimeter_paths_of).collect();
        let strip_masks = compute_per_island_strip_masks(layers, &perimeters, params.top_layers);
        for (i, strip_indices) in strip_masks.iter().enumerate() {
            if !strip_indices.is_empty() {
                remove_inner_walls_for_islands(&mut layers[i], strip_indices);
            }
        }
    }
}

/// Compute, for each layer, the `paths` indices of outer-wall paths whose
/// inner walls should be stripped.
///
/// An outer-wall path `P` at layer `i` is included in the mask iff:
///
/// 1. **Top-surface exposure**: the progressive intersection of `P`'s area
///    with the outer-wall perimeters of layers `i+1 … i+top_layers` leaves
///    some area of `P` uncovered — i.e. `P`'s top is geometrically exposed.
/// 2. **Run ends here**: either `i` is the last layer of the model, or
///    `intersect(P, perimeters[i+1])` is empty — meaning `P`'s island does
///    not exist in the layer immediately above, so this is the final exposed
///    layer of the run.
///
/// The two-step check eliminates the need for cross-layer island matching: the
/// run-end condition uses a single `intersect` query rather than tracking
/// island identity across layers.
fn compute_per_island_strip_masks(
    layers: &[SliceLayer],
    perimeters: &[Paths],
    top_layers: usize,
) -> Vec<Vec<usize>> {
    if top_layers == 0 {
        return vec![vec![]; layers.len()];
    }
    let total = perimeters.len();

    let compute_one = |layer_idx: usize, layer: &SliceLayer| -> Vec<usize> {
        layer
            .paths
            .iter()
            .enumerate()
            .filter(|(i, _)| layer.role_for_path(*i) == ExtrusionRole::OuterWall)
            .filter_map(|(path_idx, outer_path)| {
                let p_paths = Paths::new(vec![outer_path.clone()]);

                // ── Step 1: does P have exposed top surface at layer_idx? ──
                // Progressively intersect P's area with the layers above to
                // find how much of P is "covered".  The first layer above that
                // does not overlap P (or a layer boundary) collapses coverage
                // to empty, meaning P's top is exposed there.
                let mut covered = p_paths.clone();
                for j in 1..=top_layers {
                    if layer_idx + j >= total {
                        covered = Paths::new(vec![]);
                        break;
                    }
                    let neighbor = &perimeters[layer_idx + j];
                    if neighbor.is_empty() {
                        covered = Paths::new(vec![]);
                        break;
                    }
                    covered =
                        intersect(covered, neighbor.clone(), FillRule::EvenOdd).unwrap_or_default();
                    if covered.is_empty() {
                        break;
                    }
                }
                let exposed =
                    difference(p_paths.clone(), covered, FillRule::EvenOdd).unwrap_or_default();
                if exposed.is_empty() {
                    return None; // P is fully covered above — no top surface here
                }

                // ── Step 2: does the top-surface run end at layer_idx? ──
                // The run ends when P has no geometrical overlap with the next
                // layer (island disappears) or when there is no next layer.
                if layer_idx + 1 >= total {
                    return Some(path_idx); // last layer of model → run ends
                }
                let continues = intersect(
                    p_paths,
                    perimeters[layer_idx + 1].clone(),
                    FillRule::EvenOdd,
                )
                .unwrap_or_default();

                if continues.is_empty() {
                    Some(path_idx) // P ends here → strip inner walls for this island
                } else {
                    None // P continues upward → run not over yet
                }
            })
            .collect()
    };

    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        layers
            .par_iter()
            .enumerate()
            .map(|(i, layer)| compute_one(i, layer))
            .collect()
    }
    #[cfg(target_arch = "wasm32")]
    layers
        .iter()
        .enumerate()
        .map(|(i, layer)| compute_one(i, layer))
        .collect()
}

/// Remove inner walls only for the listed qualifying islands.
///
/// `strip_outer_indices` contains indices (into `layer.paths`) of outer-wall
/// paths whose associated inner walls should be stripped.  An `InnerWall`
/// path is removed only if [`Path::surrounds_path`] reports that it lies
/// inside at least one of the qualifying outer-wall paths.  All other paths
/// (outer walls, infill, surface paths) are preserved unchanged.
fn remove_inner_walls_for_islands(layer: &mut SliceLayer, strip_outer_indices: &[usize]) {
    let qualifying: Vec<_> = strip_outer_indices
        .iter()
        .filter_map(|&i| layer.paths.get(i))
        .cloned()
        .collect();
    if qualifying.is_empty() {
        return;
    }

    let mut new_paths = Paths::new(vec![]);
    let mut new_roles = Vec::new();
    let mut new_widths = Vec::new();
    let mut new_vwidths = Vec::new();
    let mut new_is_open = Vec::new();

    for (i, path) in layer.paths.iter().enumerate() {
        let role = layer.role_for_path(i);
        let should_strip = role == ExtrusionRole::InnerWall
            && qualifying.iter().any(|outer| outer.surrounds_path(path));
        if !should_strip {
            new_paths.push(path.clone());
            new_roles.push(role);
            new_widths.push(layer.width_for_path(i));
            new_vwidths.push(layer.vertex_widths_for_path(i));
            new_is_open.push(layer.is_path_open(i));
        }
    }

    layer.paths = new_paths;
    layer.path_roles = new_roles;
    layer.path_widths = new_widths;
    layer.path_vertex_widths = new_vwidths;
    layer.path_is_open = new_is_open;
}

/// Remove all inner walls from a layer, keeping outer walls and other paths.
fn remove_inner_walls_from_layer(layer: &mut SliceLayer) {
    let mut new_paths = Paths::new(vec![]);
    let mut new_roles = Vec::new();
    let mut new_widths = Vec::new();
    let mut new_vwidths = Vec::new();
    let mut new_is_open = Vec::new();

    for (i, path) in layer.paths.iter().enumerate() {
        let role = layer.role_for_path(i);
        if role != ExtrusionRole::InnerWall {
            new_paths.push(path.clone());
            new_roles.push(role);
            new_widths.push(layer.width_for_path(i));
            new_vwidths.push(layer.vertex_widths_for_path(i));
            new_is_open.push(layer.is_path_open(i));
        }
    }

    layer.paths = new_paths;
    layer.path_roles = new_roles;
    layer.path_widths = new_widths;
    layer.path_vertex_widths = new_vwidths;
    layer.path_is_open = new_is_open;
}

/// Classify wall paths whose centerline crosses unsupported air as
/// [`ExtrusionRole::OverhangPerimeter`], splitting paths at the
/// supported/unsupported boundary so that only the in-air sub-segment
/// receives bridge settings.
///
/// ## What this does
///
/// For each `OuterWall` / `InnerWall` path in a layer that has a non-empty
/// `unsupported_regions`:
///
/// 1. Every wall edge is **densified** at the actual intersection points
///    with the `unsupported_regions` polygon boundary, so that each
///    resulting sub-edge lies fully on one side (in air or on support).
/// 2. Each sub-edge is classified by an even-odd point-in-polygon test on
///    its midpoint (`IsOn` counts as inside — see geometry note).
/// 3. If **no** sub-edge is in air: path kept unchanged.
/// 4. If **all** sub-edges are in air: entire path reclassified as
///    `OverhangPerimeter`.
/// 5. Otherwise the path is split at each air/support transition.  Each
///    run of consecutive same-status sub-edges becomes one sub-path,
///    emitted as either `OverhangPerimeter` (in-air run) or the original
///    wall role (supported run).  The split point is the **exact air
///    boundary crossing**, not the nearest vertex — so a long edge that
///    only partially overlaps the unsupported region only contributes its
///    in-air portion to the overhang segment.
///
/// Splitting correctly handles real-world cases where a large hull loop has
/// only a short segment crossing over a gap (e.g. the top bar of a Benchy
/// window frame): the 50 %-threshold whole-path heuristic would never flag
/// that small segment, but per-segment classification does.
///
/// ## Geometry contract — read before changing the boundary policy
///
/// `unsupported_regions` is computed in
/// [`generate_top_bottom_surfaces_with_interior`] as
///
/// ```text
/// perimeters[i] − inflate(perimeters[i-1], +nozzle_diameter / 2)
/// ```
///
/// The `+d/2` inflation encodes the physical bead width: the previous layer's
/// bead extends `d/2` beyond its centerline.  Consequences:
///
/// * Slight outward lean (`S < d/2`): inflated envelope fully covers the new
///   area → `unsupported_regions` is empty → nothing flagged.
/// * Real overhang (`S > d/2`, ≈ 45°): a meaningful air strip appears.  Wall
///   vertices lie on the **outer boundary** of that strip, so the parity test
///   **must** count `IsOn` as inside — do not change this policy.
///
/// **Do not pre-erode `unsupported_regions`.**  An earlier version eroded by
/// `0.6 × d`, which moves the strip's outer boundary past the wall centerline
/// and suppresses all detection.
pub(crate) fn classify_overhang_perimeters(layers: &mut [SliceLayer], _nozzle_diameter_mm: f64) {
    // Per-layer work is read-only on the layer's own data (we clone
    // `unsupported_regions` up front) and writes back into a freshly built
    // set of vectors at the end.  No layer reads any other layer's state, so
    // the whole pass parallelises cleanly across layers.
    //
    // We compute the (paths, roles, widths, is_open) replacement tuples in
    // parallel on native targets, then apply them serially.  On the Benchy
    // this drops the phase from ~430 ms to a few tens of ms on a multi-core
    // host.
    #[allow(clippy::type_complexity)]
    let process_layer =
        |layer: &SliceLayer| -> Option<(Paths, Vec<ExtrusionRole>, Vec<Option<f64>>, Vec<bool>)> {
            if layer.unsupported_regions.is_empty() {
                return None;
            }
            // Local copy of the air region for boundary tests.
            let air = layer.unsupported_regions.clone();

            // Pad roles/widths so indices are always valid.  We can't mutate
            // the layer here (parallel context), so compute padded views
            // locally.
            let path_count = layer.paths.len();
            let mut padded_roles: Vec<ExtrusionRole> = layer.path_roles.clone();
            while padded_roles.len() < path_count {
                padded_roles.push(ExtrusionRole::OuterWall);
            }
            let mut padded_widths: Vec<Option<f64>> = layer.path_widths.clone();
            while padded_widths.len() < path_count {
                padded_widths.push(None);
            }

            let mut new_paths = Paths::new(vec![]);
            let mut new_roles: Vec<ExtrusionRole> = Vec::new();
            let mut new_widths: Vec<Option<f64>> = Vec::new();
            let mut new_is_open: Vec<bool> = Vec::new();

            for (path_idx, path) in layer.paths.iter().enumerate() {
                let role = padded_roles[path_idx];
                let width = padded_widths.get(path_idx).copied().flatten();
                // Whether this path was already split into an open arc by an
                // earlier pass (e.g. clip_walls_against_bridge_region).
                let is_already_open = layer.is_path_open(path_idx);

                // Only wall roles can be reclassified.
                if role != ExtrusionRole::OuterWall && role != ExtrusionRole::InnerWall {
                    new_paths.push(path.clone());
                    new_roles.push(role);
                    new_widths.push(width);
                    new_is_open.push(is_already_open);
                    continue;
                }

                let raw_pts: Vec<(f64, f64)> = path.iter().map(|p| (p.x(), p.y())).collect();
                if raw_pts.len() < 2 {
                    new_paths.push(path.clone());
                    new_roles.push(role);
                    new_widths.push(width);
                    new_is_open.push(is_already_open);
                    continue;
                }

                // Densify the path by inserting break points at every actual
                // intersection between a wall edge and an `air` polygon boundary
                // edge.  After densification each resulting sub-edge lies fully
                // on one side of the boundary, so a single midpoint test
                // classifies it unambiguously.  This is what keeps an extrusion
                // line in its original role until the exact point where it
                // enters the unsupported region — earlier vertex-only logic
                // would mark a whole long edge as overhang as soon as one
                // endpoint crossed the boundary.
                let dense_pts = densify_path_at_air_boundaries(&raw_pts, &air, is_already_open);
                let nd = dense_pts.len();
                if nd < 2 {
                    new_paths.push(path.clone());
                    new_roles.push(role);
                    new_widths.push(width);
                    new_is_open.push(is_already_open);
                    continue;
                }

                let edge_count = if is_already_open { nd - 1 } else { nd };
                // Per-edge in-air status (midpoint test against `air`).
                let mut edge_air: Vec<bool> = (0..edge_count)
                    .map(|i| {
                        let j = if is_already_open { i + 1 } else { (i + 1) % nd };
                        let mx = (dense_pts[i].0 + dense_pts[j].0) * 0.5;
                        let my = (dense_pts[i].1 + dense_pts[j].1) * 0.5;
                        point_inside_or_on_paths_eo(mx, my, &air)
                    })
                    .collect();

                // Hysteresis filter: collapse short alternating runs that arise
                // from grazing the air boundary (Centi quantisation noise, slight
                // wobble in the layer-i-1 perimeter, etc.).  A genuine overhang on
                // the Benchy hull spans many millimetres of arc; tiny < ~1 mm
                // flips are noise and turn one wall loop into dozens of fragments
                // downstream (huge travel/seam/marker overhead).
                //
                // Threshold: max(2 × nozzle_diameter, 1.5 mm).  Larger than typical
                // densifier-inserted noise, smaller than the shortest meaningful
                // overhang strip we'd want to print at bridge speed.
                let min_run_len_mm = (2.0 * _nozzle_diameter_mm).max(1.5);
                collapse_short_runs(&mut edge_air, &dense_pts, is_already_open, min_run_len_mm);

                let any_air = edge_air.iter().any(|&b| b);
                if !any_air {
                    // Entirely supported — keep as-is.
                    new_paths.push(path.clone());
                    new_roles.push(role);
                    new_widths.push(width);
                    new_is_open.push(is_already_open);
                    continue;
                }

                let any_supported = edge_air.iter().any(|&b| !b);
                if !any_supported {
                    // Entirely in air — whole path becomes OverhangPerimeter.
                    // Preserve the open/closed state of the original path.
                    new_paths.push(path.clone());
                    new_roles.push(ExtrusionRole::OverhangPerimeter);
                    new_widths.push(width);
                    new_is_open.push(is_already_open);
                    continue;
                }

                // ── Mixed path: build runs of consecutive same-status edges ──
                //
                // Each run [a..=b] (edge indices, inclusive) becomes a sub-path
                // with vertices [dense_pts[a], dense_pts[a+1], ..., dense_pts[b+1]]
                // (vertex indices wrap modulo `nd` for closed paths).  Adjacent
                // runs share their seam vertex (the exact air-boundary crossing
                // point inserted during densification), so there is no gap in
                // the printed path.
                let next_v = |vi: usize| -> usize {
                    if is_already_open {
                        vi + 1
                    } else {
                        (vi + 1) % nd
                    }
                };

                // Build runs.
                let mut runs: Vec<(Vec<(f64, f64)>, bool)> = Vec::new();

                if is_already_open {
                    // Linear walk — no wrap-around.
                    let mut run_start = 0_usize;
                    let mut run_air = edge_air[0];
                    let mut verts: Vec<(f64, f64)> = vec![dense_pts[0]];
                    for i in 0..edge_count {
                        if edge_air[i] != run_air {
                            // Flush previous run up to the seam vertex (which is
                            // dense_pts[i], the start of the changed edge).
                            verts.push(dense_pts[i]);
                            runs.push((verts, run_air));
                            run_start = i;
                            run_air = edge_air[i];
                            verts = vec![dense_pts[i]];
                        }
                        verts.push(dense_pts[i + 1]);
                    }
                    let _ = run_start;
                    runs.push((verts, run_air));
                } else {
                    // Closed loop: find the first transition between adjacent
                    // edges and start the walk on the next run so the wrap-around
                    // is well-defined.
                    let first_trans = (0..edge_count)
                        .find(|&i| edge_air[i] != edge_air[(i + 1) % edge_count])
                        .unwrap(); // safe: any_air && any_supported guarantees ≥ 1 transition
                    let start_edge = (first_trans + 1) % edge_count;

                    let mut run_air = edge_air[start_edge];
                    let mut verts: Vec<(f64, f64)> = vec![dense_pts[start_edge]];

                    for k in 0..edge_count {
                        let ei = (start_edge + k) % edge_count;
                        let v_next = next_v(ei);
                        if edge_air[ei] != run_air {
                            // Seam at dense_pts[ei] (start of the new edge).
                            // Previous run already ends at dense_pts[ei] because
                            // edge ei-1 ended there.
                            runs.push((verts, run_air));
                            run_air = edge_air[ei];
                            verts = vec![dense_pts[ei]];
                        }
                        verts.push(dense_pts[v_next]);
                    }
                    runs.push((verts, run_air));

                    // Wrap-around merge: if the first and last runs have the same
                    // status (the walk started in the middle of a run), stitch
                    // them together so the closed loop is preserved as one
                    // contiguous arc per role.
                    if runs.len() >= 2 && runs[0].1 == runs.last().unwrap().1 {
                        let last = runs.pop().unwrap();
                        debug_assert_eq!(
                            last.0.last(),
                            runs[0].0.first(),
                            "merge invariant: last run's final vertex must equal \
                             first run's opening vertex (shared seam)"
                        );
                        let mut merged = last.0;
                        merged.extend_from_slice(&runs[0].0[1..]);
                        runs[0].0 = merged;
                    }
                }

                // Emit all runs as paths.
                for (verts, is_air_seg) in runs {
                    if verts.len() < 2 {
                        continue;
                    }
                    let seg_role = if is_air_seg {
                        ExtrusionRole::OverhangPerimeter
                    } else {
                        role
                    };
                    let seg_path: Path = verts.into();
                    new_paths.push(seg_path);
                    new_roles.push(seg_role);
                    new_widths.push(width);
                    // All sub-segments from a split are open arcs — the original
                    // closed loop was broken into polyline fragments.  The G-code
                    // generator must NOT append a "close contour" move for these.
                    new_is_open.push(true);
                }
            }

            Some((new_paths, new_roles, new_widths, new_is_open))
        };

    #[cfg(not(target_arch = "wasm32"))]
    let results: Vec<Option<_>> = {
        use rayon::prelude::*;
        layers.par_iter().map(process_layer).collect()
    };
    #[cfg(target_arch = "wasm32")]
    let results: Vec<Option<_>> = layers.iter().map(process_layer).collect();

    for (layer, result) in layers.iter_mut().zip(results) {
        if let Some((new_paths, new_roles, new_widths, new_is_open)) = result {
            layer.paths = new_paths;
            layer.path_roles = new_roles;
            layer.path_widths = new_widths;
            layer.path_is_open = new_is_open;
            // Overhang-split arcs drop per-vertex widths; scalar width is used.
            layer.path_vertex_widths = Vec::new();
        }
    }
}

/// Two parametric t-values within this tolerance are considered identical.
const AIR_T_EPSILON: f64 = 1e-9;

/// Determinant magnitude below this is treated as parallel (lines don't cross).
const AIR_PARALLEL_EPSILON: f64 = 1e-12;

/// Insert break points along the path wherever a wall edge crosses an `air`
/// polygon boundary.
///
/// Each input edge `(pts[i] → pts[i+1])` is intersected against every edge
/// of every polygon in `air`.  Intersections strictly inside the wall edge
/// (`0 < t < 1`) are inserted as new vertices in parametric order.  Original
/// vertices are preserved.  For closed paths the last edge `(pts[n-1] → pts[0])`
/// is also processed; for open paths only edges `0..n-1` are processed.
fn densify_path_at_air_boundaries(
    pts: &[(f64, f64)],
    air: &Paths,
    is_open: bool,
) -> Vec<(f64, f64)> {
    let n = pts.len();
    if n < 2 {
        return pts.to_vec();
    }

    let mut out: Vec<(f64, f64)> = Vec::with_capacity(n);
    let edge_count = if is_open { n - 1 } else { n };

    for i in 0..edge_count {
        let j = if is_open { i + 1 } else { (i + 1) % n };
        let (x0, y0) = pts[i];
        let (x1, y1) = pts[j];
        out.push((x0, y0));

        // Collect all parametric intersection t values along this edge.
        let mut t_values: Vec<f64> = Vec::new();
        for poly in air.iter() {
            let p_pts: Vec<(f64, f64)> = poly.iter().map(|p| (p.x(), p.y())).collect();
            let m = p_pts.len();
            if m < 2 {
                continue;
            }
            for k in 0..m {
                if let Some(t) =
                    segment_edge_intersection_t(x0, y0, x1, y1, p_pts[k], p_pts[(k + 1) % m])
                {
                    if t > AIR_T_EPSILON && t < 1.0 - AIR_T_EPSILON {
                        t_values.push(t);
                    }
                }
            }
        }

        if t_values.is_empty() {
            continue;
        }
        t_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        t_values.dedup_by(|a, b| (*a - *b).abs() < AIR_T_EPSILON);

        for t in t_values {
            out.push((x0 + t * (x1 - x0), y0 + t * (y1 - y0)));
        }
    }

    if is_open {
        // Open paths: also push the final terminal vertex (the loop above
        // only pushes the start of each edge).
        out.push(pts[n - 1]);
    }

    out
}

/// Compute the parametric position `t ∈ [0, 1]` along the line segment
/// `(lx0, ly0) → (lx1, ly1)` where it intersects the polygon edge
/// `e0 → e1`.  Returns `None` if the segments are parallel or the intersection
/// falls outside the polygon edge's parameter range.
fn segment_edge_intersection_t(
    lx0: f64,
    ly0: f64,
    lx1: f64,
    ly1: f64,
    e0: (f64, f64),
    e1: (f64, f64),
) -> Option<f64> {
    let (ex0, ey0) = e0;
    let (ex1, ey1) = e1;

    let dx = lx1 - lx0;
    let dy = ly1 - ly0;
    let edx = ex1 - ex0;
    let edy = ey1 - ey0;

    let denom = dx * edy - dy * edx;
    if denom.abs() < AIR_PARALLEL_EPSILON {
        return None;
    }

    let t = ((ex0 - lx0) * edy - (ey0 - ly0) * edx) / denom;
    let u = ((ex0 - lx0) * dy - (ey0 - ly0) * dx) / denom;

    if (0.0..=1.0).contains(&u) {
        Some(t)
    } else {
        None
    }
}

/// Even-odd point-in-polygon test against a `Paths` set.
///
/// Returns `true` when the point lies inside or on the boundary of an
/// odd number of sub-paths.  Boundary points (`IsOn`) **count as
/// inside** — this is required for overhang classification because the
/// wall paths themselves form the *outer* boundary of
/// `unsupported_regions` (which is built as
/// `perimeters[i] − inflate(perimeters[i-1], +d/2)`).  The geometric
/// guard against false positives lives in the `+d/2` inflation, not in
/// the boundary policy.
fn point_inside_or_on_paths_eo(x: f64, y: f64, paths: &Paths) -> bool {
    let mut inside_count = 0_usize;
    for path in paths.iter() {
        let result = clipper2::point_in_polygon(clipper2::Point::new(x, y), path);
        if matches!(
            result,
            clipper2::PointInPolygonResult::IsInside | clipper2::PointInPolygonResult::IsOn
        ) {
            inside_count += 1;
        }
    }
    inside_count % 2 == 1
}

/// Hysteresis filter: collapse any contiguous run of equal-status edges whose
/// total arc length is below `min_run_len_mm` by flipping its status to that
/// of its longer neighbour.  Operates in-place on `edge_air`.
///
/// Closed paths are treated cyclically: the last and first runs are merged
/// when they share the same status before length analysis.  Open paths use
/// linear runs.
///
/// The pass is iterated until convergence — flipping a short run can let two
/// neighbouring runs merge into a longer one that was previously interrupted.
/// Convergence is bounded by `edge_air.len()` iterations because every pass
/// either flips at least one edge or terminates.
fn collapse_short_runs(
    edge_air: &mut [bool],
    dense_pts: &[(f64, f64)],
    is_open: bool,
    min_run_len_mm: f64,
) {
    let n = edge_air.len();
    if n < 2 || min_run_len_mm <= 0.0 {
        return;
    }

    // Precompute per-edge length in mm (in the same coord space as dense_pts).
    let nd = dense_pts.len();
    let edge_len: Vec<f64> = (0..n)
        .map(|i| {
            let j = if is_open { i + 1 } else { (i + 1) % nd };
            let dx = dense_pts[j].0 - dense_pts[i].0;
            let dy = dense_pts[j].1 - dense_pts[i].1;
            (dx * dx + dy * dy).sqrt()
        })
        .collect();

    for _ in 0..n {
        // Build runs as (start_edge_idx, end_edge_idx_exclusive, status, length).
        let mut runs: Vec<(usize, usize, bool, f64)> = Vec::new();
        let mut i = 0;
        while i < n {
            let s = edge_air[i];
            let mut j = i + 1;
            let mut len = edge_len[i];
            while j < n && edge_air[j] == s {
                len += edge_len[j];
                j += 1;
            }
            runs.push((i, j, s, len));
            i = j;
        }

        // Cyclic merge: if first and last runs share status, treat as one
        // run with combined length for the threshold test (we won't actually
        // merge the indices — flipping logic below handles wrap correctly).
        let cyclic_pair =
            if !is_open && runs.len() >= 2 && runs.first().unwrap().2 == runs.last().unwrap().2 {
                Some((0_usize, runs.len() - 1, runs[0].3 + runs[runs.len() - 1].3))
            } else {
                None
            };

        // Find the shortest run below threshold that still has at least one
        // neighbour with the opposite status to flip into.  Skip runs whose
        // cyclic-merged length is already above the threshold.
        let mut victim: Option<usize> = None;
        let mut victim_len = f64::MAX;
        for (idx, run) in runs.iter().enumerate() {
            let effective_len = if let Some((a, b, merged)) = cyclic_pair {
                if idx == a || idx == b {
                    merged
                } else {
                    run.3
                }
            } else {
                run.3
            };
            if effective_len >= min_run_len_mm {
                continue;
            }
            // For open paths a single-run path has nothing to flip into.
            if runs.len() == 1 {
                continue;
            }
            if effective_len < victim_len {
                victim_len = effective_len;
                victim = Some(idx);
            }
        }

        let Some(v) = victim else {
            return; // Converged.
        };

        // Flip the victim run's edges to the opposite status.
        let new_status = !runs[v].2;
        edge_air[runs[v].0..runs[v].1].fill(new_status);
        // If we merged the cyclic pair into one logical run, flip *both* halves.
        if let Some((a, b, _)) = cyclic_pair {
            if v == a || v == b {
                let other = if v == a { b } else { a };
                edge_air[runs[other].0..runs[other].1].fill(new_status);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clipper2::{Path, Paths};

    /// A wall path entirely inside the unsupported region must be reclassified
    /// as `OverhangPerimeter`.
    #[test]
    fn test_classify_overhang_perimeters_in_air() {
        // 5×5 wall path centred at (5, 5).
        let wall: Path = vec![(2.5, 2.5), (7.5, 2.5), (7.5, 7.5), (2.5, 7.5)].into();
        let mut layer = SliceLayer::new(0.4);
        layer.paths.push(wall);
        layer.path_roles.push(ExtrusionRole::OuterWall);

        // Unsupported region: the entire 10×10 layer footprint is in air.
        let air: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.unsupported_regions = Paths::new(vec![air]);

        let mut layers = vec![layer];
        classify_overhang_perimeters(&mut layers, 0.4);

        assert_eq!(
            layers[0].path_roles[0],
            ExtrusionRole::OverhangPerimeter,
            "Wall fully in air must be reclassified to OverhangPerimeter"
        );
    }

    /// A wall path entirely outside the unsupported region must keep its
    /// original role.
    #[test]
    fn test_classify_overhang_perimeters_keeps_supported_walls() {
        // Wall path at (0..2, 0..2)
        let wall: Path = vec![(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)].into();
        let mut layer = SliceLayer::new(0.4);
        layer.paths.push(wall);
        layer.path_roles.push(ExtrusionRole::InnerWall);

        // Unsupported region is far away (5..10, 5..10) — wall is fully supported.
        let air: Path = vec![(5.0, 5.0), (10.0, 5.0), (10.0, 10.0), (5.0, 10.0)].into();
        layer.unsupported_regions = Paths::new(vec![air]);

        let mut layers = vec![layer];
        classify_overhang_perimeters(&mut layers, 0.4);

        assert_eq!(
            layers[0].path_roles[0],
            ExtrusionRole::InnerWall,
            "Supported wall must keep its InnerWall role"
        );
    }

    /// Non-wall roles (Infill, Bridge, …) must never be reclassified, even when
    /// they happen to lie inside `unsupported_regions`.
    #[test]
    fn test_classify_overhang_perimeters_skips_non_wall_roles() {
        let path: Path = vec![(2.5, 2.5), (7.5, 2.5), (7.5, 7.5), (2.5, 7.5)].into();
        let mut layer = SliceLayer::new(0.4);
        layer.paths.push(path);
        layer.path_roles.push(ExtrusionRole::Infill);

        let air: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.unsupported_regions = Paths::new(vec![air]);

        let mut layers = vec![layer];
        classify_overhang_perimeters(&mut layers, 0.4);

        assert_eq!(
            layers[0].path_roles[0],
            ExtrusionRole::Infill,
            "Infill paths must never be reclassified as OverhangPerimeter"
        );
    }

    /// **Regression** — slightly outward-leaning hulls (typical Benchy hull,
    /// step `S < d/2`) put the OuterWall centerline strictly inside the
    /// inflated previous-layer support envelope, so it must NOT be flagged
    /// as an overhang.  Synthetic test: pass a raw annular even-odd strip
    /// where the wall is strictly inside both rings — parity 0, never
    /// flagged regardless of the boundary policy.
    ///
    /// (The full production guard lives in
    /// `generate_top_bottom_surfaces_with_interior` which inflates the
    /// previous perimeter by `d/2` before differencing — see
    /// `test_classify_overhang_e2e_outward_lean_no_false_positive`.)
    #[test]
    fn test_classify_overhang_outward_lean_no_false_positive() {
        // OuterWall centerline of layer i: 4.8×4.8 square at (2.6..7.4).
        let wall: Path = vec![(2.6, 2.6), (7.4, 2.6), (7.4, 7.4), (2.6, 7.4)].into();
        // perimeters[i-1] (previous outer), 5×5 at (2.5..7.5).
        let prev_outer: Path = vec![(2.5, 2.5), (7.5, 2.5), (7.5, 7.5), (2.5, 7.5)].into();
        // perimeters[i] (current outer), 5.2×5.2 at (2.4..7.6).
        let cur_outer: Path = vec![(2.4, 2.4), (7.6, 2.4), (7.6, 7.6), (2.4, 7.6)].into();

        let mut layer = SliceLayer::new(0.4);
        layer.paths.push(wall);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        // unsupported_regions = perimeters[i] − perimeters[i-1] (annular
        // even-odd strip). Centerline is strictly inside both rings →
        // parity 0 → not flagged.
        layer.unsupported_regions = Paths::new(vec![cur_outer, prev_outer]);

        let mut layers = vec![layer];
        classify_overhang_perimeters(&mut layers, 0.4);

        assert_eq!(
            layers[0].path_roles[0],
            ExtrusionRole::OuterWall,
            "Outward-leaning hull walls (step < d/2) must NOT be flagged as \
             OverhangPerimeter"
        );
    }

    /// **Regression** — a real overhang (step `S > d/2`) must be flagged.
    /// Synthetic test passing a raw annular strip; wall vertices land
    /// strictly inside the air strip so the parity test fires regardless
    /// of `IsOn` policy.  See
    /// `test_classify_overhang_e2e_real_overhang_is_flagged` for the
    /// production-geometry test where wall vertices lie on the strip's
    /// outer boundary.
    #[test]
    fn test_classify_overhang_real_overhang_is_flagged() {
        // OuterWall centerline of layer i: 5.6×5.6 square at (2.2..7.8).
        let wall: Path = vec![(2.2, 2.2), (7.8, 2.2), (7.8, 7.8), (2.2, 7.8)].into();
        // perimeters[i-1]: small inner 5×5 at (2.5..7.5).
        let prev_outer: Path = vec![(2.5, 2.5), (7.5, 2.5), (7.5, 7.5), (2.5, 7.5)].into();
        // perimeters[i]: outer 6×6 at (2.0..8.0). Centerline at d/2 = 0.2
        // inside that = 5.6×5.6.
        let cur_outer: Path = vec![(2.0, 2.0), (8.0, 2.0), (8.0, 8.0), (2.0, 8.0)].into();

        let mut layer = SliceLayer::new(0.4);
        layer.paths.push(wall);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.unsupported_regions = Paths::new(vec![cur_outer, prev_outer]);

        let mut layers = vec![layer];
        classify_overhang_perimeters(&mut layers, 0.4);

        assert_eq!(
            layers[0].path_roles[0],
            ExtrusionRole::OverhangPerimeter,
            "Real overhang (step > d/2) must be flagged as OverhangPerimeter"
        );
    }

    /// **Regression** — when a wall edge only partially crosses the
    /// unsupported region, the OverhangPerimeter sub-segment must be bounded
    /// by the **exact air boundary crossing**, not extended out to the
    /// nearest original vertex.
    ///
    /// Geometry: a 10×10 wall loop and an air strip covering the top-right
    /// corner (x ∈ [6,11], y ∈ [6,11]).  The right edge (10,0)→(10,10)
    /// crosses the air boundary at y=6, and the top edge (10,10)→(0,10)
    /// crosses at x=6.  The overhang sub-segment must therefore start at
    /// (10, 6), pass through (10, 10), and end at (6, 10) — its length is
    /// 4 + 4 = 8 mm, NOT the 20 mm the old vertex-only logic produced
    /// (which extended overhang along the entire right and top edges).
    #[test]
    fn test_classify_overhang_splits_at_exact_air_boundary() {
        let wall: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        let mut layer = SliceLayer::new(0.4);
        layer.paths.push(wall);
        layer.path_roles.push(ExtrusionRole::OuterWall);

        // Air covers the top-right quadrant (and beyond, so wall vertices
        // (10,10) is comfortably inside).
        let air: Path = vec![(6.0, 6.0), (11.0, 6.0), (11.0, 11.0), (6.0, 11.0)].into();
        layer.unsupported_regions = Paths::new(vec![air]);

        let mut layers = vec![layer];
        classify_overhang_perimeters(&mut layers, 0.4);

        // Find the OverhangPerimeter sub-segment.
        let layer0 = &layers[0];
        let overhang_idx = layer0
            .path_roles
            .iter()
            .position(|r| *r == ExtrusionRole::OverhangPerimeter)
            .expect("must produce at least one OverhangPerimeter sub-segment");

        // Sum the lengths of the overhang sub-segments.
        let mut overhang_len = 0.0_f64;
        for (i, p) in layer0.paths.iter().enumerate() {
            if layer0.path_roles[i] != ExtrusionRole::OverhangPerimeter {
                continue;
            }
            let pts: Vec<(f64, f64)> = p.iter().map(|q| (q.x(), q.y())).collect();
            for w in pts.windows(2) {
                let dx = w[1].0 - w[0].0;
                let dy = w[1].1 - w[0].1;
                overhang_len += (dx * dx + dy * dy).sqrt();
            }
        }

        // Expected: 4 mm (10,6)→(10,10) + 4 mm (10,10)→(6,10) = 8 mm.
        // Old vertex-only logic would produce ≥ 20 mm because the entire
        // right and top edges were both reclassified.
        assert!(
            (overhang_len - 8.0).abs() < 1e-6,
            "Overhang sub-segment must span only the actual in-air portion \
             (expected ~8 mm, got {overhang_len:.6} mm). \
             role index={overhang_idx}, all roles={:?}",
            layer0.path_roles
        );
    }

    /// **End-to-end regression** — production geometry where the wall path
    /// IS `perimeters[i]`.  Two layers, layer 1's perimeter shifted 0.05 mm
    /// outward (≈ 14° lean for 0.2 mm layer / 0.4 mm nozzle) — well below
    /// the `d/2 = 0.2 mm` support threshold.  Production pipeline
    /// (`generate_top_bottom_surfaces` then `classify_overhang_perimeters`)
    /// must NOT flag the wall.
    #[test]
    fn test_classify_overhang_e2e_outward_lean_no_false_positive() {
        use crate::core::surfaces::generate_top_bottom_surfaces;
        use clipper2::Path;

        let mut layer0 = SliceLayer::new(0.2);
        let prev: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer0.paths.push(prev);
        layer0.path_roles.push(ExtrusionRole::OuterWall);

        let mut layer1 = SliceLayer::new(0.2);
        // 0.05 mm outward step — sub-threshold lean.
        let cur: Path = vec![
            (-0.05, -0.05),
            (10.05, -0.05),
            (10.05, 10.05),
            (-0.05, 10.05),
        ]
        .into();
        layer1.paths.push(cur);
        layer1.path_roles.push(ExtrusionRole::OuterWall);

        let mut layers = vec![layer0, layer1];
        generate_top_bottom_surfaces(&mut layers, 0, 1, 0.2, 45.0);
        classify_overhang_perimeters(&mut layers, 0.4);

        assert_eq!(
            layers[1].path_roles[0],
            ExtrusionRole::OuterWall,
            "Sub-d/2 outward lean must not be flagged in the production pipeline"
        );
    }

    /// **End-to-end regression** — the bug the user reported: NO overhangs
    /// were detected on the Benchy because the wall path coincides with
    /// Production pipeline test: a 0.5 mm outward step on every side triggers
    /// the bridge detector (the ring-shaped unsupported area has no support from
    /// below).  After `clip_walls_against_bridge_region` the outer hull path —
    /// whose vertices land exactly on the bridge zone outer boundary (IsOn) —
    /// must be **removed**, not kept as `OverhangPerimeter`.
    ///
    /// Before the fix the hull vertices were treated as "outside" (strict
    /// IsOn = outside test), so the path survived into `classify_overhang_perimeters`
    /// and became `OverhangPerimeter`.  The bridge infill then covered the very
    /// same area → double-extrusion.  The fix counts `IsOn` as *inside*, so the
    /// hull path is clipped and no `OverhangPerimeter` can overlap with bridge lines.
    #[test]
    fn test_classify_overhang_e2e_real_overhang_is_flagged() {
        use crate::core::surfaces::generate_top_bottom_surfaces;
        use clipper2::Path;

        let mut layer0 = SliceLayer::new(0.2);
        let prev: Path = vec![(0.0, 0.0), (5.0, 0.0), (5.0, 5.0), (0.0, 5.0)].into();
        layer0.paths.push(prev);
        layer0.path_roles.push(ExtrusionRole::OuterWall);

        let mut layer1 = SliceLayer::new(0.2);
        // 0.5 mm outward step on every side — well above d/2 = 0.2 mm.
        // The ring-shaped unsupported area (0.3 mm wide) is detected as Bridge,
        // and the bridge anchor expands 0.5 mm inward.  The resulting bridge zone
        // encompasses the outer hull path entirely, so the hull path is clipped.
        let cur: Path = vec![(-0.5, -0.5), (5.5, -0.5), (5.5, 5.5), (-0.5, 5.5)].into();
        layer1.paths.push(cur);
        layer1.path_roles.push(ExtrusionRole::OuterWall);

        let mut layers = vec![layer0, layer1];
        generate_top_bottom_surfaces(&mut layers, 0, 1, 0.2, 45.0);
        classify_overhang_perimeters(&mut layers, 0.4);

        // Bridge infill must exist: the unsupported ring is filled with bridge lines.
        assert!(
            layers[1].path_roles.contains(&ExtrusionRole::Bridge),
            "Bridge infill must be generated for the ring-shaped unsupported area; \
             roles={:?}",
            layers[1].path_roles
        );
        // No OverhangPerimeter must exist: the outer hull path was clipped because
        // its vertices sat exactly on the bridge zone outer boundary (IsOn).
        // Keeping the hull as OverhangPerimeter would cause it to be extruded first,
        // then bridge infill would extrude on top — double-extrusion.
        assert!(
            !layers[1]
                .path_roles
                .contains(&ExtrusionRole::OverhangPerimeter),
            "Outer hull must be clipped (not OverhangPerimeter) when it coincides \
             with the bridge zone boundary — double-extrusion prevention; \
             roles={:?}",
            layers[1].path_roles
        );
    }
}
