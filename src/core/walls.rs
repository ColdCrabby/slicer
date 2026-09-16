use clipper2::*;

use crate::settings::params::SlicingParams;

use super::surfaces::perimeter_paths_of;
use super::types::{ExtrusionRole, OverhangClass, SliceLayer};

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
    //
    // Also strip the medial **gap-fill** beads Arachne emitted as companions to
    // those now-removed inner walls.  On a solid first layer (`bottom_layers >
    // 0`) that gap fill is fully redundant with the outer-wall bead + the solid
    // bottom surface that fills the interior — measured on a 0.4 mm Benchy, every
    // one of the 18 surviving beads (457/457 vertices) already lies inside that
    // coverage, so it only duplicates extrusion the `classic` generator never
    // lays down (classic's first layer has zero gap fill).  Guarding on a solid
    // cap means we never remove gap fill that would open a wall-band void when
    // there is no bottom surface to close it.
    if params.only_one_wall_first_layer {
        let strip_gap_fill = params.bottom_layers > 0;
        reduce_first_layer_to_single_wall(&mut layers[0], strip_gap_fill);
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

/// Reduce the first layer to a single outer wall: strip every `InnerWall` path
/// and — when `strip_gap_fill` is set — the orphaned medial `GapFill` beads that
/// Arachne generated as companions to those inner walls.
///
/// Outer walls, surfaces, bridges and every other role are preserved.  See the
/// call site in [`apply_single_wall_restrictions`] for why removing the
/// first-layer gap fill is redundant (and therefore safe) on a solid cap.
fn reduce_first_layer_to_single_wall(layer: &mut SliceLayer, strip_gap_fill: bool) {
    let mut new_paths = Paths::new(vec![]);
    let mut new_roles = Vec::new();
    let mut new_widths = Vec::new();
    let mut new_vwidths = Vec::new();
    let mut new_is_open = Vec::new();

    for (i, path) in layer.paths.iter().enumerate() {
        let role = layer.role_for_path(i);
        let drop =
            role == ExtrusionRole::InnerWall || (strip_gap_fill && role == ExtrusionRole::GapFill);
        if !drop {
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

/// How far [`unsupported_regions`](SliceLayer::unsupported_regions) is dilated
/// before it is used as the bridge-zone veto mask.
///
/// The strip's outer contour **is** the wall centreline — it is built as
/// `perimeters[i] − inflate(perimeters[i-1], d/2)` — so testing a wall's own
/// edge midpoints against it asks whether a point lies exactly on the polygon
/// it is being tested against.  Once the boolean difference has resampled that
/// contour (an order of magnitude more vertices than the wall) and `Centi`
/// quantisation has rounded it, the answer flickers edge to edge along
/// geometry that is uniform, and the parity test flips outright wherever the
/// strip pinches thin enough for a point to read `IsOn` against *both* of its
/// contours.  The dilation lifts the mask clear of the query points so it can
/// only ever veto, never decide.
///
/// Sized well above the 0.01 mm coordinate grid and far below any bridge the
/// veto has to keep excluding.
const AIR_MASK_DILATION_MM: f64 = 0.05;

/// The band index that means "no contact with the layer below" — the one that
/// carries the [`ExtrusionRole::OverhangPerimeter`] tag.
///
/// It sits one past `Deg4` so a single `u8` per edge orders the whole scale, and
/// so a run that crosses from a barely-touching wall into open air is still
/// split even though both sides grade as `Deg4`.
const BAND_AIR: u8 = 5;

/// Per-layer regions used to decide *whether* a wall segment hangs in air and
/// to grade its overhang *degree* for dynamic overhang speed (see
/// [`classify_overhang_perimeters`]).  Each region is an even-odd polygon set;
/// a segment midpoint's band is the innermost region it falls inside.
///
/// Every one is an offset of `below` — the previous layer's material footprint
/// — by the amount that puts the boundary at a given unsupported fraction of
/// the current bead.  See [`classify_overhang_perimeters`] for the table.
struct OverhangBands {
    /// `unsupported_regions` dilated by [`AIR_MASK_DILATION_MM`].  A veto only:
    /// an edge outside it is never overhang, which is what keeps walls along a
    /// bridge boundary (already handled by `clip_walls_against_bridge_region`,
    /// and subtracted out of `unsupported_regions` for that reason) from being
    /// re-flagged and double-extruded.
    air_mask: Paths,
    /// `unsupported_regions` grown by a full nozzle diameter — the
    /// neighbourhood outside which nothing can be anything but fully supported,
    /// and the window every other region here is clipped to.
    ///
    /// An edge outside it is band 0 without another test, which is most edges
    /// on most layers.  The clip is what keeps the rest of these regions thin
    /// rings rather than whole-model polygons: the densifier and every point
    /// test scale with their edge count.
    near: Paths,
    /// `inflate(below, +d/2)` — 100 % unsupported.  A centreline outside it is
    /// a full bead-width past the material below, so the bead touches nothing
    /// at all: that, and only that, is what earns the overhang role and its
    /// bridge speed, flow and fan.
    ///
    /// Always built.  Its boundary sits `d/2` *outside* the last supported
    /// centreline position, clear of every wall it is asked about, so the
    /// point-in-polygon answer is decided by geometry rather than by which side
    /// of a rounding a boolean output happened to land on.
    air: Paths,
    /// `erode(below, d/2)` — 0 % unsupported (band 0/1 boundary).
    /// `None` when bands 0 and 1 map to the same class, so the test is skipped.
    f0: Option<Paths>,
    /// `erode(below, d/4)` — the 25 % (band 1/2) boundary.
    /// `None` when bands 1 and 2 map to the same class.
    f25: Option<Paths>,
    /// `below` itself — the 50 % (band 2/3) boundary: the bead's centre sits
    /// exactly on the material edge.  `None` when bands 2 and 3 map to the same
    /// class.
    f50: Option<Paths>,
    /// `inflate(below, +d/4)` — the 75 % (band 3/4) boundary.
    /// `None` when bands 3 and 4 map to the same class.
    f75: Option<Paths>,
}

/// Inputs for dynamic overhang-degree grading passed to
/// [`classify_overhang_perimeters`] when `enable_overhang_speed` is on.
///
/// Only the *degrees* are opt-in.  The material footprint the bands are built
/// from is passed separately and unconditionally, because whether a wall hangs
/// in air is a geometric question that must not change with a speed setting.
#[derive(Clone, Copy)]
pub(crate) struct OverhangGrading {
    /// Raw band `0..=4` → the [`OverhangClass`] to emit.  The pipeline folds
    /// bands whose speed & fan behaviour equals a plain wall down to a lower
    /// class so grading only splits walls where it actually changes output.
    /// [`BAND_AIR`] is not folded: it carries the role tag.
    pub band_class: [OverhangClass; 5],
}

impl OverhangGrading {
    /// Grade every band to its own degree (used by tests).
    pub(crate) const IDENTITY_BAND_CLASS: [OverhangClass; 5] = [
        OverhangClass::None,
        OverhangClass::Deg1,
        OverhangClass::Deg2,
        OverhangClass::Deg3,
        OverhangClass::Deg4,
    ];

    /// Which degree boundaries actually separate two *different* emitted
    /// classes, in the order `(0 %, 25 %, 50 %, 75 %)`.
    ///
    /// A boundary between bands that map to the same class is pure cost: the
    /// polygon offset, the extra densification edges, and the per-edge
    /// point-in-polygon test all produce a split that prints identically.
    /// Folding the ones a profile leaves at their inherited speed is what keeps
    /// grading affordable enough to leave on.
    ///
    /// The 100 % boundary is not listed: it carries the role tag, so it is
    /// always built.
    fn needed_boundaries(&self) -> [bool; 4] {
        let c = &self.band_class;
        [c[0] != c[1], c[1] != c[2], c[2] != c[3], c[3] != c[4]]
    }
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
/// Support is **material**, and the material of layer `i-1` fills its slice
/// outline: the outer bead is laid half its own width inside that boundary, so
/// the plastic reaches the boundary itself.  Write `E` for that outline
/// (`below`) and `c` for the centreline of the bead being classified.  The bead
/// is `d` wide about `c`, so it spans `c ± d/2` and the fraction of it hanging
/// past the material below is
///
/// ```text
/// unsupported = (c + d/2 − E) / d          clamped to 0..=1
/// ```
///
/// Every boundary in this pass is that one relation solved for `c`:
///
/// | Region | Offset from `below` | Unsupported fraction |
/// | --- | --- | --- |
/// | `f0`  | `−d/2` | 0 %   — the whole bead lands on material |
/// | `f25` | `−d/4` | 25 %  |
/// | `f50` | `0`    | 50 %  — the bead's centre sits on the material edge |
/// | `f75` | `+d/4` | 75 %  |
/// | `air` | `+d/2` | 100 % — the bead touches nothing at all |
///
/// **An edge earns the overhang role only outside `air`.** Anything still
/// touching the layer below is a wall: it has something to be pressed onto and
/// prints at wall flow and wall width, however little of it is supported. The
/// *degrees* are what slow a barely-supported wall down, and they grade the
/// whole 0–100 % range — so a steep lean is handled by speed and cooling
/// without also being given bridge flow it cannot use. Measuring instead from
/// the previous layer's wall **centreline** understates `E` by half a bead
/// everywhere, which is enough on its own to tag a near-vertical funnel or a
/// gently flaring hull as bridged.
///
/// Do not test against `unsupported_regions` directly: that region is
/// `perimeters[i] − inflate(perimeters[i-1], d/2)`, so its outer contour *is*
/// the wall centreline, and asking a point-in-polygon test about a point lying
/// on its own subject polygon answers with rounding noise rather than geometry
/// — a uniform ledge comes back as a coin flip, edge by edge, and the
/// run-length hysteresis then freezes whichever arcs the noise happened to
/// clump into.  Every boundary above is offset clear of the walls it is asked
/// about, which is what makes the tests stable.
///
/// `unsupported_regions` still has one job here: dilated to `air_mask`, it
/// vetoes edges the bridge pass already owns (see [`AIR_MASK_DILATION_MM`]).
/// It is built from a *looser* threshold than `air`, so it always contains what
/// this pass flags and can only ever subtract.  Layers where it is empty are
/// skipped outright — no strip, no overhang.
pub(crate) fn classify_overhang_perimeters(
    layers: &mut [SliceLayer],
    nozzle_diameter_mm: f64,
    below_outlines: Option<&[Paths]>,
    grading: Option<OverhangGrading>,
) {
    // Per raw band 0..=4 → the class the classifier emits for it.  Bands whose
    // speed & fan behaviour is identical to a plain wall are folded to a lower
    // class (down to `None`) by the pipeline, so a supported wall is not
    // fragmented into Deg1/Deg2 arcs where grading would change nothing.
    // Defaults to the identity map (grade every band) when unset.
    let band_class: [OverhangClass; 5] = grading
        .map(|g| g.band_class)
        .unwrap_or(OverhangGrading::IDENTITY_BAND_CLASS);
    // Which degree boundaries separate two different emitted classes, at 0 %,
    // 25 %, 50 % and 75 % unsupported.  A boundary between bands that print
    // identically is skipped entirely — no offset, no densification against it,
    // no per-edge test.  The 100 % boundary is not in this list: it carries the
    // role tag, so it is always built.
    let need = grading.map(|g| g.needed_boundaries()).unwrap_or([false; 4]);
    // Precompute the per-layer boundaries.  `below_outlines[i]` is layer `i`'s
    // material footprint — its slice outline, snapshotted by the pipeline
    // before the wall generator replaced it with centrelines — so layer `i`'s
    // support is `below_outlines[i-1]` and every boundary is an offset of it,
    // per the table in this function's doc comment.  `air` (the role threshold)
    // and `air_mask` (the bridge veto) are always built; the degree boundaries
    // only when they separate two classes that actually print differently.
    let band_regions: Option<Vec<Option<OverhangBands>>> = below_outlines.map(|outlines| {
        let d = nozzle_diameter_mm;
        let offset = |from: &Paths, delta: f64| {
            inflate(from.clone(), delta, JoinType::Round, EndType::Polygon, 2.0)
        };
        let build = |i: usize| -> Option<OverhangBands> {
            if i == 0 {
                return None;
            }
            // No strip, no overhang — `process_layer` skips these layers too.
            let strip = &layers[i].unsupported_regions;
            if strip.is_empty() {
                return None;
            }
            let below = outlines.get(i - 1)?;
            if below.is_empty() {
                return None;
            }
            // Every boundary is clipped to `near` — the strip grown by a full
            // nozzle diameter.  A raw offset of a layer outline is a
            // whole-model-sized polygon, while the region that can actually
            // *discriminate* is the thin ring around the strip; both the
            // densifier and the point tests scale with that polygon's edge
            // count.  On a Benchy this is the difference between the grading
            // pass costing ~750 ms and ~100 ms.
            //
            // The clip is exact where it is used.  Intersecting with `near`
            // changes no answer for a point inside `near`, and a point outside
            // it is never asked: the ladder short-circuits to band 0 there, and
            // `air_mask` (0.05 mm around the strip) is far inside it.  A margin
            // of a whole diameter also keeps the cut edges away from the query
            // points, which matters because `point_inside_or_on_paths_eo` counts
            // `IsOn` as inside.
            let near = offset(strip, d);
            let clip_to_near = |region: Paths| -> Paths {
                if region.is_empty() || near.is_empty() {
                    return region;
                }
                intersect(region, near.clone(), FillRule::EvenOdd).unwrap_or_default()
            };
            let [need_f0, need_f25, need_f50, need_f75] = need;
            Some(OverhangBands {
                air_mask: offset(strip, AIR_MASK_DILATION_MM),
                air: clip_to_near(offset(below, d * 0.5)),
                f0: need_f0.then(|| clip_to_near(offset(below, -d * 0.5))),
                f25: need_f25.then(|| clip_to_near(offset(below, -d * 0.25))),
                f50: need_f50.then(|| clip_to_near(below.clone())),
                f75: need_f75.then(|| clip_to_near(offset(below, d * 0.25))),
                near,
            })
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            use rayon::prelude::*;
            (0..layers.len()).into_par_iter().map(build).collect()
        }
        #[cfg(target_arch = "wasm32")]
        {
            (0..layers.len()).map(build).collect()
        }
    });

    // Per-layer work is read-only on the layer's own data (we clone
    // `unsupported_regions` up front) and writes back into a freshly built
    // set of vectors at the end.  No layer reads any other layer's state, so
    // the whole pass parallelises cleanly across layers.
    //
    // We compute the (paths, roles, widths, is_open, overhang) replacement
    // tuples in parallel on native targets, then apply them serially.  On the
    // Benchy this drops the phase from ~430 ms to a few tens of ms on a
    // multi-core host.
    #[allow(clippy::type_complexity)]
    let process_layer = |layer_idx: usize,
                         layer: &SliceLayer|
     -> Option<(
        Paths,
        Vec<ExtrusionRole>,
        Vec<Option<f64>>,
        Vec<bool>,
        Vec<OverhangClass>,
    )> {
        if layer.unsupported_regions.is_empty() {
            return None;
        }
        // Local copy of the raw unsupported strip, used as the fallback test
        // when no layer outline was supplied.
        let strip = layer.unsupported_regions.clone();

        // Boundaries for this layer (absent on the first layer, or when the one
        // below has no outline).  Without them the classifier falls back to the
        // raw strip.
        let bands: Option<&OverhangBands> = band_regions
            .as_ref()
            .and_then(|b| b.get(layer_idx))
            .and_then(|o| o.as_ref());
        let grade = grading.is_some();

        // Combined densification boundaries: every boundary this layer's edges
        // are actually tested against, so each densified sub-edge lies cleanly
        // on one side of all of them.
        //
        // `unsupported_regions` itself is deliberately absent once bands are
        // available.  Its outer contour is the wall centreline, so densifying a
        // wall against it inserts a vertex at every one of the hundreds of
        // spurious self-intersections that coincidence produces — on the Benchy
        // funnel a 42-vertex loop came back with 418 — for a boundary no longer
        // consulted.  `air_mask` stands in, offset clear of the wall.
        let densify_bounds: Paths = match bands {
            Some(b) => {
                let mut acc: Vec<Path> = b.air_mask.iter().cloned().collect();
                acc.extend(b.air.iter().cloned());
                for region in [&b.f0, &b.f25, &b.f50, &b.f75].into_iter().flatten() {
                    acc.extend(region.iter().cloned());
                }
                Paths::new(acc)
            }
            None => strip.clone(),
        };

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
        // Populated only when grading (dynamic overhang speed on); left
        // empty otherwise so `path_overhang` stays absent and the generator
        // sees no override.
        let mut new_overhang: Vec<OverhangClass> = Vec::new();
        // Push a class only while grading, keeping `new_overhang` aligned
        // with `new_paths` without allocating when the feature is off.
        let push_class = |v: &mut Vec<OverhangClass>, c: OverhangClass| {
            if grade {
                v.push(c);
            }
        };

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
                push_class(&mut new_overhang, OverhangClass::None);
                continue;
            }

            let raw_pts: Vec<(f64, f64)> = path.iter().map(|p| (p.x(), p.y())).collect();
            if raw_pts.len() < 2 {
                new_paths.push(path.clone());
                new_roles.push(role);
                new_widths.push(width);
                new_is_open.push(is_already_open);
                push_class(&mut new_overhang, OverhangClass::None);
                continue;
            }

            // Densify the path by inserting break points at every actual
            // intersection between a wall edge and a boundary polygon edge.
            // When grading, the boundary set includes the degree-band
            // inflations as well as the air boundary, so each resulting
            // sub-edge lies fully on one side of every boundary it must be
            // classified against.  This is what keeps an extrusion line in
            // its original role/degree until the exact point where it
            // crosses a boundary — earlier vertex-only logic would mark a
            // whole long edge as overhang as soon as one endpoint crossed.
            let dense_pts =
                densify_path_at_air_boundaries(&raw_pts, &densify_bounds, is_already_open);
            let nd = dense_pts.len();
            if nd < 2 {
                new_paths.push(path.clone());
                new_roles.push(role);
                new_widths.push(width);
                new_is_open.push(is_already_open);
                push_class(&mut new_overhang, OverhangClass::None);
                continue;
            }

            let edge_count = if is_already_open { nd - 1 } else { nd };
            // Per-edge in-air status, from the midpoint.  An edge is in air when
            // its centreline is a full half-bead past the material below
            // (outside `air`, i.e. nothing of the bead overlaps it) *and* the
            // bridge pass has not already claimed it (`air_mask`).  The second
            // test only ever vetoes — a point outside `air` is inside
            // `perimeters[i]` and past a looser threshold than the strip was
            // built with, so it is in the raw strip by construction, and the
            // mask can subtract bridge zones but never add anything.
            //
            // Without a layer outline below there is nothing to measure against
            // and the strip is all we have; see the doc comment for why that is
            // a fallback and not the contract.
            let mut edge_air: Vec<bool> = (0..edge_count)
                .map(|i| {
                    let j = if is_already_open { i + 1 } else { (i + 1) % nd };
                    let mx = (dense_pts[i].0 + dense_pts[j].0) * 0.5;
                    let my = (dense_pts[i].1 + dense_pts[j].1) * 0.5;
                    match bands {
                        Some(b) => {
                            point_inside_or_on_paths_eo(mx, my, &b.air_mask)
                                && !point_inside_or_on_paths_eo(mx, my, &b.air)
                        }
                        None => point_inside_or_on_paths_eo(mx, my, &strip),
                    }
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
            let min_run_len_mm = (2.0 * nozzle_diameter_mm).max(1.5);
            collapse_short_runs(&mut edge_air, &dense_pts, is_already_open, min_run_len_mm);

            // Per-edge band: a degree `0..=4` for a wall that still touches the
            // layer below, or [`BAND_AIR`] for one that does not.  The degrees
            // grade the whole 0–100 % range against the offsets of `below`;
            // `BAND_AIR` sits one past `Deg4` so an edge that leaves the last
            // sliver of contact is still split off, even though both sides
            // grade as `Deg4`.  Band hysteresis then suppresses tiny degree
            // flickers, and a final pass restores the air flag the collapse may
            // have smeared, so the role tag stays exact.  Without bands the
            // band is derived purely from the air flag, and the run split below
            // reduces to the historical binary behaviour.
            let edge_band: Vec<u8> = if let Some(b) = bands {
                let mut band: Vec<u8> = (0..edge_count)
                    .map(|i| {
                        if edge_air[i] {
                            return BAND_AIR;
                        }
                        let j = if is_already_open { i + 1 } else { (i + 1) % nd };
                        let mx = (dense_pts[i].0 + dense_pts[j].0) * 0.5;
                        let my = (dense_pts[i].1 + dense_pts[j].1) * 0.5;
                        // Outside the strip's neighbourhood nothing can be
                        // anything but fully supported, and this is where most
                        // edges on most layers land — so answer without touching
                        // the boundary polygons, which are only valid here
                        // anyway (they are clipped to it).
                        if !point_inside_or_on_paths_eo(mx, my, &b.near) {
                            return band_class[0].band();
                        }
                        // Only boundaries kept in `bands` are tested; a skipped
                        // one means the bands it separates share a class, so any
                        // representative on that side maps to the same result.
                        // Walking outwards, the first boundary the midpoint is
                        // inside names its band; falling through every one of
                        // them leaves the outermost band that was asked about.
                        let inside = |region: &Option<Paths>| {
                            region
                                .as_ref()
                                .is_some_and(|r| point_inside_or_on_paths_eo(mx, my, r))
                        };
                        let raw = if inside(&b.f0) {
                            0
                        } else if inside(&b.f25) {
                            1
                        } else if inside(&b.f50) {
                            2
                        } else if inside(&b.f75) {
                            3
                        } else {
                            // Every boundary inside this point was skipped, so
                            // the bands they separate share a class: report the
                            // outermost one that was actually built, or band 0
                            // when none were.
                            [&b.f75, &b.f50, &b.f25, &b.f0]
                                .iter()
                                .position(|r| r.is_some())
                                .map_or(0, |k| 4 - k as u8)
                        };
                        // Fold to the emitted class (a band the pipeline deemed
                        // behaviourally equal to a plainer wall collapses to a
                        // lower degree, so we never split where output is
                        // unchanged).  `BAND_AIR` is never folded.
                        band_class[raw as usize].band()
                    })
                    .collect();
                collapse_short_runs_u8(&mut band, &dense_pts, is_already_open, min_run_len_mm);
                for (i, bd) in band.iter_mut().enumerate() {
                    // Neutralise any cross-air-boundary flip the collapse made,
                    // so `BAND_AIR ⇔ in air` — hence the role tag — is exact.
                    if edge_air[i] {
                        *bd = BAND_AIR;
                    } else {
                        *bd = (*bd).min(OverhangClass::Deg4.band());
                    }
                }
                band
            } else {
                edge_air
                    .iter()
                    .map(|&a| if a { BAND_AIR } else { 0 })
                    .collect()
            };

            // Uniform band → keep the path unsplit (covers both historical
            // fast paths: all-supported = band 0, all-in-air = BAND_AIR).
            let first_band = edge_band[0];
            if edge_band.iter().all(|&b| b == first_band) {
                let seg_role = if first_band >= BAND_AIR {
                    ExtrusionRole::OverhangPerimeter
                } else {
                    role
                };
                new_paths.push(path.clone());
                new_roles.push(seg_role);
                new_widths.push(width);
                new_is_open.push(is_already_open);
                push_class(&mut new_overhang, OverhangClass::from_band(first_band));
                continue;
            }

            // ── Mixed path: build runs of consecutive same-band edges ──
            //
            // Each run [a..=b] (edge indices, inclusive) becomes a sub-path
            // with vertices [dense_pts[a], dense_pts[a+1], ..., dense_pts[b+1]]
            // (vertex indices wrap modulo `nd` for closed paths).  Adjacent
            // runs share their seam vertex (the exact boundary crossing
            // point inserted during densification), so there is no gap in
            // the printed path.
            let next_v = |vi: usize| -> usize {
                if is_already_open {
                    vi + 1
                } else {
                    (vi + 1) % nd
                }
            };

            // Build runs keyed by band value (u8).
            let mut runs: Vec<(Vec<(f64, f64)>, u8)> = Vec::new();

            if is_already_open {
                // Linear walk — no wrap-around.
                let mut run_band = edge_band[0];
                let mut verts: Vec<(f64, f64)> = vec![dense_pts[0]];
                for i in 0..edge_count {
                    if edge_band[i] != run_band {
                        // Flush previous run up to the seam vertex (which is
                        // dense_pts[i], the start of the changed edge).
                        verts.push(dense_pts[i]);
                        runs.push((verts, run_band));
                        run_band = edge_band[i];
                        verts = vec![dense_pts[i]];
                    }
                    verts.push(dense_pts[i + 1]);
                }
                runs.push((verts, run_band));
            } else {
                // Closed loop: find the first transition between adjacent
                // edges and start the walk on the next run so the wrap-around
                // is well-defined.
                let first_trans = (0..edge_count)
                    .find(|&i| edge_band[i] != edge_band[(i + 1) % edge_count])
                    .unwrap(); // safe: the uniform-band fast path above guarantees ≥ 1 transition
                let start_edge = (first_trans + 1) % edge_count;

                let mut run_band = edge_band[start_edge];
                let mut verts: Vec<(f64, f64)> = vec![dense_pts[start_edge]];

                for k in 0..edge_count {
                    let ei = (start_edge + k) % edge_count;
                    let v_next = next_v(ei);
                    if edge_band[ei] != run_band {
                        // Seam at dense_pts[ei] (start of the new edge).
                        // Previous run already ends at dense_pts[ei] because
                        // edge ei-1 ended there.
                        runs.push((verts, run_band));
                        run_band = edge_band[ei];
                        verts = vec![dense_pts[ei]];
                    }
                    verts.push(dense_pts[v_next]);
                }
                runs.push((verts, run_band));

                // Wrap-around merge: if the first and last runs have the same
                // band (the walk started in the middle of a run), stitch
                // them together so the closed loop is preserved as one
                // contiguous arc per role/degree.
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
            for (verts, seg_band) in runs {
                if verts.len() < 2 {
                    continue;
                }
                let seg_role = if seg_band >= BAND_AIR {
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
                push_class(&mut new_overhang, OverhangClass::from_band(seg_band));
            }
        }

        Some((new_paths, new_roles, new_widths, new_is_open, new_overhang))
    };

    #[cfg(not(target_arch = "wasm32"))]
    let results: Vec<Option<_>> = {
        use rayon::prelude::*;
        layers
            .par_iter()
            .enumerate()
            .map(|(i, layer)| process_layer(i, layer))
            .collect()
    };
    #[cfg(target_arch = "wasm32")]
    let results: Vec<Option<_>> = layers
        .iter()
        .enumerate()
        .map(|(i, layer)| process_layer(i, layer))
        .collect();

    for (layer, result) in layers.iter_mut().zip(results) {
        if let Some((new_paths, new_roles, new_widths, new_is_open, new_overhang)) = result {
            layer.paths = new_paths;
            layer.path_roles = new_roles;
            layer.path_widths = new_widths;
            layer.path_is_open = new_is_open;
            // Empty when not grading (feature off / no previous perimeter), so
            // `overhang_for_path` keeps returning `None`.
            layer.path_overhang = new_overhang;
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

/// Degree-band hysteresis: collapse any contiguous run of equal-band edges
/// whose total arc length is below `min_run_len_mm` by re-labelling it with the
/// band of its **longer** neighbour.  The multi-valued analogue of
/// [`collapse_short_runs`], used to suppress tiny overhang-degree flickers so a
/// long overhang arc is not shattered into dozens of Deg3/Deg4 fragments (and a
/// supported wall into Deg1/Deg2 fragments).
///
/// Cross-air-boundary flips are harmless here: the caller re-clamps the band to
/// the collapsed air flag afterwards, so any Deg2→Deg3 (or Deg3→Deg2) drift a
/// merge introduces is neutralised.  Iterated to convergence, bounded by the
/// edge count.
fn collapse_short_runs_u8(
    edge_band: &mut [u8],
    dense_pts: &[(f64, f64)],
    is_open: bool,
    min_run_len_mm: f64,
) {
    let n = edge_band.len();
    if n < 2 || min_run_len_mm <= 0.0 {
        return;
    }

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
        // Build runs as (start, end_exclusive, band, length).
        let mut runs: Vec<(usize, usize, u8, f64)> = Vec::new();
        let mut i = 0;
        while i < n {
            let s = edge_band[i];
            let mut j = i + 1;
            let mut len = edge_len[i];
            while j < n && edge_band[j] == s {
                len += edge_len[j];
                j += 1;
            }
            runs.push((i, j, s, len));
            i = j;
        }

        // Cyclic merge: a closed loop whose first and last runs share a band is
        // one logical run for the length test.
        let cyclic_pair =
            if !is_open && runs.len() >= 2 && runs.first().unwrap().2 == runs.last().unwrap().2 {
                Some((0_usize, runs.len() - 1, runs[0].3 + runs[runs.len() - 1].3))
            } else {
                None
            };

        // Pick the shortest sub-threshold run that has a neighbour to merge into.
        let mut victim: Option<usize> = None;
        let mut victim_len = f64::MAX;
        for (idx, run) in runs.iter().enumerate() {
            let effective_len = match cyclic_pair {
                Some((a, b, merged)) if idx == a || idx == b => merged,
                _ => run.3,
            };
            if effective_len >= min_run_len_mm || runs.len() == 1 {
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

        // Re-label the victim (and its cyclic twin) with the longer neighbour's
        // band.  The neighbours in run order are v-1 and v+1 (cyclically).
        let prev_run = if v == 0 { runs.len() - 1 } else { v - 1 };
        let next_run = if v + 1 == runs.len() { 0 } else { v + 1 };
        // Merged-pair halves are the same logical run; skip self as neighbour.
        let neighbour = if runs[prev_run].3 >= runs[next_run].3 {
            prev_run
        } else {
            next_run
        };
        let new_band = runs[neighbour].2;
        edge_band[runs[v].0..runs[v].1].fill(new_band);
        if let Some((a, b, _)) = cyclic_pair {
            if v == a || v == b {
                let other = if v == a { b } else { a };
                edge_band[runs[other].0..runs[other].1].fill(new_band);
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
        classify_overhang_perimeters(&mut layers, 0.4, None, None);

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
        classify_overhang_perimeters(&mut layers, 0.4, None, None);

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
        classify_overhang_perimeters(&mut layers, 0.4, None, None);

        assert_eq!(
            layers[0].path_roles[0],
            ExtrusionRole::Infill,
            "Infill paths must never be reclassified as OverhangPerimeter"
        );
    }

    /// Push an open horizontal wall polyline running along `y` from `x0` to `x1`.
    fn push_open_wall_y(layer: &mut SliceLayer, y: f64, x0: f64, x1: f64, role: ExtrusionRole) {
        let p: Path = vec![(x0, y), (x1, y)].into();
        layer.paths.push(p);
        layer.path_roles.push(role);
        layer.path_widths.push(None);
        layer.path_vertex_widths.push(None);
        layer.path_is_open.push(true);
    }

    /// With dynamic overhang speed enabled, wall segments must be graded into
    /// the four degrees by what fraction of the bead hangs past the **material**
    /// below, and only a bead clear of it altogether may take the overhang role.
    ///
    /// The layer below is a square whose material edge is `y = 100`; at
    /// `nozzle = 0.4` the bead spans `y ± 0.2`, so the seams land at
    /// `y = 99.8 / 99.9 / 100.0 / 100.1 / 100.2` for 0 / 25 / 50 / 75 / 100 %
    /// unsupported.
    #[test]
    fn test_classify_overhang_degrees_grades_bands() {
        // The layer below: a 100×100 square whose top material edge is y = 100.
        let below: Paths = Paths::new(vec![vec![
            (0.0, 0.0),
            (100.0, 0.0),
            (100.0, 100.0),
            (0.0, 100.0),
        ]
        .into()]);

        let support_layer = SliceLayer::new(0.2);
        let mut wall_layer = SliceLayer::new(0.4);
        // One long wall per band, each uniform.
        for y in [99.85, 99.95, 100.05, 100.15, 100.5] {
            push_open_wall_y(&mut wall_layer, y, 10.0, 90.0, ExtrusionRole::InnerWall);
        }

        // The veto mask, built as the surface pass builds it — a looser
        // threshold than the 100 % boundary, so it contains everything the
        // classifier may flag.
        let strip: Path = vec![
            (-10.0, 100.2),
            (110.0, 100.2),
            (110.0, 200.0),
            (-10.0, 200.0),
        ]
        .into();
        wall_layer.unsupported_regions = Paths::new(vec![strip]);

        // Outline at index 0 is the layer below the wall layer (layer 1).
        let mut layers = vec![support_layer, wall_layer];
        let outlines = vec![below, Paths::default()];
        classify_overhang_perimeters(
            &mut layers,
            0.4,
            Some(&outlines),
            Some(OverhangGrading {
                band_class: OverhangGrading::IDENTITY_BAND_CLASS,
            }),
        );

        let l = &layers[1];
        // Five inputs stayed five outputs (each wall is uniform → not split).
        assert_eq!(l.paths.len(), 5, "no split expected for uniform-band walls");
        assert_eq!(l.overhang_for_path(0), OverhangClass::Deg1);
        assert_eq!(l.overhang_for_path(1), OverhangClass::Deg2);
        assert_eq!(l.overhang_for_path(2), OverhangClass::Deg3);
        assert_eq!(l.overhang_for_path(3), OverhangClass::Deg4);
        assert_eq!(l.overhang_for_path(4), OverhangClass::Deg4);
        // Only the bead clear of the material below takes the overhang role.
        // The 75–100 % one is graded Deg4 — it prints at the Deg4 speed and
        // takes the overhang fan — but it still touches, so it keeps wall role,
        // wall flow and wall width.
        for i in 0..4 {
            assert_eq!(
                l.role_for_path(i),
                ExtrusionRole::InnerWall,
                "path {i} still touches the layer below and must stay a wall"
            );
        }
        assert_eq!(l.role_for_path(4), ExtrusionRole::OverhangPerimeter);
    }

    /// A fully-unsupported wall far from any support is Deg4, and the role
    /// tag still becomes `OverhangPerimeter` — degree and role stay consistent.
    #[test]
    fn test_classify_overhang_degrees_fully_unsupported_is_deg4() {
        let wall: Path = vec![(2.5, 2.5), (7.5, 2.5), (7.5, 7.5), (2.5, 7.5)].into();
        let support_layer = SliceLayer::new(0.2);
        let mut wall_layer = SliceLayer::new(0.4);
        wall_layer.paths.push(wall);
        wall_layer.path_roles.push(ExtrusionRole::OuterWall);
        let air: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        wall_layer.unsupported_regions = Paths::new(vec![air]);

        // Support far away so the whole wall is fully unsupported (Deg4).
        let prev: Paths = Paths::new(vec![vec![
            (20.0, 20.0),
            (25.0, 20.0),
            (25.0, 25.0),
            (20.0, 25.0),
        ]
        .into()]);
        let mut layers = vec![support_layer, wall_layer];
        let support = vec![prev, Paths::default()];
        classify_overhang_perimeters(
            &mut layers,
            0.4,
            Some(&support),
            Some(OverhangGrading {
                band_class: OverhangGrading::IDENTITY_BAND_CLASS,
            }),
        );

        assert_eq!(layers[1].overhang_for_path(0), OverhangClass::Deg4);
        assert_eq!(layers[1].role_for_path(0), ExtrusionRole::OverhangPerimeter);
    }

    /// With the feature off (`overhang_support = None`) no degree data is
    /// produced, so `path_overhang` stays empty and every path resolves to
    /// `None` — the OFF path must be byte-identical to the historical behaviour.
    #[test]
    fn test_classify_overhang_degrees_off_leaves_path_overhang_empty() {
        let wall: Path = vec![(2.5, 2.5), (7.5, 2.5), (7.5, 7.5), (2.5, 7.5)].into();
        let mut layer = SliceLayer::new(0.4);
        layer.paths.push(wall);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        let air: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.unsupported_regions = Paths::new(vec![air]);

        let mut layers = vec![layer];
        classify_overhang_perimeters(&mut layers, 0.4, None, None);

        assert!(
            layers[0].path_overhang.is_empty(),
            "path_overhang must stay empty when dynamic overhang speed is off"
        );
        assert_eq!(layers[0].overhang_for_path(0), OverhangClass::None);
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
        classify_overhang_perimeters(&mut layers, 0.4, None, None);

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
        classify_overhang_perimeters(&mut layers, 0.4, None, None);

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
        classify_overhang_perimeters(&mut layers, 0.4, None, None);

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
        classify_overhang_perimeters(&mut layers, 0.4, None, None);

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
        // Duplicate the support layer so the outward-step ring is a genuine ≥2
        // layer overhang, not a 1-layer recess the min-depth gate suppresses.
        let dup = layers[0].clone();
        layers.insert(0, dup);
        generate_top_bottom_surfaces(&mut layers, 0, 1, 0.2, 45.0);
        classify_overhang_perimeters(&mut layers, 0.4, None, None);

        // Bridge infill must exist: the unsupported ring is filled with bridge lines.
        assert!(
            layers[2].path_roles.contains(&ExtrusionRole::Bridge),
            "Bridge infill must be generated for the ring-shaped unsupported area; \
             roles={:?}",
            layers[2].path_roles
        );
        // No OverhangPerimeter must exist: the outer hull path was clipped because
        // its vertices sat exactly on the bridge zone outer boundary (IsOn).
        // Keeping the hull as OverhangPerimeter would cause it to be extruded first,
        // then bridge infill would extrude on top — double-extrusion.
        assert!(
            !layers[2]
                .path_roles
                .contains(&ExtrusionRole::OverhangPerimeter),
            "Outer hull must be clipped (not OverhangPerimeter) when it coincides \
             with the bridge zone boundary — double-extrusion prevention; \
             roles={:?}",
            layers[2].path_roles
        );
    }

    /// A circular rim, sampled the way a real sliced funnel is, with the air
    /// strip built exactly as `generate_top_bottom_surfaces_with_interior`
    /// builds it — so the strip's outer contour *is* the wall path, which is the
    /// coincidence the whole boundary policy turns on.
    ///
    /// `step` is how far the rim leans out per layer; `d` the nozzle diameter.
    /// Returns the classified layer.
    fn classify_circular_rim(step: f64, d: f64) -> SliceLayer {
        use clipper2::Path;

        let ring = |r: f64| -> Path {
            // A vertex count and radius that put most coordinates off the Centi
            // grid, as sliced geometry does — rounding them is what used to make
            // the boundary test answer at random.
            (0..96)
                .map(|k| {
                    let a = std::f64::consts::TAU * f64::from(k) / 96.0;
                    (2.917 + r).mul_add(a.cos(), 0.83) // centre offset, odd radius
                })
                .zip((0..96).map(|k| {
                    let a = std::f64::consts::TAU * f64::from(k) / 96.0;
                    (2.917 + r).mul_add(a.sin(), 4.8)
                }))
                .collect::<Vec<(f64, f64)>>()
                .into()
        };

        let prev_ring = ring(0.0);
        let cur_ring = ring(step);
        let prev = Paths::new(vec![prev_ring]);
        let cur = Paths::new(vec![cur_ring.clone()]);

        // `perimeters[i] − inflate(perimeters[i-1], d/2)`, verbatim.
        let envelope = inflate(
            prev.clone(),
            d * 0.5,
            JoinType::Round,
            EndType::Polygon,
            2.0,
        );
        let air = difference(cur.clone(), envelope, FillRule::EvenOdd).unwrap_or_default();

        let mut layer0 = SliceLayer::new(0.2);
        layer0.paths.push(prev.iter().next().unwrap().clone());
        layer0.path_roles.push(ExtrusionRole::OuterWall);

        let mut layer1 = SliceLayer::new(0.2);
        layer1.paths.push(cur_ring);
        layer1.path_roles.push(ExtrusionRole::OuterWall);
        layer1.unsupported_regions = air;

        let support = vec![prev, cur];
        let mut layers = vec![layer0, layer1];
        classify_overhang_perimeters(&mut layers, d, Some(&support), None);
        layers.pop().unwrap()
    }

    /// A rim that leans out uniformly must be classified uniformly.
    ///
    /// The air strip's outer contour is the wall path itself, so testing a
    /// wall's own edge midpoints against the strip asks a point-in-polygon
    /// question about points lying on their subject polygon — and the answer
    /// comes back as rounding noise. On a Benchy funnel rim that turned one
    /// 0.34 mm ledge into four alternating verdicts around a single circle,
    /// each fragment paying its own retract, travel and seam. Measuring against
    /// `b2` instead — whose boundary sits `d/2` inboard of the wall — is what
    /// makes the answer geometric.
    #[test]
    fn test_classify_overhang_uniform_rim_is_not_fragmented() {
        // 0.35 mm past a 0.4 mm bead's centreline: 87 % unsupported, an
        // unambiguous overhang, and the step the Benchy funnel rim actually has.
        let layer = classify_circular_rim(0.35, 0.4);

        assert_eq!(
            layer.paths.len(),
            1,
            "a uniform rim must stay one path, not be split into arcs; roles={:?}",
            layer.path_roles
        );
        assert_eq!(
            layer.path_roles[0],
            ExtrusionRole::OverhangPerimeter,
            "a rim clear of the layer below is an overhang"
        );
        assert!(
            !layer.is_path_open(0),
            "an unsplit loop must stay closed so the generator still closes the contour"
        );
    }

    /// The other side of the threshold, and the case that started this: a bead
    /// that still catches the layer below is a wall, not a bridge.
    ///
    /// The Benchy funnel rim leans 0.336 mm per layer measured centreline to
    /// centreline — which reads as an 84 % overhang, and, measured from the wall
    /// centreline rather than the material edge, as a fully detached one.  But
    /// the layer below is half a bead wider than its centreline, so the new bead
    /// still lands 0.064 mm onto solid plastic.  It earns a steep *degree*; it
    /// does not earn bridge flow and bridge speed.
    #[test]
    fn test_classify_overhang_rim_still_touching_is_not_flagged() {
        // 0.136 mm past the material edge — the funnel rim, to the number.
        let layer = classify_circular_rim(0.136, 0.4);

        assert!(
            layer
                .path_roles
                .iter()
                .all(|r| *r == ExtrusionRole::OuterWall),
            "a bead still touching the layer below must stay a wall; roles={:?}",
            layer.path_roles
        );
    }

    /// Building block for the first-layer restriction tests: a layer carrying an
    /// outer wall, an inner wall, a gap-fill bead and a bottom surface.
    fn first_layer_with_all_roles() -> SliceLayer {
        let outer: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        let inner: Path = vec![(1.0, 1.0), (9.0, 1.0), (9.0, 9.0), (1.0, 9.0)].into();
        let gap: Path = vec![(4.0, 4.0), (6.0, 4.0)].into();
        let surface: Path = vec![(2.0, 2.0), (8.0, 2.0), (8.0, 8.0), (2.0, 8.0)].into();
        let mut layer = SliceLayer::new(0.2);
        for (p, role) in [
            (outer, ExtrusionRole::OuterWall),
            (inner, ExtrusionRole::InnerWall),
            (gap, ExtrusionRole::GapFill),
            (surface, ExtrusionRole::BottomSurface),
        ] {
            layer.paths.push(p);
            layer.path_roles.push(role);
        }
        layer
    }

    /// On a solid first layer (`bottom_layers > 0`) the single-wall restriction
    /// must drop both the inner walls **and** their orphaned companion gap-fill
    /// beads, leaving the outer wall and the bottom surface — matching `classic`,
    /// which emits no first-layer gap fill.
    #[test]
    fn test_first_layer_single_wall_strips_gap_fill_on_solid_cap() {
        let mut layers = vec![first_layer_with_all_roles()];
        let params = SlicingParams {
            only_one_wall_first_layer: true,
            bottom_layers: 3,
            ..Default::default()
        };
        apply_single_wall_restrictions(&mut layers, &params);

        let roles = &layers[0].path_roles;
        assert!(
            !roles.contains(&ExtrusionRole::InnerWall),
            "inner walls must be stripped on the first layer; roles={roles:?}"
        );
        assert!(
            !roles.contains(&ExtrusionRole::GapFill),
            "orphaned gap fill must be stripped on a solid first layer; roles={roles:?}"
        );
        assert!(
            roles.contains(&ExtrusionRole::OuterWall)
                && roles.contains(&ExtrusionRole::BottomSurface),
            "outer wall and bottom surface must be preserved; roles={roles:?}"
        );
        assert_eq!(
            layers[0].paths.len(),
            roles.len(),
            "paths and roles must stay index-aligned after stripping"
        );
    }

    /// With no solid bottom (`bottom_layers == 0`) there is no surface to close a
    /// wall-band void, so first-layer gap fill must be **retained** even though
    /// inner walls are still stripped.
    #[test]
    fn test_first_layer_single_wall_keeps_gap_fill_without_solid_cap() {
        let mut layers = vec![first_layer_with_all_roles()];
        let params = SlicingParams {
            only_one_wall_first_layer: true,
            bottom_layers: 0,
            ..Default::default()
        };
        apply_single_wall_restrictions(&mut layers, &params);

        let roles = &layers[0].path_roles;
        assert!(
            !roles.contains(&ExtrusionRole::InnerWall),
            "inner walls must still be stripped; roles={roles:?}"
        );
        assert!(
            roles.contains(&ExtrusionRole::GapFill),
            "gap fill must be kept when there is no solid bottom cap; roles={roles:?}"
        );
    }
}
