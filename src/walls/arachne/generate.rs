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
//!    Where a ring would come back as a sliver and trace both flanks of a feature
//!    on top of itself, that part is cut out first ([`cut_sliver_parts`]) and
//!    left to the medial pass, which lays one centred bead instead.
//!
//! 2. **Medial-axis variable-width beads.** The residual material the offset
//!    loops leave behind — the centre of a feature too thin to fit another full
//!    loop — is filled with a bead that follows the polygon's medial axis, its
//!    width set to the *local* residual thickness (clamped to
//!    `[min, max]`).  This is the Arachne benefit: continuous, correctly-sized
//!    fill of thin/tapering features instead of a single central blob.
//!
//!    Where that residual reaches the model surface the bead *is* the feature —
//!    a rib, fin or divider no perimeter could cover — and it is emitted as an
//!    open [`ExtrusionRole::OuterWall`], because there it is the wall. There is
//!    no separate thin-wall feature type; see [`MedialKind`].
//!
//!    Before the beads are walked, the residual **skeleton is de-noised** so the
//!    fill is *layer-to-layer coherent*: a segment Voronoi grows a short spur at
//!    every faceted boundary vertex, which splits an otherwise-continuous gap
//!    spine into a chain per spur junction.  Left alone those shards land in a
//!    slightly different place on every layer (a curved hull's residual ring
//!    dissolving into a wandering cloud of stubs).  Pruning the short spurs (see
//!    [`super::skeleton::Skeleton::prune_short_leaf_chains`]) drops their
//!    junctions to degree 2 so the spine reassembles into a few long continuous
//!    beads — on the 3DBenchy hull this thirds the gap-fill bead count and
//!    triples the mean bead length with no change to void coverage.
//!
//! At a medial node whose distance-to-boundary is `r`, the number of full loops
//! that reach it (from each side) is `n = ⌊r/d − ½⌋ + 1`, capped at
//! `wall_count`.  The residual thickness is `2·(r − n·d)`; a bead is emitted only
//! when the region is geometry-limited (`n < wall_count`) and that residual is
//! thick enough for the kind of residual it is — down to the minimum bead width
//! for filler, and below it, widened, for a feature.  This shares the offsets'
//! distance field, so loops and medial beads never overlap.
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

    // Offset loops + residual medial fill, evaluated per island so a thick infill
    // core on one island cannot suppress a thin gap on another.
    //
    // `thin` — the island's own material too narrow for two beads — is computed
    // once here and used by both passes, which is what keeps them agreeing: the
    // ring pass declines to trace exactly the material the medial pass then
    // covers with one centred bead. Two separate answers to "is this too thin"
    // is how a rib ends up traced twice by one pass and skipped by the other.
    for island in split_islands(&normalised) {
        let thin = thin_material(&island, params.nozzle_diameter_mm);
        let loops = emit_offset_loops(
            &island,
            &thin,
            params,
            tol,
            &mut new_paths,
            &mut new_roles,
            &mut new_widths,
            &mut new_vwidths,
            &mut new_open,
        );
        // Residual medial fill always runs.  Variable-width fill of thin
        // features *is* the Arachne generator, and this is also the only thing
        // that fills the sliver between the innermost walls.  `thin_walls` is a
        // classic-generator option (see the schema gate) and deliberately has no
        // effect here.
        emit_residual_medial_fill(
            &island,
            &thin,
            &loops,
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

/// Emit concentric constant-width (`d`) perimeter loops for one island (holes
/// included), returning each loop centerline so the caller can derive the
/// residual the loops leave behind.  The outermost ring is tagged
/// [`ExtrusionRole::OuterWall`]; deeper rings are [`ExtrusionRole::InnerWall`].
///
/// Up to `wall_count` rings are placed.  When [`WallParams::extra_perimeters`] is
/// set and the core left after the nominal walls is *uniformly* narrower than
/// `extra_perimeters_max_gap_mm`, extra rings keep being added until that thin
/// core collapses, so a narrow gap is filled with perimeters rather than sparse
/// infill.  Loops are buffered and flushed in the order dictated by
/// [`WallParams::external_perimeters_first`] (outer-first when `true`, otherwise
/// innermost-first so the outer wall prints last).
#[allow(clippy::too_many_arguments)]
fn emit_offset_loops(
    island: &Paths,
    thin: &Paths,
    params: &WallParams,
    tol: f64,
    paths: &mut Paths,
    roles: &mut Vec<ExtrusionRole>,
    widths: &mut Vec<Option<f64>>,
    vwidths: &mut Vec<Option<Vec<f64>>>,
    open: &mut Vec<bool>,
) -> Vec<Path> {
    let d = params.nozzle_diameter_mm;
    let max_gap_half = params.extra_perimeters_max_gap_mm * 0.5;
    let mut loop_centerlines: Vec<Path> = Vec::new();
    // (centerline, is_outer) buffered outer→inner; flushed in configured order.
    let mut buffered: Vec<(Path, bool)> = Vec::new();
    let mut last = island.clone();
    let mut k = 0usize;
    loop {
        if k >= params.wall_count {
            // Beyond the nominal walls, only keep going for `extra_perimeters`
            // and only while the whole residual core is thinner than the gap
            // threshold (a wider core is left for sparse infill).
            if !params.extra_perimeters || last.is_empty() {
                break;
            }
            let core_has_thick_part = !inflate(
                last.clone(),
                -max_gap_half,
                JoinType::Miter,
                EndType::Polygon,
                2.0,
            )
            .is_empty();
            if core_has_thick_part {
                break;
            }
        }
        let is_outer = k == 0;
        // Erode the current region inward by `d` **once**.  This single result
        // feeds both consumers that previously each recomputed it:
        //   * the inner-loop morphological opening (dilate it back by `d`), and
        //   * the advance to the next shell (`last`).
        let eroded = inflate(last.clone(), -d, JoinType::Round, EndType::Polygon, 2.0);
        // Inner loops are offset from the *opened* remaining region.  A full loop
        // in a neck thinner than 2·d would trace both surfaces on top of itself —
        // the coincident inner beads that render as an over-extruded seam.
        // Opening drops that neck so the loop closes cleanly around it, leaving
        // the neck to the variable-width medial fill.  The outer ring is never
        // opened so the perimeter keeps tracing the model surface exactly.
        let base = if is_outer {
            last.clone()
        } else if eroded.is_empty() {
            Paths::new(vec![])
        } else {
            simplify(
                inflate(eroded.clone(), d, JoinType::Round, EndType::Polygon, 2.0),
                tol,
                false,
            )
        };
        let inset = if base.is_empty() {
            Paths::new(vec![])
        } else {
            simplify(
                inflate(base, -0.5 * d, JoinType::Round, EndType::Polygon, 2.0),
                tol,
                false,
            )
        };
        // The outer ring is the one offset straight off the model boundary, so
        // it is the one that can come back as a sliver: in a feature too thin
        // for two beads the ring traces both flanks on top of itself.  Cut
        // those parts out and let the medial pass lay one centred bead of the
        // feature's own width instead.  Inner rings need no such test — the
        // opening above already leaves their source region at least `2·d` thick.
        let inset = if is_outer && !thin.is_empty() {
            difference(inset.clone(), thin.clone(), FillRule::NonZero).unwrap_or(inset)
        } else {
            inset
        };
        for p in inset.iter() {
            buffered.push((p.clone(), is_outer));
            loop_centerlines.push(p.clone());
        }
        last = simplify(eroded, tol, false);
        if last.is_empty() {
            break;
        }
        k += 1;
    }

    // Flush buffered loops in the configured perimeter order.
    if params.external_perimeters_first {
        for (p, is_outer) in buffered {
            push_loop(p, is_outer, d, paths, roles, widths, vwidths, open);
        }
    } else {
        for (p, is_outer) in buffered.into_iter().rev() {
            push_loop(p, is_outer, d, paths, roles, widths, vwidths, open);
        }
    }

    loop_centerlines
}

/// Push a single constant-width (`width`) perimeter loop centerline into the
/// parallel layer vectors.
#[allow(clippy::too_many_arguments)]
fn push_loop(
    path: Path,
    is_outer: bool,
    width: f64,
    paths: &mut Paths,
    roles: &mut Vec<ExtrusionRole>,
    widths: &mut Vec<Option<f64>>,
    vwidths: &mut Vec<Option<Vec<f64>>>,
    open: &mut Vec<bool>,
) {
    roles.push(if is_outer {
        ExtrusionRole::OuterWall
    } else {
        ExtrusionRole::InnerWall
    });
    widths.push(Some(width));
    vwidths.push(None);
    open.push(false);
    paths.push(path);
}

/// The island's own material too narrow to hold two beads side by side — the
/// single definition of "thin" this generator works from.
///
/// It is a morphological opening: erode by `0.75·d` and dilate back, keeping
/// only material whose own inscribed radius reaches `0.75·d`, i.e. at least
/// `1.5·d` thick.  The difference is everything one medial bead can still cover
/// on its own, because a medial bead's width is capped at `wall_line_width_max`
/// (1.5·d by default) — at the crossover the one bead and the feature are the
/// same width, so nothing steps.
///
/// **Both passes read this one answer**, and that is the point of computing it
/// here rather than inside either of them:
///
/// - [`emit_offset_loops`] subtracts it from the outer ring. A ring encloses a
///   band, and where that band is `t` thick it runs down both flanks, `t` apart,
///   laying `2·d` of material into `t + d` of space; as `t → 0` it degenerates
///   into a hairpin that extrudes the feature twice. Whether it degenerates *all
///   the way* to nothing is decided by how the boundary lands on the `Centi`
///   grid — that is, by the model's rotation — which is the one thing a wall
///   generator must not depend on.
/// - [`emit_residual_medial_fill`] uses it to tell a **feature** bead from a
///   **gap** bead, and they are treated differently in every respect that
///   matters (see [`MedialKind`]).
///
/// When the two passes answered that question separately — the ring by measuring
/// its own sliver, the medial pass by asking how close the residual came to the
/// surface — they disagreed, and a rib would be traced by the ring *and* skipped
/// by the medial pass, or the reverse, depending on the layer.
///
/// Taking the mark on the island rather than on the ring also disposes of the
/// **corner crumb** an opening always leaves at a sharp convex corner, with no
/// size threshold to tune: the ring sits `d/2` inside the boundary, so at a 45°
/// corner or blunter the crumb reaches less far than that and cannot bevel it,
/// while the medial pass discards it because the ring covers it.  Sharper than
/// 45° the crumb does reach — which is right, because a spike that acute really
/// is thinner than two beads for the length of its tip.
fn thin_material(island: &Paths, d: f64) -> Paths {
    let core = inflate(
        island.clone(),
        -0.75 * d,
        JoinType::Round,
        EndType::Polygon,
        2.0,
    );
    if core.is_empty() {
        // Nothing here is thick enough for two beads anywhere.
        return island.clone();
    }
    let thick = inflate(core, 0.75 * d, JoinType::Round, EndType::Polygon, 2.0);
    difference(island.clone(), thick, FillRule::NonZero).unwrap_or_default()
}

/// What a medial bead *is*, which decides both how thin a feature it will still
/// cover and what it is printed as.
///
/// The residual the offset loops leave behind holds two physically different
/// things, and the same generator emits both:
///
/// | Kind | What it is | Printed as |
/// | --- | --- | --- |
/// | `Feature` | a rib, fin, divider or neck too thin for a perimeter — the model itself | an open `OuterWall` bead: it *is* the wall there |
/// | `Gap` | filler in the sliver between the innermost loops | `GapFill` |
///
/// The distinction is not cosmetic. A feature must be printed even when it is
/// narrower than the minimum bead — widened to that minimum, because a divider
/// the user modelled is better slightly fat than absent — while a gap narrower
/// than the minimum bead must stay empty, since widening a sliver between two
/// walls only over-extrudes a band that is already full.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MedialKind {
    Feature,
    Gap,
}

impl MedialKind {
    fn role(self) -> ExtrusionRole {
        match self {
            Self::Feature => ExtrusionRole::OuterWall,
            Self::Gap => ExtrusionRole::GapFill,
        }
    }

    /// Thinnest local thickness this kind will still lay a bead into.
    ///
    /// A gap stops at the minimum printable bead width. A feature keeps going
    /// [`MIN_FEATURE_WIDTH_FACTOR`] below it and is widened up to it instead.
    fn min_thickness(self, params: &WallParams) -> f64 {
        match self {
            Self::Feature => MIN_FEATURE_WIDTH_FACTOR * params.wall_line_width_min_mm,
            Self::Gap => params.wall_line_width_min_mm,
        }
    }
}

/// How far below the minimum bead width a feature may be and still earn a bead,
/// as a fraction of that minimum.
///
/// Widening is the only way to print a feature the nozzle cannot lay a bead as
/// narrow as, and it buys the feature by over-extruding: a cavity `t` wide given
/// a `min_w` bead receives `min_w/t` times the material it has room for, and the
/// excess goes somewhere — proud of the surface, or into the layer above. At
/// this factor the worst case is twice, which a 0.4 mm card divider under a
/// 0.6 mm nozzle is nowhere near (it asks for 1.28×) and a modelling artefact —
/// the feather edge of a boolean, a tangent sliver — is well past.
///
/// Dropping the feature instead is what the generator used to do, and it is the
/// worse failure: it deleted every slot of the card caddy the moment the nozzle
/// was wider than the dividers, leaving a solid block where the part's whole
/// purpose was the slots.
const MIN_FEATURE_WIDTH_FACTOR: f64 = 0.5;

/// The Clipper coordinate grid, in millimetres.
///
/// Every path this module hands to Clipper is quantised to it, so it is the
/// resolution below which "geometry" is really rounding.
const CENTI_GRID_MM: f64 = 0.01;

/// How long a dip below the printable width a run will carry before it counts as
/// the run's end, as a multiple of the nozzle diameter.
///
/// One bead: an interruption shorter than the line being drawn cannot be drawn.
/// Stopping and restarting across it deposits the same material in two beads
/// rather than one, pays a travel for the privilege, and leaves the two ends
/// squeezing into the space between them anyway — so all the break achieves is a
/// pair of loose ends where the model has a continuous feature. A tapering
/// chamfer is where this shows: its thickness crosses the minimum bead width
/// back and forth as the facets under it change, and breaking at every crossing
/// turned one bead into a file of millimetre dabs.
///
/// A dip *above* the maximum is never bridged — see the walk in
/// [`emit_medial_beads`].
const BRIDGE_DIP_FACTOR: f64 = 1.0;

/// Fill the residual an island's offset `loops` leave uncovered with medial
/// gap-fill beads.
///
/// The material a loop deposits is a `d`-wide band about its centerline; the
/// union of those bands is the covered area, and the island's material minus
/// that union is the true thin residual (an enclosed gap between the innermost
/// walls).  Deriving coverage from the centerlines — rather than the eroded
/// offset polygon — avoids the dead zone where the onion-peel emits degenerate
/// sliver loops into a thin band instead of leaving it to be filled.
///
/// ## Why this pass is unconditional
///
/// The residual covers two physically different cases — a **thin feature** whose
/// bead *is* the model geometry (engraved text, a tapering rib, a card holder's
/// slot fins), and ordinary **gap fill** in the sliver between the innermost
/// loops — and Arachne prints **both, always**.
///
/// Variable-width medial fill of sub-perimeter features is the whole point of
/// this generator, so there is no
/// [`thin_walls`](crate::settings::params::SlicingParams::thin_walls) switch
/// here: that option is the *classic* generator's way of asking for the same
/// behaviour and is gated to it (in the schema and in
/// [`super::super::beads`]).  Making it also silence Arachne would let a setting
/// the UI hides for this generator silently delete real geometry.
#[allow(clippy::too_many_arguments)]
fn emit_residual_medial_fill(
    island: &Paths,
    thin: &Paths,
    loops: &[Path],
    params: &WallParams,
    paths: &mut Paths,
    roles: &mut Vec<ExtrusionRole>,
    widths: &mut Vec<Option<f64>>,
    vwidths: &mut Vec<Option<Vec<f64>>>,
    open: &mut Vec<bool>,
) {
    let d = params.nozzle_diameter_mm;
    // Each loop deposits a `d`-wide band about its centerline; inflating all
    // centerlines at once (`EndType::Joined` doubles a closed path into a band)
    // yields their union directly — one Clipper offset instead of an
    // accumulate-and-reunion loop that cloned the growing coverage every pass.
    let covered = if loops.is_empty() {
        Paths::new(vec![])
    } else {
        inflate(
            Paths::new(loops.to_vec()),
            0.5 * d,
            JoinType::Round,
            EndType::Joined,
            2.0,
        )
    };
    let uncovered =
        difference(island.clone(), covered.clone(), FillRule::NonZero).unwrap_or_default();

    // A medial bead is at least `min_len` long and `min_w` wide, so its extruded
    // footprint — which must lie inside the residual — has area ≥ `min_len ·
    // min_w`.  A residual sub-region below that bound can never host a printable
    // bead, so skip its (expensive) Voronoi/skeleton build.  97 % of Benchy
    // residual slivers fall here; the skip is loss-free.
    let min_len = gap_fill_min_run_len_mm(params);
    let min_area = params.wall_line_width_min_mm * min_len;

    // Two passes over two regions, because the two things in the residual are
    // different (see [`MedialKind`]) — and **neither region is carved out of the
    // other**:
    //
    //   feature — `thin`: the island's own material too narrow for two beads,
    //             taken straight from the model
    //   gap     — `uncovered`: what no ring laid down, taken straight from the
    //             deposition
    //
    // They overlap, and each pass discards the overlap at the level of a
    // finished *run* rather than by clipping its region first. Clipping is what
    // used to break this. A rib is a clean rectangle in the model and its medial
    // axis is a clean spine; intersect it with the complement of some ring's
    // band and two of its sides become that ring's offset curve — which
    // shortens the spine and studs the boundary with vertices the Voronoi
    // answers with spurs. A 4 mm rib came out as a 3.4 mm bead that way, and on
    // the layers where the boolean shattered it, as nothing at all.
    //
    // Discarding by run leaves every surviving bead exactly as the medial axis
    // drew it:
    //
    //   a feature run mostly under a ring's band is the crumb every opening
    //   leaves at a sharp convex corner — real material, but the ring is already
    //   there;
    //   a gap run mostly inside `thin` is a feature, and belongs to the pass
    //   that knows to widen it rather than drop it.
    //
    // *Mostly*, by length, and not *wholly*: a rib's collar at the root is
    // covered while its length is not, and the bead wants to reach the wall it
    // stands on.
    for (region, kind, reject) in [
        (thin, MedialKind::Feature, &covered),
        (&uncovered, MedialKind::Gap, thin),
    ] {
        for sub in split_islands(region) {
            let area = sub.iter().map(|p| p.signed_area()).sum::<f64>().abs();
            if area < min_area {
                continue;
            }
            medial_fill(
                &sub, kind, reject, params, paths, roles, widths, vwidths, open,
            );
        }
    }
}

/// Build the Voronoi diagram, catching both the crate's `Err` results and its
/// occasional numerical panics, so a single degenerate layer can never abort
/// the whole slice.
///
/// On `wasm32` (`panic = abort`) `catch_unwind` cannot intercept a panic; the
/// input sanitisation in [`build_segment_voronoi`] is the defence there.
fn build_voronoi_safe(paths: &Paths) -> Option<(Diagram, [f64; 2])> {
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
fn split_islands(paths: &Paths) -> Vec<Paths> {
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

/// Spur-prune ratio for medial gap fill: drop a leaf medial edge whose boundary
/// end collapses below this fraction of its interior neighbour's radius, so
/// faceting spurs don't shatter a gap spine.
const GAP_SPUR_PRUNE_RATIO: f64 = 0.6;

/// Length below which a medial **leaf spur** is pruned from the residual
/// skeleton, as a multiple of the nozzle diameter (see
/// [`super::skeleton::Skeleton::prune_short_leaf_chains`]).
///
/// The radius-ratio prune ([`GAP_SPUR_PRUNE_RATIO`]) only removes spurs that
/// dive toward the boundary; in a *uniform*-thickness residual band every facet
/// vertex still grows a short spur whose radius matches the spine, and those are
/// what fragment the spine into a chain-per-junction.  Pruning any spur shorter
/// than `2·d` (0.8 mm at a 0.4 mm nozzle) clears that facet noise so the spine
/// reassembles into a few long continuous beads.
///
/// Tuned on the 3DBenchy hull: it roughly **thirds** the gap-fill bead count
/// (~3200 → ~1050) and **triples** the mean bead length (~2.2 mm → ~7 mm), while
/// lifting the layer-to-layer footprint IoU (~0.73 → ~0.76).  `2·d` is the
/// largest floor that leaves the thin-wall void coverage of every corpus model
/// unchanged (a larger floor starts nibbling fine embossed-logo detail); the
/// coincidence-free wall property is untouched because walls are not modified.
const GAP_SPUR_MIN_LEN_FACTOR: f64 = 2.0;

/// Minimum emitted gap-fill **run** length, as a multiple of the nozzle
/// diameter, used when the caller leaves `gap_fill_min_length_mm` at its `0`
/// "auto" default.
///
/// A run shorter than this deposits a mechanically-insignificant dab of
/// material (below `2·d × min_width` ≈ 0.3 mm² at a 0.4 mm nozzle) yet still
/// costs a full retract → travel → un-retract cycle to reach — the "tiny
/// inner-body splat" that wastes print time and invites filament grinding.  The
/// sub-`2·d` residual such a run would have filled is already bridged by the
/// squish of the wall beads flanking it: the `classic` generator leaves the
/// identical curved/tapering wall-band corners bead-free with no measurable
/// wall-zone void (verified with [`tools/gcode-analysis/voids.py`]).  Dropping
/// them clears the splat swarm (~270 isolated beads on the 3DBenchy) with no
/// change in void coverage.
///
/// It matches [`GAP_SPUR_MIN_LEN_FACTOR`] on purpose: a run below the same `2·d`
/// floor that separates a real gap spine from facet-noise spurs is itself facet
/// noise once isolated as its own bead.  A user who wants the old, denser
/// behaviour can set an explicit smaller `gap_fill_min_length_mm`.
const GAP_FILL_MIN_RUN_LEN_FACTOR: f64 = 2.0;

/// Resolve the effective minimum gap-fill run length (mm): the explicit
/// `gap_fill_min_length_mm` when the user set one (`> 0`), else
/// [`GAP_FILL_MIN_RUN_LEN_FACTOR`] × nozzle diameter.  Shared by the residual
/// pre-filter ([`emit_residual_medial_fill`]) and the per-run emit filter
/// ([`emit_medial_beads`]) so the two never disagree on what counts as a
/// printable run.
fn gap_fill_min_run_len_mm(params: &WallParams) -> f64 {
    if params.gap_fill_min_length_mm > 0.0 {
        params.gap_fill_min_length_mm
    } else {
        GAP_FILL_MIN_RUN_LEN_FACTOR * params.nozzle_diameter_mm
    }
}

/// Medial-fill a (thin) region: emit variable-width open beads along its medial
/// axis wherever the local thickness (2·radius) is one this `kind` of residual
/// will cover.  Thicker sub-regions get no bead — they are left for infill.
///
/// A Voronoi failure (error or numerical panic) degrades gracefully to no fill
/// for this region; the perimeter loops already placed are unaffected.
#[allow(clippy::too_many_arguments)]
fn medial_fill(
    region: &Paths,
    kind: MedialKind,
    reject: &Paths,
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
    // Skeletonise the whole residual and let `emit_medial_beads` decide, per
    // node, what is thin enough to fill: a bead is laid only where the local
    // thickness is a printable width (`≤ gap_max`), so a thick infill cavity in
    // the same island contributes no bead while a thin neck opening into it is
    // still filled as ONE continuous run.  An earlier version split the thin
    // shell off first (an erode/dilate difference); that shredded every gap into
    // thousands of sub-millimetre stubs which then had to be dropped as noise —
    // reopening the very gaps it was meant to close.  Skeletonising the residual
    // whole keeps each gap a single continuous chain that a modest `min_len`
    // cleanly separates from the short spurs a cavity boundary throws off.
    // Flatten the staircase the coordinate grid leaves on any boundary that is
    // not axis-aligned, **before** the Voronoi sees it.
    //
    // Clipper works on a `Centi` (0.01 mm) integer grid, so a rib turned off the
    // axes has each flank quantised into a flight of 0.01 mm steps. A segment
    // Voronoi answers a staircase with a spur per step and a spine that zigzags
    // between them — on a 5 mm rib at 45°, a 10 mm chain of junctions where the
    // same rib at 0° gives one straight edge. That is the whole of this
    // generator's old rotation-dependence: every threshold downstream (spur
    // pruning, chain length, run length) was being asked to tell the model's
    // shape from the grid's, one consequence at a time.
    //
    // The tolerance is two grid steps: far below anything printable — a bead is
    // at least `wall_line_width_min` wide, seventeen times this at a 0.4 mm
    // nozzle — and just above the noise it exists to erase.
    let region = &simplify(region.clone(), 2.0 * CENTI_GRID_MM, false);
    let Some((diagram, offset)) = build_voronoi_safe(region) else {
        return;
    };
    // Prune the boundary spurs the segment Voronoi grows at every facet vertex
    // so a **gap** becomes continuous degree-2 spines rather than a burst of
    // stubs.  Two complementary passes: a radius-ratio prune (removes spurs that
    // *dive* toward the boundary) followed by a length prune (removes the short
    // facet-noise spurs a *uniform*-thickness residual band grows, which the
    // ratio test misses because their radius matches the spine).  Clearing that
    // noise drops the spurs' junctions to degree 2, so `chains()` reassembles
    // each gap into a few long continuous beads instead of a bead-per-junction
    // shard set that wandered from layer to layer.
    //
    // Both prunes run on a feature's skeleton too.  They are safe there because
    // the simplify above has already taken the staircase out: what made the
    // radius-ratio prune eat a rib was the grid, not the rib. On a clean spine
    // its leaf test stops at the first edge whose radius matches its neighbour's,
    // which is the spine itself; on a staircase the radii jittered, so it kept
    // finding another leaf and chewed the bead back from both ends.
    let spur_min_len = GAP_SPUR_MIN_LEN_FACTOR * params.nozzle_diameter_mm;
    let skel = build_skeleton(region, &diagram, offset)
        .prune_boundary_spurs(GAP_SPUR_PRUNE_RATIO)
        .prune_short_leaf_chains(spur_min_len);
    for chain in skel.chains() {
        emit_medial_beads(
            &chain,
            &skel.nodes,
            kind,
            reject,
            params,
            paths,
            roles,
            widths,
            vwidths,
            open,
        );
    }
}

/// Whether most of `run`, by length, lies inside `region`.
///
/// This is how each residual pass discards what belongs to the other without
/// distorting what it keeps: the test is applied to a finished run, so every
/// surviving bead is exactly what the medial axis drew.
///
/// *Most*, rather than *all*, because the two regions genuinely overlap at the
/// ends — a rib's collar where it meets the wall is covered by the wall's own
/// ring, and a gap that opens into a thin neck runs a little way into it. A
/// majority is the one threshold that needs no tuning: a crumb at a convex
/// corner is covered along its whole length, and a rib is uncovered along its
/// whole length but for that collar.
fn run_mostly_inside(run: &[(f64, f64)], region: &Paths) -> bool {
    // Both regions are Clipper unions, so a point is inside when an odd number
    // of contours contain it (a hole flips the parity).
    let inside = |x: f64, y: f64| -> bool {
        region
            .iter()
            .filter(|p| {
                matches!(
                    point_in_polygon(Point::new(x, y), p),
                    PointInPolygonResult::IsInside | PointInPolygonResult::IsOn
                )
            })
            .count()
            % 2
            == 1
    };
    if region.is_empty() {
        return false;
    }
    let (mut within, mut total) = (0.0, 0.0);
    for w in run.windows(2) {
        let seg = (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
        let mid = ((w[0].0 + w[1].0) * 0.5, (w[0].1 + w[1].1) * 0.5);
        total += seg;
        if inside(mid.0, mid.1) {
            within += seg;
        }
    }
    total > 0.0 && within > 0.5 * total
}

/// Emit medial beads for one chain.
///
/// Walks the chain, accumulating a run while the local thickness `2·radius` stays
/// inside this `kind`'s envelope and flushing a bead (per-vertex width = local
/// thickness, clamped to a printable width) whenever it leaves.  Runs shorter
/// than `min_len` are dropped as faceting noise.
///
/// The residual skeleton has already had its short spurs pruned (see
/// [`medial_fill`]), so each chain arrives as a long continuous spine rather
/// than a burst of stubs — the runs this emits are correspondingly few and long.
#[allow(clippy::too_many_arguments)]
fn emit_medial_beads(
    chain: &[usize],
    nodes: &[super::skeleton::SkeletonNode],
    kind: MedialKind,
    reject: &Paths,
    params: &WallParams,
    paths: &mut Paths,
    roles: &mut Vec<ExtrusionRole>,
    widths: &mut Vec<Option<f64>>,
    vwidths: &mut Vec<Option<Vec<f64>>>,
    open: &mut Vec<bool>,
) {
    let min_w = params.wall_line_width_min_mm;
    // Fill residuals up to 2.5·d, but never lay a bead wider than the configured
    // max line width — a single gap bead at 2.5·d over-extrudes far past what the
    // user asked for (visible as blobs / dimensional bulge, especially layer 1).
    let max_w = params.wall_line_width_max_mm;
    let gap_max = 2.5 * params.nozzle_diameter_mm;
    // How thin this kind of residual may get before it stops earning a bead. A
    // gap stops at the minimum printable width; a feature keeps going and is
    // widened up to it, which is the only reason a nozzle wider than the model's
    // own ribs can print them at all.
    let min_t = kind.min_thickness(params);
    let role = kind.role();
    let min_len = gap_fill_min_run_len_mm(params);
    let mut run: Vec<(f64, f64)> = Vec::new();
    let mut run_w: Vec<f64> = Vec::new();

    let flush = |run: &mut Vec<(f64, f64)>,
                 run_w: &mut Vec<f64>,
                 paths: &mut Paths,
                 roles: &mut Vec<ExtrusionRole>,
                 widths: &mut Vec<Option<f64>>,
                 vwidths: &mut Vec<Option<Vec<f64>>>,
                 open: &mut Vec<bool>| {
        if run.len() >= 2 && polyline_len(run) >= min_len && !run_mostly_inside(run, reject) {
            // Per-vertex width = local gap thickness, clamped to a printable
            // width; the scalar width is the run mean for callers that ignore
            // the per-vertex array.
            let vw: Vec<f64> = run_w.iter().map(|t| t.clamp(min_w, max_w)).collect();
            let mean = vw.iter().sum::<f64>() / vw.len() as f64;
            let path: Path = std::mem::take(run).into();
            paths.push(path);
            roles.push(role);
            widths.push(Some(mean));
            vwidths.push(Some(vw));
            open.push(true);
        }
        run.clear();
        run_w.clear();
    };

    // Nodes held back because the local thickness dipped below `min_t`, waiting
    // to see whether the dip is long enough to be a real end.
    let mut pending: Vec<(f64, f64)> = Vec::new();
    let mut pending_w: Vec<f64> = Vec::new();
    let mut pending_len = 0.0_f64;
    let mut prev: Option<(f64, f64)> = None;

    for &i in chain {
        let t = 2.0 * nodes[i].radius; // local wall thickness at this node
        let p = (nodes[i].x, nodes[i].y);
        let step = prev.map_or(0.0, |q| (p.0 - q.0).hypot(p.1 - q.1));
        prev = Some(p);

        if t > gap_max {
            // Not ours at all: the chain has run out of the thin material and
            // into a cavity, which infill fills.  A real end, never bridged —
            // bridging one would drag a bead straight across the cavity.
            pending.clear();
            pending_w.clear();
            pending_len = 0.0;
            flush(&mut run, &mut run_w, paths, roles, widths, vwidths, open);
            continue;
        }

        if t < min_t {
            // Too thin to lay a bead into *here*.  Hold it: a dip shorter than
            // one bead is not a gap in the material, it is a gap in the
            // measurement.  A tapering chamfer's thickness crosses this bound
            // back and forth as the facets under it change, and breaking the run
            // at every crossing is what turned one continuous bead into a file
            // of millimetre dabs, each paying a full travel to reach.  The two
            // ends of such a break deposit into the space between them anyway.
            if run.is_empty() {
                continue;
            }
            pending_len += step;
            if pending_len > BRIDGE_DIP_FACTOR * params.nozzle_diameter_mm {
                pending.clear();
                pending_w.clear();
                pending_len = 0.0;
                flush(&mut run, &mut run_w, paths, roles, widths, vwidths, open);
            } else {
                pending.push(p);
                pending_w.push(t);
            }
            continue;
        }

        // Printable.  Absorb a short dip, or end the run at a long one.
        if !run.is_empty() && !pending.is_empty() {
            pending_len += step;
            if pending_len > BRIDGE_DIP_FACTOR * params.nozzle_diameter_mm {
                flush(&mut run, &mut run_w, paths, roles, widths, vwidths, open);
            } else {
                run.append(&mut pending);
                run_w.append(&mut pending_w);
            }
        }
        pending.clear();
        pending_w.clear();
        pending_len = 0.0;
        run.push(p);
        run_w.push(t);
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

    /// Build a layer holding one closed contour, tagged as a raw perimeter the
    /// way `slice_mesh` hands them over.
    fn layer_with_contour(pts: Vec<(f64, f64)>) -> SliceLayer {
        let mut layer = SliceLayer::new(0.2);
        let contour: Path = pts.into();
        layer.paths.push(contour);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.path_widths.push(None);
        layer
    }

    /// A 20 mm square body with a `w`-wide, 5 mm rib off its right face, turned
    /// `deg` degrees about the origin.
    fn body_with_rib(w: f64, deg: f64) -> Vec<(f64, f64)> {
        let h = w / 2.0;
        let (s, c) = deg.to_radians().sin_cos();
        [
            (0.0, 0.0),
            (20.0, 0.0),
            (20.0, 10.0 - h),
            (25.0, 10.0 - h),
            (25.0, 10.0 + h),
            (20.0, 10.0 + h),
            (20.0, 20.0),
            (0.0, 20.0),
        ]
        .into_iter()
        .map(|(x, y)| (x * c - y * s, x * s + y * c))
        .collect()
    }

    /// Every medial bead of a layer, as `(role, length)`.
    fn medial_beads(layer: &SliceLayer) -> Vec<(ExtrusionRole, f64)> {
        (0..layer.paths.len())
            .filter(|&i| layer.is_medial_bead(i))
            .map(|i| {
                let pts: Vec<(f64, f64)> = layer
                    .paths
                    .iter()
                    .nth(i)
                    .unwrap()
                    .iter()
                    .map(|p| (p.x(), p.y()))
                    .collect();
                (layer.role_for_path(i), polyline_len(&pts))
            })
            .collect()
    }

    /// A rib too thin for a perimeter is the model itself, not filler between
    /// walls — so the bead that covers it *is* the wall there: an open
    /// `OuterWall` carrying its own per-vertex widths.
    #[test]
    fn a_thin_rib_is_an_open_outer_wall_bead() {
        let mut layer = layer_with_contour(body_with_rib(0.4, 0.0));
        generate_arachne_walls_for_layer(&mut layer, &wall_params());

        let rib = (0..layer.paths.len())
            .find(|&i| layer.is_medial_bead(i))
            .expect("the rib should be covered by a medial bead");
        assert_eq!(
            layer.role_for_path(rib),
            ExtrusionRole::OuterWall,
            "a bead that is the feature prints as the wall it is"
        );
        assert!(layer.is_path_open(rib), "a medial bead is an open polyline");
        let xs: Vec<f64> = layer
            .paths
            .iter()
            .nth(rib)
            .unwrap()
            .iter()
            .map(|p| p.x())
            .collect();
        assert!(
            xs.iter().cloned().fold(f64::MIN, f64::max) > 20.0,
            "the bead should run out along the rib, got {xs:?}"
        );
    }

    /// The counterpart: the sliver left between the perimeters of a wall band
    /// never reaches the surface, so it stays gap fill.
    #[test]
    fn a_wall_band_residual_stays_gap_fill() {
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

        let beads = medial_beads(&layer);
        assert!(
            beads.iter().any(|&(r, _)| r == ExtrusionRole::GapFill),
            "the wall-band sliver is filler between perimeters: {beads:?}"
        );
        assert!(
            beads.iter().all(|&(r, _)| r == ExtrusionRole::GapFill),
            "nothing here reaches the surface uncovered: {beads:?}"
        );
    }

    /// The toolpath is a property of the model, not of its angle to the axes.
    ///
    /// A rib near one nozzle wide is where that used to break: the outer ring is
    /// offset straight off the boundary, so whether the offset collapsed or left
    /// a sliver came down to how the flanks landed on the coordinate grid. At 0°
    /// the rib became one centred bead and at 45° a hairpin loop that extruded
    /// it twice.
    #[test]
    fn a_rib_is_sliced_the_same_at_every_angle() {
        let params = wall_params();
        for w in [0.3, 0.4, 0.5] {
            let measure = |deg: f64| {
                let mut layer = layer_with_contour(body_with_rib(w, deg));
                generate_arachne_walls_for_layer(&mut layer, &params);
                let beads = medial_beads(&layer);
                let rib: f64 = beads
                    .iter()
                    .filter(|&&(r, _)| r == ExtrusionRole::OuterWall)
                    .map(|&(_, l)| l)
                    .sum();
                let loops = (0..layer.paths.len())
                    .filter(|&i| !layer.is_path_open(i))
                    .count();
                (rib, loops)
            };
            let (flat_len, flat_loops) = measure(0.0);
            let (turned_len, turned_loops) = measure(45.0);
            assert!(
                flat_len > 4.0,
                "a {w} mm rib should be covered end to end, got {flat_len:.2} mm"
            );
            assert!(
                (flat_len - turned_len).abs() < 0.5,
                "a {w} mm rib: {flat_len:.2} mm of bead flat vs {turned_len:.2} mm turned 45°"
            );
            assert_eq!(
                flat_loops, turned_loops,
                "a {w} mm rib grew a hairpin loop when turned 45°"
            );
        }
    }

    /// A *field* of ribs, the way a card caddy or a fan grille has them, turned
    /// against the axes.
    ///
    /// The single-rib test above passes on geometry a whole field still fails:
    /// what goes wrong at a rotation is not that every rib breaks but that a few
    /// of them do, on a few layers, and one rib in isolation does not sample
    /// that. Measuring the field's *total* bead length is what catches it — the
    /// same measurement that showed the caddy printing 42 % of its dividers at
    /// 45° before the sliver cut, and 98 % after.
    #[test]
    fn a_field_of_ribs_survives_being_turned() {
        let params = wall_params();
        let field = |deg: f64| -> (f64, usize) {
            let (sn, cs) = deg.to_radians().sin_cos();
            // A 40 mm bar with twelve 0.4 mm ribs, 3 mm long, 3 mm apart.
            let mut pts: Vec<(f64, f64)> = vec![(0.0, 0.0), (40.0, 0.0), (40.0, 4.0)];
            for k in (0..12).rev() {
                let x = 2.0 + k as f64 * 3.0;
                pts.push((x + 0.4, 4.0));
                pts.push((x + 0.4, 7.0));
                pts.push((x, 7.0));
                pts.push((x, 4.0));
            }
            pts.push((0.0, 4.0));
            let turned: Vec<(f64, f64)> = pts
                .into_iter()
                .map(|(x, y)| (x * cs - y * sn, x * sn + y * cs))
                .collect();
            let mut layer = layer_with_contour(turned);
            generate_arachne_walls_for_layer(&mut layer, &params);
            let bead: f64 = medial_beads(&layer).iter().map(|&(_, l)| l).sum();
            let loops = (0..layer.paths.len())
                .filter(|&i| !layer.is_path_open(i))
                .count();
            (bead, loops)
        };
        let (flat, flat_loops) = field(0.0);
        // A rib's medial axis stops short of its tip, so twelve 3 mm ribs come
        // to ~29 mm rather than 36; what matters here is that the figure holds
        // when the field is turned.
        assert!(
            flat > 25.0,
            "twelve 3 mm ribs should come to ~29 mm of bead, got {flat:.1}"
        );
        for deg in [15.0, 30.0, 45.0, 60.0] {
            let (turned, loops) = field(deg);
            assert!(
                turned > 0.9 * flat,
                "turned {deg}°, the rib field lost bead: {turned:.1} mm vs {flat:.1} mm flat"
            );
            assert_eq!(
                loops,
                flat_loops,
                "turned {deg}°, the rib field grew {} extra closed loops — a ring \
                 doubling back over a rib",
                loops as i64 - flat_loops as i64
            );
        }
    }

    /// A feature narrower than the minimum bead width is printed slightly fat,
    /// never dropped: a 0.4 mm card divider under a 0.6 mm nozzle is the whole
    /// point of the part, and the alternative is a caddy with no slots.
    #[test]
    fn a_rib_narrower_than_the_minimum_bead_is_widened_not_deleted() {
        let params = WallParams::from_slicing_params(&SlicingParams {
            nozzle_diameter_mm: 0.6,
            ..SlicingParams::default()
        });
        assert!(
            params.wall_line_width_min_mm > 0.4,
            "this test needs a nozzle whose minimum bead is wider than the rib"
        );
        let mut layer = layer_with_contour(body_with_rib(0.4, 0.0));
        generate_arachne_walls_for_layer(&mut layer, &params);

        let rib = (0..layer.paths.len())
            .find(|&i| layer.is_medial_bead(i))
            .expect("the rib must still be printed, widened to the minimum bead");
        let w = layer
            .width_for_path(rib)
            .expect("a medial bead has a width");
        assert!(
            (w - params.wall_line_width_min_mm).abs() < 1e-6,
            "the rib should be widened to exactly the minimum bead, got {w}"
        );
    }

    #[test]
    fn produces_closed_offset_walls_for_thick_square() {
        // 20 mm square, 3 walls → 3 concentric closed loops, no gap bead
        // (centre residual is infill, not a wall).
        let mut layer = layer_with_square(20.0);
        generate_arachne_walls_for_layer(&mut layer, &wall_params());

        // Default order is inner-first: the single outer wall prints LAST.
        let closed: Vec<usize> = (0..layer.paths.len())
            .filter(|&i| !layer.is_path_open(i))
            .collect();
        assert!(closed.len() >= 3, "expected >=3 concentric wall loops");
        assert_eq!(
            layer.role_for_path(*closed.last().unwrap()),
            ExtrusionRole::OuterWall,
            "outer wall should be the last closed loop by default"
        );
        let outer_count = (0..layer.paths.len())
            .filter(|&i| layer.role_for_path(i) == ExtrusionRole::OuterWall)
            .count();
        assert_eq!(outer_count, 1, "exactly one outer wall expected");
        // Every bead carries an explicit width.
        assert!(layer.path_widths.iter().all(|w| w.is_some()));
    }

    #[test]
    fn external_perimeters_first_puts_outer_wall_first() {
        let mut layer = layer_with_square(20.0);
        let params = WallParams::from_slicing_params(&SlicingParams {
            external_perimeters_first: true,
            ..SlicingParams::default()
        });
        generate_arachne_walls_for_layer(&mut layer, &params);

        assert_eq!(
            layer.role_for_path(0),
            ExtrusionRole::OuterWall,
            "outer wall should print first when external_perimeters_first = true"
        );
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
    fn arachne_ignores_thin_walls() {
        // `thin_walls` is a *classic-generator* option (and is gated to it in the
        // schema), so it must not change Arachne's output at all — neither the
        // inter-perimeter gap fill nor a standalone thin feature.  A setting the
        // UI hides for this generator must never silently delete geometry.
        let cases: [(&str, Path); 2] = [
            // 1.16 mm bar (= 2.9·d): a residual *between* the two loops.
            (
                "inter-perimeter residual",
                vec![(-10.0, -0.58), (10.0, -0.58), (10.0, 0.58), (-10.0, 0.58)].into(),
            ),
            // 0.4 mm rib (= 1·d): too thin for any loop — the bead *is* the feature.
            (
                "standalone thin feature",
                vec![(-8.0, -0.2), (8.0, -0.2), (8.0, 0.2), (-8.0, 0.2)].into(),
            ),
        ];

        for (label, shape) in cases {
            let build = |thin_walls: bool| -> usize {
                let mut layer = SliceLayer::new(0.2);
                layer.paths.push(shape.clone());
                layer.path_roles.push(ExtrusionRole::OuterWall);
                layer.path_widths.push(None);
                let params = WallParams::from_slicing_params(&SlicingParams {
                    thin_walls,
                    ..SlicingParams::default()
                });
                generate_arachne_walls_for_layer(&mut layer, &params);
                (0..layer.paths.len())
                    .filter(|&i| layer.is_path_open(i))
                    .count()
            };

            let on = build(true);
            let off = build(false);
            assert!(on >= 1, "{label}: control should emit a medial bead");
            assert_eq!(
                on, off,
                "{label}: Arachne must ignore thin_walls (got {on} on / {off} off)"
            );
        }
    }

    #[test]
    fn extra_perimeters_fills_a_thin_core_with_loops() {
        // A 3.4 mm-wide bar: the nominal 3 walls (2.4 mm) fit, leaving a 1.0 mm
        // core — thin enough for `extra_perimeters` yet wide enough to host one
        // more loop pair.  By default that core is a medial gap bead; with
        // `extra_perimeters` it is filled with an additional closed loop instead.
        let bar =
            || -> Path { vec![(-15.0, -1.7), (15.0, -1.7), (15.0, 1.7), (-15.0, 1.7)].into() };

        let mut base = SliceLayer::new(0.2);
        base.paths.push(bar());
        base.path_roles.push(ExtrusionRole::OuterWall);
        base.path_widths.push(None);
        generate_arachne_walls_for_layer(&mut base, &wall_params());
        let base_loops = (0..base.paths.len())
            .filter(|&i| !base.is_path_open(i))
            .count();

        let mut extra = SliceLayer::new(0.2);
        extra.paths.push(bar());
        extra.path_roles.push(ExtrusionRole::OuterWall);
        extra.path_widths.push(None);
        let params = WallParams::from_slicing_params(&SlicingParams {
            extra_perimeters: true,
            ..SlicingParams::default()
        });
        generate_arachne_walls_for_layer(&mut extra, &params);
        let extra_loops = (0..extra.paths.len())
            .filter(|&i| !extra.is_path_open(i))
            .count();

        assert!(
            extra_loops > base_loops,
            "extra_perimeters should add closed loops in the thin core \
             (base {base_loops}, extra {extra_loops})"
        );
    }

    #[test]
    fn extra_perimeters_leaves_a_thick_body_alone() {
        // A 20 mm square has a wide core → extra_perimeters must NOT turn it into
        // concentric loops; it stays at the nominal wall count.
        let mut base = layer_with_square(20.0);
        generate_arachne_walls_for_layer(&mut base, &wall_params());
        let base_loops = (0..base.paths.len())
            .filter(|&i| !base.is_path_open(i))
            .count();

        let mut extra = layer_with_square(20.0);
        let params = WallParams::from_slicing_params(&SlicingParams {
            extra_perimeters: true,
            ..SlicingParams::default()
        });
        generate_arachne_walls_for_layer(&mut extra, &params);
        let extra_loops = (0..extra.paths.len())
            .filter(|&i| !extra.is_path_open(i))
            .count();

        assert_eq!(
            base_loops, extra_loops,
            "extra_perimeters must not add loops to a wide (infill) core"
        );
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

    #[test]
    fn gap_fill_min_run_len_auto_default_is_two_nozzle() {
        // With the param left at its 0 "auto" default, the floor is 2·d — the
        // faceting-noise floor that keeps sub-2·d splat beads out.
        let mut p = wall_params();
        p.gap_fill_min_length_mm = 0.0;
        p.nozzle_diameter_mm = 0.4;
        assert!((gap_fill_min_run_len_mm(&p) - 0.8).abs() < 1e-9);

        // An explicit value overrides the auto default verbatim.
        p.gap_fill_min_length_mm = 0.3;
        assert!((gap_fill_min_run_len_mm(&p) - 0.3).abs() < 1e-9);
    }

    #[test]
    fn short_residual_gap_yields_no_splat_bead() {
        // A 0.6 mm-long, ~0.5 mm-thick residual pocket is below the 2·d = 0.8 mm
        // auto floor, so it must NOT emit an isolated gap-fill "splat" bead.
        let mut layer = SliceLayer::new(0.2);
        // A short thin rib: 0.6 mm long (x), 0.5 mm thick (y).
        let rib: Path = vec![(-0.3, -0.25), (0.3, -0.25), (0.3, 0.25), (-0.3, 0.25)].into();
        layer.paths.push(rib);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.path_widths.push(None);

        generate_arachne_walls_for_layer(&mut layer, &wall_params());

        let open_beads = (0..layer.paths.len())
            .filter(|&i| layer.is_path_open(i))
            .count();
        assert_eq!(
            open_beads, 0,
            "a sub-2·d residual must not emit a gap-fill splat bead, got {open_beads}"
        );
    }
}
