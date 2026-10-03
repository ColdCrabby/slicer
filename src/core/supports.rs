//! Support-structure generation.
//!
//! Detects overhangs steeper than [`SlicingParams::support_threshold_angle`],
//! projects the unsupported area downward with horizontal (XY) and vertical (Z)
//! clearance from the model, classifies dense **interface** contact layers, and
//! fills the result with walls and a grid/zig-zag pattern tagged
//! [`ExtrusionRole::Support`].  Both support styles are produced:
//!
//! | Style                    | Load-bearing body                                   | Printed as |
//! |--------------------------|-----------------------------------------------------|------------|
//! | [`SupportType::Normal`]  | the full overhang footprint, straight down          | one contour round each column, sparse grid inside |
//! | [`SupportType::Tree`]    | branches from tips under the overhang to the bed or a top surface | a tube per branch — one wall, two on thick trunks — sparse core |
//!
//! `Tree` lives in [`super::tree_support`]: tips sampled under the contact
//! pads drift together and merge into trunks, steer around the model using
//! precomputed avoidance, and thicken toward the ground.  Both styles share
//! everything here — overhang detection, paint, the dense interface pads that
//! cover the whole overhang at the contact layers, and the fill — so a tree
//! only has to hold the pads up, not cover the overhang itself.
//!
//! # Build-plate-only
//!
//! [`SlicingParams::support_on_build_plate_only`] restricts support to what can
//! descend to the bed through empty space.  For normal support a contact pad is
//! dropped when it overlaps the model's accumulated footprint below it, so the
//! overhang above prints unsupported rather than growing a column that lands on
//! — and scars, or cannot be freed from — the print.  A tree branch can lean
//! around the model, so the tree applies the option per branch instead: a
//! branch that cannot reach the bed is pruned, and a pad no branch holds is not
//! printed.  Losing those overhangs is the point of the option, not a
//! shortcoming of it.
//!
//! # Pipeline placement
//!
//! Runs after infill and before path ordering (TSP) so support paths are
//! ordered alongside the rest of the layer.  It only reads `OuterWall` paths
//! (via [`perimeter_paths_of`]) to derive the model footprint, so it never
//! disturbs wall/surface/infill geometry.

use clipper2::*;

use crate::settings::params::{SlicingParams, SupportType};

use super::support_paint::SupportPaintMasks;
use super::surfaces::{generate_rectilinear_infill, perimeter_paths_of};
use super::types::{ExtrusionRole, SliceLayer};

/// Overhang islands smaller than this (mm²) are ignored — they are slicing
/// noise along near-vertical faceted walls, not genuine unsupported area.
const SUPPORT_MIN_OVERHANG_AREA_MM2: f64 = 1.0;

/// Support-region islands smaller than this (mm²) are dropped after projection —
/// too small to print a stable column.
const SUPPORT_MIN_REGION_AREA_MM2: f64 = 1.0;

/// Tree trunks are thin by design, so a trunk island is kept down to this
/// fraction of [`SUPPORT_MIN_REGION_AREA_MM2`]: dropping a disc the model has
/// clipped would cut the trunk and leave the branch above standing on air.
const TREE_MIN_REGION_AREA_FRACTION: f64 = 0.25;

/// Extra horizontal tolerance (mm) added to the per-layer overhang step so that
/// the tiny facet-to-facet jitter of a near-vertical wall does not register as
/// an overhang.
const OVERHANG_FACET_TOLERANCE_MM: f64 = 0.05;

/// How much wider tree tips are spaced when interface layers sit on them.
///
/// At the plain support pitch a broad flat overhang grew a canopy of thin tubes
/// denser in material than the grid normal support lays there.  Half as far
/// again cut a cap held 20 mm up from 0.92× to 0.64× the filament of normal
/// support, while the interface lines still only span about 3 mm between tips;
/// twice the pitch saved a little more but left them bridging over 4 mm.
const TREE_TIP_PITCH_UNDER_INTERFACE: f64 = 1.5;

/// Tree branches at least this many branch diameters wide get a second wall.
///
/// A branch alone is a tube one bead thick around a sparse core, which is
/// plenty stiff at its own width.  A trunk twice that wide is carrying merged
/// branches, and its one bead is spread round a much longer wall; the second
/// wall is where it pays for itself.  Doubling every branch past a fixed
/// 3 mm instead put a second wall on nearly every branch a few millimetres
/// below its tip, roughly doubling the material in each trunk.
const TREE_DOUBLE_WALL_BRANCH_DIAMETERS: f64 = 2.0;

/// Minimum length of an emitted support run, as a multiple of the nozzle
/// diameter.  Mirrors the gap-fill splat filter: a run below this is an
/// isolated dab that still costs a full retract → travel → un-retract to
/// reach, and supports nothing at that size.  Tree clipping in particular
/// leaves sub-bead trunk fragments against the model boundary.
const SUPPORT_MIN_RUN_LEN_NOZZLE_MULT: f64 = 2.0;

/// Total drawn length of a path, counting the closing segment when it is a
/// closed loop.
fn path_run_len(path: &Path, closed: bool) -> f64 {
    let pts: Vec<(f64, f64)> = path.iter().map(|p| (p.x(), p.y())).collect();
    if pts.len() < 2 {
        return 0.0;
    }
    let mut total = 0.0;
    for w in pts.windows(2) {
        total += (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
    }
    if closed {
        let (a, b) = (pts[pts.len() - 1], pts[0]);
        total += (b.0 - a.0).hypot(b.1 - a.1);
    }
    total
}

/// ── Clipper2 helpers with empty-input guards ──────────────────────────────
///
/// Clipper2 boolean ops on an empty operand can throw; these wrappers keep the
/// accumulation loop total and readable.
pub(super) fn poly_union(a: &Paths, b: &Paths) -> Paths {
    if a.is_empty() {
        return b.clone();
    }
    if b.is_empty() {
        return a.clone();
    }
    union(a.clone(), b.clone(), FillRule::NonZero).unwrap_or_else(|_| a.clone())
}

pub(super) fn poly_difference(a: &Paths, b: &Paths) -> Paths {
    if a.is_empty() || b.is_empty() {
        return a.clone();
    }
    difference(a.clone(), b.clone(), FillRule::NonZero).unwrap_or_else(|_| a.clone())
}

pub(super) fn poly_intersect(a: &Paths, b: &Paths) -> Paths {
    if a.is_empty() || b.is_empty() {
        return Paths::new(vec![]);
    }
    intersect(a.clone(), b.clone(), FillRule::NonZero).unwrap_or_default()
}

pub(super) fn poly_inflate(a: &Paths, delta: f64) -> Paths {
    if a.is_empty() || delta.abs() < 1e-9 {
        return a.clone();
    }
    inflate(a.clone(), delta, JoinType::Round, EndType::Polygon, 2.0)
}

fn filter_small(paths: &Paths, min_area_mm2: f64) -> Paths {
    if min_area_mm2 <= 0.0 || paths.is_empty() {
        return paths.clone();
    }
    Paths::new(
        paths
            .iter()
            .filter(|p| p.signed_area().abs() >= min_area_mm2)
            .cloned()
            .collect(),
    )
}

/// Morphological close: dilate by `r`, then erode by `r`.
///
/// Welds sub-`r` gaps between neighbouring sub-paths shut while leaving the
/// outer boundary where it was.  Note it cannot *widen* an isolated feature —
/// only bridge the space between two of them.
fn morphological_close(paths: &Paths, r: f64) -> Paths {
    if paths.is_empty() || r <= 1e-9 {
        return paths.clone();
    }
    let grown = poly_inflate(paths, r);
    if grown.is_empty() {
        return paths.clone();
    }
    let shrunk = poly_inflate(&grown, -r);
    if shrunk.is_empty() {
        paths.clone()
    } else {
        shrunk
    }
}

/// The part of one layer's support region that can actually be printed.
///
/// Two rules, both about the support itself rather than the overhang it holds:
///
/// - **Nothing narrower than a bead** — an opening by half a bead erases it.
///   A strip that thin cannot hold a bead by construction, and the island
///   contour inset half a pitch from its edge breaks it into specks.
/// - **No island below `min_area` of net material** — one too small to stand
///   as a column, or to hold a loop around any fill, supports nothing yet costs
///   a full retract → travel → un-retract to reach.
///
/// Together they remove the support that crept into places it cannot work: a
/// recess too narrow to fit a column between its walls once the XY clearance is
/// taken off both sides, or the hairline the clearance leaves where a column
/// grazes the model — on a Benchy, a sliver up the hull on every other layer.
fn printable_support(region: &Paths, bead: f64, min_area: f64) -> Paths {
    if region.is_empty() {
        return region.clone();
    }
    let r = bead * 0.5;
    let eroded = poly_inflate(region, -r);
    if eroded.is_empty() {
        return eroded;
    }
    let opened = poly_inflate(&eroded, r);
    // The opening can round a corner past the region it came from; never print
    // outside what the column and the clearance allowed.
    let opened = poly_intersect(&opened, region);
    super::infill::drop_small_islands(opened, min_area)
}

/// Accumulate the per-layer contacts top-down into the region that needs
/// support at each layer, **before** the XY clearance is taken out.
///
/// A contact is only the *newly* exposed sliver at its layer
/// (`footprint[i] − inflate(footprint[i−1], max_step)`), so down a continuous
/// slope successive contacts are concentric rings separated by exactly
/// `max_step`.  Accumulated verbatim they never touch: a 60° frustum produced
/// 94 sub-paths about 0.1 mm wide — hairlines the fill scanline then discarded,
/// which is why a plain 60° overhang came out with essentially no support at
/// any threshold.
///
/// Closing the accumulation by just over half that gap welds the rings into the
/// solid annulus between the model and the widest overhang above it, which is
/// what the support body physically is.  The close only bridges *between*
/// rings; it never pushes the outer boundary out, so the supported area is
/// unchanged — only its connectivity is.
///
/// The model's own cross-section (`solid`, the layer's material out to its
/// surface) is taken out of the accumulation at every layer on the way down: a
/// column that comes down onto the model stands on it and stops there.  Carried
/// past it, the column fell straight through solid material and resurfaced in
/// whatever opened up below — a cabin roof's support reappeared as a speck in
/// each letter sunk into the underside of a Benchy's keel, 36 mm further down.
fn accumulate_support_area(
    add_at: &[Paths],
    solid: &[Paths],
    n: usize,
    close_r: f64,
) -> Vec<Paths> {
    let mut acc = Paths::new(vec![]);
    let mut out = vec![Paths::new(vec![]); n];
    for i in (0..n).rev() {
        if !add_at[i].is_empty() {
            acc = poly_union(&acc, &add_at[i]);
            acc = morphological_close(&acc, close_r);
        }
        if !acc.is_empty() {
            acc = poly_difference(&acc, &solid[i]);
        }
        out[i] = acc.clone();
    }
    out
}

/// Generate support structures for all layers, appending
/// [`ExtrusionRole::Support`] paths in place.
///
/// `pristine` is each layer's **un-split** `OuterWall` centreline outline,
/// snapshotted by the pipeline before surface generation and overhang
/// classification.  It is not an optimisation: `classify_overhang_perimeters`
/// retags an overhanging wall as [`ExtrusionRole::OverhangPerimeter`] and
/// splits the loop into open arcs, so by the time supports run a steep slope
/// has **no** `OuterWall` paths left to read.  Deriving the footprint from the
/// mutated layer therefore saw nothing on exactly the models that need support
/// most — a 60° frustum reported 49 of 50 footprints empty and got no support
/// at any threshold.  Pass `None` only when the layers have not been through
/// that pass (unit tests build them directly).
///
/// A no-op when `support_enabled` is false or the model has fewer than two
/// layers (nothing can overhang the bed on the first layer).
pub fn generate_supports(
    layers: &mut [SliceLayer],
    params: &SlicingParams,
    pristine: Option<&[Paths]>,
) {
    generate_supports_with_paint(layers, params, pristine, &SupportPaintMasks::default());
}

/// Generate support, honouring per-facet paint.
///
/// Identical to [`generate_supports`] but for two extra terms in the overhang
/// step:
///
/// ```text
/// auto      = footprint[i] − inflate(footprint[i−1], max_step)   // as before
/// auto      = auto − blocker[i]
/// enforced  = (footprint[i] ∩ enforcer[i]) − inflate(footprint[i−1], facet_tol)
/// enforced  = enforced − blocker[i]
/// overhang  = auto ∪ enforced
/// ```
///
/// An enforcer is measured against the layer below with only the facet-noise
/// tolerance, **not** the threshold step: it asks for support however gentle
/// the overhang, so a slope the angle rule passes over gets support where it is
/// painted — with automatic detection on or off.  Gating it on `max_step` as
/// well made it a strict subset of what detection already found, so painting an
/// enforcer added nothing anywhere the rule had not.
///
/// Everything downstream — accumulation, the tree simulation, interface caps,
/// clearance, fill — is untouched, because by that point painted and detected
/// overhangs are the same kind of thing.
///
/// Empty masks reproduce [`generate_supports`] exactly.
pub fn generate_supports_with_paint(
    layers: &mut [SliceLayer],
    params: &SlicingParams,
    pristine: Option<&[Paths]>,
    paint: &SupportPaintMasks,
) {
    if !params.support_enabled {
        return;
    }
    let n = layers.len();
    if n < 2 {
        return;
    }

    let lh = params.layer_height.max(1e-4);
    let ext_w = params.nozzle_diameter_mm.max(0.1);

    // Maximum horizontal shift, per layer, that a wall can advance without
    // needing support.  `support_threshold_angle` is measured from vertical:
    // 45° → shift = layer_height (the classic 45° rule); a smaller angle
    // triggers support on gentler overhangs.
    let angle = params.support_threshold_angle.clamp(0.0, 89.0);
    let max_step = lh * angle.to_radians().tan() + OVERHANG_FACET_TOLERANCE_MM;

    // ── 1. Model footprint per layer (union of OuterWall contours) ──────────
    let footprints: Vec<Paths> = (0..n)
        .map(|i| {
            let outer = match pristine {
                Some(snapshot) if i < snapshot.len() => snapshot[i].clone(),
                _ => perimeter_paths_of(&layers[i]),
            };
            if outer.is_empty() {
                Paths::new(vec![])
            } else {
                union(outer, Paths::new(vec![]), FillRule::NonZero).unwrap_or_default()
            }
        })
        .collect();

    // ── 2. Overhang region per layer ───────────────────────────────────────
    // The part of layer i not covered by layer i-1 grown outward by `max_step`,
    // then reconciled with whatever the user painted.
    //
    // Blockers are widened by half a bead before subtraction: a painted region
    // and a detected overhang have independently-derived boundaries, and
    // subtracting one from the other leaves a sliver of support along the seam
    // otherwise.
    let blocker_grow = crate::core::outer_wall_nominal_width_mm(params) * 0.5;
    let painted = !paint.is_empty();
    // `support_auto` off means the overhang rule is skipped entirely and
    // support comes only from paint.
    let auto_detect = params.support_auto;

    let mut overhang: Vec<Paths> = vec![Paths::new(vec![]); n];
    // The enforced contribution alone, tracked in parallel with `overhang` so
    // build-plate-only (3b below) can restore it after filtering: a painted
    // enforcer is an instruction to support *here*, even by resting on the
    // model instead of the plate.
    let mut enforced_overhang: Vec<Paths> = vec![Paths::new(vec![]); n];
    for i in 1..n {
        if footprints[i].is_empty() {
            continue;
        }
        let grown_prev = poly_inflate(&footprints[i - 1], max_step);

        let mut detected = if auto_detect {
            let raw = poly_difference(&footprints[i], &grown_prev);
            // The noise filter belongs to *detection* only. A painted enforcer
            // is an explicit instruction, however small the facet — passing it
            // through this would silently ignore fine paint and read as the
            // brush not working.
            filter_small(&raw, SUPPORT_MIN_OVERHANG_AREA_MM2)
        } else {
            Paths::new(vec![])
        };

        if painted {
            let blocker = paint.blocker_at(i);
            if !blocker.is_empty() && !detected.is_empty() {
                detected = poly_difference(&detected, &poly_inflate(&blocker, blocker_grow));
            }

            let enforcer = paint.enforcer_at(i);
            if !enforcer.is_empty() {
                // Clip to the model's own cross-section: paint on a surface
                // that is not actually exposed at this height describes no
                // overhang, and support hanging in free air beside the part
                // helps nobody.
                let mut enforced = poly_intersect(&footprints[i], &enforcer);
                // Material resting on the layer below needs nothing added under
                // it — but *any* outward step counts, not only one past the
                // threshold. The facet tolerance is still taken off, so a
                // painted vertical wall does not sprout slivers from facet
                // jitter; down a slope the rings it leaves are that tolerance
                // apart, which the accumulation's close welds shut.
                enforced = poly_difference(
                    &enforced,
                    &poly_inflate(&footprints[i - 1], OVERHANG_FACET_TOLERANCE_MM),
                );
                // A blocker wins where the two overlap, so a broad enforcer can
                // be trimmed with a few strokes rather than repainted.
                if !blocker.is_empty() {
                    enforced = poly_difference(&enforced, &poly_inflate(&blocker, blocker_grow));
                }
                detected = poly_union(&detected, &enforced);
                enforced_overhang[i] = enforced;
            }
        }

        overhang[i] = detected;
    }

    // ── 3. Register each overhang at its top-contact (activation) layer ─────
    // An overhang at layer i is contacted `z_gap` layers below it, leaving an
    // air gap for clean removal.
    let z_gap = params.support_z_gap_layers;
    // The clearance is measured from the model's **surface**, but `footprints`
    // are outer-wall bead *centrelines*, which sit half a bead inside it. So
    // inflating by `support_xy_distance_mm` alone leaves only
    // `xy − half_bead` of real air — 0.6 mm of a requested 0.8 mm at defaults.
    // Add the half bead back so the number in the settings is the gap the user
    // actually gets.
    let xy = params.support_xy_distance_mm.max(0.0)
        + crate::core::outer_wall_nominal_width_mm(params) * 0.5;
    let mut add_at: Vec<Paths> = vec![Paths::new(vec![]); n];
    // The enforced share of `add_at`, registered at the same activation layer
    // — see the build-plate-only bypass in 3b below.
    let mut enforced_add_at: Vec<Paths> = vec![Paths::new(vec![]); n];
    #[allow(clippy::needless_range_loop)]
    for i in 1..n {
        if overhang[i].is_empty() {
            continue;
        }
        // An overhang nearer the bed than the gap leaves no layer to stand
        // support on. Clamping it onto layer 0 instead printed a speck straight
        // under it with no gap at all — one in each shallow recess of a part's
        // underside.
        let Some(activate) = i.checked_sub(1 + z_gap) else {
            continue;
        };
        add_at[activate] = poly_union(&add_at[activate], &overhang[i]);
        if !enforced_overhang[i].is_empty() {
            enforced_add_at[activate] =
                poly_union(&enforced_add_at[activate], &enforced_overhang[i]);
        }
    }

    // ── 3b. Build-plate-only: drop contacts that cannot reach the bed ───────
    //
    // `covered[i]` is the model's accumulated footprint *strictly below* layer
    // `i`, grown by the same XY clearance the descending column is clipped
    // with.  Growing it matters: a pad that merely clears the model by less
    // than `xy` would survive an un-grown test and then be eaten away during
    // the descent, leaving a floating stub instead of a column.
    //
    // Anything overlapping that region would land on the print rather than the
    // plate, so with `support_on_build_plate_only` it is removed outright and
    // the overhang above it prints unsupported — the trade this option exists
    // to make.  The vector is only built when the option is on.
    //
    // Tree support is exempt: a branch can lean around the model to reach the
    // plate from a contact a straight column could not, so the tree applies the
    // option itself, branch by branch.
    let is_tree = params.support_type == SupportType::Tree;
    let plate_only = params.support_on_build_plate_only && !is_tree;
    let covered: Vec<Paths> = if plate_only {
        let mut acc = Paths::new(vec![]);
        let mut out = Vec::with_capacity(n);
        for fp in footprints.iter() {
            out.push(acc.clone());
            acc = poly_union(&acc, &poly_inflate(fp, xy));
        }
        out
    } else {
        Vec::new()
    };

    if plate_only {
        for i in 0..n {
            if add_at[i].is_empty() {
                continue;
            }
            let reachable = poly_difference(&add_at[i], &covered[i]);
            let mut kept = filter_small(&reachable, SUPPORT_MIN_OVERHANG_AREA_MM2);
            // A painted enforcer asked for support at this exact place; unlike
            // an auto-detected overhang, it is not sacrificed just because the
            // column would have to rest on the model instead of the plate. The
            // column-projection step below still stops at the model surface
            // either way, so this cannot punch through geometry, only rest on
            // it.
            if !enforced_add_at[i].is_empty() {
                kept = poly_union(&kept, &enforced_add_at[i]);
            }
            add_at[i] = kept;
        }
    }

    // ── 4. Build the load-bearing column per layer (model already subtracted) ─
    //
    // Two strategies produce a per-layer `columns[i]` region:
    //   • Normal — the full overhang footprint projected straight down (a grid
    //     column), subtracting the model + XY clearance each layer.
    //   • Tree — branches grown from tips under the contact pads down to the bed
    //     or the model ([`super::tree_support`]).  Dense interface pads (added
    //     below) still cover the full overhang at the contact layers, so the
    //     branches only have to hold the pads up.
    let iface = params.support_interface_layers;

    // Weld the per-layer contact rings into a printable body before either
    // style consumes them.  Half the inter-ring gap is the minimum that closes
    // it; 0.6 leaves margin for the round-join approximation, and the nozzle
    // floor covers a near-vertical threshold where `max_step` is tiny.
    let close_r = (max_step * 0.6).max(ext_w * 0.5);
    // The model out to its surface: footprints are wall centrelines, half a
    // bead inside it.
    let half_bead = crate::core::outer_wall_nominal_width_mm(params) * 0.5;
    let solid: Vec<Paths> = footprints
        .iter()
        .map(|fp| poly_inflate(fp, half_bead))
        .collect();
    let mut support_area = accumulate_support_area(&add_at, &solid, n, close_r);

    // A blocker forbids support *at that place*, not merely at the contact
    // above it — otherwise a column seeded elsewhere descends straight through
    // a region the user painted to keep clear, which is exactly the cavity and
    // cosmetic-face case blockers exist for.  Applied after accumulation so it
    // cuts the descending body as well as the contact.
    if painted {
        for (i, area) in support_area.iter_mut().enumerate() {
            if area.is_empty() {
                continue;
            }
            let blocker = paint.blocker_at(i);
            if !blocker.is_empty() {
                *area = poly_difference(area, &poly_inflate(&blocker, blocker_grow));
            }
        }
    }

    // The support area that first appears at each layer — the welded equivalent
    // of `add_at`.  Tree seeds its contact tips here and the top interface caps
    // are cut from it, so neither works off the raw hairline rings.
    let new_area: Vec<Paths> = (0..n)
        .map(|i| {
            if i + 1 < n {
                poly_difference(&support_area[i], &support_area[i + 1])
            } else {
                support_area[i].clone()
            }
        })
        .collect();

    // The pads printed as top interface: every contact pad for normal support,
    // only the ones a branch actually holds up for tree support.
    let (columns, contact_pads): (Vec<Paths>, Vec<Paths>) = if is_tree {
        let z: Vec<f64> = layers.iter().map(|l| l.z).collect();
        let obstacles: Vec<Paths> = if painted {
            (0..n)
                .map(|i| poly_inflate(&paint.blocker_at(i), blocker_grow))
                .collect()
        } else {
            vec![Paths::new(vec![]); n]
        };
        let tree = super::tree_support::generate(
            &super::tree_support::TreeInput {
                z: &z,
                footprints: &footprints,
                solid: &solid,
                contacts: &new_area,
                enforced: &enforced_add_at,
                obstacles: &obstacles,
            },
            &tree_settings(params, xy, z_gap),
        );
        (tree.branches, tree.supported_contacts)
    } else {
        (
            project_normal_columns(&support_area, &footprints, &covered, n, xy),
            new_area,
        )
    };

    // Per-layer printed regions, split into interface (dense) and body.  Every
    // layer is independent from here on, so both passes run a layer per task.
    let bead = crate::core::support_nominal_width_mm(params);
    let min_island_area = if is_tree {
        SUPPORT_MIN_REGION_AREA_MM2 * TREE_MIN_REGION_AREA_FRACTION
    } else {
        SUPPORT_MIN_REGION_AREA_MM2
    };

    let regions: Vec<(Paths, Paths)> = per_layer(n, |i| {
        let empty = || (Paths::new(vec![]), Paths::new(vec![]));
        let column = &columns[i];

        // Horizontal clearance frame for this layer (model + XY distance).
        let clip = poly_inflate(&footprints[i], xy);

        // Top interface: the welded contact pad(s) whose band covers this layer
        // — [i, i+iface-1] — giving a dense, flat surface under the overhang
        // regardless of how thin the load-bearing trunk is.  Cut from
        // `new_area` rather than the raw contacts, so a sloped overhang gets a
        // real cap instead of a hairline ring.
        let mut top_full = Paths::new(vec![]);
        if iface > 0 {
            for pad in contact_pads.iter().take((i + iface).min(n)).skip(i) {
                if !pad.is_empty() {
                    top_full = poly_union(&top_full, pad);
                }
            }
        }
        let top_if = poly_difference(&top_full, &clip);

        if column.is_empty() && top_if.is_empty() {
            return empty();
        }

        // The printed footprint is the union of the load-bearing column and the
        // (possibly wider) top contact pad — less whatever cannot be printed.
        let total = printable_support(&poly_union(column, &top_if), bead, min_island_area);
        if total.is_empty() {
            return empty();
        }
        let top_if = poly_intersect(&top_if, &total);

        // Bottom interface: where the column rests on model within `iface`
        // layers below (contact that must detach cleanly).
        let mut below = Paths::new(vec![]);
        if iface > 0 {
            let lo = i.saturating_sub(iface);
            for f in footprints.iter().take(i).skip(lo) {
                below = poly_union(&below, f);
            }
        }
        let bot_if = poly_intersect(&total, &below);

        let iface_region = poly_union(&top_if, &bot_if);
        let body_region = poly_difference(&total, &iface_region);
        (iface_region, body_region)
    });

    // ── 5. Fill each layer's support regions and append Support paths ───────
    let body_dens = params.support_density.clamp(0.02, 1.0);
    let iface_dens = params.support_interface_density.clamp(0.05, 1.0);
    // Density is expressed against the flow *spacing*, not the nominal bead
    // width — the same identity the infill and surface fills obey. Pitching on
    // the raw nozzle diameter while the generator charges each line at its
    // spacing is what makes a requested density come out wrong on any nozzle
    // but the reference one.
    let fill_spacing = crate::core::extrusion_flow_spacing_mm(
        crate::core::support_nominal_width_mm(params),
        params.layer_height,
    );
    let body_spacing = fill_spacing / body_dens;
    let iface_spacing = fill_spacing / iface_dens;
    // A tree branch is a tube: one wall, or two once it is thick enough to
    // carry real load, around a sparse core.  Normal support keeps a single
    // contour around each column.
    let double_wall_area = if is_tree {
        let d = params.support_tree_branch_diameter.max(bead) * TREE_DOUBLE_WALL_BRANCH_DIAMETERS;
        std::f64::consts::FRAC_PI_4 * d * d
    } else {
        f64::INFINITY
    };
    let min_len = params.min_infill_extrusion_mm;
    let min_run = ext_w * SUPPORT_MIN_RUN_LEN_NOZZLE_MULT;

    let strands: Vec<(Vec<Path>, Vec<Path>)> = per_layer(n, |i| {
        let (iface_r, body) = &regions[i];
        if body.is_empty() && iface_r.is_empty() {
            return (Vec::new(), Vec::new());
        }

        // Alternate direction each layer for inter-layer bonding.
        let even = i % 2 == 0;
        let body_angle = if even { 0.0 } else { 90.0 };
        let iface_angle = if even { 45.0 } else { 135.0 };

        let mut loops: Vec<Path> = Vec::new();
        let mut fills: Vec<Path> = Vec::new();

        // One contour around each support island, then the fill inside it.
        //
        // Without a contour the scanline is the only thing drawn, so any island
        // narrower than a couple of line pitches degenerates into a row of
        // disconnected dashes — each one paying a full retract → travel →
        // un-retract to deposit a speck. A thin tree branch came out as one or
        // two sub-bead specks per layer that way; on a Benchy 93.8 % of tree
        // support segments were under 2 mm. A loop turns each island into one
        // continuous extrusion and gives the fill something to tie into.
        let printed = poly_union(body, iface_r);
        let (walls, inner) = support_walls(&printed, fill_spacing, double_wall_area);

        // An island thinner than one bead cannot hold a contour; fall back to
        // filling it directly rather than dropping it.
        let (body_fill, iface_fill) = if walls.is_empty() {
            (body.clone(), iface_r.clone())
        } else {
            loops.extend(walls);
            if inner.is_empty() {
                (Paths::new(vec![]), Paths::new(vec![]))
            } else {
                (
                    poly_intersect(&inner, body),
                    poly_intersect(&inner, iface_r),
                )
            }
        };

        if !body_fill.is_empty() {
            fills.extend(support_fill(
                &body_fill,
                body_spacing,
                body_angle,
                min_len,
                is_tree,
            ));
        }
        if !iface_fill.is_empty() {
            fills.extend(support_fill(
                &iface_fill,
                iface_spacing,
                iface_angle,
                min_len,
                is_tree,
            ));
        }

        loops.retain(|path| path_run_len(path, true) >= min_run);
        fills.retain(|path| path_run_len(path, false) >= min_run);
        (loops, fills)
    });

    for (layer, (loops, fills)) in layers.iter_mut().zip(strands) {
        for path in loops {
            push_support_path(layer, path, false);
        }
        for path in fills {
            push_support_path(layer, path, true);
        }
    }
}

/// Map `f` over every layer index, in order — one layer per task where the
/// platform has threads, one after another where it does not (the browser
/// build).
pub(super) fn per_layer<T: Send>(n: usize, f: impl Fn(usize) -> T + Sync + Send) -> Vec<T> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        (0..n).into_par_iter().map(f).collect()
    }
    #[cfg(target_arch = "wasm32")]
    {
        (0..n).map(f).collect()
    }
}

/// The tree's shape, from the settings.  Out-of-range values are clamped
/// rather than rejected: a tip wider than the branch, or a preferred lean past
/// the steepest one, has an obvious intended meaning.
fn tree_settings(
    params: &SlicingParams,
    xy: f64,
    z_gap: usize,
) -> super::tree_support::TreeSettings {
    let bead = crate::core::support_nominal_width_mm(params);
    let max_angle = params.support_tree_branch_angle.clamp(5.0, 80.0);
    let preferred = params.support_tree_preferred_angle.clamp(0.0, max_angle);
    let tip_radius = (params.support_tree_tip_diameter * 0.5).max(bead * 0.5);
    let branch_radius = (params.support_tree_branch_diameter * 0.5).max(tip_radius);
    let density = params.support_density.clamp(0.02, 1.0);
    super::tree_support::TreeSettings {
        max_angle: max_angle.to_radians(),
        preferred_angle: preferred.to_radians(),
        tip_radius,
        branch_radius,
        growth_per_mm: params
            .support_tree_branch_diameter_angle
            .clamp(0.0, 45.0)
            .to_radians()
            .tan(),
        xy,
        z_gap,
        tip_spacing: tip_pitch(params, bead, density, tip_radius),
        rest_on_model: !params.support_on_build_plate_only,
    }
}

/// Fill strands for one support region.
///
/// A tree layer is dozens of separate branch cross-sections, and the
/// serpentine fill joins the end of one scan line to the next by sorted order
/// across the whole region — up to two pitches away, which at a sparse support
/// pitch is over 5 mm.  On a branch layer that joined neighbouring branches
/// with a strand extruded across the air between them.  Filling each branch on
/// its own keeps every strand inside the branch it belongs to.
fn support_fill(
    region: &Paths,
    spacing: f64,
    angle: f64,
    min_len: f64,
    per_island: bool,
) -> Vec<Path> {
    if !per_island {
        return generate_rectilinear_infill(region, spacing, angle, min_len)
            .iter()
            .cloned()
            .collect();
    }
    let mut out = Vec::new();
    for island in super::infill::group_islands(region) {
        let mut paths = vec![island.0];
        paths.extend(island.1);
        out.extend(
            generate_rectilinear_infill(&Paths::new(paths), spacing, angle, min_len)
                .iter()
                .cloned(),
        );
    }
    out
}

/// Distance between tree tips under an overhang.
///
/// The pitch normal support lays its lines at, so a density means the same
/// thing in both styles — and never so tight that tips touch.  Under dense
/// interface layers the tips spread to [`TREE_TIP_PITCH_UNDER_INTERFACE`] times
/// that: the interface lines span the gaps between tips, so the extra tips
/// only add branches to a canopy that is already the costliest part of a tree.
fn tip_pitch(params: &SlicingParams, bead: f64, density: f64, tip_radius: f64) -> f64 {
    let pitch = (bead / density).max(tip_radius * 2.0 + bead);
    if params.support_interface_layers > 0 {
        pitch * TREE_TIP_PITCH_UNDER_INTERFACE
    } else {
        pitch
    }
}

/// The wall loops around each support island, and the region left inside
/// them for the fill.
///
/// Every island gets one loop half a pitch in from its edge.  An island with at
/// least `double_wall_area` of material gets a second loop a pitch further in,
/// and its fill shrinks to match.  No loops come back when nothing is wide
/// enough to hold one.
fn support_walls(printed: &Paths, spacing: f64, double_wall_area: f64) -> (Vec<Path>, Paths) {
    let half = spacing * 0.5;
    let first = poly_inflate(printed, -half);
    if first.is_empty() {
        return (Vec::new(), Paths::new(vec![]));
    }
    let mut loops: Vec<Path> = first.iter().cloned().collect();
    let mut inner = poly_inflate(printed, -spacing);
    if double_wall_area.is_finite() {
        let mut thick: Vec<Path> = Vec::new();
        for island in super::infill::group_islands(printed) {
            if super::infill::island_net_area(&island) >= double_wall_area {
                thick.push(island.0);
                thick.extend(island.1);
            }
        }
        if !thick.is_empty() {
            let thick = Paths::new(thick);
            loops.extend(poly_inflate(&thick, -(half + spacing)).iter().cloned());
            inner = poly_union(
                &poly_difference(&inner, &thick),
                &poly_inflate(&thick, -2.0 * spacing),
            );
        }
    }
    (loops, inner)
}

/// Project the accumulated support area straight down (classic grid support).
///
/// Returns a per-layer load-bearing column with the model + XY clearance already
/// subtracted.  `support_area[i]` is the welded region from
/// [`accumulate_support_area`], clipped here by `inflate(footprint, xy)`.
///
/// `covered` is the build-plate-only mask (empty when the option is off): the
/// accumulated model footprint below each layer.  Contacts are already filtered
/// against it, and a straight-down column cannot wander, so subtracting it here
/// is belt-and-braces — it makes "no support ever rests on the model" hold by
/// construction rather than by argument.
fn project_normal_columns(
    support_area: &[Paths],
    footprints: &[Paths],
    covered: &[Paths],
    n: usize,
    xy: f64,
) -> Vec<Paths> {
    let mut out = vec![Paths::new(vec![]); n];
    for i in 0..n {
        if support_area[i].is_empty() {
            continue;
        }
        let clip = poly_inflate(&footprints[i], xy);
        let mut column = poly_difference(&support_area[i], &clip);
        if let Some(below) = covered.get(i) {
            column = poly_difference(&column, below);
        }
        out[i] = filter_small(&column, SUPPORT_MIN_REGION_AREA_MM2);
    }
    out
}

/// Append a single support path to `layer`, keeping the parallel per-path
/// vectors aligned.
///
/// `open` distinguishes a fill strand (open polyline) from an island contour
/// (closed loop, which the generator closes back to its first vertex).
///
/// Support carries **no** explicit width: an explicit width short-circuits the
/// generator's fill-role branch in `resolve_width_mm`, which is what charges a
/// support line the volume of the strip it fills rather than a full nominal
/// bead. Leaving it `None` keeps the pitch and the flow deriving from the same
/// nominal width.
fn push_support_path(layer: &mut SliceLayer, path: Path, open: bool) {
    let target = layer.paths.len();
    // Pad any parallel vectors that lagged behind (earlier stages such as
    // infill only push `paths` + `path_roles`); pad with each accessor's own
    // default so a short vector cannot silently relabel an earlier path.
    while layer.path_roles.len() < target {
        layer.path_roles.push(ExtrusionRole::default());
    }
    while layer.path_widths.len() < target {
        layer.path_widths.push(None);
    }
    while layer.path_vertex_widths.len() < target {
        layer.path_vertex_widths.push(None);
    }
    while layer.path_is_open.len() < target {
        layer.path_is_open.push(false);
    }

    layer.paths.push(path);
    layer.path_roles.push(ExtrusionRole::Support);
    layer.path_widths.push(None);
    layer.path_vertex_widths.push(None);
    layer.path_is_open.push(open);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::ExtrusionRole;
    use clipper2::Path;

    /// Build a square OuterWall contour centered at (cx, cy) with the given
    /// half-size, as a single-island layer at height `z`.
    fn square_layer(z: f64, cx: f64, cy: f64, half: f64) -> SliceLayer {
        let mut layer = SliceLayer::new(z);
        let mut p = Path::new(vec![]);
        p.push(Point::new(cx - half, cy - half));
        p.push(Point::new(cx + half, cy - half));
        p.push(Point::new(cx + half, cy + half));
        p.push(Point::new(cx - half, cy + half));
        layer.paths.push(p);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer
    }

    fn support_path_count(layer: &SliceLayer) -> usize {
        (0..layer.paths.len())
            .filter(|&i| layer.role_for_path(i) == ExtrusionRole::Support)
            .count()
    }

    /// Total length (mm) of all Support polylines across every layer.
    fn support_total_len(layers: &[SliceLayer]) -> f64 {
        let mut total = 0.0;
        for layer in layers {
            for (i, path) in layer.paths.iter().enumerate() {
                if layer.role_for_path(i) != ExtrusionRole::Support {
                    continue;
                }
                let pts: Vec<(f64, f64)> = path.iter().map(|p| (p.x(), p.y())).collect();
                for w in pts.windows(2) {
                    let dx = w[1].0 - w[0].0;
                    let dy = w[1].1 - w[0].1;
                    total += (dx * dx + dy * dy).sqrt();
                }
            }
        }
        total
    }

    fn params_with_supports() -> SlicingParams {
        SlicingParams {
            support_enabled: true,
            layer_height: 0.2,
            nozzle_diameter_mm: 0.4,
            support_threshold_angle: 45.0,
            ..SlicingParams::default()
        }
    }

    #[test]
    fn disabled_supports_are_a_noop() {
        let mut layers = vec![
            square_layer(0.2, 0.0, 0.0, 5.0),
            square_layer(0.4, 20.0, 0.0, 5.0),
        ];
        let params = SlicingParams {
            support_enabled: false,
            ..params_with_supports()
        };
        generate_supports(&mut layers, &params, None);
        assert_eq!(support_path_count(&layers[0]), 0);
        assert_eq!(support_path_count(&layers[1]), 0);
    }

    #[test]
    fn straight_wall_needs_no_support() {
        // Two identical stacked squares — a vertical wall, no overhang.
        let mut layers = vec![
            square_layer(0.2, 0.0, 0.0, 5.0),
            square_layer(0.4, 0.0, 0.0, 5.0),
        ];
        let params = params_with_supports();
        generate_supports(&mut layers, &params, None);
        assert_eq!(support_path_count(&layers[1]), 0);
        assert_eq!(support_path_count(&layers[0]), 0);
    }

    #[test]
    fn floating_overhang_generates_support_below() {
        // A block that appears in mid-air, laterally offset from the base so it
        // overhangs nothing beneath it — must be supported down to the bed.
        let mut layers = Vec::new();
        // Layers 0..3: base column at origin.
        for k in 0..3 {
            layers.push(square_layer(0.2 * (k as f64 + 1.0), 0.0, 0.0, 4.0));
        }
        // Layers 3..6: a shelf far to the side (its underside is unsupported).
        for k in 3..6 {
            layers.push(square_layer(0.2 * (k as f64 + 1.0), 30.0, 0.0, 6.0));
        }
        let params = params_with_supports();
        generate_supports(&mut layers, &params, None);

        // The overhang shelf sits at layers 3..6; support must be produced on at
        // least one layer below it (with the default 1-layer Z gap).
        let total_support: usize = layers.iter().map(support_path_count).sum();
        assert!(
            total_support > 0,
            "expected support paths under the floating overhang"
        );
        // Support is emitted as closed island contours plus open fill strands,
        // and carries no explicit width override — the generator resolves it to
        // the role's flow spacing so pitch and flow agree.
        let mut closed = 0;
        let mut open = 0;
        for layer in &layers {
            for i in 0..layer.paths.len() {
                if layer.role_for_path(i) == ExtrusionRole::Support {
                    if layer.is_path_open(i) {
                        open += 1;
                    } else {
                        closed += 1;
                    }
                    assert!(
                        layer.path_widths.get(i).copied().flatten().is_none(),
                        "support must not pin an explicit width"
                    );
                }
            }
        }
        assert!(
            closed > 0,
            "each support island should get a perimeter contour"
        );
        assert!(open > 0, "support islands should also be filled");
    }

    #[test]
    fn tree_and_normal_both_produce_support() {
        let build = |ty: SupportType| {
            let mut layers = Vec::new();
            for k in 0..3 {
                layers.push(square_layer(0.2 * (k as f64 + 1.0), 0.0, 0.0, 3.0));
            }
            for k in 3..7 {
                layers.push(square_layer(0.2 * (k as f64 + 1.0), 25.0, 0.0, 8.0));
            }
            let params = SlicingParams {
                support_type: ty,
                ..params_with_supports()
            };
            generate_supports(&mut layers, &params, None);
            layers.iter().map(support_path_count).sum::<usize>()
        };
        assert!(build(SupportType::Normal) > 0, "normal support expected");
        assert!(build(SupportType::Tree) > 0, "tree support expected");
    }

    #[test]
    fn tree_uses_materially_less_filament_than_normal() {
        // A small cap on a thin post 30 mm up.  Normal fills the whole
        // underside with a grid column all the way down; tree branches gather
        // into a few trunks and must use materially less filament.
        //
        // The height is the point: trees pay off for overhangs well above the
        // bed.  Under a wide plate only a few millimetres up the branches have
        // no room to gather, and separate trunks cost about what a sparse grid
        // does.
        let build = |ty: SupportType| {
            let mut layers = Vec::new();
            for k in 0..150 {
                layers.push(square_layer(0.2 * (k as f64 + 1.0), 0.0, 0.0, 1.0));
            }
            for k in 150..152 {
                layers.push(square_layer(0.2 * (k as f64 + 1.0), 0.0, 0.0, 4.0));
            }
            let params = SlicingParams {
                support_type: ty,
                ..params_with_supports()
            };
            generate_supports(&mut layers, &params, None);
            support_total_len(&layers)
        };
        let normal = build(SupportType::Normal);
        let tree = build(SupportType::Tree);
        assert!(
            normal > 0.0 && tree > 0.0,
            "both styles must produce support (normal={normal:.0}mm, tree={tree:.0}mm)"
        );
        assert!(
            tree < normal * 0.9,
            "tree must use materially less filament than normal \
             (tree={tree:.0}mm, normal={normal:.0}mm)"
        );
    }

    /// Stack a base block, a thin tower on it, and a wide cap on the tower.
    /// The cap's underside overhangs in every direction: the part reaching past
    /// the base can drop to the bed, the part directly above the base cannot.
    fn tiered_stack(base_half: f64, cap_half: f64) -> Vec<SliceLayer> {
        let mut layers = Vec::new();
        for k in 0..10 {
            layers.push(square_layer(0.2 * (k as f64 + 1.0), -20.0, 0.0, base_half));
        }
        for k in 10..30 {
            layers.push(square_layer(0.2 * (k as f64 + 1.0), -20.0, 0.0, 3.0));
        }
        for k in 30..32 {
            layers.push(square_layer(0.2 * (k as f64 + 1.0), -20.0, 0.0, cap_half));
        }
        layers
    }

    /// Count support vertices falling inside an axis-aligned XY box, at any Z.
    fn support_vertices_in_box(layers: &[SliceLayer], half: f64) -> usize {
        let mut n = 0;
        for layer in layers {
            for (i, path) in layer.paths.iter().enumerate() {
                if layer.role_for_path(i) != ExtrusionRole::Support {
                    continue;
                }
                for p in path.iter() {
                    if (p.x() - -20.0).abs() < half && p.y().abs() < half {
                        n += 1;
                    }
                }
            }
        }
        n
    }

    #[test]
    fn build_plate_only_keeps_reachable_support_and_drops_the_rest() {
        let build = |plate_only: bool| {
            let mut layers = tiered_stack(8.0, 20.0);
            let params = SlicingParams {
                support_on_build_plate_only: plate_only,
                ..params_with_supports()
            };
            generate_supports(&mut layers, &params, None);
            layers
        };

        let anywhere = build(false);
        let plate_only = build(true);
        let len_any = support_total_len(&anywhere);
        let len_plate = support_total_len(&plate_only);

        assert!(len_any > 0.0, "baseline must produce support");
        assert!(
            len_plate > 0.0,
            "the overhang reaching past the base is plate-reachable and must \
             still be supported"
        );
        assert!(
            len_plate < len_any,
            "build-plate-only must drop the columns that would land on the \
             model (plate_only={len_plate:.0}mm, anywhere={len_any:.0}mm)"
        );

        // The guarantee the option exists to make: nothing rests on the base
        // block, at any height. The baseline is expected to violate it.
        assert!(
            support_vertices_in_box(&anywhere, 8.0) > 0,
            "baseline is expected to rest support on the base block"
        );
        assert_eq!(
            support_vertices_in_box(&plate_only, 8.0),
            0,
            "build-plate-only must never place support over the model"
        );
    }

    #[test]
    fn build_plate_only_sacrifices_an_overhang_with_no_path_to_the_bed() {
        // The cap now sits entirely within the base footprint, so every column
        // under it would land on the print. Sacrificing that overhang is the
        // documented trade, not a bug.
        let mut layers = tiered_stack(20.0, 15.0);
        let params = SlicingParams {
            support_on_build_plate_only: true,
            ..params_with_supports()
        };
        generate_supports(&mut layers, &params, None);
        assert_eq!(
            support_total_len(&layers),
            0.0,
            "an overhang with no route to the bed must be left unsupported"
        );

        // Same geometry without the option still gets its (model-borne) support,
        // so the emptiness above is the option talking and not dead geometry.
        let mut baseline = tiered_stack(20.0, 15.0);
        generate_supports(&mut baseline, &params_with_supports(), None);
        assert!(
            support_total_len(&baseline) > 0.0,
            "without the option this overhang is supported off the model"
        );
    }

    /// Square paint mask centred at (cx, cy), matching `square_layer`'s
    /// contour so an enforcer can be sized to exactly cover a model footprint.
    fn square_paths(cx: f64, cy: f64, half: f64) -> Paths {
        let mut p = Path::new(vec![]);
        p.push(Point::new(cx - half, cy - half));
        p.push(Point::new(cx + half, cy - half));
        p.push(Point::new(cx + half, cy + half));
        p.push(Point::new(cx - half, cy + half));
        Paths::new(vec![p])
    }

    #[test]
    fn build_plate_only_still_supports_a_painted_enforcer_with_no_path_to_the_bed() {
        // Same "stranded overhang" geometry as the sacrifice test above, but
        // the cap's underside is painted as an enforcer this time. A painted
        // enforcer is a direct instruction to support that exact spot, so
        // build-plate-only must not sacrifice it just because the column has
        // to rest on the model instead of reaching the plate.
        let mut layers = tiered_stack(20.0, 15.0);
        let n = layers.len();
        let mut enforcers = vec![Paths::new(vec![]); n];
        // Layer 30 is where the cap first appears (see `tiered_stack`), which
        // is where its underside overhang is registered in step 2. The mask is
        // intersected with the real footprint, so sizing it generously is
        // harmless.
        enforcers[30] = square_paths(-20.0, 0.0, 15.0);
        let paint = SupportPaintMasks {
            enforcers,
            blockers: vec![Paths::new(vec![]); n],
        };
        let params = SlicingParams {
            support_on_build_plate_only: true,
            ..params_with_supports()
        };
        generate_supports_with_paint(&mut layers, &params, None, &paint);
        assert!(
            support_total_len(&layers) > 0.0,
            "a painted enforcer must be supported even with no path to the plate"
        );
    }

    #[test]
    fn build_plate_only_holds_for_tree_supports_too() {
        // Unlike a straight column, a branch may pass high above the base and
        // lean out past it to reach the plate, grazing its edge on the way — so
        // the guarantee is not "nothing above the base" but "nothing standing on
        // it": the air gap above the base stays clear, and so does everything
        // over its interior just above it, where a resting branch would be.
        let mut layers = tiered_stack(8.0, 20.0);
        let params = SlicingParams {
            support_type: SupportType::Tree,
            support_on_build_plate_only: true,
            ..params_with_supports()
        };
        generate_supports(&mut layers, &params, None);
        assert!(
            support_path_count(&layers[0]) > 0,
            "tree must still reach the plate under the reachable overhang"
        );
        assert_eq!(
            support_vertices_in_box(&layers[10..11], 8.0),
            0,
            "the air gap above the base must stay clear"
        );
        assert_eq!(
            support_vertices_in_box(&layers[10..15], 7.0),
            0,
            "no tree branch may stand on the base under build-plate-only"
        );
    }

    /// A square layer with a square hole through it: `square_layer` plus a
    /// clockwise inner contour, so the footprint is a frame.
    fn frame_layer(z: f64, half: f64, hole_half: f64) -> SliceLayer {
        let mut layer = square_layer(z, 0.0, 0.0, half);
        let mut hole = Path::new(vec![]);
        hole.push(Point::new(-hole_half, -hole_half));
        hole.push(Point::new(-hole_half, hole_half));
        hole.push(Point::new(hole_half, hole_half));
        hole.push(Point::new(hole_half, -hole_half));
        layer.paths.push(hole);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer
    }

    /// Count support vertices inside the centred box `|x|, |y| < half` on the
    /// given layers.
    fn support_vertices_near_origin(layers: &[SliceLayer], half: f64) -> usize {
        let mut n = 0;
        for layer in layers {
            for (i, path) in layer.paths.iter().enumerate() {
                if layer.role_for_path(i) != ExtrusionRole::Support {
                    continue;
                }
                n += path
                    .iter()
                    .filter(|p| p.x().abs() < half && p.y().abs() < half)
                    .count();
            }
        }
        n
    }

    #[test]
    fn an_enforcer_supports_a_slope_the_angle_rule_passes_over() {
        // Steps out 0.1 mm per 0.2 mm layer — about 27° from vertical, well
        // inside the 45° rule. Painted, it must be supported whether automatic
        // detection runs or not: an enforcer measured against the threshold
        // step only ever reached what detection had already found.
        let build = || -> Vec<SliceLayer> {
            (0..30)
                .map(|k| square_layer(0.2 * (k as f64 + 1.0), 0.0, 0.0, 4.0 + 0.1 * k as f64))
                .collect()
        };
        let mut unpainted = build();
        generate_supports(&mut unpainted, &params_with_supports(), None);
        assert_eq!(
            support_total_len(&unpainted),
            0.0,
            "the slope is self-supporting, so nothing is detected"
        );

        for auto in [true, false] {
            let mut layers = build();
            let n = layers.len();
            let paint = SupportPaintMasks {
                enforcers: vec![square_paths(0.0, 0.0, 20.0); n],
                blockers: vec![Paths::new(vec![]); n],
            };
            let params = SlicingParams {
                support_auto: auto,
                ..params_with_supports()
            };
            generate_supports_with_paint(&mut layers, &params, None, &paint);
            assert!(
                support_total_len(&layers) > 50.0,
                "a painted slope must be supported (support_auto = {auto})"
            );
        }
    }

    #[test]
    fn support_stops_on_the_model_instead_of_falling_through_it() {
        // A slab with a pocket in its underside, a post on the slab, and a cap
        // on the post. The cap's support lands on the slab's top; carried any
        // further, it dropped through the solid slab and came out in the
        // pocket. The pocket's own ceiling is blocked, so anything found in it
        // fell through from above.
        for ty in [SupportType::Normal, SupportType::Tree] {
            let mut layers = Vec::new();
            for k in 0..3 {
                layers.push(frame_layer(0.2 * (k as f64 + 1.0), 10.0, 4.0));
            }
            for k in 3..10 {
                layers.push(square_layer(0.2 * (k as f64 + 1.0), 0.0, 0.0, 10.0));
            }
            for k in 10..20 {
                layers.push(square_layer(0.2 * (k as f64 + 1.0), 0.0, 0.0, 1.5));
            }
            for k in 20..22 {
                layers.push(square_layer(0.2 * (k as f64 + 1.0), 0.0, 0.0, 9.0));
            }
            let n = layers.len();
            let mut blockers = vec![Paths::new(vec![]); n];
            blockers[3] = square_paths(0.0, 0.0, 5.0);
            let paint = SupportPaintMasks {
                enforcers: vec![Paths::new(vec![]); n],
                blockers,
            };
            let params = SlicingParams {
                support_type: ty,
                ..params_with_supports()
            };
            generate_supports_with_paint(&mut layers, &params, None, &paint);

            assert!(
                support_total_len(&layers[10..20]) > 0.0,
                "{ty:?}: the cap must be supported off the slab"
            );
            assert_eq!(
                support_vertices_near_origin(&layers[..3], 4.0),
                0,
                "{ty:?}: support must not pass through the slab into the pocket under it"
            );
        }
    }

    #[test]
    fn an_overhang_nearer_the_bed_than_the_gap_gets_no_support() {
        // A pocket one layer deep in the underside: its ceiling is on layer 1,
        // and the default one-layer gap is layer 0 itself. Clamping the contact
        // onto layer 0 printed support straight under the ceiling, with no gap.
        let mut layers = vec![frame_layer(0.2, 10.0, 4.0)];
        for k in 1..6 {
            layers.push(square_layer(0.2 * (k as f64 + 1.0), 0.0, 0.0, 10.0));
        }
        let params = params_with_supports();
        assert_eq!(params.support_z_gap_layers, 1);
        generate_supports(&mut layers, &params, None);
        assert_eq!(support_total_len(&layers), 0.0);

        // With no gap there is room, and the same pocket is supported.
        let mut touching = vec![frame_layer(0.2, 10.0, 4.0)];
        for k in 1..6 {
            touching.push(square_layer(0.2 * (k as f64 + 1.0), 0.0, 0.0, 10.0));
        }
        let params = SlicingParams {
            support_z_gap_layers: 0,
            ..params_with_supports()
        };
        generate_supports(&mut touching, &params, None);
        assert!(support_total_len(&touching) > 0.0);
    }

    #[test]
    fn printable_support_drops_slivers_and_specks_but_keeps_a_column() {
        let bead = 0.4;
        let mut region = square_paths(0.0, 0.0, 5.0);
        // A hairline strip a quarter of a bead wide, apart from the column.
        let mut sliver = Path::new(vec![]);
        sliver.push(Point::new(10.0, -5.0));
        sliver.push(Point::new(10.1, -5.0));
        sliver.push(Point::new(10.1, 5.0));
        sliver.push(Point::new(10.0, 5.0));
        region.push(sliver);
        // A speck wide enough for a bead but far too small to stand.
        region.push(square_paths(-10.0, 0.0, 0.3).iter().next().unwrap().clone());

        let kept = printable_support(&region, bead, SUPPORT_MIN_REGION_AREA_MM2);
        let area: f64 = kept.iter().map(|p| p.signed_area()).sum();
        assert!(
            (area - 100.0).abs() < 1.0,
            "the 10 × 10 column survives intact (area {area:.2})"
        );
        let b = kept.bounds();
        assert!(
            b.min.x() > -5.1 && b.max.x() < 5.1,
            "the sliver and the speck are gone (bounds x {:.2}..{:.2})",
            b.min.x(),
            b.max.x()
        );
    }
}
