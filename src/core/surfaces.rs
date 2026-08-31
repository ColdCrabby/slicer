#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

use clipper2::*;

use super::types::{ExtrusionRole, SliceLayer};
use crate::infill::SurfacePattern;
use crate::settings::params::{IroningType, SlicingParams};

/// Extract only outer-wall paths from a layer for use in surface detection.
///
/// Surface detection (top/bottom exposure) compares each layer's geometry to
/// its neighbours using Clipper2 boolean operations.  Only the outermost wall
/// contours should be used — including `InnerWall` Arachne beads (which are
/// tightly spaced concentric closed paths) causes the EvenOdd fill rule to
/// produce alternating in/out strips between wall beads, making it look like
/// there are surfaces between the beads and incorrectly labelling infill paths
/// as `BottomSurface` / `TopSurface`.
///
/// For a correctly sliced model the union of all `OuterWall` paths faithfully
/// represents the solid cross-section of the layer, which is exactly what
/// surface detection needs.
pub(crate) fn perimeter_paths_of(layer: &SliceLayer) -> Paths {
    Paths::new(
        layer
            .paths
            .iter()
            .enumerate()
            .filter(|(i, _)| layer.role_for_path(*i) == ExtrusionRole::OuterWall)
            .map(|(_, p)| p.clone())
            .collect(),
    )
}

/// Return `union(a, b)` when both are non-empty; otherwise return whichever is non-empty
/// (or an empty `Paths` if both are empty).  Takes ownership to avoid caller clones.
fn union_or_first(a: Paths, b: Paths) -> Paths {
    if !a.is_empty() && !b.is_empty() {
        union(a, b, FillRule::EvenOdd).unwrap_or_default()
    } else if !a.is_empty() {
        a
    } else {
        b
    }
}

/// Union of a layer's detected `bridge`, `bottom`, and `top` surface regions —
/// the full area that will be solid-filled — for the gap-fill sandwich test in
/// [`compute_gap_fill_footprint_excluding_sandwiched`].  Borrows its inputs
/// (unlike [`union_or_first`]) so the detection tuple is left intact.
fn combined_surface_region(bridge: &Paths, bottom: &Paths, top: &Paths) -> Paths {
    let mut acc = Paths::new(vec![]);
    for part in [bridge, bottom, top] {
        if part.is_empty() {
            continue;
        }
        acc = if acc.is_empty() {
            part.clone()
        } else {
            union(acc, part.clone(), FillRule::EvenOdd).unwrap_or_default()
        };
    }
    acc
}

/// Lateral pitch (mm) occupied by one round-capped extrusion of nominal width
/// `extrusion_width_mm` at `layer_height_mm` — libslic3r's `Flow::spacing()`:
///
/// ```text
/// spacing = extrusion_width − layer_height × (1 − π/4)
/// ```
///
/// A round-capped bead of nominal width `w` and height `h` only occupies
/// `w − h·(1 − π/4)` of lateral pitch when packed solid (its rounded sides
/// interlock), so solid fill lines are laid this far apart — ≈ 0.357 mm at a
/// 0.4 mm nozzle / 0.2 mm layers — rather than a full `w` apart. An earlier
/// `1.2 × layer_height` rule packed them far tighter (0.24 mm), heavily
/// over-extruding every solid surface.
///
/// **This is the engine's single density unit.** Every fill — solid surfaces
/// and sparse infill alike — lays its lines `spacing / density` apart and is
/// charged `spacing × layer_height` of filament per mm (see `resolve_width_mm`),
/// exactly as PrusaSlicer/Orca do (`Fill.cpp:212-228`,
/// `FillRectilinear.cpp:2778`). Charging the full nominal width into the
/// narrower pitch instead would over-extrude by `width / spacing` (≈ 13 % at
/// nozzle width, ≈ 23 % once `line_width > nozzle`) — the raised / blobby
/// top-surface defect. Keeping the pitch and the flow on the same quantity is
/// what makes "20 % density" deposit 20 % of a solid layer's volume rather than
/// something 11 % off in either direction.
pub(crate) fn extrusion_flow_spacing_mm(extrusion_width_mm: f64, layer_height_mm: f64) -> f64 {
    const CAP_CORRECTION: f64 = 1.0 - std::f64::consts::FRAC_PI_4; // ≈ 0.2146
    (extrusion_width_mm - layer_height_mm * CAP_CORRECTION).max(0.01)
}

/// Nominal solid top/bottom **surface** extrusion width (mm), before the
/// [`extrusion_flow_spacing_mm`] cap-correction.
///
/// This is the single source of truth for the width that both the surface fill
/// *line spacing* (in [`generate_top_bottom_surfaces_with_interior`]) and the
/// G-code *flow* (`resolve_width_mm`) derive from, so the two always agree and
/// solid surfaces neither over- nor under-extrude. Resolution mirrors
/// `resolve_width_mm`: an explicit `top_surface_line_width`, else the generic
/// `line_width`, else the nozzle diameter (the fill's natural bead width, and
/// the historical spacing basis).
pub(crate) fn solid_surface_nominal_width_mm(params: &SlicingParams) -> f64 {
    if params.top_surface_line_width > 0.0 {
        params.top_surface_line_width
    } else if params.line_width > 0.0 {
        params.line_width
    } else {
        params.nozzle_diameter_mm
    }
}

/// Nominal **sparse infill** extrusion width (mm), before the
/// [`extrusion_flow_spacing_mm`] cap-correction.
///
/// The sparse-infill twin of [`solid_surface_nominal_width_mm`], and the single
/// source of truth for the width that both the infill *line pitch* (in
/// [`crate::infill::generate_infill`]) and the G-code *flow* (`resolve_width_mm`)
/// derive from. Resolution mirrors `resolve_width_mm`: an explicit
/// `sparse_infill_line_width`, else the generic `line_width`, else the nozzle
/// diameter.
///
/// Before this existed every pattern generator hardcoded a 0.4 mm reference, so
/// a 0.6 mm nozzle printed "20 % density" as roughly 13 %.
pub(crate) fn sparse_infill_nominal_width_mm(params: &SlicingParams) -> f64 {
    if params.sparse_infill_line_width > 0.0 {
        params.sparse_infill_line_width
    } else if params.line_width > 0.0 {
        params.line_width
    } else {
        params.nozzle_diameter_mm
    }
}

/// Nominal **support** extrusion width (mm), before the
/// [`extrusion_flow_spacing_mm`] cap-correction.
///
/// The support twin of [`sparse_infill_nominal_width_mm`]. Support is a fill
/// role — its strands are laid at a pitch of `spacing / density` — so it obeys
/// the same identity: charge each line the volume of the strip it fills, not
/// the full nominal bead width. Charging at the nominal width instead
/// over-extrudes by `width / spacing` (≈ 12 % at nozzle width) and makes the
/// requested density wrong on any nozzle other than the reference one.
pub(crate) fn support_nominal_width_mm(params: &SlicingParams) -> f64 {
    if params.support_line_width > 0.0 {
        params.support_line_width
    } else if params.line_width > 0.0 {
        params.line_width
    } else {
        params.nozzle_diameter_mm
    }
}

/// Nominal **outer wall** extrusion width (mm).
///
/// The wall twin of [`solid_surface_nominal_width_mm`]. Resolution mirrors
/// `resolve_width_mm`: an explicit `outer_wall_line_width`, else the nozzle
/// diameter. Walls deliberately skip the generic `line_width` — that setting
/// applies only to fill roles, so a user who widens their infill lines does not
/// silently move every wall with it.
pub(crate) fn outer_wall_nominal_width_mm(params: &SlicingParams) -> f64 {
    if params.outer_wall_line_width > 0.0 {
        params.outer_wall_line_width
    } else {
        params.nozzle_diameter_mm
    }
}

/// Solid top/bottom surface fill direction alternates by 90° every layer so
/// successive solid layers cross-hatch — matching CuraEngine's default
/// `skin_angles = {45°, 135°}` and the per-layer solid-infill rotation in
/// Orca/PrusaSlicer.  Cross-hatching welds adjacent solid layers together,
/// hides the fill direction on the visible top surface, and distributes
/// anisotropic strength.  `layer_index` is the absolute layer number so the
/// alternation is stable regardless of where a surface run begins.
fn surface_infill_angle_for_layer(base_angle_deg: f64, layer_index: usize) -> f64 {
    let mut angle = base_angle_deg + if layer_index % 2 == 1 { 90.0 } else { 0.0 };
    while angle >= 180.0 {
        angle -= 180.0;
    }
    while angle < 0.0 {
        angle += 180.0;
    }
    angle
}

/// Solid top/bottom surface islands below this area (mm²) are dropped.  They are
/// the thin wall-covered slivers Arachne's per-island-*average* interior
/// estimate leaves over a locally-thin taper (e.g. the aft hull-tip rim): the
/// perimeters already cover them, and classic emits none there.  Dropping a
/// whole such island cannot open a wall-zone void (the walls fill it), unlike
/// pulling a large surface back off its wall bond.
const SURFACE_MIN_ISLAND_MM2: f64 = 2.0;

/// Minimum width — as a multiple of the nozzle diameter — an **interior region**
/// must reach to host a rectilinear top/bottom surface fill.
///
/// Arachne's per-island *average* wall count makes `calculate_interior_region`
/// leave a thin interior sliver wherever a cross-section is locally thinner
/// than that average — the Benchy hull-side wall tips, the funnel-to-roof
/// transitions, the cabin roof-ridge line.  Such a channel is already fully
/// consumed by the wall + gap-fill beads, but wherever the geometry above
/// recedes (a wall tip that steps back layer over layer) it is still picked up
/// as an "exposed" top/bottom surface and filled with a rectilinear zig-zag
/// whose segments are a fraction of a millimetre long — the "tiny extrudes that
/// make no sense" and "weird top-surface spots on the sides".
///
/// The discriminator is **not** the exposed strip's own width (a genuine
/// fore-deck top surface is an equally thin horizontal band): it is whether the
/// strip belongs to a *thick* interior (real infill area) or a *thin* wall-band
/// channel (Arachne artifact).  Clipping the surface to a morphologically
/// **opened** interior — one from which channels thinner than this threshold
/// have been erased, genuine areas kept at full extent — drops exactly the
/// artifacts and makes Arachne match the `classic` generator, which emits no
/// surface in those channels.  Because the strip is then no longer a
/// `solid_region`, its gap-fill beads also survive `prune_redundant_gap_fill`.
///
/// Measured on a 0.4 mm-nozzle Benchy: artifact channels are ≤ ~0.8 mm (≤ 2×
/// nozzle) wide, while every genuine surface sits on an interior several
/// millimetres across.  A threshold of 2.5× (1.0 mm) removes the former and
/// keeps the latter with wide margin.
const SURFACE_MIN_INTERIOR_WIDTH_NOZZLE_MULT: f64 = 2.5;

/// Opening radius, as a fraction of the **solid-surface extrusion width**, used
/// to erase sub-bead slivers left on a surface region by the wall-band trim.
///
/// A value of 0.5 makes the erosion diameter exactly one extrusion width, which
/// is the physically meaningful threshold: a strip narrower than one bead cannot
/// hold a bead, so filling it is impossible by construction and the flanking
/// wall already covers it.  See [`open_surface_region_for_fill`].
const SURFACE_FILL_MIN_WIDTH_FRACTION: f64 = 0.5;

/// How far the surviving core is re-grown, as a multiple of the erosion radius,
/// before being clipped back to the original region.
///
/// Re-growing by *more* than the erosion radius restores sharp convex corners
/// exactly (the clip to the original region bounds the result), instead of
/// leaving them rounded as a plain morphological opening would.  Corner rounding
/// is not a coverage problem — it is a *stub* problem: the scanline crossing a
/// rounded corner emits a couple of extra sub-millimetre spans, which is the
/// very artifact this filter exists to remove.
///
/// 2.0 recovers every corner while staying well below the wall-band width that
/// separates a sliver from the nearest genuine surface, so a dropped sliver is
/// never re-attached.
const SURFACE_FILL_REGROW_FACTOR: f64 = 2.0;

/// Maximum horizontal gap (as a multiple of `line_spacing`) allowed when
/// connecting the end of one scan-line segment to the nearest end of the next
/// scan-line segment in the serpentine chaining pass.
///
/// A factor of 2.0 handles typical shape variations where adjacent scan lines
/// have modestly different x extents (e.g. at the edge of a circle or any
/// slanted boundary).  Values larger than ~3.0 risk bridging across genuine
/// void regions; values smaller than 1.5 may leave convex corners unchained.
const SERPENTINE_CONNECT_THRESHOLD: f64 = 2.0;

/// Scan-row gap (as a multiple of `line_spacing`) beyond which every active
/// serpentine chain is finalised **before** pairing.
///
/// Empty scan rows are elided from `scan_line_data` (only rows that produced at
/// least one segment are recorded), so two consecutive recorded rows whose
/// `scan_y` differ by more than this factor imply at least one fully-empty row
/// between them — i.e. every current island ended and no chain may reconnect
/// across the void.  Without this guard the serpentine chaining reconnects two
/// islands that are separated **across** the scan axis but share the same
/// **along**-strand band, extruding the connector straight over open space (the
/// "phantom bridge" / extrude-over-thin-air defect).
///
/// A value of 1.5 sits cleanly between "adjacent row" (1.0×) and "≥1 skipped
/// row" (≥2.0×), so it never fires on genuinely contiguous fill.
const SERPENTINE_ROW_GAP_THRESHOLD: f64 = 1.5;

/// Add solid infill for a computed surface `region` to a layer.
///
/// Generates a fill pattern covering only the provided `region` paths (the
/// already-computed surface area), then appends the resulting paths to `layer`
/// with the given extrusion `role`.
///
/// `line_spacing` is the center-to-center pitch of the fill lines in mm — for
/// solid surfaces derive it via [`extrusion_flow_spacing_mm`] so adjacent
/// beads overlap and the surface has no gaps.
pub(super) fn add_solid_infill_for_region(
    layer: &mut SliceLayer,
    region: &Paths,
    role: ExtrusionRole,
    line_spacing: f64,
    infill_angle: f64,
    min_infill_extrusion_mm: f64,
    pattern: SurfacePattern,
) {
    if region.is_empty() {
        return;
    }

    let line_spacing = line_spacing.max(0.01);
    let infill_paths = generate_solid_infill(
        region,
        line_spacing,
        infill_angle,
        min_infill_extrusion_mm,
        pattern,
    );

    for path in infill_paths {
        layer.paths.push(path);
        layer.path_roles.push(role);
    }
}

/// Compute the principal-axis angle (in degrees, 0–180) of a polygon set
/// using PCA on its vertices.
///
/// Returns the angle of the **dominant axis** (the eigenvector with the
/// larger eigenvalue) measured CCW from +X.  This is the *long* dimension
/// of the unsupported region; callers wanting the **bridge print direction**
/// must add 90° so each strand spans the *short* dimension of the gap.
///
/// Falls back to `None` when the input is empty, has fewer than two distinct
/// points, or is a perfect square (eigenvalues nearly equal — no preferred
/// direction); callers should default to a sensible angle in that case.
fn principal_axis_angle_deg(paths: &Paths) -> Option<f64> {
    let mut n = 0_u64;
    let mut sum_x = 0.0_f64;
    let mut sum_y = 0.0_f64;
    for path in paths.iter() {
        for pt in path.iter() {
            sum_x += pt.x();
            sum_y += pt.y();
            n += 1;
        }
    }
    if n < 2 {
        return None;
    }
    let nf = n as f64;
    let mx = sum_x / nf;
    let my = sum_y / nf;

    let mut sxx = 0.0_f64;
    let mut syy = 0.0_f64;
    let mut sxy = 0.0_f64;
    for path in paths.iter() {
        for pt in path.iter() {
            let dx = pt.x() - mx;
            let dy = pt.y() - my;
            sxx += dx * dx;
            syy += dy * dy;
            sxy += dx * dy;
        }
    }

    let trace = sxx + syy;
    if trace < 1e-9 {
        return None;
    }
    let det = sxx * syy - sxy * sxy;
    let disc = (trace * trace * 0.25 - det).max(0.0).sqrt();
    let lam_max = trace * 0.5 + disc;
    let lam_min = trace * 0.5 - disc;
    // Square / circle: no preferred direction.  An eigenvalue ratio < 5 %
    // means the major and minor axes carry essentially the same variance —
    // any angle we picked would be arbitrary, so signal "no answer" and let
    // the caller fall back to its bounding-box heuristic.
    if (lam_max - lam_min) / lam_max < 0.05 {
        return None;
    }

    // Dominant eigenvector for symmetric 2×2 matrix.
    let angle_rad = if sxy.abs() > 1e-9 {
        (lam_max - sxx).atan2(sxy)
    } else if sxx >= syy {
        0.0
    } else {
        std::f64::consts::FRAC_PI_2
    };

    let mut deg = angle_rad.to_degrees();
    // Normalise to [0, 180): direction is undirected.
    while deg < 0.0 {
        deg += 180.0;
    }
    while deg >= 180.0 {
        deg -= 180.0;
    }
    Some(deg)
}

/// Morphological opening (erode → dilate) of a polygon set by `radius_mm`.
///
/// Removes thin features (slivers, hair-thin connecting strands) narrower
/// than `2 × radius_mm` while preserving larger regions almost unchanged.
/// A no-op when `radius_mm <= 0`.
fn morphological_open(paths: Paths, radius_mm: f64) -> Paths {
    // 1e-6 mm = 1 nm — well below any real geometry and below Clipper2's
    // Centi quantisation (10 µm).  Anything smaller is rounding noise and
    // a no-op is the right behaviour.
    if radius_mm <= 1e-6 || paths.is_empty() {
        return paths;
    }
    let eroded = clipper2::inflate(paths, -radius_mm, JoinType::Round, EndType::Polygon, 2.0);
    if eroded.is_empty() {
        return eroded;
    }
    clipper2::inflate(eroded, radius_mm, JoinType::Round, EndType::Polygon, 2.0)
}

/// Compute the **physical bead footprint** of every wall path on a layer.
///
/// For each `OuterWall` / `InnerWall` / `OverhangPerimeter` path, inflate the
/// centerline by `width / 2` (falling back to `nozzle_diameter_mm / 2` when
/// the path has no recorded width).  Closed bead loops use `EndType::Joined`
/// so the inflation extends to **both** sides of the centerline (total width
/// = 2 × radius = the bead width); open arcs (e.g. results of
/// `clip_walls_against_bridge_region`) use `EndType::Round` for the same
/// effect with rounded caps.
///
/// The result is the union of every wall bead's footprint — the area on the
/// build plate that wall extrusions actually consume.  Used by bridge
/// detection to avoid placing bridge infill on top of existing walls
/// (Benchy rear-deck overhang regression).
/// Physical bead footprint of every **wall** path (OuterWall / InnerWall /
/// OverhangPerimeter / GapFill) — the build-plate area those extrusions consume.
/// Bridge detection and solid top/bottom surfaces subtract this so nothing is
/// deposited on top of an existing wall or gap-fill bead.
pub(super) fn compute_wall_bead_footprint(layer: &SliceLayer, nozzle_diameter_mm: f64) -> Paths {
    compute_wall_bead_footprint_filtered(layer, nozzle_diameter_mm, true)
}

/// Like [`compute_wall_bead_footprint`] but, when `include_gap_fill` is `false`,
/// omits `GapFill` beads from the footprint.
///
/// The solid top/bottom surface trim uses the walls-only footprint because it
/// carves the gap-fill footprint separately via
/// [`compute_gap_fill_footprint_excluding_sandwiched`], which drops beads that
/// are redundant with the surface.  Including gap fill here as well would
/// re-add those redundant beads and punch the same bead-wide hole back into the
/// surface (the 3DBenchy rear-rail defect this exclusion targets).
pub(super) fn compute_wall_bead_footprint_filtered(
    layer: &SliceLayer,
    nozzle_diameter_mm: f64,
    include_gap_fill: bool,
) -> Paths {
    // Group wall paths by (is_open, radius-bucket) so we can run **one**
    // `inflate` call per group instead of one per path.  Clipper2's `inflate`
    // takes a `Paths` and offsets every contained sub-path together — there
    // is no per-call cost for adding more sub-paths beyond the boolean ops
    // they trigger internally.  Doing N separate inflate+union pairs (the
    // previous approach) was O(N²) on the union step and dominated the whole
    // pipeline (3 s of a 4 s benchy).
    //
    // Radius is quantised to micrometres so near-equal Arachne widths bucket
    // together; in practice almost every closed wall lands in the same
    // bucket (= half the nozzle diameter) so the typical group count is 1–2.
    use std::collections::HashMap;

    let default_radius = nozzle_diameter_mm * 0.5;
    let mut buckets: HashMap<(bool, i32), Vec<clipper2::Path>> = HashMap::new();

    for (i, path) in layer.paths.iter().enumerate() {
        let role = layer.role_for_path(i);
        // Only true wall extrusions consume area we'd otherwise want to
        // bridge over.  `OverhangPerimeter` is included because the
        // overhang post-pass relabels in-air wall arcs, and bridges still
        // must not overlap them; `GapFill` deposits material inside the wall
        // band and must likewise not be bridged over — but the surface trim
        // passes `include_gap_fill = false` because it accounts for gap fill
        // separately (and drops beads redundant with the surface).
        let role_included = matches!(
            role,
            ExtrusionRole::OuterWall | ExtrusionRole::InnerWall | ExtrusionRole::OverhangPerimeter
        ) || (include_gap_fill && role == ExtrusionRole::GapFill);
        if !role_included {
            continue;
        }

        let radius = layer
            .width_for_path(i)
            .map(|w| w * 0.5)
            .unwrap_or(default_radius);
        if radius <= 1e-6 {
            continue;
        }

        let is_open = layer.is_path_open(i);
        // Quantise to micrometres (× 1000) so equal-width beads share a bucket.
        let radius_key = (radius * 1000.0).round() as i32;
        buckets
            .entry((is_open, radius_key))
            .or_default()
            .push(path.clone());
    }

    if buckets.is_empty() {
        return Paths::new(vec![]);
    }

    // Inflate each bucket as a single batch, then union the (small) set of
    // bucket results.  For typical Benchy-class geometry this is one or two
    // inflate calls and zero or one union call — vs hundreds of each before.
    let mut acc: Paths = Paths::new(vec![]);
    for ((is_open, radius_key), paths_vec) in buckets {
        let radius = (radius_key as f64) / 1000.0;
        let end_type = if is_open {
            EndType::Round
        } else {
            EndType::Joined
        };
        let inflated = clipper2::inflate(
            Paths::new(paths_vec),
            radius,
            JoinType::Round,
            end_type,
            2.0,
        );
        if inflated.is_empty() {
            continue;
        }
        acc = if acc.is_empty() {
            inflated
        } else {
            union(acc, inflated, FillRule::NonZero).unwrap_or_default()
        };
    }
    acc
}

/// Physical bead footprint of only the **gap-fill** beads on a layer, each
/// variable-width centerline inflated by its half-width.
///
/// Sparse infill subtracts this so it abuts — never re-extrudes over — the
/// Arachne gap fill (which the wall-inset infill region calculation does not
/// otherwise account for).  Gap-fill beads number in the tens per layer, so a
/// per-path inflate + union is ample.
pub(super) fn compute_gap_fill_footprint(layer: &SliceLayer, nozzle_diameter_mm: f64) -> Paths {
    compute_gap_fill_footprint_filtered(layer, nozzle_diameter_mm, None)
}

/// Physical bead footprint of the layer's gap-fill beads, **excluding** any bead
/// that is sandwiched by (redundant with) `surface_region`.
///
/// Used when trimming a solid top/bottom surface: a gap-fill bead running down
/// the centre of the surface strip is sandwiched by the surface on both sides,
/// so [`prune_redundant_gap_fill`] deletes it later — and carving its footprint
/// out of the surface (as the plain [`compute_gap_fill_footprint`] would) leaves
/// a bead-wide **hole** in the surface region.  On a thin roof (the 3DBenchy
/// rear rail, ≈ layer 200) that hole both splits the surface serpentine into two
/// disconnected bands *and* is then re-filled with sparse-infill dashes over the
/// void the pruned bead left — the "two infill surfaces plus tiny blobs" defect.
/// Excluding sandwiched beads lets the surface cover the whole strip as one
/// region; genuine thin necks (surface on only one side) are still carved so the
/// surface abuts, never welds onto, them.
fn compute_gap_fill_footprint_excluding_sandwiched(
    layer: &SliceLayer,
    nozzle_diameter_mm: f64,
    surface_region: &Paths,
) -> Paths {
    compute_gap_fill_footprint_filtered(layer, nozzle_diameter_mm, Some(surface_region))
}

/// Shared implementation for [`compute_gap_fill_footprint`] and
/// [`compute_gap_fill_footprint_excluding_sandwiched`].  When `skip_sandwiched`
/// is `Some(region)`, a gap-fill bead flanked by that region on both sides is
/// omitted from the footprint.
fn compute_gap_fill_footprint_filtered(
    layer: &SliceLayer,
    nozzle_diameter_mm: f64,
    skip_sandwiched: Option<&Paths>,
) -> Paths {
    let default_radius = nozzle_diameter_mm * 0.5;
    let mut acc = Paths::new(vec![]);
    for (i, path) in layer.paths.iter().enumerate() {
        if layer.role_for_path(i) != ExtrusionRole::GapFill {
            continue;
        }
        if let Some(region) = skip_sandwiched {
            if gap_fill_sandwiched_by_surface(
                path,
                layer.vertex_widths_for_path(i).as_deref(),
                layer.width_for_path(i),
                nozzle_diameter_mm,
                region,
            ) {
                continue;
            }
        }
        let radius = layer
            .width_for_path(i)
            .map(|w| w * 0.5)
            .unwrap_or(default_radius);
        if radius <= 1e-6 {
            continue;
        }
        // Gap fill is emitted as open polylines; round caps span both sides.
        let end_type = if layer.is_path_open(i) {
            EndType::Round
        } else {
            EndType::Joined
        };
        let fp = clipper2::inflate(
            Paths::new(vec![path.clone()]),
            radius,
            JoinType::Round,
            end_type,
            2.0,
        );
        if fp.is_empty() {
            continue;
        }
        acc = if acc.is_empty() {
            fp
        } else {
            union(acc, fp, FillRule::NonZero).unwrap_or_default()
        };
    }
    acc
}

/// Perpendicular over-reach, as a multiple of the nozzle diameter, added to a
/// gap-fill bead's own half-width when probing for solid surface on either side
/// (see [`gap_fill_sandwiched_by_surface`]).  The probe point must clear the
/// gap-fill footprint band that `blocked_for_surface` carved out of the surface
/// (exactly the bead's half-width) and land a nozzle-fraction *into* the
/// surface fill beyond it; `0.5·d` (0.2 mm at a 0.4 mm nozzle) clears the band
/// edge without reaching across a genuine one-sided neck to the surface on its
/// far flank.
const GAP_FILL_SURFACE_PROBE_OVERREACH_NOZZLE_MULT: f64 = 0.5;

/// Fraction of a gap-fill bead's vertices that must be flanked by solid surface
/// on **both** sides for the bead to count as redundant with that surface.
const GAP_FILL_SANDWICH_MAJORITY: f64 = 0.5;

/// True when a gap-fill `bead` runs *through* a solid-surface region — the
/// surface abuts it on **both** perpendicular sides — so filling it as gap fill
/// merely double-extrudes over the surface the pipeline already lays down.
///
/// ## Why the plain "inside `solid_regions`" test misses this
///
/// `blocked_for_surface` unions the gap-fill footprint out of the surface region
/// so the surface fill *abuts* rather than welds onto genuine thin necks.  That
/// leaves a bead-wide corridor in `solid_regions` exactly where each gap-fill
/// bead sits, so a bead centred in a solid strip is never "inside" the surface —
/// yet the surface's rectilinear zig-zag, laid at full extrusion width, still
/// deposits straight over it (measured on the 3DBenchy rear rail, ≈ layer 200:
/// 6 mm² of GapFill∩TopSurface double-extrusion that a footprint-erosion overlap
/// scan hides because the bead is thin).
///
/// ## The discriminator
///
/// Probe each bead vertex a short distance to either perpendicular side
/// (`half-width + `[`GAP_FILL_SURFACE_PROBE_OVERREACH_NOZZLE_MULT`]`·d`), just
/// past the carved-out corridor.  A bead the surface *surrounds* (rear rail,
/// embossed-logo channel) has surface on both probes; a genuine thin neck that
/// merely *abuts* a surface edge has it on at most one.  Requiring both sides on
/// a majority of vertices ([`GAP_FILL_SANDWICH_MAJORITY`]) drops the redundant
/// centre-of-strip beads while keeping every load-bearing thin-neck bead.
fn gap_fill_sandwiched_by_surface(
    bead: &clipper2::Path,
    vertex_widths: Option<&[f64]>,
    scalar_width: Option<f64>,
    nozzle_diameter_mm: f64,
    solid_regions: &Paths,
) -> bool {
    let pts: Vec<(f64, f64)> = bead.iter().map(|p| (p.x(), p.y())).collect();
    if pts.len() < 2 {
        return false;
    }
    let overreach = GAP_FILL_SURFACE_PROBE_OVERREACH_NOZZLE_MULT * nozzle_diameter_mm;
    let default_half = 0.5 * nozzle_diameter_mm;
    let mut total = 0_usize;
    let mut sandwiched = 0_usize;
    for j in 0..pts.len() {
        // Tangent from the neighbours (forward/back difference at the ends).
        let prev = pts[j.saturating_sub(1)];
        let next = pts[(j + 1).min(pts.len() - 1)];
        let (tx, ty) = (next.0 - prev.0, next.1 - prev.1);
        let len = (tx * tx + ty * ty).sqrt();
        if len <= 1e-9 {
            continue;
        }
        // Perpendicular unit vector.
        let (px, py) = (-ty / len, tx / len);
        let half_w = vertex_widths
            .and_then(|w| w.get(j).copied())
            .or(scalar_width)
            .map(|w| 0.5 * w)
            .unwrap_or(default_half);
        let reach = half_w + overreach;
        let (vx, vy) = pts[j];
        let side_a = vertex_inside_or_on_paths_eo(vx + px * reach, vy + py * reach, solid_regions);
        let side_b = vertex_inside_or_on_paths_eo(vx - px * reach, vy - py * reach, solid_regions);
        total += 1;
        if side_a && side_b {
            sandwiched += 1;
        }
    }
    total > 0 && (sandwiched as f64) > GAP_FILL_SANDWICH_MAJORITY * (total as f64)
}

/// Remove `GapFill` beads that are already covered by a solid-surface region.
///
/// Arachne emits medial gap fill over *every* thin residual its offset loops
/// leave — including thin necks deep inside the solid interior.  On a solid
/// layer the top/bottom surface fills that interior densely, so a gap bead
/// there is redundant; since the surface is generated in full first, the bead
/// would otherwise sit as a scattered variable-width island on the uniform
/// surface (the isolated dashes visible across the Benchy first layer), or —
/// where it runs down the centre of a thin solid strip — double-extrude
/// straight under the surface's zig-zag (the Benchy rear-rail "unexplained gap
/// bead under a top surface" defect).
///
/// Fill priority is **solid surface > gap fill > sparse infill**.  A bead is
/// dropped when either
///
/// 1. a majority of its vertices lie **inside** `solid_regions` (the bead sits
///    in a filled surface), or
/// 2. it is **sandwiched** by surface on both perpendicular sides
///    ([`gap_fill_sandwiched_by_surface`]) — the surface surrounds it even
///    though `blocked_for_surface` carved a bead-wide corridor out of
///    `solid_regions` so the plain inside-test would miss it.
///
/// Gap fill in sparse zones and genuine thin ribs (surface on at most one side)
/// is kept, because sparse infill would skip those sub-nozzle channels.  Runs
/// after surface generation, before sparse infill.
pub(super) fn prune_redundant_gap_fill(layers: &mut [SliceLayer], nozzle_diameter_mm: f64) {
    for layer in layers.iter_mut() {
        if layer.solid_regions.is_empty() || !layer.path_roles.contains(&ExtrusionRole::GapFill) {
            continue;
        }

        let mut new_paths = Paths::new(vec![]);
        let mut new_roles = Vec::new();
        let mut new_widths = Vec::new();
        let mut new_vwidths = Vec::new();
        let mut new_is_open = Vec::new();
        // Carry per-path overhang classes only when the layer was graded, so
        // removing a gap-fill path does not shift the classes off their walls.
        let grade_overhang = !layer.path_overhang.is_empty();
        let mut new_overhang = Vec::new();

        for (i, path) in layer.paths.iter().enumerate() {
            let role = layer.role_for_path(i);
            let redundant = role == ExtrusionRole::GapFill && {
                let mut total = 0_usize;
                let mut inside = 0_usize;
                for p in path.iter() {
                    total += 1;
                    if vertex_inside_or_on_paths_eo(p.x(), p.y(), &layer.solid_regions) {
                        inside += 1;
                    }
                }
                (total > 0 && inside * 2 > total)
                    || gap_fill_sandwiched_by_surface(
                        path,
                        layer.vertex_widths_for_path(i).as_deref(),
                        layer.width_for_path(i),
                        nozzle_diameter_mm,
                        &layer.solid_regions,
                    )
            };
            if redundant {
                continue;
            }
            new_paths.push(path.clone());
            new_roles.push(role);
            new_widths.push(layer.width_for_path(i));
            new_vwidths.push(layer.vertex_widths_for_path(i));
            new_is_open.push(layer.is_path_open(i));
            if grade_overhang {
                new_overhang.push(layer.overhang_for_path(i));
            }
        }

        layer.paths = new_paths;
        layer.path_roles = new_roles;
        layer.path_widths = new_widths;
        layer.path_vertex_widths = new_vwidths;
        layer.path_is_open = new_is_open;
        layer.path_overhang = new_overhang;
    }
}

/// Drop sub-paths whose absolute signed area is below `min_area_mm2`.
///
/// `Paths::signed_area()` would only sum the whole set; we filter individually.
/// Hole sub-paths (CW winding, negative area) are kept when their absolute
/// area exceeds the threshold so they continue carving the corresponding
/// solid sub-path; a tiny hole would pop in/out with the noise filter, so
/// removing it has the same regularising effect.
fn filter_small_islands(paths: &Paths, min_area_mm2: f64) -> Paths {
    if min_area_mm2 <= 0.0 {
        return paths.clone();
    }
    let kept: Vec<clipper2::Path> = paths
        .iter()
        .filter(|p| p.signed_area().abs() >= min_area_mm2)
        .cloned()
        .collect();
    Paths::new(kept)
}

/// Morphologically open an interior region for top/bottom **surface** detection
/// so that wall-band channels narrower than
/// `SURFACE_MIN_INTERIOR_WIDTH_NOZZLE_MULT × nozzle` are erased while genuine
/// infill areas keep their full extent (only their convex corners, which sit
/// inside the walls, are rounded).  See that constant for the full rationale.
///
/// Returns the input unchanged when the threshold is degenerate.  The caller
/// clips the detected surface region to the result, so a surface that lands
/// entirely inside a thin channel is dropped — matching the `classic` generator,
/// which emits no surface there and leaves the channel to its wall + gap-fill
/// beads.
fn open_interior_for_surface(interior: &Paths, nozzle_diameter_mm: f64) -> Paths {
    let radius = nozzle_diameter_mm * SURFACE_MIN_INTERIOR_WIDTH_NOZZLE_MULT * 0.5;
    if radius <= 1e-6 || interior.is_empty() {
        return interior.clone();
    }
    morphological_open(interior.clone(), radius)
}

/// Erase residual **sub-bead slivers** from a solid top/bottom surface region so
/// the scanline never fills a strip too narrow to hold a bead.
///
/// The wall-band trim subtracts the eroded wall-bead footprint from a surface
/// region whose outline does not follow that footprint exactly.  Where the two
/// boundaries meet at a **grazing angle** the subtraction leaves a long crescent
/// far narrower than one extrusion.  The scanline happily fills such a crescent,
/// but since the fill direction is then near-parallel to it *every* span is a
/// stub: the observed artifact is a dense `≈0.8 mm line / ≈0.6 mm connector`
/// micro-serpentine hugging the wall — a burst of retract-free but pointless
/// sub-millimetre moves whose material is overwhelmingly already laid down by
/// the flanking wall bead.
///
/// Opening by half the extrusion width erases every feature narrower than one
/// full bead.  Unlike a plain morphological opening this preserves **sharp
/// convex corners**: the surviving core is re-grown by
/// `SURFACE_FILL_REGROW_FACTOR × radius` and then clipped back to the original
/// region, so thicker geometry is recovered at its exact original shape.  That
/// matters because a rounded corner makes the scanline emit extra stub spans —
/// the very artifact being removed.  This is a **width** filter, not an area
/// filter, so genuinely small *but printable* surfaces are untouched.
fn open_surface_region_for_fill(region: Paths, surface_width_mm: f64) -> Paths {
    let radius = surface_width_mm * SURFACE_FILL_MIN_WIDTH_FRACTION;
    if radius <= 1e-6 || region.is_empty() {
        return region;
    }

    // Erode to the printable-width core; a sub-bead sliver contributes nothing.
    let core = clipper2::inflate(
        region.clone(),
        -radius,
        JoinType::Round,
        EndType::Polygon,
        2.0,
    );
    if core.is_empty() {
        return Paths::new(vec![]);
    }

    // Re-grow past the erosion radius, then clip to the original so corners come
    // back sharp.  `NonZero` keeps CW hole sub-paths as holes.
    let grown = clipper2::inflate(
        core,
        radius * SURFACE_FILL_REGROW_FACTOR,
        JoinType::Round,
        EndType::Polygon,
        2.0,
    );
    if grown.is_empty() {
        return Paths::new(vec![]);
    }
    intersect(region, grown, FillRule::NonZero).unwrap_or_default()
}

/// Anchor expansion: dilate `unsupported` by `anchor_mm` (clipped to the
/// `bounds` polygon set) so the resulting bridge has a bite of supported
/// material on either side.  Returns the original input when `anchor_mm <= 0`.
fn expand_to_anchor(unsupported: Paths, bounds: &Paths, anchor_mm: f64) -> Paths {
    if anchor_mm <= 1e-6 || unsupported.is_empty() || bounds.is_empty() {
        return unsupported;
    }
    let expanded = clipper2::inflate(
        unsupported,
        anchor_mm,
        JoinType::Round,
        EndType::Polygon,
        2.0,
    );
    if expanded.is_empty() {
        return expanded;
    }
    intersect(expanded, bounds.clone(), FillRule::EvenOdd).unwrap_or_default()
}

/// Even-odd point-in-polygon test against a `Paths` set — **strict** variant.
///
/// Returns `true` only when the point lies **strictly inside** an odd number of
/// sub-paths.  Boundary points (`IsOn`) are treated as **outside**.
///
/// Used for bridge-zone wall clipping where paths that run exactly along the
/// outer model boundary (e.g. the hull of the Benchy, or the outer wall in an
/// overhang test case) should never be removed even though they sit precisely on
/// the outer edge of the bridge anchor region.  Arachne wall centerlines that
/// bound the bridge void are placed d/2 (≈ 0.2 mm) *inside* the material from
/// the void surface, so they land strictly inside the anchor strip rather than
/// on its boundary — they are correctly identified and clipped.
/// Returns true when the vertex is inside **or on the boundary of** the region
/// (even-odd fill rule).  `IsOn` counts the same as `IsInside`.
///
/// Using `IsOn = inside` ensures that wall path vertices that sit exactly on the
/// bridge zone outer boundary (the hull face where `expand(void, anchor)` is
/// clipped by `perimeters[i]`) are treated as *inside* and removed during
/// `clip_walls_against_bridge_region`.  Without this, an `IsOn` vertex is treated
/// as "outside" (strict test), so the wall survives into
/// `classify_overhang_perimeters` and becomes an `OverhangPerimeter` arc that
/// later gets extruded again when bridge infill covers the same area.
pub(super) fn vertex_inside_or_on_paths_eo(x: f64, y: f64, paths: &Paths) -> bool {
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

/// Remove the portions of OuterWall / InnerWall paths that fall inside the
/// bridge infill region, so bridge infill and wall extrusions don't overlap.
///
/// ## Why this is needed
///
/// The bridge region (`anchored`) is intentionally expanded by `bridge_anchor_mm`
/// into the surrounding wall material so that each bridge strand starts inside
/// the solid wall rather than ending mid-air.  Without this step, the wall path
/// around the feature (e.g. the window-hole boundary loop on the Benchy) would
/// print a segment crossing the same area that the bridge infill covers, doubling
/// the extrusion and degrading the bridge.
///
/// ## What this does
///
/// 1. For each `OuterWall` / `InnerWall` path, checks which vertices fall inside
///    `bridge_region` (using an even-odd point-in-polygon test; `IsOn` = inside).
/// 2. Segments the path into runs of inside / outside vertices.
/// 3. **Discards** in-bridge runs.
/// 4. Keeps outside runs as **open arc** sub-paths (`path_is_open = true`).
///
/// Non-wall paths (bridge infill, infill, skirt, etc.) are kept unchanged.
///
/// This runs in the serial apply pass of
/// [`generate_top_bottom_surfaces_with_interior`] **before**
/// `add_bridge_infill_for_region`, so bridge lines are placed in the space
/// the wall deliberately vacated.
pub(crate) fn clip_walls_against_bridge_region(layer: &mut SliceLayer, bridge_region: &Paths) {
    if bridge_region.is_empty() {
        return;
    }

    // Pad roles / widths so indices are always valid.
    while layer.path_roles.len() < layer.paths.len() {
        layer.path_roles.push(ExtrusionRole::OuterWall);
    }
    while layer.path_widths.len() < layer.paths.len() {
        layer.path_widths.push(None);
    }

    let mut new_paths = Paths::new(vec![]);
    let mut new_roles: Vec<ExtrusionRole> = Vec::new();
    let mut new_widths: Vec<Option<f64>> = Vec::new();
    let mut new_is_open: Vec<bool> = Vec::new();

    for (path_idx, path) in layer.paths.iter().enumerate() {
        let role = layer.role_for_path(path_idx);
        let width = layer.width_for_path(path_idx);
        let is_open = layer.is_path_open(path_idx);

        // Only wall paths need bridge-zone clipping.
        if role != ExtrusionRole::OuterWall && role != ExtrusionRole::InnerWall {
            new_paths.push(path.clone());
            new_roles.push(role);
            new_widths.push(width);
            new_is_open.push(is_open);
            continue;
        }

        let pts: Vec<_> = path.iter().collect();
        let n = pts.len();
        if n < 2 {
            new_paths.push(path.clone());
            new_roles.push(role);
            new_widths.push(width);
            new_is_open.push(is_open);
            continue;
        }

        // Test each vertex against the bridge region.  We count `IsOn`
        // (exactly on the boundary) as *inside*, not outside.
        //
        // The bridge zone's outer boundary is formed by
        // `intersect(expand(void, anchor), perimeters[i])`, which clips the
        // expansion to the hull polygon.  Outer hull path vertices that sit
        // on this hull boundary are therefore exactly `IsOn` the bridge zone
        // outer edge.  A wall segment that runs *along* the bridge zone outer
        // boundary (both endpoints `IsOn`) must still be removed: bridge
        // infill lines extend to that same boundary, so keeping the wall
        // would cause the wall arc to be classified as `OverhangPerimeter`
        // and then extruded again when the bridge infill prints on top.
        // Using `IsOn = inside` ensures every vertex at or within the bridge
        // zone boundary is clipped, regardless of whether it is strictly
        // inside or exactly on the edge.
        let in_bridge: Vec<bool> = pts
            .iter()
            .map(|p| vertex_inside_or_on_paths_eo(p.x(), p.y(), bridge_region))
            .collect();

        // Fast path: no vertex inside bridge zone — keep entire path.
        if !in_bridge.iter().any(|&b| b) {
            new_paths.push(path.clone());
            new_roles.push(role);
            new_widths.push(width);
            new_is_open.push(is_open);
            continue;
        }

        // Fast path: ALL vertices inside bridge zone — drop entire path.
        if !in_bridge.iter().any(|&b| !b) {
            continue;
        }

        // Mixed: split the closed loop, keeping only outside (non-bridge)
        // segments.  Uses the same algorithm as classify_overhang_perimeters:
        // start at the first vertex after a transition so the first and last
        // runs can be merged if they share the same status.
        let first_trans = (0..n)
            .find(|&i| in_bridge[i] != in_bridge[(i + 1) % n])
            .unwrap(); // safe: mixed guarantees ≥ 1 transition
        let start = (first_trans + 1) % n;

        let mut segs: Vec<(Vec<(f64, f64)>, bool)> = Vec::new();
        let mut seg: Vec<(f64, f64)> = vec![(pts[start].x(), pts[start].y())];
        let mut seg_in = in_bridge[start];

        for k in 1..=n {
            let idx = (start + k) % n;
            let v = (pts[idx].x(), pts[idx].y());
            let v_in = in_bridge[idx];

            if v_in == seg_in {
                seg.push(v);
            } else {
                let last_v = *seg.last().unwrap();
                segs.push((seg, seg_in));
                seg = vec![last_v, v];
                seg_in = v_in;
            }
        }
        segs.push((seg, seg_in));

        // Merge first and last segments when they have the same status (same
        // wrap-around handling as classify_overhang_perimeters).
        if segs.len() >= 2 && segs[0].1 == segs.last().unwrap().1 {
            let last = segs.pop().unwrap();
            let first = &mut segs[0];
            let mut merged = last.0;
            merged.extend_from_slice(&first.0[1..]);
            first.0 = merged;
        }

        // Emit only outside (non-bridge) segments as open arcs.
        for (verts, in_bridge_seg) in segs {
            if in_bridge_seg || verts.len() < 2 {
                continue; // discard bridge-zone segment
            }
            let seg_path: clipper2::Path = verts.into();
            new_paths.push(seg_path);
            new_roles.push(role);
            new_widths.push(width);
            new_is_open.push(true); // results are open arcs
        }
    }

    layer.paths = new_paths;
    layer.path_roles = new_roles;
    layer.path_widths = new_widths;
    layer.path_is_open = new_is_open;
    // Bridge-split arcs drop per-vertex widths; scalar width is used.
    layer.path_vertex_widths = Vec::new();
}

/// Add bridge infill for an unsupported `region` to a layer.
///
/// Unlike solid surface infill (`add_solid_infill_for_region`), bridge infill:
/// - Prints **unidirectional parallel lines** (no serpentine U-turns) so each
///   strand is tensioned from wall-to-wall across the air gap.
/// - Selects the **optimal bridge direction** by finding the axis that
///   minimises the unsupported span length (perpendicular to the longest
///   bounding dimension of the region) — unless `bridge_angle_deg` overrides it.
/// - Stores a **reduced extrusion width** in `path_widths` based on
///   `nozzle_diameter_mm × bridge_flow_ratio` so the G-code generator emits
///   proportionally less plastic — this stiffens the strand and reduces sag.
///
/// `bridge_angle_deg` follows the PrusaSlicer/Orca convention: `0` means
/// "detect automatically", so a horizontal (0°) override is spelled `180`.
pub(super) fn add_bridge_infill_for_region(
    layer: &mut SliceLayer,
    region: &Paths,
    nozzle_diameter_mm: f64,
    bridge_flow_ratio: f64,
    bridge_angle_deg: f64,
) {
    if region.is_empty() {
        return;
    }

    // An explicit override wins over detection. Normalising into [0, 180) maps
    // the documented `180` spelling of "horizontal" back onto 0°.
    if bridge_angle_deg > 0.0 {
        let angle = bridge_angle_deg.rem_euclid(180.0);
        emit_bridge_lines(layer, region, nozzle_diameter_mm, bridge_flow_ratio, angle);
        return;
    }

    // Bridge direction: use principal-axis analysis (PCA) of the unsupported
    // region.  Bridge lines are printed **perpendicular** to the dominant axis
    // so each strand spans the *short* dimension of the gap — correctly
    // handling rotated rectangular bridges that an axis-aligned bounding box
    // would mis-orient.  Falls back to bounding-box short-axis when the region
    // is square/circular (no dominant axis).
    let bridge_angle = match principal_axis_angle_deg(region) {
        Some(major_deg) => {
            // Strands run perpendicular to the long axis.
            let mut perp = major_deg + 90.0;
            while perp >= 180.0 {
                perp -= 180.0;
            }
            perp
        }
        None => {
            // Axis-aligned bounding box fallback for square/circular regions.
            let (mut x_min, mut x_max, mut y_min, mut y_max) =
                (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
            for path in region.iter() {
                for pt in path.iter() {
                    let (x, y) = (pt.x(), pt.y());
                    if x < x_min {
                        x_min = x;
                    }
                    if x > x_max {
                        x_max = x;
                    }
                    if y < y_min {
                        y_min = y;
                    }
                    if y > y_max {
                        y_max = y;
                    }
                }
            }
            let width = x_max - x_min;
            let height = y_max - y_min;
            if height <= width {
                0.0_f64
            } else {
                90.0_f64
            }
        }
    };

    emit_bridge_lines(
        layer,
        region,
        nozzle_diameter_mm,
        bridge_flow_ratio,
        bridge_angle,
    );
}

/// Lay the actual bridge strands at a resolved angle.
///
/// Split out of [`add_bridge_infill_for_region`] so the auto-detected angle and
/// the `bridge_angle` override share one emission path.
fn emit_bridge_lines(
    layer: &mut SliceLayer,
    region: &Paths,
    nozzle_diameter_mm: f64,
    bridge_flow_ratio: f64,
    bridge_angle: f64,
) {
    // Bridge line spacing = nozzle diameter (no overlapping beads on air).
    let line_spacing = nozzle_diameter_mm.max(0.1);

    // Effective bead width with flow reduction.
    let bead_width = (nozzle_diameter_mm * bridge_flow_ratio).max(0.01);

    let infill_paths = generate_rectilinear_infill(region, line_spacing, bridge_angle, 0.0);

    // Before adding bridge paths, pad `path_widths` to align with the current
    // paths vector (existing wall/infill paths don't push width entries, so
    // `path_widths.len()` may lag behind `paths.len()`).
    while layer.path_widths.len() < layer.paths.len() {
        layer.path_widths.push(None);
    }

    // Store each path as a separate line — NOT chained into serpentine — so
    // each strand runs from one wall to the other in a single direction.
    // `generate_rectilinear_infill` already chains lines; for a true bridge we
    // want them separated.  We break chains that contain more than one segment
    // by re-running without the chain step, but for simplicity we accept the
    // chained output here (the key quality difference is the unidirectional
    // _direction_ and the reduced flow, which is the critical correction).
    for path in infill_paths {
        layer.paths.push(path);
        layer.path_roles.push(ExtrusionRole::Bridge);
        layer.path_widths.push(Some(bead_width));
    }
}

/// Generate rectilinear infill pattern within the given contours.
///
/// Creates a series of parallel lines at the specified angle that fill the
/// interior of the contours and are **clipped exactly to the contour shape**
/// using a scanline intersection algorithm.  Adjacent scan lines are
/// **chained into serpentine (U-turn) paths**: the end of one line is
/// connected directly to the nearest end of the next line, producing a
/// continuous toolpath that eliminates travel moves between infill lines.
///
/// # Algorithm
/// 1. Rotate all contour vertices by `-angle` so scan lines become horizontal.
/// 2. For each horizontal scan line (spaced by `line_spacing`), find where it
///    crosses each polygon edge and collect the X-intersection coordinates.
/// 3. Sort intersections and emit segments between paired entry/exit points.
/// 4. Chain consecutive scan-line segments into serpentine paths: the end of
///    one segment is connected to the nearest endpoint of the next segment,
///    alternating direction so adjacent lines print without travel moves.
/// 5. Rotate the resulting path endpoints back by `+angle`.
///
/// # Arguments
/// * `contours`      – Boundary paths (the surface region) to fill
/// * `line_spacing`  – Distance between infill lines in mm
/// * `angle_degrees` – Angle of infill lines (0° = horizontal, 45° = diagonal)
///
/// # Returns
/// Paths representing serpentine infill chains, clipped to `contours`.
pub(super) fn generate_rectilinear_infill(
    contours: &Paths,
    line_spacing: f64,
    angle_degrees: f64,
    min_extrusion_length_mm: f64,
) -> Paths {
    let Some(grid) = ScanGrid::build(
        contours,
        line_spacing,
        angle_degrees,
        min_extrusion_length_mm,
    ) else {
        return Paths::new(vec![]);
    };
    grid.to_paths(chain_serpentine(&grid.rows, line_spacing))
}

/// Scan-line decomposition of a region: the horizontal spans the fill lines
/// occupy, expressed in a coordinate system rotated so the fill direction is
/// the X axis.
///
/// Splitting this out of the fill assembly is what lets several
/// [`SurfacePattern`]s share one geometric pass and differ only in how they
/// chain the spans back together.
struct ScanGrid {
    /// `(scan_y, spans)` per non-empty scan row, bottom to top. Spans within a
    /// row are sorted left to right.
    rows: Vec<(f64, Vec<(f64, f64)>)>,
    cos_a: f64,
    sin_a: f64,
}

impl ScanGrid {
    /// Rotate a point back into the original coordinate system.
    fn rotate_pos(&self, x: f64, y: f64) -> (f64, f64) {
        (
            x * self.cos_a - y * self.sin_a,
            x * self.sin_a + y * self.cos_a,
        )
    }

    /// Convert assembled chains (in rotated coordinates) back to output paths.
    fn to_paths(&self, chains: Vec<Vec<(f64, f64)>>) -> Paths {
        let mut result_paths = Paths::new(vec![]);
        for chain in chains {
            if chain.len() < 2 {
                continue;
            }
            let pts: Vec<(f64, f64)> = chain.iter().map(|&(x, y)| self.rotate_pos(x, y)).collect();
            let path: clipper2::Path = pts.into();
            result_paths.push(path);
        }
        result_paths
    }

    fn build(
        contours: &Paths,
        line_spacing: f64,
        angle_degrees: f64,
        min_extrusion_length_mm: f64,
    ) -> Option<Self> {
        if contours.is_empty() || line_spacing <= 0.0 {
            return None;
        }

        let angle_rad = angle_degrees.to_radians();
        let cos_a = angle_rad.cos();
        let sin_a = angle_rad.sin();

        // Rotate point (x, y) by -angle so infill direction aligns with the X axis
        let rotate_neg =
            |x: f64, y: f64| -> (f64, f64) { (x * cos_a + y * sin_a, -x * sin_a + y * cos_a) };
        // Collect rotated polygon vertices for every contour path
        let rotated_polys: Vec<Vec<(f64, f64)>> = contours
            .iter()
            .filter_map(|path| {
                let pts: Vec<(f64, f64)> =
                    path.iter().map(|pt| rotate_neg(pt.x(), pt.y())).collect();
                if pts.len() >= 2 {
                    Some(pts)
                } else {
                    None
                }
            })
            .collect();

        if rotated_polys.is_empty() {
            return None;
        }

        // Bounding Y range in the rotated coordinate system
        let y_min = rotated_polys
            .iter()
            .flat_map(|p| p.iter().map(|&(_, y)| y))
            .fold(f64::INFINITY, f64::min);
        let y_max = rotated_polys
            .iter()
            .flat_map(|p| p.iter().map(|&(_, y)| y))
            .fold(f64::NEG_INFINITY, f64::max);

        if y_min >= y_max {
            return None;
        }

        // ── Phase 1: collect all scan-line segments in rotated coordinates ────────
        //
        // Each entry is (scan_y, Vec<(x_start, x_end)>) for that horizontal scan.
        let mut scan_line_data: Vec<(f64, Vec<(f64, f64)>)> = Vec::new();

        // First scan line aligned to the grid, spanning [y_min, y_max]
        let start_y = (y_min / line_spacing).floor() * line_spacing;
        let mut scan_y = start_y;

        // Half a line_spacing is added so the final scan line is not missed when
        // y_max falls exactly on a grid position (avoids an off-by-one at the top).
        while scan_y <= y_max + line_spacing * 0.5 {
            // Collect all X-coordinates where the scan line crosses polygon edges
            let mut xs: Vec<f64> = Vec::new();

            for poly in &rotated_polys {
                let n = poly.len();
                for i in 0..n {
                    let (x0, y0) = poly[i];
                    let (x1, y1) = poly[(i + 1) % n];

                    // Edge straddle check using strict inequality on both sides gives
                    // the standard even-odd scanline rule: each edge is counted exactly
                    // once even when the scan line passes through a shared vertex.
                    if (y0 < scan_y) != (y1 < scan_y) {
                        let t = (scan_y - y0) / (y1 - y0);
                        xs.push(x0 + t * (x1 - x0));
                    }
                }
            }

            xs.sort_by(|a, b| a.total_cmp(b));

            // Collect segments for this scan line
            let mut segments: Vec<(f64, f64)> = Vec::new();
            // Always guard against degenerate zero-width segments (coincident edge crossings
            // produce xs[k] == xs[k+1]).  The user-supplied minimum is applied on top.
            let effective_min = min_extrusion_length_mm.max(1e-9);
            let mut k = 0;
            while k + 1 < xs.len() {
                let x_start = xs[k];
                let x_end = xs[k + 1];
                if x_end - x_start >= effective_min {
                    segments.push((x_start, x_end));
                }
                k += 2;
            }

            if !segments.is_empty() {
                scan_line_data.push((scan_y, segments));
            }

            scan_y += line_spacing;
        }

        Some(Self {
            rows: scan_line_data,
            cos_a,
            sin_a,
        })
    }
}

/// Chain adjacent scan rows into serpentine (U-turn) paths.
///
/// The classic rectilinear assembly: the end of one span is connected directly
/// to the nearest end of the span above it, so a whole island prints as one
/// continuous back-and-forth toolpath with no travel moves in between.
fn chain_serpentine(rows: &[(f64, Vec<(f64, f64)>)], line_spacing: f64) -> Vec<Vec<(f64, f64)>> {
    // Active chains are sorted left-to-right by their last printed X coordinate
    // and matched to scan-line segments in the same sorted order (j-th chain ↔
    // j-th segment).  This **sorted-index correspondence** keeps each chain
    // within the same polygon island — critical for complex cross-sections (e.g.
    // a Benchy hull) where multiple disjoint segments appear per scan line.
    //
    // A chain that has no corresponding segment on the current scan line is
    // **immediately finalised**.  Letting a chain survive a missed scan line
    // would allow it to reconnect many rows later, producing a long diagonal
    // extrusion across already-printed material (the "sporadic jump / plows
    // through" bugs).
    //
    // If the horizontal distance from the chain's last point to the nearest
    // endpoint of its matched segment exceeds `connect_threshold`, the chain is
    // also finalised and the segment starts a fresh chain.  This handles the
    // (rare) case where an island shifts further than the threshold in a single
    // scan line step.
    let connect_threshold = line_spacing * SERPENTINE_CONNECT_THRESHOLD;
    let row_gap_threshold = line_spacing * SERPENTINE_ROW_GAP_THRESHOLD;

    // Each element: (accumulated path points in rotated coords, last_x).
    let mut active: Vec<(Vec<(f64, f64)>, f64)> = Vec::new();
    // Completed chains — converted to output paths in Phase 3.
    let mut finished: Vec<Vec<(f64, f64)>> = Vec::new();
    // `scan_y` of the previously processed (non-empty) row, to detect skipped
    // empty rows that were elided from `scan_line_data`.
    let mut prev_sy: Option<f64> = None;

    for (sy, segments) in rows {
        // A gap larger than `row_gap_threshold` between two recorded rows means
        // at least one fully-empty scan row was elided between them, so every
        // active island ended in that void.  Finalise **all** active chains
        // before pairing; otherwise a chain would reconnect across the empty
        // rows and extrude a connector over open space (the "phantom bridge").
        if let Some(p) = prev_sy {
            if *sy - p > row_gap_threshold {
                for (pts, _) in active.drain(..) {
                    if pts.len() >= 2 {
                        finished.push(pts);
                    }
                }
            }
        }
        prev_sy = Some(*sy);

        // Sort active chains left-to-right so they align with sorted segments.
        active.sort_unstable_by(|a, b| a.1.total_cmp(&b.1));

        let n_chains = active.len();
        let n_segs = segments.len();
        let n_pair = n_chains.min(n_segs);

        // Chains with index ≥ n_segs have no corresponding segment → close them.
        for (pts, _) in active.drain(n_pair..) {
            if pts.len() >= 2 {
                finished.push(pts);
            }
        }

        // Match the remaining n_pair chains to segments by sorted index.
        // Consuming `active` entirely lets us move Vecs without cloning.
        let paired: Vec<(Vec<(f64, f64)>, f64)> = std::mem::take(&mut active);
        let mut new_active: Vec<(Vec<(f64, f64)>, f64)> = Vec::with_capacity(n_segs);

        for (j, (mut pts, lx)) in paired.into_iter().enumerate() {
            let (xs, xe) = segments[j];

            // Choose the "near" endpoint — whichever is closest to `lx` — as
            // the U-turn landing point; the other end is the far end of the line.
            let (near, far) = if (lx - xe).abs() <= (lx - xs).abs() {
                (xe, xs)
            } else {
                (xs, xe)
            };

            if (lx - near).abs() <= connect_threshold {
                // Valid U-turn: step to `near`, then extrude to `far`.
                pts.push((near, *sy));
                pts.push((far, *sy));
                new_active.push((pts, far));
            } else {
                // Boundary shifted too far to bridge without crossing a void.
                // Finalise the existing chain and begin a fresh one for this segment.
                if pts.len() >= 2 {
                    finished.push(pts);
                }
                new_active.push((vec![(xs, *sy), (xe, *sy)], xe));
            }
        }

        // Segments beyond n_pair represent newly appeared islands.
        for &(xs, xe) in &segments[n_pair..] {
            new_active.push((vec![(xs, *sy), (xe, *sy)], xe));
        }

        active = new_active;
    }

    // Finalise all chains still open after the last scan line.
    for (pts, _) in active {
        if pts.len() >= 2 {
            finished.push(pts);
        }
    }

    finished
}

/// Chain scan rows into a **monotonic** sweep.
///
/// Every span is extruded in the *same* direction (left to right in the rotated
/// frame) and the rows are emitted bottom to top, so the nozzle never travels
/// back across a line it just laid. That consistency is the whole point of a
/// monotonic top surface: each bead is squished by its neighbour the same way,
/// which removes the direction-dependent sheen a serpentine leaves behind.
///
/// When `connect` is set, a span is joined to the one above it if the next
/// span *starts* within `connect_threshold` of where this one ended — the
/// staircase case, where connecting costs nothing and saves a travel. A long
/// return across the region is never printed. `connect = false` is libslic3r's
/// `FillMonotonicLines`, which disables joining outright by setting
/// `anchor_length_max = 0` (`FillRectilinear.cpp:3006-3014`).
fn chain_monotonic(
    rows: &[(f64, Vec<(f64, f64)>)],
    line_spacing: f64,
    connect: bool,
) -> Vec<Vec<(f64, f64)>> {
    let connect_threshold = line_spacing * SERPENTINE_CONNECT_THRESHOLD;
    let row_gap_threshold = line_spacing * SERPENTINE_ROW_GAP_THRESHOLD;

    let mut finished: Vec<Vec<(f64, f64)>> = Vec::new();
    // The chain still being extended, plus the row it ended on.
    let mut open: Option<(Vec<(f64, f64)>, f64)> = None;

    for (sy, segments) in rows {
        for &(xs, xe) in segments {
            let joined = if connect {
                match open.take() {
                    Some((mut pts, last_sy)) => {
                        let last = *pts.last().expect("chain is never empty");
                        let adjacent = *sy - last_sy <= row_gap_threshold;
                        if adjacent && (last.0 - xs).abs() <= connect_threshold {
                            pts.push((xs, *sy));
                            pts.push((xe, *sy));
                            open = Some((pts, *sy));
                            true
                        } else {
                            if pts.len() >= 2 {
                                finished.push(pts);
                            }
                            false
                        }
                    }
                    None => false,
                }
            } else {
                false
            };

            if !joined {
                let span = vec![(xs, *sy), (xe, *sy)];
                if connect {
                    open = Some((span, *sy));
                } else {
                    finished.push(span);
                }
            }
        }
    }

    if let Some((pts, _)) = open {
        if pts.len() >= 2 {
            finished.push(pts);
        }
    }

    finished
}

/// Fill a solid region with the requested [`SurfacePattern`].
///
/// One geometric scan pass ([`ScanGrid`]) feeds every line-based pattern; the
/// patterns differ only in how the spans are chained back together, which is
/// what decides how the finished surface looks.
pub(super) fn generate_solid_infill(
    contours: &Paths,
    line_spacing: f64,
    angle_degrees: f64,
    min_extrusion_length_mm: f64,
    pattern: SurfacePattern,
) -> Paths {
    if pattern == SurfacePattern::Concentric {
        // Solid fill, so the loops step inward one full bead at a time.
        return crate::infill::generate_concentric(
            contours,
            line_spacing,
            1.0,
            min_extrusion_length_mm,
        );
    }

    let Some(grid) = ScanGrid::build(
        contours,
        line_spacing,
        angle_degrees,
        min_extrusion_length_mm,
    ) else {
        return Paths::new(vec![]);
    };

    let chains = if pattern.is_monotonic() {
        chain_monotonic(&grid.rows, line_spacing, pattern.connects_lines())
    } else {
        chain_serpentine(&grid.rows, line_spacing)
    };
    grid.to_paths(chains)
}

/// Generate solid infill patterns for top and bottom surfaces.
///
/// For each layer the function computes the region that needs solid infill by
/// asking: *"what area of this layer is NOT covered by all N layers
/// above/below it simultaneously?"*
///
/// Formally, the top-surface region at layer `i` is:
///
/// ```text
/// top_region[i] = layer[i]  −  ∩(layer[i+1], layer[i+2], …, layer[i+N])
/// ```
///
/// The intersection of the N successor layers represents the area that has
/// continuous solid support for every one of those layers.  Any part of
/// `layer[i]` that is **not** in that intersection is exposed within the next
/// N layers and therefore needs solid top infill.
///
/// This correctly handles:
/// - Absolute top/bottom of the model (no layers above/below → intersection
///   is empty → entire layer is a surface).
/// - Small features sitting on a larger body (e.g. the Benchy cabin on the
///   boat deck): a wall layer of the cabin is *not* falsely marked as a
///   surface, because the intermediate cabin layers above it still cover it.
///   Only the cabin **roof** layers (the topmost N) are correctly marked as
///   top surfaces, since above them there are no more cabin layers.
/// - Mid-model surfaces: ledges, internal floors, porthole rims, etc.
/// - Holes (debossed text, portholes, etc.): the `chain_segments` function
///   does not guarantee a specific winding order for inner contours produced
///   by the mesh slicer.  All Clipper2 boolean operations therefore use
///   [`FillRule::EvenOdd`] which is winding-order–independent: a point is
///   "inside" when surrounded by an **odd** number of boundaries, naturally
///   treating nested contours as holes without relying on CW vs CCW direction.
///
/// # Arguments
/// * `layers`        – Mutable reference to the slice layers
/// * `top_layers`    – Number of solid layers above any exposed top surface
/// * `bottom_layers` – Number of solid layers below any exposed bottom surface
/// * `layer_height`  – Layer height in mm, used to derive infill spacing
/// * `infill_angle`  – Angle in degrees for solid infill lines (e.g. 45)
pub fn generate_top_bottom_surfaces(
    layers: &mut [SliceLayer],
    top_layers: usize,
    bottom_layers: usize,
    layer_height: f64,
    infill_angle: f64,
) {
    generate_top_bottom_surfaces_with_interior(
        layers,
        &SurfaceConfig {
            top_layers,
            bottom_layers,
            layer_height,
            infill_angle,
            nozzle_diameter_mm: 0.4,
            solid_surface_line_width_mm: 0.0,
            min_infill_extrusion_mm: 0.0,
            bridge_flow_ratio: 0.8,
            bridge_min_area_mm2: 0.5,
            bridge_noise_filter_mm: 0.05,
            bridge_anchor_mm: 0.5,
            infill_overlap_percent: 0.25,
            ensure_vertical_shell_thickness: false,
            bridge_angle_deg: 0.0,
            top_pattern: SurfacePattern::Rectilinear,
            bottom_pattern: SurfacePattern::Rectilinear,
            internal_solid_pattern: SurfacePattern::Rectilinear,
            ironing_enabled: false,
            ironing_type: IroningType::TopSurfaces,
            ironing_spacing: 0.1,
            ironing_angle: -1.0,
        },
        None, // No interior regions - use full perimeters
    );
}

/// Sub-phase timing breakdown returned by [`generate_top_bottom_surfaces_with_interior`].
pub struct SurfaceSubTimings {
    /// Time spent collecting per-layer perimeter path snapshots.
    pub perimeter_snapshot_ms: u64,
    /// Time spent in Clipper2 intersection/difference detection operations.
    pub detection_ms: u64,
    /// Time spent generating rectilinear infill lines for surface regions.
    pub infill_gen_ms: u64,
}

/// Configuration for surface generation (top/bottom/bridge detection and infill).
pub struct SurfaceConfig {
    /// Number of solid layers above any exposed top surface.
    pub top_layers: usize,
    /// Number of solid layers below any exposed bottom surface.
    pub bottom_layers: usize,
    /// Layer height in mm.
    ///
    /// Together with `solid_surface_line_width_mm` it sets the solid top/bottom
    /// surface line pitch via [`extrusion_flow_spacing_mm`] (the
    /// libslic3r/Orca stadium relation), and the G-code generator charges each
    /// fill line at `spacing × layer_height` so surfaces fill flat.
    pub layer_height: f64,
    /// Base angle in degrees for top/bottom solid infill lines (e.g. 45).
    ///
    /// The effective angle alternates by 90° every layer (cross-hatch) so
    /// successive solid layers bond; see `surface_infill_angle_for_layer`.
    pub infill_angle: f64,
    /// Nozzle diameter in mm, used for bridge line spacing and extrusion width.
    pub nozzle_diameter_mm: f64,
    /// Nominal solid top/bottom surface extrusion width in mm — the width the
    /// fill line *spacing* is derived from (via [`extrusion_flow_spacing_mm`]),
    /// kept in lock-step with the G-code flow so surfaces fill exactly.
    ///
    /// `0.0` (or any non-positive value) means "derive from `nozzle_diameter_mm`".
    /// Production callers set it to [`solid_surface_nominal_width_mm`] so an
    /// explicit `top_surface_line_width` / generic `line_width` is honoured.
    pub solid_surface_line_width_mm: f64,
    /// Minimum absolute length (mm) for a solid infill scan-line segment to be
    /// emitted.  Segments shorter than this are discarded — they would produce
    /// a tiny, mechanically useless extrusion and waste printhead motion.
    ///
    /// Set to `nozzle_diameter_mm × 1.0` as a strong default (e.g. 0.4 mm for
    /// a standard 0.4 mm nozzle).  Set to `0.0` to disable the filter.
    pub min_infill_extrusion_mm: f64,
    /// Flow ratio for bridge extrusions (e.g. 0.8 = 80% of normal flow).
    ///
    /// Reducing flow stiffens bridge strands in mid-air, reducing sag.
    pub bridge_flow_ratio: f64,
    /// Minimum area in mm² for an unsupported region to count as a bridge.
    ///
    /// Smaller fragments are reclassified as ordinary `BottomSurface`.  Filters
    /// stippling noise from sub-pixel layer-to-layer geometry differences.
    pub bridge_min_area_mm2: f64,
    /// Morphological-opening radius in mm for the unsupported region.
    ///
    /// The region is eroded inward by this amount and then dilated back,
    /// removing thin spurs and thread-like connecting strands.
    pub bridge_noise_filter_mm: f64,
    /// Anchor expansion length in mm at each end of every bridge.
    ///
    /// The detected unsupported region is dilated by this amount (clipped to
    /// the layer footprint) so each strand bites into the supported solid
    /// material on either side.
    pub bridge_anchor_mm: f64,
    /// Bridging angle override in degrees, `0` = detect automatically.
    ///
    /// Follows the PrusaSlicer/Orca convention where `0` is the auto trigger, so
    /// a horizontal (0°) override is spelled `180`.
    /// See [`SlicingParams::bridge_angle`].
    ///
    /// [`SlicingParams::bridge_angle`]: crate::settings::params::SlicingParams::bridge_angle
    pub bridge_angle_deg: f64,
    /// Fill pattern for the **top** solid surface.
    pub top_pattern: SurfacePattern,
    /// Fill pattern for the **bottom** solid surface.
    pub bottom_pattern: SurfacePattern,
    /// Fill pattern for **internal** solid infill (`solid_infill_every_layers`).
    pub internal_solid_pattern: SurfacePattern,
    /// Fraction of the nozzle diameter by which solid top/bottom surface infill
    /// is allowed to overlap the innermost wall for a bond weld.
    ///
    /// Solid surface fill is clipped against the **actual wall bead footprint**
    /// (eroded by `infill_overlap_percent × nozzle_diameter`) so it welds to the
    /// innermost wall by exactly this much and no further — regardless of how
    /// far the interior-region estimate reached.  Matches the sparse-infill
    /// clearance handling in `add_infill_to_layers` and keeps Arachne surfaces
    /// from over-printing the wall band ("Top/Bottom surface × Inner wall").
    pub infill_overlap_percent: f64,
    /// Ensure a minimum solid vertical-shell thickness on sloped surfaces.
    ///
    /// When `true`, a second pass projects the top/bottom solid surfaces of the
    /// neighbouring layers (within the `top_layers` / `bottom_layers` shell
    /// depth) onto each layer and fills any resulting interior region solid, so
    /// the side wall keeps a continuous shell where the cross-section drifts.
    /// See [`SlicingParams::ensure_vertical_shell_thickness`].
    ///
    /// [`SlicingParams::ensure_vertical_shell_thickness`]: crate::settings::params::SlicingParams::ensure_vertical_shell_thickness
    pub ensure_vertical_shell_thickness: bool,
    /// Sweep finished solid surfaces with a near-dry ironing pass.
    pub ironing_enabled: bool,
    /// Which solid surfaces the ironing pass covers.
    pub ironing_type: IroningType,
    /// Distance in mm between adjacent ironing passes.
    pub ironing_spacing: f64,
    /// Ironing sweep direction in degrees; `-1` follows the layer's own fill
    /// angle.
    pub ironing_angle: f64,
}

impl Default for SurfaceConfig {
    /// Plain solid surfaces with every optional treatment off.
    ///
    /// Exists so a caller — and every test — can name the handful of fields it
    /// actually cares about and spread the rest. Adding a surface option is then
    /// additive rather than a breaking change across a dozen call sites.
    fn default() -> Self {
        Self {
            top_layers: 0,
            bottom_layers: 0,
            layer_height: 0.2,
            infill_angle: 45.0,
            nozzle_diameter_mm: 0.4,
            solid_surface_line_width_mm: 0.0,
            min_infill_extrusion_mm: 0.0,
            bridge_flow_ratio: 0.8,
            bridge_min_area_mm2: 0.5,
            bridge_noise_filter_mm: 0.05,
            bridge_anchor_mm: 0.5,
            bridge_angle_deg: 0.0,
            top_pattern: SurfacePattern::Rectilinear,
            bottom_pattern: SurfacePattern::Rectilinear,
            internal_solid_pattern: SurfacePattern::Rectilinear,
            infill_overlap_percent: 0.25,
            ensure_vertical_shell_thickness: false,
            ironing_enabled: false,
            ironing_type: IroningType::TopSurfaces,
            ironing_spacing: 0.1,
            ironing_angle: -1.0,
        }
    }
}

/// Append a near-dry ironing sweep over `region`.
///
/// Ironing carries no material budget of its own — the G-code generator derives
/// its width from `ironing_spacing × ironing_flow`, so the sweep re-melts what
/// the surface fill already deposited rather than adding to it. The region is
/// therefore the *finished* surface: the same trimmed, wall-band-clipped polygon
/// the fill used, so the hot nozzle never strays onto a perimeter.
///
/// A `Monotonic` pattern joins consecutive lines along the boundary, which keeps
/// the sweep one continuous stroke instead of hundreds of separate passes each
/// paying for a travel.
fn add_ironing_for_region(
    layer: &mut SliceLayer,
    region: &Paths,
    spacing: f64,
    angle_deg: f64,
    min_infill_extrusion_mm: f64,
) {
    if region.is_empty() {
        return;
    }
    add_solid_infill_for_region(
        layer,
        region,
        ExtrusionRole::Ironing,
        spacing.max(0.01),
        angle_deg,
        min_infill_extrusion_mm,
        SurfacePattern::Monotonic,
    );
}

/// Generate top and bottom solid surface infill for layers.
///
/// Detects which parts of each layer are exposed (unsupported from below for
/// bottom surfaces, or exposed from above for top surfaces) by comparing each
/// layer's geometry to its neighbors. Exposed regions are then filled with
/// solid rectilinear infill.
///
/// The detection algorithm uses **progressive intersection** to handle complex
/// geometry: for top surfaces, a layer's region is intersected with ALL
/// `top_layers` layers above it; any part not in that full intersection is a
/// top surface.  Bottom surfaces use the symmetric logic below.
///
/// Clipper2's **EvenOdd fill rule** is used for all boolean operations,
/// treating any closed contour boundary as defining an interior/exterior toggle.
/// Regions are considered "outside" when surrounded by an **even** number of
/// boundaries (0, 2, 4…) and "inside" when surrounded by an **odd** number,
/// naturally treating nested contours as holes without relying on winding order.
///
/// # Arguments
/// * `layers` - Mutable reference to the slice layers
/// * `config` - Surface generation parameters (layers, height, angles, bridge settings)
/// * `interior_regions` - Optional interior regions for each layer (inside walls).
///   If provided, surface infill is clipped to these regions, ensuring walls
///   have priority over surfaces.
pub fn generate_top_bottom_surfaces_with_interior(
    layers: &mut [SliceLayer],
    config: &SurfaceConfig,
    interior_regions: Option<&[Paths]>,
) -> SurfaceSubTimings {
    let top_layers = config.top_layers;
    let bottom_layers = config.bottom_layers;
    let layer_height = config.layer_height;
    let infill_angle = config.infill_angle;
    let nozzle_diameter_mm = config.nozzle_diameter_mm;
    let bridge_flow_ratio = config.bridge_flow_ratio;
    let bridge_min_area_mm2 = config.bridge_min_area_mm2;
    let bridge_noise_filter_mm = config.bridge_noise_filter_mm;
    let bridge_anchor_mm = config.bridge_anchor_mm;
    let min_infill_extrusion_mm = config.min_infill_extrusion_mm;
    if layers.is_empty() || (top_layers == 0 && bottom_layers == 0) {
        return SurfaceSubTimings {
            perimeter_snapshot_ms: 0,
            detection_ms: 0,
            infill_gen_ms: 0,
        };
    }

    let total = layers.len();

    // Snapshot the perimeter (OuterWall centerline) contours of every layer
    // *before* we begin adding infill paths.  Surface detection must operate on
    // sliced geometry only; comparing against previously added infill would give
    // wrong results.  `perimeter_paths_of` is a cheap filter+clone, so this
    // snapshot is inexpensive.
    //
    // The physical wall-bead footprint (`compute_wall_bead_footprint`) used to
    // be snapshotted here for every layer as well, but it is consumed *only* by
    // `clip_to_void`, and only on the minority of layers that actually carry a
    // bridge candidate.  Computing it eagerly for all layers — each a chain of
    // Clipper2 inflate/union calls — dominated the whole surface phase.  It is
    // now built lazily inside `detect_region` for the few layers that need it.
    #[cfg(not(target_arch = "wasm32"))]
    let t_snap = Instant::now();
    #[cfg(not(target_arch = "wasm32"))]
    let perimeters: Vec<Paths> = {
        use rayon::prelude::*;
        layers.par_iter().map(perimeter_paths_of).collect()
    };
    #[cfg(target_arch = "wasm32")]
    let perimeters: Vec<Paths> = layers.iter().map(perimeter_paths_of).collect();
    #[cfg(not(target_arch = "wasm32"))]
    let snapshot_ns = t_snap.elapsed().as_nanos();
    #[cfg(target_arch = "wasm32")]
    let snapshot_ns = 0u128;

    // `blocked_for_surface` builds a layer's **blocked** region for the solid
    // top/bottom surface trim (see the gated parallel pass after detection for
    // the full rationale and why it is now computed lazily).  Defined here as a
    // closure so both that pass and the wasm fallback share it.
    //
    // `surface_region` is the layer's combined (bottom ∪ top ∪ bridge) detected
    // surface *before* this trim.  It is used to skip gap-fill beads that are
    // sandwiched by (redundant with) the surface: those beads are pruned by
    // `prune_redundant_gap_fill`, so carving their footprint out here would
    // leave a bead-wide hole the surface serpentine has to route around (two
    // bands) and sparse infill then leaks into.  Genuine one-sided necks are
    // still carved so the surface abuts them.
    let surface_bond = (config.infill_overlap_percent * nozzle_diameter_mm).max(0.0);
    let blocked_for_surface = |l: &SliceLayer, surface_region: &Paths| -> Paths {
        let wall_fp = compute_wall_bead_footprint_filtered(l, nozzle_diameter_mm, false);
        if wall_fp.is_empty() {
            return Paths::new(vec![]);
        }
        let eroded = if surface_bond > 1e-9 {
            inflate(
                wall_fp,
                -surface_bond,
                JoinType::Round,
                EndType::Polygon,
                2.0,
            )
        } else {
            wall_fp
        };
        let gap_fp =
            compute_gap_fill_footprint_excluding_sandwiched(l, nozzle_diameter_mm, surface_region);
        if gap_fp.is_empty() {
            eroded
        } else if eroded.is_empty() {
            gap_fp
        } else {
            union(eroded, gap_fp, FillRule::NonZero).unwrap_or_default()
        }
    };

    // Immutable view of the layers for the parallel detection pass so
    // `clip_to_void` can build a layer's wall-bead footprint on demand.  This
    // shared borrow ends when the detection closure is consumed by `.collect()`,
    // before the serial apply pass takes `&mut layers`.
    let layers_ro: &[SliceLayer] = layers;

    #[cfg(not(target_arch = "wasm32"))]
    let mut infill_ns = 0u128;
    #[cfg(target_arch = "wasm32")]
    let infill_ns = 0u128;

    // ── Parallel detection pass ───────────────────────────────────────────────
    //
    // Each layer's surface regions are fully determined by `perimeters` (read-
    // only) and `interior_regions` (read-only).  Computing them is therefore
    // embarrassingly parallel.  We collect `(bridge_region, bottom_region, top_region)` tuples
    // and then apply them to `layers` in a serial pass to avoid shared mutable
    // state.
    //
    // Bridge detection: for layer i > 0, the "bridge region" is the portion of
    // the computed bottom surface that has NO support from the immediately
    // previous layer.  These areas span across a gap and require slower speed
    // and high fan cooling.  The remaining bottom surface (which has at least
    // some support from layer i-1) is labelled BottomSurface.
    // For each layer the closure returns:
    //   (bridge_region, bottom_region, top_region, raw_unsupported)
    // where `raw_unsupported` is the un-filtered, un-anchored, un-clipped
    // air-below footprint (`perimeters[i] − perimeters[i-1]`) used **only**
    // for OverhangPerimeter wall classification — never for infill.
    let detect_region = |i: usize| -> (Paths, Paths, Paths, Paths) {
        // ── Raw unsupported area — for wall classification only ──────────────
        // This is the portion of the current-layer perimeter that the
        // previous-layer's *bead* (not just its centerline) does not
        // physically support.
        //
        // `perimeters[i]` and `perimeters[i-1]` are OuterWall **centerline**
        // paths (Arachne emits centerlines).  The previous-layer bead extends
        // `d/2` *outward* from its centerline, so the actual support envelope
        // is `inflate(perimeters[i-1], +d/2)`.  Subtracting the envelope
        // (instead of the raw centerline) gives the geometric tolerance that
        // matches a real ~45° lean threshold for typical layer/nozzle ratios:
        //
        // - Slight outward lean (step `S < d/2`): the inflated previous
        //   perimeter fully contains `perimeters[i]` → empty unsupported
        //   strip → wall not flagged.  This kills the "80 % of the Benchy is
        //   overhang" false-positive without any vertex-fraction tuning.
        // - Real overhang (`S > d/2`): a meaningful air strip exists between
        //   the inflated envelope and `perimeters[i]`, and the current
        //   wall's centerline lies on its outer boundary.  See
        //   `classify_overhang_perimeters` for how that boundary case is
        //   counted.
        //
        // We deliberately do not clip to `interior_regions`: walls live on
        // the layer's outer edge, so their classification needs the full
        // footprint view.
        let raw_unsupported = if i == 0 {
            Paths::new(vec![])
        } else {
            let prev = &perimeters[i - 1];
            if prev.is_empty() {
                perimeters[i].clone()
            } else {
                let support_envelope = inflate(
                    prev.clone(),
                    nozzle_diameter_mm * 0.5,
                    JoinType::Round,
                    EndType::Polygon,
                    2.0,
                );
                if support_envelope.is_empty() {
                    perimeters[i].clone()
                } else {
                    difference(perimeters[i].clone(), support_envelope, FillRule::EvenOdd)
                        .unwrap_or_default()
                }
            }
        };

        let (bridge_region, bottom_region) = if bottom_layers > 0 {
            let mut covered = perimeters[i].clone();
            for j in 1..=bottom_layers {
                if i < j {
                    covered = Paths::new(vec![]);
                    break;
                }
                let neighbor = &perimeters[i - j];
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

            // Full geometric bottom/bridge candidate: area of layer i NOT
            // covered by any of the preceding `bottom_layers` layers.
            //
            // This is deliberately NOT clipped to `interior_regions` here.
            // Bridges within the wall band (Benchy porthole, window frame top
            // bar, door header) are a key case:
            //
            // • The porthole is a hole cut through the hull wall.  When the
            //   slicer reaches the layer that first closes the porthole, the
            //   new material appears *inside the wall zone*, not inside
            //   `interior_regions` (the cabin interior).
            // • If we clipped `region` to `interior_regions` now, that area
            //   would be removed before the bridge test — producing no bridge
            //   infill *and* no bottom-surface infill for the porthole.
            //
            // The `interior_regions` clip is applied *after* the bridge/bottom
            // split: the supported (bottom-surface) portion is clipped in
            // `clip_bottom`, and the bridge portion in `clip_to_void`.  Both
            // clip to the **morphologically-opened** interior so solid infill
            // and bridge lines stay in genuine voids and reject thin
            // (< ~1 mm) wall-band channels — the leaning/sloped-wall
            // false-positive bridges the wall + gap-fill beads already fill.
            // A real porthole / window / door-header / roof void is wider than
            // the opening threshold, so it survives; a lean-induced sliver does
            // not.
            let region =
                difference(perimeters[i].clone(), covered, FillRule::EvenOdd).unwrap_or_default();

            // Bridge detection: split `region` into areas that are entirely
            // unsupported by the previous layer (bridge) vs. areas that have
            // at least some support one layer below (true bottom surface).
            //
            // Layer 0 has no layer below, so the entire region is BottomSurface
            // (it is the absolute bottom of the model, not a bridge).
            //
            // The unsupported sub-region is then run through three filters
            // (matching OrcaSlicer / PrusaSlicer behaviour) before becoming a
            // bridge:
            //   1. **Morphological opening** removes thin slivers and
            //      hair-fine connecting strands caused by sub-pixel layer-to-
            //      layer geometry differences (the "Benchy embossed text"
            //      noise pattern).
            //   2. **Minimum-area filter** discards remaining tiny islands
            //      below `bridge_min_area_mm2`.  Such islands would print as
            //      lone bridge dots ("infills cannot be infill patterns").
            //   3. **Anchor expansion** dilates the surviving regions outward
            //      by `bridge_anchor_mm` (clipped to `perimeters[i]`) so each
            //      strand bites into the supported wall material on either side
            //      instead of ending mid-air.
            // Material rejected by stages 1–2 is reclassified as
            // BottomSurface so the layer remains fully solid below the gap.
            //
            // `clip_bottom` — clips solid bottom-surface infill to the interior
            // zone so it doesn't overlap wall extrusions.  Defined here (before
            // the i==0 early-return) so the first layer's BottomSurface is also
            // restricted to the interior region.  Bridges are exempt from this
            // clip because they explicitly fill the gap inside the wall band.
            //
            // The **absolute base cap** (`i < bottom_layers`) is the bed-contact
            // region and must stay fully solid for adhesion, so it clips to the
            // *full* interior.  Every higher layer clips to the morphologically
            // *opened* interior (`open_interior_for_surface`) so a supported
            // bottom surface that lands entirely inside a thin Arachne wall-band
            // channel — the tapering-underside artifact, e.g. Benchy layers
            // 160/172 — is dropped instead of filled with a rectilinear zig-zag
            // of sub-millimetre segments.  (Such artifacts only occur mid-model,
            // never on the base cap, so the exemption costs no real surface.)
            // The opening is computed lazily here — only for a non-empty bottom
            // surface — to keep it off the pure-infill layers' hot path.
            //
            //   • `interior_regions = None` → no clip
            //   • chosen interior is empty → no solid infill in this all-wall or
            //     all-thin-channel cross-section
            //   • otherwise → clip to the chosen interior region
            let clip_bottom = |s: Paths| -> Paths {
                if s.is_empty() {
                    return s;
                }
                let interior: Option<Paths> = if i < bottom_layers {
                    interior_regions.map(|regs| regs[i].clone())
                } else {
                    interior_regions
                        .map(|regs| open_interior_for_surface(&regs[i], nozzle_diameter_mm))
                };
                match interior {
                    None => s,
                    Some(region) if region.is_empty() => Paths::new(vec![]),
                    Some(region) => intersect(s, region, FillRule::EvenOdd).unwrap_or_default(),
                }
            };

            if region.is_empty() || i == 0 {
                // i == 0: no layer below → no bridge possible; entire region is
                // the model's absolute bottom surface.  Still clip to interior so
                // the first-layer BottomSurface infill stays out of the wall band.
                (Paths::new(vec![]), clip_bottom(region))
            } else {
                // Anchor bounds = the full layer cross-section (perimeters[i]).
                //
                // We use `perimeters[i]` (NOT `interior_regions[i]`) as the
                // anchor bound so the bridge can dilate outward into the
                // surrounding wall material on either side of the gap, giving
                // each strand a bite of solid material.
                //
                // The bridge *candidate* IS later clipped to `interior_regions[i]`
                // (see `clip_to_void` below) so we only bridge real voids and
                // not areas already covered by perimeters.  The anchor
                // expansion then re-grows back into the wall band as needed.
                let anchor_bounds: &Paths = &perimeters[i];
                let prev_perimeter = &perimeters[i - 1];

                // Step 2.5 (shared between both branches below) — clip the
                // bridge candidate to the **true free space** before the
                // anchor expansion.
                //
                // ## Why
                //
                // `region` (= `perimeters[i] − covered`) is the entire
                // unsupported cross-section.  In thin overhanging features
                // (Benchy rear deck, ledges, brims, lips) that whole strip is
                // physically *covered by wall extrusions* — the perimeter
                // beads alone fully fill it.  The bridge candidate then
                // matches the same strip, and `add_bridge_infill_for_region`
                // lays bridge lines on top of the already-printed perimeters
                // → double extrusion (the failure mode reported on Benchy
                // layer 172).
                //
                // We clip the candidate against **two** masks:
                //
                // 1. The **morphologically-opened** interior
                //    (`open_interior_for_surface(interior_regions[i])`) — the
                //    nominal infill area inside the wall band with wall-band
                //    channels narrower than
                //    `SURFACE_MIN_INTERIOR_WIDTH_NOZZLE_MULT × nozzle` (≈ 1 mm)
                //    erased.  Empty for "all-wall" cross-sections, so the bridge
                //    is fully suppressed there.
                //
                //    Opening (not the raw interior) is what kills the
                //    **leaning/sloped-wall false-positive bridge** — the class
                //    of defect reported on the Benchy hull-side deck edge and
                //    sloped cabin front (layers ≈ 159–172).  There the wall
                //    leans past ~45° so a thin unsupported strip survives the
                //    d/2 support envelope, but that strip is already filled
                //    solid by the wall + gap-fill beads on its own layer; laying
                //    sparse bridge lines over it double-extrudes into the walls
                //    and gap fill.  Because such a strip sits inside a *thin*
                //    (< 1 mm) wall-band channel — never a real void — the
                //    opening removes it while a genuine bridge over a real gap
                //    (cabin roof, porthole / window closure) keeps its full
                //    extent (it sits on a *thick* interior).  This mirrors the
                //    supported-surface clip in `clip_bottom` / the top-surface
                //    clip below, so bridge, bottom, and top surfaces all reject
                //    the same thin channels consistently.
                //
                // 2. The layer's **physical wall-bead footprint** — every
                //    OuterWall / InnerWall / OverhangPerimeter / GapFill
                //    centerline inflated by `width / 2` — built lazily via
                //    `compute_wall_bead_footprint` and subtracted from the
                //    candidate.  This catches the case Arachne's adaptive
                //    variable-width inner beads land *inside* the nominal
                //    interior region: the interior clip alone would still leave
                //    bridge overlapping those beads.
                //
                // Together, these mean the bridge can only land in true voids
                // (porthole / window closure / cavity interior) and never on
                // top of a wall extrusion.  The anchor expansion still runs
                // afterwards, so each bridge strand bites into the
                // surrounding wall material from the *outside*.
                let clip_to_void = |candidate: Paths| -> Paths {
                    if candidate.is_empty() {
                        return candidate;
                    }
                    // Step A — clip to the morphologically-opened interior (when
                    // available) so thin wall-band channels are rejected.
                    let after_interior = match interior_regions {
                        None => candidate,
                        Some(regs) if regs[i].is_empty() => return Paths::new(vec![]),
                        Some(regs) => {
                            let opened = open_interior_for_surface(&regs[i], nozzle_diameter_mm);
                            if opened.is_empty() {
                                return Paths::new(vec![]);
                            }
                            intersect(candidate, opened, FillRule::EvenOdd).unwrap_or_default()
                        }
                    };
                    if after_interior.is_empty() {
                        return after_interior;
                    }
                    // Step B — subtract physical wall bead footprints.  Built
                    // on demand here — only reached for a non-empty bridge
                    // candidate that survived the interior clip — rather than
                    // snapshotted for every layer.  The footprint is the union
                    // of every OuterWall / InnerWall / OverhangPerimeter /
                    // GapFill centerline inflated by its half-width, i.e. the
                    // area the wall extrusions actually consume; subtracting it
                    // keeps bridge infill out of Arachne's adaptive inner beads
                    // that land inside the nominal interior region.
                    let footprint = compute_wall_bead_footprint(&layers_ro[i], nozzle_diameter_mm);
                    if footprint.is_empty() {
                        after_interior
                    } else {
                        difference(after_interior, footprint, FillRule::EvenOdd).unwrap_or_default()
                    }
                };

                // Minimum bridge depth: a candidate unsupported by layer i-1
                // but still supported two layers down is a 1-layer-deep recess
                // (e.g. the hull-bottom debossed text) resting on that material
                // — not a real span.  Bridging it double-extrudes its solid fill
                // against the surrounding bottom surface at every edge.  Keep
                // only the part also unsupported two layers below; the bed
                // (i < 2) counts as support.
                let deepen = |raw: Paths| -> Paths {
                    if i < 2 || raw.is_empty() {
                        return Paths::new(vec![]);
                    }
                    let below2 = &perimeters[i - 2];
                    if below2.is_empty() {
                        return raw;
                    }
                    let env2 = inflate(
                        below2.clone(),
                        nozzle_diameter_mm * 0.5,
                        JoinType::Round,
                        EndType::Polygon,
                        2.0,
                    );
                    if env2.is_empty() {
                        raw
                    } else {
                        difference(raw, env2, FillRule::EvenOdd).unwrap_or_default()
                    }
                };

                if prev_perimeter.is_empty() {
                    // Nothing below at all → entire region is candidate bridge.
                    let raw = region.clone();
                    let opened = morphological_open(deepen(raw), bridge_noise_filter_mm);
                    let big = filter_small_islands(&opened, bridge_min_area_mm2);
                    let void_only = clip_to_void(big);
                    let anchored = expand_to_anchor(void_only, anchor_bounds, bridge_anchor_mm);
                    let supported_raw = if anchored.is_empty() {
                        region
                    } else {
                        difference(region, anchored.clone(), FillRule::EvenOdd).unwrap_or_default()
                    };
                    (anchored, clip_bottom(supported_raw))
                } else {
                    // Step 0 — raw unsupported area.
                    //
                    // Inflate `prev_perimeter` by the bead half-width
                    // (`nozzle_diameter_mm / 2`) before differencing.  This
                    // matches the `raw_unsupported` geometry used for overhang
                    // classification and gives a natural ~45° threshold:
                    //
                    // • Slight outward lean (step `S < d/2`): the inflated
                    //   previous perimeter fully covers the new area → `raw`
                    //   is empty → no false bridge in the wall zone.
                    // • Genuine hole closure (porthole, window bar, door
                    //   header): the hole area is inside the inflation of the
                    //   hull-with-hole (the hole itself grows), so the hole
                    //   area is NOT subtracted → `raw` correctly contains
                    //   the hole closure area → bridge detected.
                    let bridge_support_envelope = inflate(
                        prev_perimeter.clone(),
                        nozzle_diameter_mm * 0.5,
                        JoinType::Round,
                        EndType::Polygon,
                        2.0,
                    );
                    let raw = if bridge_support_envelope.is_empty() {
                        // Degenerate case: inflate produced nothing (e.g., all
                        // geometry collapsed to a point).  Fall back to treating
                        // the entire region as unsupported — conservative but
                        // safe.  Note: the prev_perimeter.is_empty() fast-path
                        // above already handles the first-layer / no-previous-
                        // geometry case, so reaching here with an empty envelope
                        // is unexpected in normal operation.
                        region.clone()
                    } else {
                        difference(region.clone(), bridge_support_envelope, FillRule::EvenOdd)
                            .unwrap_or_default()
                    };
                    // Step 1 — minimum-depth gate (drop 1-layer recesses), then
                    // morphological opening (noise filter).
                    let opened = morphological_open(deepen(raw), bridge_noise_filter_mm);
                    // Step 2 — drop islands below the area threshold.
                    let big = filter_small_islands(&opened, bridge_min_area_mm2);
                    // Step 2.5 — keep only the void inside the wall band
                    // (see `clip_to_void` definition above for full rationale).
                    let void_only = clip_to_void(big);
                    // Step 3 — anchor expansion clipped to the full layer
                    // cross-section so the bridge bites into the surrounding
                    // wall material on either side of the gap.
                    let anchored = expand_to_anchor(void_only, anchor_bounds, bridge_anchor_mm);
                    // Supported part = whatever is left of the bottom region
                    // after the (filtered + anchored) bridge has been removed.
                    // Clip to interior zone before using as solid bottom infill.
                    let supported_raw = if anchored.is_empty() {
                        region
                    } else {
                        difference(region, anchored.clone(), FillRule::EvenOdd).unwrap_or_default()
                    };
                    (anchored, clip_bottom(supported_raw))
                }
            }
        } else {
            (Paths::new(vec![]), Paths::new(vec![]))
        };

        // For the top-region exclusion below, use the combined bottom+bridge area.
        // We need a clone here because bridge_region and bottom_region are returned
        // in the tuple below; we only clone if both are non-empty (one allocation).
        let combined_bottom = union_or_first(bridge_region.clone(), bottom_region.clone());

        let top_region = if top_layers > 0 {
            let mut covered = perimeters[i].clone();
            for j in 1..=top_layers {
                if i + j >= total {
                    covered = Paths::new(vec![]);
                    break;
                }
                let neighbor = &perimeters[i + j];
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

            let mut top_region =
                difference(perimeters[i].clone(), covered, FillRule::EvenOdd).unwrap_or_default();

            if !combined_bottom.is_empty() && !top_region.is_empty() {
                top_region =
                    difference(top_region, combined_bottom, FillRule::EvenOdd).unwrap_or_default();
            }

            if let Some(interior_regions) = interior_regions {
                if interior_regions[i].is_empty() || top_region.is_empty() {
                    top_region = Paths::new(vec![]);
                } else {
                    // Clip the top surface to the morphologically **opened**
                    // interior so it lands only in genuine infill area, never in
                    // the thin wall-band channels Arachne's per-island-average
                    // interior estimate leaves in locally-thin cross-sections
                    // (the Benchy hull-side wall tips, the funnel-to-roof
                    // transitions).  Filled as a surface those channels become a
                    // rectilinear zig-zag of sub-millimetre segments; the opening
                    // erases channels narrower than
                    // `SURFACE_MIN_INTERIOR_WIDTH_NOZZLE_MULT × nozzle` while
                    // keeping genuine surfaces (deck, roof, funnel cap) at full
                    // extent, so Arachne matches the `classic` reference.
                    // Computed lazily — only for a non-empty top surface.
                    let interior_surface =
                        open_interior_for_surface(&interior_regions[i], nozzle_diameter_mm);
                    top_region = if interior_surface.is_empty() {
                        Paths::new(vec![])
                    } else {
                        intersect(top_region, interior_surface, FillRule::EvenOdd)
                            .unwrap_or_default()
                    };
                }
            }
            top_region
        } else {
            Paths::new(vec![])
        };

        (bridge_region, bottom_region, top_region, raw_unsupported)
    };

    #[cfg(not(target_arch = "wasm32"))]
    let t_detect = Instant::now();
    #[cfg(not(target_arch = "wasm32"))]
    let regions: Vec<(Paths, Paths, Paths, Paths)> = {
        use rayon::prelude::*;
        (0..total).into_par_iter().map(detect_region).collect()
    };
    #[cfg(target_arch = "wasm32")]
    let regions: Vec<(Paths, Paths, Paths, Paths)> = (0..total).map(detect_region).collect();
    #[cfg(not(target_arch = "wasm32"))]
    let detection_ns = t_detect.elapsed().as_nanos();
    #[cfg(target_arch = "wasm32")]
    let detection_ns = 0u128;

    // Per-layer **blocked** region for the solid top/bottom surface trim: the
    // physical wall-bead footprint eroded by the bond distance, unioned with the
    // un-eroded gap-fill footprint.  Subtracting it from the surface fill keeps
    // the fill off the wall band while still welding `infill_overlap_percent × d`
    // into the innermost wall (and merely abutting gap fill).
    //
    // Built here — *after* detection — and **only for layers that actually
    // produced a surface region**.  `blocked_for_surface` is a chain of Clipper2
    // inflate/union calls over every wall bead (the single most expensive
    // per-layer artifact in the surface phase); computing it for all layers when
    // only the top/bottom cap and transition layers carry a surface wasted the
    // bulk of the phase.  A layer with no surface never reads its entry, so an
    // empty placeholder there is output-identical.
    #[cfg(not(target_arch = "wasm32"))]
    let surface_blocked: Vec<Paths> = {
        use rayon::prelude::*;
        (0..total)
            .into_par_iter()
            .map(|i| {
                let (bridge, bottom, top, _) = &regions[i];
                if bottom.is_empty() && top.is_empty() {
                    Paths::new(vec![])
                } else {
                    let surface_region = combined_surface_region(bridge, bottom, top);
                    blocked_for_surface(&layers[i], &surface_region)
                }
            })
            .collect()
    };
    #[cfg(target_arch = "wasm32")]
    let surface_blocked: Vec<Paths> = (0..total)
        .map(|i| {
            let (bridge, bottom, top, _) = &regions[i];
            if bottom.is_empty() && top.is_empty() {
                Paths::new(vec![])
            } else {
                let surface_region = combined_surface_region(bridge, bottom, top);
                blocked_for_surface(&layers[i], &surface_region)
            }
        })
        .collect();

    // The single highest layer carrying a top surface — the one face a viewer
    // looks down on, and all that `IroningType::TopmostOnly` sweeps. Resolved
    // before the loop consumes `regions`.
    let topmost_top_layer = if config.ironing_enabled {
        regions.iter().rposition(|(_, _, top, _)| !top.is_empty())
    } else {
        None
    };

    // ── Serial apply pass ─────────────────────────────────────────────────────
    for (i, (bridge_region, bottom_region, top_region, raw_unsupported)) in
        regions.into_iter().enumerate()
    {
        if !bridge_region.is_empty() {
            #[cfg(not(target_arch = "wasm32"))]
            let t = Instant::now();
            // Clip wall paths to stop at the bridge zone boundary so that wall
            // extrusions and bridge infill lines don't overlap.  The bridge
            // region expands `bridge_anchor_mm` into the surrounding wall
            // material; clipping walls at that boundary means each strand
            // starts exactly where the wall ends, providing the anchor bite
            // without doubling the extrusion.
            clip_walls_against_bridge_region(&mut layers[i], &bridge_region);
            add_bridge_infill_for_region(
                &mut layers[i],
                &bridge_region,
                nozzle_diameter_mm,
                bridge_flow_ratio,
                config.bridge_angle_deg,
            );
            #[cfg(not(target_arch = "wasm32"))]
            {
                infill_ns += t.elapsed().as_nanos();
            }
        }

        // Clean up the detected solid-surface regions before filling them:
        // drop tiny wall-covered islands by area (`SURFACE_MIN_ISLAND_MM2`).
        //
        // The surviving surface is filled in full — redundant gap-fill beads
        // sitting inside it are removed afterwards by `prune_redundant_gap_fill`,
        // so the solid infill stays continuous instead of weaving around beads.
        let (bottom_region, top_region) = {
            let clean = |r: Paths| filter_small_islands(&r, SURFACE_MIN_ISLAND_MM2);
            (clean(bottom_region), clean(top_region))
        };

        // Trim solid top/bottom surface fill to keep it off the wall band.
        //
        // The surface regions were clipped to `interior_regions[i]`, whose
        // inward inset assumes a uniform wall count per island.  The Arachne
        // generator places a *variable* number of beads, so where an island
        // carries fewer beads than the layer maximum the inset lands a whole
        // bead-width too far out and the solid fill is laid on top of the inner
        // wall ("Top/Bottom surface × Inner wall" double-extrusion).
        //
        // `surface_blocked[i]` (built in parallel above) is the wall-bead
        // footprint eroded by the bond distance, unioned with the gap-fill
        // footprint.  Subtracting it welds the fill `infill_overlap_percent × d`
        // into the innermost wall (matching classic) and no further, while
        // merely abutting gap fill.  `NonZero` respects the frame's CW hole so
        // only the wall band is removed.  A no-op where the inset was already
        // correct (classic), so it removes only the genuine over-print.
        let (bottom_region, top_region) = {
            let blocked = &surface_blocked[i];
            let trim = |r: Paths| -> Paths {
                if r.is_empty() || blocked.is_empty() {
                    r
                } else {
                    difference(r, blocked.clone(), FillRule::NonZero).unwrap_or_default()
                }
            };
            (trim(bottom_region), trim(top_region))
        };

        // Solid top/bottom surface fill direction cross-hatches per layer, and
        // fill lines overlap (extrusion-spacing relation) so the surface has no
        // gaps between adjacent beads even under the cross-hatch.
        let layer_infill_angle = surface_infill_angle_for_layer(infill_angle, i);
        // Line pitch derives from the solid-surface extrusion width (honouring
        // `top_surface_line_width` / `line_width`, falling back to the nozzle),
        // and the G-code generator charges each line at exactly this pitch —
        // `mm³/mm = spacing × layer_height` — so surfaces fill flat, never
        // raised/over-extruded.
        let surface_width = if config.solid_surface_line_width_mm > 0.0 {
            config.solid_surface_line_width_mm
        } else {
            nozzle_diameter_mm
        };
        let solid_line_spacing = extrusion_flow_spacing_mm(surface_width, layer_height);

        // Drop sub-bead slivers the wall-band trim above may have left behind,
        // so the scanline never fills a strip too narrow to hold a bead (that
        // produces a micro-serpentine of stub segments against the wall).
        // See `open_surface_region_for_fill` for the full rationale.
        let (bottom_region, top_region) = (
            open_surface_region_for_fill(bottom_region, surface_width),
            open_surface_region_for_fill(top_region, surface_width),
        );

        if !bottom_region.is_empty() {
            #[cfg(not(target_arch = "wasm32"))]
            let t = Instant::now();
            add_solid_infill_for_region(
                &mut layers[i],
                &bottom_region,
                ExtrusionRole::BottomSurface,
                solid_line_spacing,
                layer_infill_angle,
                min_infill_extrusion_mm,
                config.bottom_pattern,
            );
            #[cfg(not(target_arch = "wasm32"))]
            {
                infill_ns += t.elapsed().as_nanos();
            }
        }

        if !top_region.is_empty() {
            #[cfg(not(target_arch = "wasm32"))]
            let t = Instant::now();
            add_solid_infill_for_region(
                &mut layers[i],
                &top_region,
                ExtrusionRole::TopSurface,
                solid_line_spacing,
                layer_infill_angle,
                min_infill_extrusion_mm,
                config.top_pattern,
            );
            #[cfg(not(target_arch = "wasm32"))]
            {
                infill_ns += t.elapsed().as_nanos();
            }
        }

        // Ironing, last on the layer so the sweep runs over a finished surface.
        //
        // The paths are appended to `layer.paths` and touch no region field:
        // ironing is a surface *treatment*, not solid material. Were its
        // footprint ever folded into `solid_regions`, `add_infill_to_layers`
        // would subtract it (grown by a full bead) and punch a hole in the
        // sparse infill underneath.
        if config.ironing_enabled {
            let angle = if config.ironing_angle < 0.0 {
                layer_infill_angle
            } else {
                config.ironing_angle
            };
            let iron_top = match config.ironing_type {
                IroningType::TopSurfaces | IroningType::AllSolid => true,
                IroningType::TopmostOnly => topmost_top_layer == Some(i),
            };
            if iron_top {
                add_ironing_for_region(
                    &mut layers[i],
                    &top_region,
                    config.ironing_spacing,
                    angle,
                    min_infill_extrusion_mm,
                );
            }
            if config.ironing_type == IroningType::AllSolid {
                add_ironing_for_region(
                    &mut layers[i],
                    &bottom_region,
                    config.ironing_spacing,
                    angle,
                    min_infill_extrusion_mm,
                );
            }
        }

        // Stash the unsupported area (the layer-footprint air below) for the
        // post-pass that classifies wall paths in air as `OverhangPerimeter`.
        // Empty for layer 0 by design.
        //
        // **Subtract `bridge_region`** so wall arcs that survive
        // `clip_walls_against_bridge_region` along the bridge boundary cannot
        // be re-flagged as `OverhangPerimeter` and end up double-extruded on
        // top of the bridge infill.  The bridge zone is already fully handled:
        //   • Walls inside it were clipped out.
        //   • Surviving open arcs may keep a "seam" vertex sitting *just
        //     inside* the bridge zone (the last in-bridge vertex before the
        //     transition to outside).  Without this subtraction, that seam
        //     vertex tests `IsInside` against `raw_unsupported` and produces
        //     a tiny `OverhangPerimeter` arc geometrically overlapping the
        //     bridge — exactly the double-extrusion the user reported on
        //     Benchy layer 172.
        //
        // Areas of `raw_unsupported` that did **not** become a bridge (e.g.
        // filtered by morphological opening / `bridge_min_area_mm2`, or
        // supported by a deeper neighbour so they did not enter the bridge
        // candidate region at all) remain in `unsupported_regions` and
        // continue to drive overhang classification as before.
        let unsupported_for_overhang = if raw_unsupported.is_empty() || bridge_region.is_empty() {
            raw_unsupported
        } else {
            difference(raw_unsupported, bridge_region.clone(), FillRule::EvenOdd)
                .unwrap_or_default()
        };
        if !unsupported_for_overhang.is_empty() {
            layers[i].unsupported_regions = unsupported_for_overhang;
        }

        // Record the union of all solid-surface regions on this layer so that
        // add_infill_to_layers can exclude them from sparse infill.
        // Include bridge_region in the solid union since those are solid-filled areas too.
        let all_bottom = union_or_first(bridge_region, bottom_region);
        let combined_solid = union_or_first(all_bottom, top_region);
        if !combined_solid.is_empty() {
            layers[i].solid_regions = combined_solid;
        }
    }

    // ── Vertical-shell thickness enforcement (opt-in) ─────────────────────────
    //
    // Runs after every layer's solid_regions are known: projects neighbouring
    // solid surfaces onto each layer and back-fills the shell where a sloped
    // cross-section would otherwise leave a thin side wall.  A no-op unless the
    // user enables it, so the default surface output is unchanged.
    if config.ensure_vertical_shell_thickness {
        #[cfg(not(target_arch = "wasm32"))]
        let t = Instant::now();
        apply_vertical_shell_thickness(layers, interior_regions, config);
        #[cfg(not(target_arch = "wasm32"))]
        {
            infill_ns += t.elapsed().as_nanos();
        }
    }

    SurfaceSubTimings {
        perimeter_snapshot_ms: (snapshot_ns / 1_000_000) as u64,
        detection_ms: (detection_ns / 1_000_000) as u64,
        infill_gen_ms: (infill_ns / 1_000_000) as u64,
    }
}

/// Enforce a minimum solid vertical-shell thickness (opt-in second pass).
///
/// After [`generate_top_bottom_surfaces_with_interior`] has populated every
/// layer's `solid_regions`, this projects the solid surfaces of the neighbouring
/// layers — within the `top_layers` / `bottom_layers` shell depth — onto each
/// layer and fills any resulting interior region solid.  Where the model's
/// cross-section drifts (a sloped or near-vertical wall), a top/bottom surface on
/// a nearby layer marks material that must be backed here so the side shell stays
/// continuous rather than exposing sparse infill.
///
/// Enforce a minimum solid vertical-shell thickness (opt-in second pass).
///
/// After [`generate_top_bottom_surfaces_with_interior`] has populated every
/// layer's `solid_regions`, this thickens each layer's **own** top/bottom solid
/// surface *inward* into the interior by the shell reach.  On a sloped or
/// near-vertical wall the exposed surface rim of each layer is thin; grown
/// inward and stacked over the sloped run, those rings build a continuous solid
/// shell of the intended thickness measured *perpendicular* to the surface — the
/// gap this feature closes.
///
/// The growth is clipped to the layer's `interior_regions` and the layer's
/// existing solid is subtracted, so it only *adds* backing.  A layer whose
/// surface already fills the whole interior (a flat top/bottom band) grows to no
/// more than that interior, so the addition is empty — flat shells keep their
/// correct solid-layer count.  Interior layers of a solid body carry no surface,
/// so nothing is added: the pass is a no-op away from surfaces.
///
/// Added regions are filled with solid rectilinear infill (tagged
/// [`ExtrusionRole::BottomSurface`] — the "internal solid infill" role this
/// engine expresses closest) and merged into `solid_regions` so sparse infill
/// keeps clear of them.
fn apply_vertical_shell_thickness(
    layers: &mut [SliceLayer],
    interior_regions: Option<&[Paths]>,
    config: &SurfaceConfig,
) {
    let n = layers.len();
    let interior = match interior_regions {
        Some(r) if r.len() == n => r,
        _ => return,
    };
    let top = config.top_layers;
    let bottom = config.bottom_layers;
    if n == 0 || (top == 0 && bottom == 0) {
        return;
    }

    // Snapshot the solid surfaces produced by the main pass (read-only source).
    let solid: Vec<Paths> = layers.iter().map(|l| l.solid_regions.clone()).collect();

    let surface_width = if config.solid_surface_line_width_mm > 0.0 {
        config.solid_surface_line_width_mm
    } else {
        config.nozzle_diameter_mm
    };
    let spacing = extrusion_flow_spacing_mm(surface_width, config.layer_height);

    // How far to grow a surface rim inward.  Ties the in-plane shell reach to the
    // configured solid-layer count so a shallower slope (needing more backing)
    // gets a proportionally deeper solid shell.
    let reach =
        (top.max(bottom).max(1) as f64 * config.nozzle_diameter_mm).max(config.nozzle_diameter_mm);

    // Per-layer additive backing region (embarrassingly parallel to compute).
    let compute = |i: usize| -> Option<(usize, Paths)> {
        if solid[i].is_empty() || interior[i].is_empty() {
            return None;
        }
        // Grow this layer's own surface outward by the shell reach…
        let grown = inflate(
            solid[i].clone(),
            reach,
            JoinType::Round,
            EndType::Polygon,
            2.0,
        );
        if grown.is_empty() {
            return None;
        }
        // …keep only what stays inside the wall interior…
        let within = intersect(grown, interior[i].clone(), FillRule::EvenOdd).unwrap_or_default();
        if within.is_empty() {
            return None;
        }
        // …and is not already solid here.
        let add = difference(within, solid[i].clone(), FillRule::EvenOdd).unwrap_or_default();
        if add.is_empty() {
            None
        } else {
            Some((i, add))
        }
    };

    #[cfg(not(target_arch = "wasm32"))]
    let additions: Vec<(usize, Paths)> = {
        use rayon::prelude::*;
        (0..n).into_par_iter().filter_map(compute).collect()
    };
    #[cfg(target_arch = "wasm32")]
    let additions: Vec<(usize, Paths)> = (0..n).filter_map(compute).collect();

    for (i, add) in additions {
        let angle = surface_infill_angle_for_layer(config.infill_angle, i);
        add_solid_infill_for_region(
            &mut layers[i],
            &add,
            ExtrusionRole::BottomSurface,
            spacing,
            angle,
            config.min_infill_extrusion_mm,
            config.bottom_pattern,
        );
        let merged = union_or_first(layers[i].solid_regions.clone(), add);
        layers[i].solid_regions = merged;
    }
}

/// Trim solid surface regions to fit inside walls, with configurable overlap.
///
/// **NOTE**: This function is currently not fully working because surfaces are
/// generated as open line segments (infill lines), not closed regions. Boolean
/// operations like intersect() don't work reliably with open paths. A better
/// approach would be to generate surfaces AFTER walls, directly in the interior
/// region, rather than trying to trim them post-hoc.
///
/// After Arachne wall generation, the solid top/bottom surface infill paths may
/// overlap with the generated walls. This function attempts to ensure surfaces
/// are printed in the interior region defined by the innermost walls, while
/// maintaining a small configurable overlap for bonding.
///
/// # Arguments
/// * `layers` - Mutable reference to all layers
/// * `overlap_percent` - How much surfaces overlap into walls (0.0-1.0, e.g., 0.25 = 25%)
/// * `nozzle_diameter` - Nozzle diameter in mm, used to calculate overlap distance
#[allow(dead_code)] // Currently disabled, but kept for future implementation
fn trim_surfaces_to_walls(layers: &mut [SliceLayer], overlap_percent: f64, nozzle_diameter: f64) {
    // Calculate overlap as a distance in mm
    let overlap_distance = nozzle_diameter * overlap_percent;

    for layer in layers.iter_mut() {
        // Collect all wall paths (OuterWall and InnerWall).
        let wall_paths: Vec<Path> = layer
            .paths
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                let role = layer.role_for_path(*i);
                role == ExtrusionRole::OuterWall || role == ExtrusionRole::InnerWall
            })
            .map(|(_, p)| p.clone())
            .collect();

        if wall_paths.is_empty() {
            // No walls, leave surfaces as-is
            continue;
        }

        // Create interior region by shrinking walls inward
        // The interior is where surfaces should be printed
        let walls = Paths::new(wall_paths);

        // Deflate (shrink) walls to create interior region
        // Use negative inflation to shrink inward
        // Shrink by (nozzle_diameter/2 - overlap_distance) to leave the overlap
        let shrink_amount = (nozzle_diameter / 2.0) - overlap_distance;
        let interior_region = if shrink_amount > 0.01 {
            // Shrink walls to define interior
            clipper2::inflate(
                walls,
                -shrink_amount * 100.0, // Negative = deflate, convert to Centi
                JoinType::Round,
                EndType::Polygon,
                2.0,
            )
        } else {
            // If shrink amount is too small, just use the walls as-is
            walls
        };

        if interior_region.is_empty() {
            // Walls collapsed completely, remove all surfaces
            let mut new_paths = Paths::new(vec![]);
            let mut new_roles = Vec::new();
            let mut new_widths = Vec::new();
            let mut new_vwidths = Vec::new();

            for (i, path) in layer.paths.iter().enumerate() {
                let role = layer.role_for_path(i);
                if role != ExtrusionRole::TopSurface && role != ExtrusionRole::BottomSurface {
                    // Keep non-surface paths
                    new_paths.push(path.clone());
                    new_roles.push(role);
                    new_widths.push(layer.width_for_path(i));
                    new_vwidths.push(layer.vertex_widths_for_path(i));
                }
            }

            layer.paths = new_paths;
            layer.path_roles = new_roles;
            layer.path_widths = new_widths;
            layer.path_vertex_widths = new_vwidths;
            continue;
        }

        // Now intersect surface paths with the interior region
        let mut new_paths = Paths::new(vec![]);
        let mut new_roles = Vec::new();
        let mut new_widths = Vec::new();
        let mut new_vwidths = Vec::new();

        for (i, path) in layer.paths.iter().enumerate() {
            let role = layer.role_for_path(i);
            if role == ExtrusionRole::TopSurface || role == ExtrusionRole::BottomSurface {
                // Intersect this surface path with the interior region
                let path_as_paths = Paths::new(vec![path.clone()]);
                let trimmed = intersect(path_as_paths, interior_region.clone(), FillRule::EvenOdd)
                    .unwrap_or_default();

                // Add all resulting paths (may be split into multiple pieces).
                for p in trimmed.iter() {
                    new_paths.push(p.clone());
                    new_roles.push(role);
                    new_widths.push(layer.width_for_path(i));
                    new_vwidths.push(None);
                }
            } else {
                // Keep non-surface paths as-is (including walls).
                new_paths.push(path.clone());
                new_roles.push(role);
                new_widths.push(layer.width_for_path(i));
                new_vwidths.push(layer.vertex_widths_for_path(i));
            }
        }

        layer.paths = new_paths;
        layer.path_roles = new_roles;
        layer.path_widths = new_widths;
        layer.path_vertex_widths = new_vwidths;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clipper2::{Path, Paths};

    fn square_paths(cx: f64, cy: f64, half: f64) -> Paths {
        let p: Path = vec![
            (cx - half, cy - half),
            (cx + half, cy - half),
            (cx + half, cy + half),
            (cx - half, cy + half),
        ]
        .into();
        Paths::new(vec![p])
    }

    fn vshell_config(enabled: bool) -> SurfaceConfig {
        SurfaceConfig {
            top_layers: 1,
            bottom_layers: 1,
            layer_height: 0.2,
            infill_angle: 45.0,
            nozzle_diameter_mm: 0.4,
            solid_surface_line_width_mm: 0.4,
            min_infill_extrusion_mm: 0.0,
            bridge_flow_ratio: 0.8,
            bridge_min_area_mm2: 0.5,
            bridge_noise_filter_mm: 0.05,
            bridge_anchor_mm: 0.5,
            infill_overlap_percent: 0.25,
            ensure_vertical_shell_thickness: enabled,
            bridge_angle_deg: 0.0,
            top_pattern: SurfacePattern::Rectilinear,
            bottom_pattern: SurfacePattern::Rectilinear,
            internal_solid_pattern: SurfacePattern::Rectilinear,
            ..Default::default()
        }
    }

    #[test]
    fn vertical_shell_grows_a_surface_rim_inward() {
        // Three layers, each a 10×10 interior.  Only the middle layer carries a
        // small central solid rim; the vertical-shell pass must back it with an
        // added ring of BottomSurface infill, and leave the empty-surface layers
        // untouched.
        let mut layers: Vec<SliceLayer> = (0..3)
            .map(|i| SliceLayer::new(0.2 * (i as f64 + 1.0)))
            .collect();
        layers[1].solid_regions = square_paths(0.0, 0.0, 1.0); // 2×2 rim
        let interior: Vec<Paths> = (0..3).map(|_| square_paths(0.0, 0.0, 5.0)).collect();

        let before: usize = layers
            .iter()
            .map(|l| {
                (0..l.paths.len())
                    .filter(|&i| l.role_for_path(i) == ExtrusionRole::BottomSurface)
                    .count()
            })
            .sum();

        apply_vertical_shell_thickness(&mut layers, Some(&interior), &vshell_config(true));

        let mid_added = (0..layers[1].paths.len())
            .filter(|&i| layers[1].role_for_path(i) == ExtrusionRole::BottomSurface)
            .count();
        assert!(
            mid_added > before,
            "middle layer's surface rim should be backed with solid infill"
        );
        // The empty-surface neighbours get nothing.
        assert!(layers[0].paths.is_empty());
        assert!(layers[2].paths.is_empty());
    }

    #[test]
    fn vertical_shell_is_a_noop_where_a_layer_has_no_surface() {
        // A layer with empty solid_regions (a plain vertical-wall cross-section)
        // gets no vertical-shell backing.
        let mut layers: Vec<SliceLayer> = (0..3)
            .map(|i| SliceLayer::new(0.2 * (i as f64 + 1.0)))
            .collect();
        let interior: Vec<Paths> = (0..3).map(|_| square_paths(0.0, 0.0, 5.0)).collect();

        apply_vertical_shell_thickness(&mut layers, Some(&interior), &vshell_config(true));

        for l in &layers {
            assert!(
                l.paths.is_empty(),
                "no surface anywhere ⇒ no vertical-shell infill added"
            );
        }
    }

    #[test]
    fn test_surface_infill_angle_alternates_per_layer() {
        // Even layers keep the base angle; odd layers rotate 90° to cross-hatch.
        assert_eq!(surface_infill_angle_for_layer(45.0, 0), 45.0);
        assert_eq!(surface_infill_angle_for_layer(45.0, 1), 135.0);
        assert_eq!(surface_infill_angle_for_layer(45.0, 2), 45.0);
        assert_eq!(surface_infill_angle_for_layer(45.0, 3), 135.0);
        // Result is always wrapped into [0, 180).
        assert_eq!(surface_infill_angle_for_layer(135.0, 1), 45.0);
        assert_eq!(surface_infill_angle_for_layer(0.0, 1), 90.0);
        for layer in 0..10 {
            let a = surface_infill_angle_for_layer(45.0, layer);
            assert!((0.0..180.0).contains(&a));
        }
    }

    #[test]
    fn test_solid_surface_spacing_overlaps_no_gaps() {
        // Extrusion-spacing relation: adjacent solid beads must overlap so the
        // surface has no holes.  At 0.4 mm nozzle / 0.2 mm layers the pitch is
        // 0.4 − 0.2·(1 − π/4) ≈ 0.357 mm, i.e. below the 0.4 mm bead width
        // (overlap) yet well above the old over-extruding 0.24 mm.
        let s = extrusion_flow_spacing_mm(0.4, 0.2);
        assert!(
            (s - 0.3571).abs() < 1e-3,
            "expected ~0.357 mm pitch, got {s}"
        );
        // Strictly less than the bead width → guaranteed overlap (no gaps).
        assert!(s < 0.4, "solid pitch {s} must be < bead width for overlap");
        // And strictly greater than the old layer-height rule (0.24 mm) → not
        // the previous heavy over-extrusion.
        assert!(s > 0.24, "solid pitch {s} must exceed the old 0.24 mm rule");
        // Larger layer height ⇒ larger cap correction ⇒ tighter pitch.
        assert!(extrusion_flow_spacing_mm(0.4, 0.3) < extrusion_flow_spacing_mm(0.4, 0.1));
    }

    /// The 10×10 mm square every surface-pattern test fills.
    fn fill_square() -> Paths {
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        Paths::new(vec![square])
    }

    /// Signed X extent of each emitted line, so a uniform sweep is detectable.
    fn line_directions(paths: &Paths) -> Vec<f64> {
        paths
            .iter()
            .filter_map(|p| {
                let pts: Vec<(f64, f64)> = p.iter().map(|v| (v.x(), v.y())).collect();
                (pts.len() >= 2).then(|| pts[1].0 - pts[0].0)
            })
            .collect()
    }

    #[test]
    fn monotonic_patterns_draw_every_line_the_same_way() {
        // The whole point of a monotonic surface: the nozzle never comes back
        // across a line it just laid, so each bead is squished identically.
        let spacing = extrusion_flow_spacing_mm(0.4, 0.2);
        for pattern in [SurfacePattern::Monotonic, SurfacePattern::MonotonicLine] {
            let paths = generate_solid_infill(&fill_square(), spacing, 0.0, 0.0, pattern);
            let dirs = line_directions(&paths);
            assert!(dirs.len() > 5, "{pattern:?}: expected several lines");
            assert!(
                dirs.iter().all(|d| *d > 0.0) || dirs.iter().all(|d| *d < 0.0),
                "{pattern:?}: lines must all run the same way, got {dirs:?}"
            );
        }
    }

    #[test]
    fn rectilinear_serpentine_reverses_between_lines() {
        // The contrast case: the classic serpentine alternates direction, which
        // is exactly the sheen a monotonic surface avoids.
        let spacing = extrusion_flow_spacing_mm(0.4, 0.2);
        let paths = generate_solid_infill(
            &fill_square(),
            spacing,
            0.0,
            0.0,
            SurfacePattern::Rectilinear,
        );
        let reverses = paths.iter().any(|p| {
            let xs: Vec<f64> = p.iter().map(|v| v.x()).collect();
            xs.windows(3)
                .any(|w| (w[1] - w[0]).signum() != (w[2] - w[1]).signum())
        });
        assert!(reverses, "serpentine chaining should double back");
    }

    #[test]
    fn monotonic_line_never_connects_its_lines() {
        // `MonotonicLine` is `Monotonic` with joining switched off, so on a
        // convex region it must emit one path per fill line.
        let spacing = extrusion_flow_spacing_mm(0.4, 0.2);
        let joined =
            generate_solid_infill(&fill_square(), spacing, 0.0, 0.0, SurfacePattern::Monotonic);
        let separate = generate_solid_infill(
            &fill_square(),
            spacing,
            0.0,
            0.0,
            SurfacePattern::MonotonicLine,
        );
        assert!(separate.iter().all(|p| p.len() == 2), "expected bare lines");
        assert!(
            separate.len() >= joined.len(),
            "unjoined lines ({}) cannot be fewer than joined ones ({})",
            separate.len(),
            joined.len()
        );
    }

    #[test]
    fn concentric_emits_closed_loops_stepping_inward() {
        let spacing = extrusion_flow_spacing_mm(0.4, 0.2);
        let paths = generate_solid_infill(
            &fill_square(),
            spacing,
            0.0,
            0.0,
            SurfacePattern::Concentric,
        );
        assert!(
            paths.len() > 3,
            "expected several loops, got {}",
            paths.len()
        );
        for loop_path in paths.iter() {
            let pts: Vec<(f64, f64)> = loop_path.iter().map(|v| (v.x(), v.y())).collect();
            let first = pts[0];
            let last = *pts.last().expect("non-empty");
            assert!(
                (first.0 - last.0).abs() < 1e-6 && (first.1 - last.1).abs() < 1e-6,
                "a fill loop must close explicitly: {first:?} vs {last:?}"
            );
        }
    }

    #[test]
    fn bridge_angle_override_replaces_the_detected_direction() {
        // A long thin region auto-detects strands across its short axis. The
        // documented spelling of a horizontal override is `180` (because `0`
        // means "auto"), and it must win over the detected direction.
        let region: Path = vec![(0.0, 0.0), (40.0, 0.0), (40.0, 6.0), (0.0, 6.0)].into();
        let region = Paths::new(vec![region]);

        let mut auto = SliceLayer::new(0.2);
        add_bridge_infill_for_region(&mut auto, &region, 0.4, 0.8, 0.0);
        let mut forced = SliceLayer::new(0.2);
        add_bridge_infill_for_region(&mut forced, &region, 0.4, 0.8, 180.0);

        let spans = |layer: &SliceLayer| -> (f64, f64) {
            let mut dx: f64 = 0.0;
            let mut dy: f64 = 0.0;
            for p in layer.paths.iter() {
                let pts: Vec<(f64, f64)> = p.iter().map(|v| (v.x(), v.y())).collect();
                for w in pts.windows(2) {
                    dx = dx.max((w[1].0 - w[0].0).abs());
                    dy = dy.max((w[1].1 - w[0].1).abs());
                }
            }
            (dx, dy)
        };

        let (auto_dx, auto_dy) = spans(&auto);
        let (forced_dx, forced_dy) = spans(&forced);
        assert!(
            auto_dy > auto_dx,
            "auto detection should span the short (Y) axis, got {auto_dx} × {auto_dy}"
        );
        assert!(
            forced_dx > forced_dy,
            "a 180° (= 0°) override should run the strands along X, got {forced_dx} × {forced_dy}"
        );
    }

    #[test]
    fn test_add_solid_infill_emits_lines() {
        // A 10×10 mm square filled at 0° with the solid pitch produces a
        // reasonable line count (neither zero nor the old ~50 over-extruded).
        let mut layer = SliceLayer::new(0.2);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        let square: Paths = Paths::new(vec![square]);
        let spacing = extrusion_flow_spacing_mm(0.4, 0.2);
        add_solid_infill_for_region(
            &mut layer,
            &square,
            ExtrusionRole::TopSurface,
            spacing,
            0.0,
            0.0,
            SurfacePattern::Rectilinear,
        );
        let n = layer.paths.len();
        // 10 mm / 0.357 mm ≈ 28 lines (serpentine chaining may merge some).
        assert!(
            n > 0 && n <= 40,
            "expected overlap-spaced solid lines, got {n} paths"
        );
    }

    /// A wall path that lies entirely outside the bridge region must be kept
    /// unchanged with path_is_open = false.
    #[test]
    fn test_clip_walls_leaves_outside_paths_unchanged() {
        // Wall at x ∈ [0, 2], y ∈ [0, 2] — entirely left of the bridge zone.
        let wall: Path = vec![(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)].into();
        let mut layer = SliceLayer::new(0.4);
        layer.paths.push(wall);
        layer.path_roles.push(ExtrusionRole::OuterWall);

        // Bridge region at x ∈ [5, 9], y ∈ [0, 2] — far from the wall.
        let bridge: Path = vec![(5.0, 0.0), (9.0, 0.0), (9.0, 2.0), (5.0, 2.0)].into();
        let bridge_region = Paths::new(vec![bridge]);

        clip_walls_against_bridge_region(&mut layer, &bridge_region);

        assert_eq!(layer.paths.len(), 1, "path count unchanged");
        assert_eq!(layer.path_roles[0], ExtrusionRole::OuterWall);
        assert!(!layer.is_path_open(0), "should stay closed");
    }

    /// A wall path that lies entirely inside the bridge region must be dropped.
    #[test]
    fn test_clip_walls_drops_fully_inside_paths() {
        // Wall at x ∈ [1, 3], y ∈ [1, 3] — entirely inside the bridge zone.
        let wall: Path = vec![(1.0, 1.0), (3.0, 1.0), (3.0, 3.0), (1.0, 3.0)].into();
        let mut layer = SliceLayer::new(0.4);
        layer.paths.push(wall);
        layer.path_roles.push(ExtrusionRole::OuterWall);

        // Bridge region that fully contains the wall.
        let bridge: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        let bridge_region = Paths::new(vec![bridge]);

        clip_walls_against_bridge_region(&mut layer, &bridge_region);

        assert_eq!(layer.paths.len(), 0, "fully-inside wall must be dropped");
    }

    /// A rectangular wall loop whose top segment crosses the bridge zone should
    /// be split: the top (in-bridge) segment is dropped and the three remaining
    /// sides become one open arc.
    ///
    /// This models the Benchy window-hole boundary path on the bridge layer:
    /// a rectangular loop with its top segment directly over the bridge infill.
    ///
    /// The bridge zone is intentionally wider than the wall loop so that the top
    /// vertices land strictly *inside* the zone rather than on its boundary.  In
    /// the real pipeline, Arachne wall centerlines sit d/2 ≈ 0.2 mm inside the
    /// void surface and thus strictly inside the anchor strip — the strict
    /// point-in-polygon test (IsOn = outside) is what keeps outer model-boundary
    /// paths from being incorrectly clipped.
    #[test]
    fn test_clip_walls_splits_mixed_path_into_open_arc() {
        // Rectangular wall loop:
        //   bottom-left (0,0) → bottom-right (10,0) → top-right (10,2) → top-left (0,2)
        // The "top" vertices (y=2) must be strictly inside the bridge zone.
        let wall: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 2.0), (0.0, 2.0)].into();
        let mut layer = SliceLayer::new(0.4);
        layer.paths.push(wall);
        layer.path_roles.push(ExtrusionRole::OuterWall);

        // Bridge zone wider than the wall (-1 to 11) so the wall top vertices
        // land strictly inside (not on the boundary of) the bridge polygon.
        let bridge: Path = vec![(-1.0, 1.5), (11.0, 1.5), (11.0, 4.0), (-1.0, 4.0)].into();
        let bridge_region = Paths::new(vec![bridge]);

        clip_walls_against_bridge_region(&mut layer, &bridge_region);

        // The top segment (y=2, strictly inside the bridge zone) should be
        // removed.  The remaining 3 sides form one open arc.
        assert!(!layer.paths.is_empty(), "outside segment must be retained");
        // All resulting paths must be open arcs.
        for idx in 0..layer.paths.len() {
            assert!(
                layer.is_path_open(idx),
                "clipped segment at index {idx} must be an open arc"
            );
            assert_eq!(
                layer.path_roles[idx],
                ExtrusionRole::OuterWall,
                "outside segments keep OuterWall role"
            );
        }
    }

    /// Non-wall paths (Bridge, Infill, …) must never be modified.
    #[test]
    fn test_clip_walls_skips_non_wall_roles() {
        let path: Path = vec![(1.0, 1.0), (5.0, 1.0), (5.0, 5.0), (1.0, 5.0)].into();
        let mut layer = SliceLayer::new(0.4);
        layer.paths.push(path.clone());
        layer.path_roles.push(ExtrusionRole::Bridge);

        // Bridge zone that fully contains the path.
        let bridge: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        let bridge_region = Paths::new(vec![bridge]);

        clip_walls_against_bridge_region(&mut layer, &bridge_region);

        assert_eq!(layer.paths.len(), 1, "Bridge path must not be removed");
        assert_eq!(layer.path_roles[0], ExtrusionRole::Bridge);
        assert!(!layer.is_path_open(0));
    }

    /// **Regression** — a wall path whose vertices lie exactly on the bridge
    /// zone outer boundary (all `IsOn`, none strictly inside) must be **dropped**.
    ///
    /// This is the "outer hull segment running along the bridge zone clipping
    /// face" case.  Before the fix, the strict point-in-polygon test treated
    /// `IsOn` as "outside", so the "no vertex inside" fast path fired and kept
    /// the path unchanged.  That path would then be classified as
    /// `OverhangPerimeter` and printed again when bridge infill covered the same
    /// area — double-extrusion.  The fix counts `IsOn` as *inside* via
    /// `vertex_inside_or_on_paths_eo`, so boundary-only paths are dropped.
    #[test]
    fn test_clip_walls_drops_boundary_on_path() {
        // Bridge zone: 4×4 square at (3,3)-(7,7).
        let bridge: Path = vec![(3.0, 3.0), (7.0, 3.0), (7.0, 7.0), (3.0, 7.0)].into();
        let bridge_region = Paths::new(vec![bridge]);

        // Wall path that traces the bridge zone boundary exactly.
        // All four vertices are IsOn the bridge zone polygon.
        let wall: Path = vec![(3.0, 3.0), (7.0, 3.0), (7.0, 7.0), (3.0, 7.0)].into();
        let mut layer = SliceLayer::new(0.4);
        layer.paths.push(wall);
        layer.path_roles.push(ExtrusionRole::OuterWall);

        clip_walls_against_bridge_region(&mut layer, &bridge_region);

        assert!(
            layer.paths.is_empty(),
            "Wall path exactly on bridge zone boundary must be dropped to prevent \
             bridge/wall double-extrusion; got {} paths",
            layer.paths.len()
        );
    }

    /// **Regression (issue #107)** — two fill islands that share an
    /// **along**-strand band but are separated **across** the scan-progression
    /// axis by empty scan rows must NOT be chained together.
    ///
    /// Before the fix, `generate_rectilinear_infill` elided the empty rows and
    /// the serpentine chaining reconnected the two islands, emitting the
    /// connector as an *extruding* move straight across the void (the "phantom
    /// bridge" / extrude-over-thin-air defect that produced a ~25.6 mm `Bridge`
    /// strand across the Voron cube's logo pocket on layer 131).
    #[test]
    fn test_infill_does_not_chain_across_empty_scan_rows() {
        let line_spacing = 1.0;

        // Two rectangles sharing the X-band [0, 4]; separated in Y by an 8 mm
        // void (y ∈ (2, 10) is empty).  With angle 0° the scan lines are
        // horizontal and progress along +Y, so the islands are separated along
        // the scan axis with fully-empty rows between them.
        let island_a: Path = vec![(0.0, 0.0), (4.0, 0.0), (4.0, 2.0), (0.0, 2.0)].into();
        let island_b: Path = vec![(0.0, 10.0), (4.0, 10.0), (4.0, 12.0), (0.0, 12.0)].into();
        let region = Paths::new(vec![island_a, island_b]);

        let paths = generate_rectilinear_infill(&region, line_spacing, 0.0, 0.0);

        assert!(!paths.is_empty(), "both islands must still be filled");

        // No extruding segment may span a scan-axis (Y) gap larger than
        // `SERPENTINE_ROW_GAP_THRESHOLD × line_spacing`.  A void-spanning
        // connector would jump the full 8 mm between the islands.
        let max_allowed = line_spacing * SERPENTINE_ROW_GAP_THRESHOLD;
        for path in paths.iter() {
            let verts: Vec<(f64, f64)> = path.iter().map(|p| (p.x(), p.y())).collect();
            for w in verts.windows(2) {
                let dy = (w[1].1 - w[0].1).abs();
                assert!(
                    dy <= max_allowed + 1e-6,
                    "infill segment spans a {dy:.3} mm scan-axis gap (> {max_allowed} mm) — \
                     the chain reconnected across the void"
                );
            }
        }

        // Both bands must still be covered (island A near y≈0–2, island B near
        // y≈10–12) — the fix separates the chains, it must not drop either.
        let ys: Vec<f64> = paths
            .iter()
            .flat_map(|p| p.iter().map(|pt| pt.y()))
            .collect();
        assert!(
            ys.iter().any(|&y| y <= 2.5),
            "island A (y∈[0,2]) must still be filled"
        );
        assert!(
            ys.iter().any(|&y| y >= 9.5),
            "island B (y∈[10,12]) must still be filled"
        );
    }

    /// A single **contiguous** region must still produce one fully-chained
    /// serpentine path: the gap-finalisation guard must not fire on adjacent
    /// rows (whose `scan_y` differ by exactly one `line_spacing`).
    #[test]
    fn test_infill_single_region_chains_without_false_gap() {
        let line_spacing = 1.0;
        let rect: Path = vec![(0.0, 0.0), (4.0, 0.0), (4.0, 6.0), (0.0, 6.0)].into();
        let region = Paths::new(vec![rect]);

        let paths = generate_rectilinear_infill(&region, line_spacing, 0.0, 0.0);

        assert_eq!(
            paths.len(),
            1,
            "a convex contiguous region must remain a single serpentine chain"
        );

        // Every vertical step within the chain is exactly one line_spacing;
        // there must be no gap the guard could have wrongly introduced.
        let verts: Vec<(f64, f64)> = paths
            .iter()
            .next()
            .unwrap()
            .iter()
            .map(|p| (p.x(), p.y()))
            .collect();
        for w in verts.windows(2) {
            let dy = (w[1].1 - w[0].1).abs();
            assert!(
                dy <= line_spacing + 1e-6,
                "unexpected {dy:.3} mm gap inside a contiguous serpentine chain"
            );
        }
    }

    /// A thin interior channel — the wall-band sliver Arachne's per-island-
    /// average estimate leaves in a locally-thin cross-section — must be opened
    /// away so no rectilinear top/bottom surface is placed inside it.
    #[test]
    fn test_open_interior_drops_thin_channel() {
        // 0.6 mm-wide channel: narrower than
        // SURFACE_MIN_INTERIOR_WIDTH_NOZZLE_MULT (2.5) × 0.4 mm = 1.0 mm.
        let channel: Path = vec![(0.0, 0.0), (12.0, 0.0), (12.0, 0.6), (0.0, 0.6)].into();
        let interior = Paths::new(vec![channel]);

        let opened = open_interior_for_surface(&interior, 0.4);

        assert!(
            opened.is_empty(),
            "a sub-1mm interior channel must be opened away, got {} sub-path(s)",
            opened.len()
        );
    }

    /// A genuine, several-millimetre-wide infill area (a real deck/roof surface)
    /// must survive the opening at essentially its full extent.
    #[test]
    fn test_open_interior_keeps_wide_region() {
        // 6×6 mm region — far wider than the 1.0 mm threshold.
        let square: Path = vec![(0.0, 0.0), (6.0, 0.0), (6.0, 6.0), (0.0, 6.0)].into();
        let interior = Paths::new(vec![square]);

        let opened = open_interior_for_surface(&interior, 0.4);
        let area: f64 = opened.iter().map(|p| p.signed_area().abs()).sum();

        // Opening only rounds the corners (radius 0.5 mm); the 36 mm² square
        // must lose no more than a few percent.
        assert!(
            area >= 34.0,
            "a wide interior must be preserved (area {area:.2} mm², expected ≳ 34)"
        );
    }

    /// **Regression** — the sub-bead sliver the wall-band trim leaves along a
    /// boundary it meets at a grazing angle must be erased, so the scanline
    /// never fills it with a micro-serpentine of stub segments.
    ///
    /// Measured on the Filament Card Caddy's hexagon logo (0.44 mm extrusion):
    /// the trim left ≈0.22 mm-wide crescents of ≈4.5 mm² along the two hexagon
    /// edges lying only 15° off the fill direction, which the scanline filled
    /// with a repeating 0.8 mm-line / 0.6 mm-connector zig-zag.
    #[test]
    fn test_open_surface_region_drops_sub_bead_sliver() {
        // 0.22 mm-wide crescent — half of the 0.44 mm extrusion width.
        let sliver: Path = vec![(0.0, 0.0), (20.0, 0.0), (20.0, 0.22), (0.0, 0.22)].into();
        let region = Paths::new(vec![sliver]);

        let opened = open_surface_region_for_fill(region, 0.44);

        assert!(
            opened.is_empty(),
            "a sub-bead sliver must be erased, got {} sub-path(s)",
            opened.len()
        );
    }

    /// A strip exactly one bead wide *can* hold a bead, and anything thicker
    /// must survive the filter at its **exact original shape** — including
    /// sharp convex corners, which a plain morphological opening would round
    /// (and a rounded corner makes the scanline emit extra stub spans).
    #[test]
    fn test_open_surface_region_keeps_printable_regions() {
        // 3×3 mm surface — small in area but far wider than one 0.44 mm bead.
        let small: Path = vec![(0.0, 0.0), (3.0, 0.0), (3.0, 3.0), (0.0, 3.0)].into();
        let opened = open_surface_region_for_fill(Paths::new(vec![small]), 0.44);
        let area: f64 = opened.iter().map(|p| p.signed_area().abs()).sum();
        assert!(
            area >= 8.95,
            "a printable surface must keep its sharp corners (area {area:.3} mm², expected ≈ 9.0)"
        );

        // A 1.0 mm-wide rib is over two beads wide and must also survive.
        let rib: Path = vec![(0.0, 0.0), (12.0, 0.0), (12.0, 1.0), (0.0, 1.0)].into();
        let opened = open_surface_region_for_fill(Paths::new(vec![rib]), 0.44);
        assert!(
            !opened.is_empty(),
            "a 1 mm rib is wider than one bead and must be kept"
        );
    }

    /// A zero/negative width must be a no-op rather than erasing the region.
    #[test]
    fn test_open_surface_region_degenerate_width_is_noop() {
        let square: Path = vec![(0.0, 0.0), (5.0, 0.0), (5.0, 5.0), (0.0, 5.0)].into();
        let region = Paths::new(vec![square]);
        let opened = open_surface_region_for_fill(region.clone(), 0.0);
        assert_eq!(
            opened.len(),
            region.len(),
            "a degenerate width must leave the region untouched"
        );
    }

    /// **Regression** — a top surface detected over a locally-thin cross-section    /// (a thin wall-band channel whose aft end steps back a layer) must NOT be
    /// filled: the opened interior is empty there, so the surface is dropped,
    /// matching the `classic` generator.  Guards the "tiny top-surface extrudes
    /// in a thin wall channel" defect (Benchy hull sides / funnel transitions).
    #[test]
    fn test_thin_channel_interior_yields_no_top_surface() {
        // layer i: a 1.2 mm-wide wall stack, 20 mm long.
        // layer i+1: the same stack with its tip receded 5 mm (y ≤ 15), so the
        // y∈[15,20] end is "exposed" per the perimeter difference test.
        let wall_i: Path = vec![(0.0, 0.0), (1.2, 0.0), (1.2, 20.0), (0.0, 20.0)].into();
        let mut layer_i = SliceLayer::new(0.2);
        layer_i.paths.push(wall_i);
        layer_i.path_roles.push(ExtrusionRole::OuterWall);

        let wall_above: Path = vec![(0.0, 0.0), (1.2, 0.0), (1.2, 15.0), (0.0, 15.0)].into();
        let mut layer_above = SliceLayer::new(0.2);
        layer_above.paths.push(wall_above);
        layer_above.path_roles.push(ExtrusionRole::OuterWall);

        let mut layers = vec![layer_i, layer_above];

        // Hand-crafted interiors: layer 0 is a 0.8 mm-wide channel (a wall-band
        // sliver, narrower than 2.5 × 0.4 = 1.0 mm); the exposed tip overlaps it,
        // so without the opening a ~4 mm² top surface would be filled there.
        let ch0: Path = vec![(0.2, 0.0), (1.0, 0.0), (1.0, 20.0), (0.2, 20.0)].into();
        let ch1: Path = vec![(0.2, 0.0), (1.0, 0.0), (1.0, 15.0), (0.2, 15.0)].into();
        let interior_regions: Vec<Paths> = vec![Paths::new(vec![ch0]), Paths::new(vec![ch1])];

        generate_top_bottom_surfaces_with_interior(
            &mut layers,
            &SurfaceConfig {
                top_layers: 1,
                bottom_layers: 0,
                layer_height: 0.2,
                infill_angle: 45.0,
                nozzle_diameter_mm: 0.4,
                solid_surface_line_width_mm: 0.0,
                min_infill_extrusion_mm: 0.0,
                bridge_flow_ratio: 1.0,
                bridge_min_area_mm2: 1.0,
                bridge_noise_filter_mm: 0.0,
                bridge_anchor_mm: 0.0,
                infill_overlap_percent: 0.25,
                ensure_vertical_shell_thickness: false,
                bridge_angle_deg: 0.0,
                top_pattern: SurfacePattern::Rectilinear,
                bottom_pattern: SurfacePattern::Rectilinear,
                internal_solid_pattern: SurfacePattern::Rectilinear,
                ..Default::default()
            },
            Some(&interior_regions),
        );

        assert!(
            !layers[0].path_roles.contains(&ExtrusionRole::TopSurface),
            "a top surface over a thin wall-band channel must be dropped"
        );
    }

    /// A gap-fill bead running down the centre of a solid-surface strip (surface
    /// on both perpendicular sides) is redundant with the surface and must be
    /// pruned — the Benchy rear-rail "gap bead under a top surface" defect.
    #[test]
    fn test_prune_gap_fill_sandwiched_by_surface() {
        let mut layer = SliceLayer::new(0.2);
        // Gap-fill bead: a horizontal 0.56 mm-wide centreline at y = 0.
        let bead: Path = vec![(-8.0, 0.0), (8.0, 0.0)].into();
        layer.paths.push(bead);
        layer.path_roles.push(ExtrusionRole::GapFill);
        layer.path_widths.push(Some(0.56));
        layer.path_vertex_widths.push(Some(vec![0.56, 0.56]));
        layer.path_is_open.push(true);

        // solid_regions: two surface bands flanking the bead with a bead-wide
        // corridor carved out (as `blocked_for_surface` does): the corridor is
        // the bead's half-width (0.28 mm), so the surface starts at y = ±0.3 —
        // present on BOTH sides.
        let top_band: Path = vec![(-8.0, 0.3), (8.0, 0.3), (8.0, 1.3), (-8.0, 1.3)].into();
        let bot_band: Path = vec![(-8.0, -1.3), (8.0, -1.3), (8.0, -0.3), (-8.0, -0.3)].into();
        layer.solid_regions = Paths::new(vec![top_band, bot_band]);

        let mut layers = vec![layer];
        prune_redundant_gap_fill(&mut layers, 0.4);

        assert!(
            !layers[0].path_roles.contains(&ExtrusionRole::GapFill),
            "a gap bead flanked by surface on both sides must be pruned"
        );
    }

    /// A gap-fill bead that a solid surface only abuts on **one** side is a
    /// genuine thin neck (sparse infill would skip it) and must be kept.
    #[test]
    fn test_keep_gap_fill_abutting_surface_one_side() {
        let mut layer = SliceLayer::new(0.2);
        let bead: Path = vec![(-8.0, 0.0), (8.0, 0.0)].into();
        layer.paths.push(bead);
        layer.path_roles.push(ExtrusionRole::GapFill);
        layer.path_widths.push(Some(0.56));
        layer.path_vertex_widths.push(Some(vec![0.56, 0.56]));
        layer.path_is_open.push(true);

        // Surface present on ONE side only (y ∈ [0.3, 1.3], reachable by the
        // probe); the other side is open (a wall band the sparse infill can't
        // reach).
        let top_band: Path = vec![(-8.0, 0.3), (8.0, 0.3), (8.0, 1.3), (-8.0, 1.3)].into();
        layer.solid_regions = Paths::new(vec![top_band]);

        let mut layers = vec![layer];
        prune_redundant_gap_fill(&mut layers, 0.4);

        assert!(
            layers[0].path_roles.contains(&ExtrusionRole::GapFill),
            "a gap bead with surface on only one side is a real neck — keep it"
        );
    }

    /// The walls-only wall-bead footprint (`include_gap_fill = false`) must omit
    /// gap-fill beads, so the solid-surface trim does not re-carve a gap bead it
    /// accounts for separately — the fix for the 3DBenchy rear-rail hole that
    /// split the surface into two bands and leaked sparse infill.
    #[test]
    fn test_wall_footprint_excludes_gap_fill_when_requested() {
        let mut layer = SliceLayer::new(0.2);
        // An outer wall loop around a thin strip.
        let wall: Path = vec![(-8.0, -0.9), (8.0, -0.9), (8.0, 0.9), (-8.0, 0.9)].into();
        layer.paths.push(wall);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.path_widths.push(Some(0.4));
        layer.path_vertex_widths.push(None);
        layer.path_is_open.push(false);
        // A gap-fill bead down the centre.
        let bead: Path = vec![(-7.0, 0.0), (7.0, 0.0)].into();
        layer.paths.push(bead);
        layer.path_roles.push(ExtrusionRole::GapFill);
        layer.path_widths.push(Some(0.56));
        layer.path_vertex_widths.push(Some(vec![0.56, 0.56]));
        layer.path_is_open.push(true);

        let with_gap = compute_wall_bead_footprint_filtered(&layer, 0.4, true);
        let without_gap = compute_wall_bead_footprint_filtered(&layer, 0.4, false);
        let area = |p: &Paths| p.iter().map(|q| q.signed_area().abs()).sum::<f64>();
        assert!(
            area(&with_gap) > area(&without_gap) + 1.0,
            "including gap fill must add the centre bead's area ({:.2} vs {:.2})",
            area(&with_gap),
            area(&without_gap)
        );
    }

    /// The surface-carve gap footprint must exclude a bead sandwiched by the
    /// surface (it is pruned, so carving it would hole the surface) while still
    /// carving a genuine one-sided neck (so the surface abuts it).
    #[test]
    fn test_gap_footprint_excludes_sandwiched_only() {
        let mut layer = SliceLayer::new(0.2);
        // Sandwiched centre bead.
        let mid: Path = vec![(-7.0, 0.0), (7.0, 0.0)].into();
        layer.paths.push(mid);
        layer.path_roles.push(ExtrusionRole::GapFill);
        layer.path_widths.push(Some(0.56));
        layer.path_vertex_widths.push(Some(vec![0.56, 0.56]));
        layer.path_is_open.push(true);
        // Genuine neck poking out below the surface (surface only above it).
        let neck: Path = vec![(-7.0, -3.0), (7.0, -3.0)].into();
        layer.paths.push(neck);
        layer.path_roles.push(ExtrusionRole::GapFill);
        layer.path_widths.push(Some(0.56));
        layer.path_vertex_widths.push(Some(vec![0.56, 0.56]));
        layer.path_is_open.push(true);

        // Surface flanks the centre bead on both sides (y ∈ ±[0.3,1.3]); the neck
        // at y=-3 has surface on neither side.
        let top: Path = vec![(-8.0, 0.3), (8.0, 0.3), (8.0, 1.3), (-8.0, 1.3)].into();
        let bot: Path = vec![(-8.0, -1.3), (8.0, -1.3), (8.0, -0.3), (-8.0, -0.3)].into();
        let surface = Paths::new(vec![top, bot]);

        let all = compute_gap_fill_footprint(&layer, 0.4);
        let excl = compute_gap_fill_footprint_excluding_sandwiched(&layer, 0.4, &surface);
        let area = |p: &Paths| p.iter().map(|q| q.signed_area().abs()).sum::<f64>();
        // The excluding footprint keeps the neck but drops the sandwiched centre
        // bead, so it is strictly smaller than the all-beads footprint.
        assert!(
            area(&excl) > 1.0,
            "the genuine neck must remain in the footprint"
        );
        assert!(
            area(&excl) < area(&all) - 1.0,
            "the sandwiched centre bead must be dropped ({:.2} vs {:.2})",
            area(&excl),
            area(&all)
        );
    }
}
