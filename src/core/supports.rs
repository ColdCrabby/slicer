//! Support-structure generation.
//!
//! Detects overhangs steeper than [`SlicingParams::support_threshold_angle`],
//! projects the unsupported area downward with horizontal (XY) and vertical (Z)
//! clearance from the model, classifies dense **interface** contact layers, and
//! fills the resulting columns with a grid/zig-zag pattern tagged
//! [`ExtrusionRole::Support`].  Both support styles are produced:
//!
//! | Style                    | Load-bearing column                         | Merging |
//! |--------------------------|---------------------------------------------|---------|
//! | [`SupportType::Normal`]  | full overhang footprint (straight-down grid) | none |
//! | [`SupportType::Tree`]    | node-drop branches that lean inward + converge | tips within `merge_dist` collapse |
//!
//! `Tree` is a node-drop (influence-drop) simulation — the standard organic
//! support model, reduced to essentials: contact tips are sampled from each
//! overhang, migrate toward their local centroid each layer (so **edge tips lean
//! inward** and a wide field contracts into a few trunks), merge when they meet,
//! and reject any step that would enter the model.  Wide interface caps still
//! cover the full overhang at the contact layers, so the trunks stay thin and
//! tree uses markedly less filament than a grid column.  It is **not** a
//! physically optimal collision-avoiding tree with base flaring (that is a much
//! larger algorithm); the honest limitation is surfaced by
//! [`SlicingParams::unsupported_feature_warnings`].
//!
//! # Build-plate-only
//!
//! [`SlicingParams::support_on_build_plate_only`] restricts support to columns
//! that can descend to the bed through empty space.  A contact pad is dropped
//! when it overlaps the model's accumulated footprint below it, so the overhang
//! above prints unsupported rather than growing a column that lands on — and
//! scars, or cannot be freed from — the print.  Losing those overhangs is the
//! point of the option, not a shortcoming of it.
//!
//! # Pipeline placement
//!
//! Runs after infill and before path ordering (TSP) so support paths are
//! ordered alongside the rest of the layer.  It only reads `OuterWall` paths
//! (via [`perimeter_paths_of`]) to derive the model footprint, so it never
//! disturbs wall/surface/infill geometry.

use clipper2::*;

use crate::settings::params::{SlicingParams, SupportType};

use super::surfaces::{generate_rectilinear_infill, perimeter_paths_of};
use super::types::{ExtrusionRole, SliceLayer};

/// Overhang islands smaller than this (mm²) are ignored — they are slicing
/// noise along near-vertical faceted walls, not genuine unsupported area.
const SUPPORT_MIN_OVERHANG_AREA_MM2: f64 = 1.0;

/// Support-region islands smaller than this (mm²) are dropped after projection —
/// too small to print a stable column.
const SUPPORT_MIN_REGION_AREA_MM2: f64 = 1.0;

/// Extra horizontal tolerance (mm) added to the per-layer overhang step so that
/// the tiny facet-to-facet jitter of a near-vertical wall does not register as
/// an overhang.
const OVERHANG_FACET_TOLERANCE_MM: f64 = 0.05;

/// Tree support — maximum branch angle from vertical.  A branch may lean this
/// far per layer (`dx = tan(angle)·layer_height`), so tips converge into trunks
/// as they descend.  40° matches the organic-support default of mature slicers.
const TREE_BRANCH_ANGLE_DEG: f64 = 40.0;

/// Tree support — trunk radius as a multiple of the nozzle diameter.  Trunks are
/// deliberately thin; the wide interface caps carry the actual contact surface.
const TREE_TRUNK_NOZZLE_MULT: f64 = 1.5;

/// Tree support — contact-tip sampling spacing as a multiple of the nozzle
/// diameter.  Sparser than a grid column, so tree uses less material.
const TREE_TIP_SPACING_NOZZLE_MULT: f64 = 6.0;

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
fn poly_union(a: &Paths, b: &Paths) -> Paths {
    if a.is_empty() {
        return b.clone();
    }
    if b.is_empty() {
        return a.clone();
    }
    union(a.clone(), b.clone(), FillRule::NonZero).unwrap_or_else(|_| a.clone())
}

fn poly_difference(a: &Paths, b: &Paths) -> Paths {
    if a.is_empty() || b.is_empty() {
        return a.clone();
    }
    difference(a.clone(), b.clone(), FillRule::NonZero).unwrap_or_else(|_| a.clone())
}

fn poly_intersect(a: &Paths, b: &Paths) -> Paths {
    if a.is_empty() || b.is_empty() {
        return Paths::new(vec![]);
    }
    intersect(a.clone(), b.clone(), FillRule::NonZero).unwrap_or_default()
}

fn poly_inflate(a: &Paths, delta: f64) -> Paths {
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

/// Accumulate the per-layer contacts top-down into the region that needs
/// support at each layer, **before** any model clearance is taken out.
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
fn accumulate_support_area(add_at: &[Paths], n: usize, close_r: f64) -> Vec<Paths> {
    let mut acc = Paths::new(vec![]);
    let mut out = vec![Paths::new(vec![]); n];
    for i in (0..n).rev() {
        if !add_at[i].is_empty() {
            acc = poly_union(&acc, &add_at[i]);
            acc = morphological_close(&acc, close_r);
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
    // The part of layer i not covered by layer i-1 grown outward by `max_step`.
    let mut overhang: Vec<Paths> = vec![Paths::new(vec![]); n];
    for i in 1..n {
        if footprints[i].is_empty() {
            continue;
        }
        let grown_prev = poly_inflate(&footprints[i - 1], max_step);
        let raw = poly_difference(&footprints[i], &grown_prev);
        overhang[i] = filter_small(&raw, SUPPORT_MIN_OVERHANG_AREA_MM2);
    }

    // ── 3. Register each overhang at its top-contact (activation) layer ─────
    // An overhang at layer i is contacted `z_gap` layers below it, leaving an
    // air gap for clean removal.
    let z_gap = params.support_z_gap_layers;
    let xy = params.support_xy_distance_mm.max(0.0);
    let mut add_at: Vec<Paths> = vec![Paths::new(vec![]); n];
    #[allow(clippy::needless_range_loop)]
    for i in 1..n {
        if overhang[i].is_empty() {
            continue;
        }
        let activate = (i as isize - 1 - z_gap as isize).max(0) as usize;
        add_at[activate] = poly_union(&add_at[activate], &overhang[i]);
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
    let plate_only = params.support_on_build_plate_only;
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
            add_at[i] = filter_small(&reachable, SUPPORT_MIN_OVERHANG_AREA_MM2);
        }
    }

    // ── 4. Build the load-bearing column per layer (model already subtracted) ─
    //
    // Two strategies produce a per-layer `columns[i]` region:
    //   • Normal — the full overhang footprint projected straight down (a grid
    //     column), subtracting the model + XY clearance each layer.
    //   • Tree — a node-drop simulation: contact tips are sampled, migrate
    //     toward their local centroid each layer (so edge tips lean inward and
    //     converge), merge when they meet, avoid the model, and rasterise into
    //     thin trunks.  Wide interface caps (added below) still cover the full
    //     overhang at the contact layers, so the trunks can stay thin.
    let is_tree = params.support_type == SupportType::Tree;
    let iface = params.support_interface_layers;

    // Weld the per-layer contact rings into a printable body before either
    // style consumes them.  Half the inter-ring gap is the minimum that closes
    // it; 0.6 leaves margin for the round-join approximation, and the nozzle
    // floor covers a near-vertical threshold where `max_step` is tiny.
    let close_r = (max_step * 0.6).max(ext_w * 0.5);
    let support_area = accumulate_support_area(&add_at, n, close_r);

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

    let columns: Vec<Paths> = if is_tree {
        simulate_tree_columns(&new_area, &footprints, &covered, n, lh, ext_w, xy)
    } else {
        project_normal_columns(&support_area, &footprints, &covered, n, xy)
    };

    // Per-layer printed regions, split into interface (dense) and body.
    let mut body_regions: Vec<Paths> = vec![Paths::new(vec![]); n];
    let mut iface_regions: Vec<Paths> = vec![Paths::new(vec![]); n];

    for i in 0..n {
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
            for pad in new_area.iter().take((i + iface).min(n)).skip(i) {
                if !pad.is_empty() {
                    top_full = poly_union(&top_full, pad);
                }
            }
        }
        let top_if = poly_difference(&top_full, &clip);

        if column.is_empty() && top_if.is_empty() {
            continue;
        }

        // The printed footprint is the union of the load-bearing column and the
        // (possibly wider) top contact pad.
        let total = poly_union(column, &top_if);

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

        iface_regions[i] = iface_region;
        body_regions[i] = body_region;
    }

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
    // Tree trunks are already thin and sparse (few columns), so they are filled
    // near-solid for strength; normal support fills the whole overhang area at
    // the configured (sparse) density.
    let body_spacing = if is_tree {
        fill_spacing / body_dens.max(0.6)
    } else {
        fill_spacing / body_dens
    };
    let iface_spacing = fill_spacing / iface_dens;
    let min_len = params.min_infill_extrusion_mm;

    for i in 0..n {
        let body = &body_regions[i];
        let iface_r = &iface_regions[i];
        if body.is_empty() && iface_r.is_empty() {
            continue;
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
        // un-retract to deposit a speck. A tree trunk is a ~1.2 mm disc, which
        // came out as one or two sub-bead specks per layer; on a Benchy 93.8 %
        // of tree support segments were under 2 mm. A loop turns each island
        // into one continuous extrusion and gives the fill something to tie
        // into.
        let printed = poly_union(body, iface_r);
        let half = fill_spacing * 0.5;
        let contour = poly_inflate(&printed, -half);

        // An island thinner than one bead cannot hold a contour; fall back to
        // filling it directly rather than dropping it.
        let (body_fill, iface_fill) = if contour.is_empty() {
            (body.clone(), iface_r.clone())
        } else {
            loops.extend(contour.iter().cloned());
            let inner = poly_inflate(&printed, -fill_spacing);
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
            let lines = generate_rectilinear_infill(&body_fill, body_spacing, body_angle, min_len);
            fills.extend(lines.iter().cloned());
        }
        if !iface_fill.is_empty() {
            let lines =
                generate_rectilinear_infill(&iface_fill, iface_spacing, iface_angle, min_len);
            fills.extend(lines.iter().cloned());
        }

        let min_run = ext_w * SUPPORT_MIN_RUN_LEN_NOZZLE_MULT;
        for path in loops {
            if path_run_len(&path, true) >= min_run {
                push_support_path(&mut layers[i], path, false);
            }
        }
        for path in fills {
            if path_run_len(&path, false) >= min_run {
                push_support_path(&mut layers[i], path, true);
            }
        }
    }
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

/// Tree / organic support via a node-drop simulation.
///
/// This is the standard influence-drop model used by mature slicers, reduced to
/// its essentials:
///
/// 1. **Seed** — at each activation layer, the overhang pad is sampled into a
///    grid of contact tips.
/// 2. **Merge** — tips closer than `merge_dist` collapse to their centroid, so
///    branches that meet become one trunk.
/// 3. **Migrate** — each tip moves toward the centroid of its neighbours, capped
///    at `tan(TREE_BRANCH_ANGLE)·layer_height` per layer.  A uniform interior is
///    balanced (no motion), but **edge tips see neighbours only on their inner
///    side and therefore lean inward**, so a wide field of tips contracts into a
///    few trunks as it descends — the characteristic tree shape.
/// 4. **Avoid the model** — a migration step that would land a tip inside the
///    model + XY clearance is rejected (the branch slides rather than diving in).
/// 5. **Rasterise** — the surviving tips are stamped as discs of radius
///    `TREE_TRUNK_NOZZLE_MULT·nozzle` and the model is subtracted.
///
/// Returns a per-layer load-bearing column (model already subtracted).  It is
/// deterministic: sampling, merging and migration are all order-stable.
fn simulate_tree_columns(
    new_area: &[Paths],
    footprints: &[Paths],
    covered: &[Paths],
    n: usize,
    layer_height: f64,
    ext_w: f64,
    xy: f64,
) -> Vec<Paths> {
    let max_dx = TREE_BRANCH_ANGLE_DEG.to_radians().tan() * layer_height;
    let trunk_r = ext_w * TREE_TRUNK_NOZZLE_MULT;
    let sample_sp = ext_w * TREE_TIP_SPACING_NOZZLE_MULT;
    let merge_dist = sample_sp * 0.75;
    let neigh = sample_sp * 1.6;

    let mut nodes: Vec<(f64, f64)> = Vec::new();
    let mut out = vec![Paths::new(vec![]); n];

    for i in (0..n).rev() {
        // 1. Seed new contact tips from the support area first appearing here.
        if !new_area[i].is_empty() {
            for p in sample_region_points(&new_area[i], sample_sp) {
                nodes.push(p);
            }
        }
        if nodes.is_empty() {
            continue;
        }

        // 2. Merge tips that have come within merge_dist of each other.
        nodes = merge_close_points(&nodes, merge_dist);

        // 3 + 4. Migrate toward the local centroid, rejecting steps into the model.
        // Under build-plate-only a branch must also never lean out over model
        // that lies *below* this layer: unlike a straight-down column, a tree
        // tip moves in XY, so a seed that started plate-reachable can drift
        // over the print unless every step is re-checked.
        let forbidden = poly_inflate(&footprints[i], xy);
        let below = covered.get(i);
        let grid = SpatialGrid::build(&nodes, neigh);
        let mut moved = Vec::with_capacity(nodes.len());
        for &(x, y) in &nodes {
            let (cx, cy, cnt) = grid.local_centroid(&nodes, x, y, neigh);
            let mut np = (x, y);
            if cnt > 1 {
                let (dx, dy) = (cx - x, cy - y);
                let d = (dx * dx + dy * dy).sqrt();
                if d > 1e-9 {
                    let s = max_dx.min(d) / d;
                    let cand = (x + dx * s, y + dy * s);
                    let into_model = point_in_paths_eo(cand.0, cand.1, &forbidden);
                    let over_model = below.is_some_and(|c| point_in_paths_eo(cand.0, cand.1, c));
                    if !into_model && !over_model {
                        np = cand;
                    }
                }
            }
            moved.push(np);
        }
        nodes = moved;

        // 5. Rasterise the trunks and subtract the model.
        let discs = stamp_discs(&nodes, trunk_r);
        let mut column = poly_difference(&discs, &forbidden);
        if let Some(c) = below {
            column = poly_difference(&column, c);
        }
        out[i] = filter_small(&column, SUPPORT_MIN_REGION_AREA_MM2 * 0.25);
    }

    out
}

/// Sample a polygon region into a regular grid of interior points, spacing
/// `spacing` mm apart.  Uses an even-odd scanline so holes (e.g. the void of an
/// annular overhang) are correctly excluded.  Deterministic: points are emitted
/// in row-major (y, then x) order.
fn sample_region_points(region: &Paths, spacing: f64) -> Vec<(f64, f64)> {
    if region.is_empty() || spacing <= 1e-6 {
        return Vec::new();
    }
    let (mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY);
    for c in region.iter() {
        for p in c.iter() {
            min_y = min_y.min(p.y());
            max_y = max_y.max(p.y());
        }
    }
    if !min_y.is_finite() || min_y >= max_y {
        return Vec::new();
    }

    let mut pts = Vec::new();
    let mut y = (min_y / spacing).ceil() * spacing;
    while y <= max_y {
        // X coordinates where the horizontal scan line crosses region edges.
        let mut xs: Vec<f64> = Vec::new();
        for c in region.iter() {
            let verts: Vec<(f64, f64)> = c.iter().map(|p| (p.x(), p.y())).collect();
            let m = verts.len();
            for k in 0..m {
                let (x0, y0) = verts[k];
                let (x1, y1) = verts[(k + 1) % m];
                // Half-open straddle test: each edge counted once at a shared vertex.
                if (y0 <= y) != (y1 <= y) {
                    let t = (y - y0) / (y1 - y0);
                    xs.push(x0 + t * (x1 - x0));
                }
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut k = 0;
        while k + 1 < xs.len() {
            let (xa, xb) = (xs[k], xs[k + 1]);
            let mut x = (xa / spacing).ceil() * spacing;
            while x <= xb {
                pts.push((x, y));
                x += spacing;
            }
            k += 2;
        }
        y += spacing;
    }
    pts
}

/// A uniform spatial hash over 2D points for O(1)-ish neighbour queries.
struct SpatialGrid {
    cell: f64,
    map: std::collections::HashMap<(i64, i64), Vec<usize>>,
}

impl SpatialGrid {
    fn key(cell: f64, x: f64, y: f64) -> (i64, i64) {
        ((x / cell).floor() as i64, (y / cell).floor() as i64)
    }

    fn build(points: &[(f64, f64)], cell: f64) -> Self {
        let cell = cell.max(1e-3);
        let mut map: std::collections::HashMap<(i64, i64), Vec<usize>> =
            std::collections::HashMap::new();
        for (i, &(x, y)) in points.iter().enumerate() {
            map.entry(Self::key(cell, x, y)).or_default().push(i);
        }
        Self { cell, map }
    }

    /// Indices of points within the 3×3 cell neighbourhood of `(x, y)`.
    fn neighbours(&self, x: f64, y: f64) -> Vec<usize> {
        let (cx, cy) = Self::key(self.cell, x, y);
        let mut out = Vec::new();
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(v) = self.map.get(&(cx + dx, cy + dy)) {
                    out.extend_from_slice(v);
                }
            }
        }
        out
    }

    /// Centroid of the points within `radius` of `(x, y)` (including itself),
    /// plus the count.  Used as the inward-pulling migration target.
    fn local_centroid(
        &self,
        points: &[(f64, f64)],
        x: f64,
        y: f64,
        radius: f64,
    ) -> (f64, f64, usize) {
        let r2 = radius * radius;
        let (mut sx, mut sy, mut cnt) = (0.0, 0.0, 0usize);
        for i in self.neighbours(x, y) {
            let (px, py) = points[i];
            let (dx, dy) = (px - x, py - y);
            if dx * dx + dy * dy <= r2 {
                sx += px;
                sy += py;
                cnt += 1;
            }
        }
        if cnt == 0 {
            (x, y, 0)
        } else {
            (sx / cnt as f64, sy / cnt as f64, cnt)
        }
    }
}

/// Collapse points closer than `dist` into their group centroid.  Greedy and
/// order-stable (points are visited in input order); each unvisited point claims
/// all not-yet-claimed points within `dist` via a spatial grid.
fn merge_close_points(points: &[(f64, f64)], dist: f64) -> Vec<(f64, f64)> {
    if points.len() < 2 || dist <= 1e-6 {
        return points.to_vec();
    }
    let grid = SpatialGrid::build(points, dist);
    let d2 = dist * dist;
    let mut claimed = vec![false; points.len()];
    let mut out = Vec::new();
    for i in 0..points.len() {
        if claimed[i] {
            continue;
        }
        let (x, y) = points[i];
        let (mut sx, mut sy, mut cnt) = (x, y, 1usize);
        claimed[i] = true;
        for j in grid.neighbours(x, y) {
            if j <= i || claimed[j] {
                continue;
            }
            let (px, py) = points[j];
            let (dx, dy) = (px - x, py - y);
            if dx * dx + dy * dy <= d2 {
                claimed[j] = true;
                sx += px;
                sy += py;
                cnt += 1;
            }
        }
        out.push((sx / cnt as f64, sy / cnt as f64));
    }
    out
}

/// Stamp a disc of radius `r` at every node and union them into one region.
/// Each node becomes a near-zero-length segment inflated with round caps, so a
/// single `inflate` call produces every disc at once.
fn stamp_discs(nodes: &[(f64, f64)], r: f64) -> Paths {
    if nodes.is_empty() || r <= 1e-6 {
        return Paths::new(vec![]);
    }
    let eps = (r * 0.01).max(1e-3);
    let segs: Vec<Path> = nodes
        .iter()
        .map(|&(x, y)| {
            let mut p = Path::new(vec![]);
            p.push(Point::new(x - eps, y));
            p.push(Point::new(x + eps, y));
            p
        })
        .collect();
    let inflated = inflate(Paths::new(segs), r, JoinType::Round, EndType::Round, 2.0);
    if inflated.is_empty() {
        return inflated;
    }
    union(inflated.clone(), Paths::new(vec![]), FillRule::NonZero).unwrap_or(inflated)
}

/// Even-odd point-in-polygon test against a `Paths` set (holes respected).
/// Returns `true` when `(x, y)` lies inside an odd number of contours.
fn point_in_paths_eo(x: f64, y: f64, paths: &Paths) -> bool {
    if paths.is_empty() {
        return false;
    }
    let mut inside = false;
    for c in paths.iter() {
        let verts: Vec<(f64, f64)> = c.iter().map(|p| (p.x(), p.y())).collect();
        let m = verts.len();
        for k in 0..m {
            let (xi, yi) = verts[k];
            let (xj, yj) = verts[(k + 1) % m];
            if (yi > y) != (yj > y) {
                let x_cross = xi + (y - yi) / (yj - yi) * (xj - xi);
                if x < x_cross {
                    inside = !inside;
                }
            }
        }
    }
    inside
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
        // Regression lock for the tree rewrite: a tall thin base under a wide
        // flat plate.  The plate underside is a large overhang.  Normal fills it
        // with a full grid column; tree drops sparse converging branches and
        // must therefore use materially less filament.  (Before the rewrite the
        // two were byte-identical — this test would have failed.)
        let build = |ty: SupportType| {
            let mut layers = Vec::new();
            for k in 0..20 {
                layers.push(square_layer(0.2 * (k as f64 + 1.0), 0.0, 0.0, 2.0));
            }
            for k in 20..22 {
                layers.push(square_layer(0.2 * (k as f64 + 1.0), 0.0, 0.0, 16.0));
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
            tree < normal * 0.85,
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

    #[test]
    fn build_plate_only_holds_for_tree_supports_too() {
        // Tree tips migrate in XY, so a seed that starts plate-reachable can
        // drift over the print unless every step is re-checked.
        let mut layers = tiered_stack(8.0, 20.0);
        let params = SlicingParams {
            support_type: SupportType::Tree,
            support_on_build_plate_only: true,
            ..params_with_supports()
        };
        generate_supports(&mut layers, &params, None);
        assert!(
            support_total_len(&layers) > 0.0,
            "tree must still support the reachable overhang"
        );
        assert_eq!(
            support_vertices_in_box(&layers, 8.0),
            0,
            "no tree branch may drift over the model under build-plate-only"
        );
    }
}
