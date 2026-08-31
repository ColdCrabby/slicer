use clipper2::*;

use super::types::{ExtrusionRole, SliceLayer};

/// Extra margin — as a multiple of the nozzle diameter — by which
/// `layer.solid_regions` is **grown before being subtracted** from the sparse-
/// infill area.
///
/// `solid_regions` is a nominal polygon, but the top/bottom surface is actually
/// printed as a rectilinear serpentine whose stepped extent only approximates
/// it, and the surface pass has already trimmed it back off the wall band.
/// Subtracting the raw outline therefore leaves a thin crescent **sliver**
/// between the solid region and the wall all along a curved perimeter.  The
/// scanline shatters that sliver into a swarm of sub-millimetre dashes — 31 on
/// 3DBenchy layer 41 alone — each an isolated dab costing a full
/// retract → travel → un-retract, churning far more filament through the nozzle
/// than it deposits (the jam risk) for no structural gain: the space is already
/// flanked by the solid surface on one side and a wall bead on the other.
///
/// One bead width (`1.0 × d`) is what actually clears the sliver: the surface's
/// own fill lines are `d` wide about their centerlines, so the nominal polygon
/// under-states the deposited material by up to half a bead on each side.
/// Measured on 3DBenchy layer 41, isolated sub-1.5 mm infill paths fall 33 → 6
/// at `1.0`, while `0.5` leaves all 33 (the sliver is simply wider than half a
/// bead). Larger values buy almost nothing (`2.0` → 5) and only pull sparse
/// infill further from the surface it should abut.
///
/// **Keying this to `solid_regions` is the whole point.** It makes the
/// correction an exact **no-op on layers that carry no solid surface**, so a
/// genuinely thin wall-to-wall cavity — the hollow-box mid-height layers of the
/// filament caddy, which are walls + sparse lattice and nothing else — keeps its
/// full lattice (verified: wall-zone void and infill length both unchanged to
/// the last digit at every margin tested).  An earlier attempt used a blanket
/// morphological *opening* of the whole infill area; that cannot tell an
/// artifact sliver from a real thin cavity and erased the caddy's lattice
/// outright — its wall-zone void more than doubled (62 → 146 mm²) and 35 % of
/// its infill vanished, which the slicing quality gate correctly caught.
const SOLID_MARGIN_NOZZLE_MULT: f64 = 1.0;

/// Minimum area, as a multiple of `nozzle_diameter²`, that a **connected**
/// sparse-infill region must have to be worth entering at all.
///
/// At a 0.4 mm nozzle this is `12.5 × 0.16 = 2.0 mm²` — a region so small that
/// the scanline can only lay a single sub-2 mm dash in it.  Reaching that dash
/// costs a full retract → travel → un-retract to deposit ~0.1 mm³ of
/// disconnected material: the "tiny infill splat" (the Benchy bow tip, where the
/// hull narrows to a wedge that walls plus gap fill already fill, is the
/// canonical case at ≈ 1.6 mm²).
///
/// **This is an *area* filter on whole connected regions, never a width filter
/// on the infill area as a whole.** That distinction is what makes it safe: a
/// genuinely thin cavity that deserves a lattice — the filament caddy's
/// hollow-box mid-height layers — is a *large* connected region that merely
/// happens to be narrow, so it is untouched. Measured across the QA corpus the
/// separation is not marginal but categorical: the caddy has **no** infill
/// region at all between `0.01 mm²` and `10 mm²`, so the threshold sits in an
/// empty band two orders of magnitude wide. Morphologically *opening* the infill
/// area instead — the earlier attempt recorded on [`SOLID_MARGIN_NOZZLE_MULT`] —
/// cannot make that distinction and destroyed the caddy lattice.
const INFILL_MIN_REGION_AREA_NOZZLE_MULT: f64 = 12.5;

/// Identify connected sparse-infill regions smaller than
/// [`INFILL_MIN_REGION_AREA_NOZZLE_MULT`] × `d²`.
///
/// Groups the Clipper2 contour soup into islands (one CCW outer plus the CW
/// holes it encloses), measures each island's **net** area (outer minus its
/// holes, so a ring is judged by its material, not its bounding extent), and
/// returns the islands too small to be worth entering.
///
/// The caller removes the *generated paths* that fall inside these regions
/// rather than removing the regions before generation. That matters: the
/// scanline seeds its phase from the bounding box of the whole infill area, so
/// deleting an outlying sliver first would shift every infill line on the layer
/// (measured: 27 mm of line movement on a Benchy layer whose only dropped
/// regions totalled 0.05 mm²). Filtering afterwards is exactly subtractive.
fn splat_infill_regions(area: &Paths, nozzle_diameter_mm: f64) -> Vec<Paths> {
    let min_area = INFILL_MIN_REGION_AREA_NOZZLE_MULT * nozzle_diameter_mm * nozzle_diameter_mm;
    if min_area <= 0.0 || area.is_empty() {
        return Vec::new();
    }

    group_islands(area)
        .into_iter()
        .filter(|island| island_net_area(island) < min_area)
        .map(|(outer, holes)| {
            let mut paths = vec![outer];
            paths.extend(holes);
            Paths::new(paths)
        })
        .collect()
}

/// One CCW outer contour and the CW holes it **directly** encloses.
type Island = (clipper2::Path, Vec<clipper2::Path>);

/// Group a flat Clipper2 contour list into islands.
///
/// Each hole is assigned to the **smallest** outer that surrounds it, which is
/// its immediate parent. Clipper2 returns contours flat, so in a nested run
/// (`D ⊂ C ⊂ B ⊂ A` — a plate with a hole holding a hollow post, or a hull with
/// a cabin) `A.surrounds_path(D)` is true as well. Attributing `D` to `A` too
/// would be wrong twice over: the inner island's void would be subtracted from
/// the outer island's material, and `D` would be emitted **twice**. A repeated
/// path flips the even-odd parity `point_in_region_even_odd` counts on, so the
/// enclosed void would read as solid and be filled straight across.
fn group_islands(paths: &Paths) -> Vec<Island> {
    let contours: Vec<clipper2::Path> = paths.iter().cloned().collect();
    let outers: Vec<&clipper2::Path> = contours.iter().filter(|p| p.signed_area() > 0.0).collect();

    let mut islands: Vec<Island> = outers.iter().map(|o| ((*o).clone(), Vec::new())).collect();

    for hole in contours.iter().filter(|p| p.signed_area() < 0.0) {
        let parent = outers
            .iter()
            .enumerate()
            .filter(|(_, o)| o.surrounds_path(hole))
            .min_by(|(_, a), (_, b)| a.signed_area().abs().total_cmp(&b.signed_area().abs()));
        if let Some((idx, _)) = parent {
            islands[idx].1.push(hole.clone());
        }
    }

    islands
}

/// Net material of an island: its outer area minus the holes it encloses, so a
/// ring is judged by what it actually prints rather than its bounding extent.
fn island_net_area(island: &Island) -> f64 {
    island.0.signed_area().abs() - island.1.iter().map(|h| h.signed_area().abs()).sum::<f64>()
}

/// True when `path` lies inside one of the `splats` regions.
///
/// Tests **segment midpoints**, not vertices: a generated infill line has its
/// endpoints exactly *on* the region boundary, where the integer-scaled
/// point-in-polygon test can land either side. Midpoints are strictly interior,
/// and a majority vote tolerates a serpentine whose connector runs along an
/// edge.
fn path_is_in_a_splat_region(path: &clipper2::Path, splats: &[Paths]) -> bool {
    let pts: Vec<(f64, f64)> = path.iter().map(|p| (p.x(), p.y())).collect();
    if pts.len() < 2 {
        return pts
            .first()
            .is_some_and(|p| splat_contains(p.0, p.1, splats));
    }
    let mut inside = 0usize;
    for w in pts.windows(2) {
        let mx = 0.5 * (w[0].0 + w[1].0);
        let my = 0.5 * (w[0].1 + w[1].1);
        if splat_contains(mx, my, splats) {
            inside += 1;
        }
    }
    inside * 2 > pts.len() - 1
}

/// True when `(x, y)` falls in any splat region.
fn splat_contains(x: f64, y: f64, splats: &[Paths]) -> bool {
    splats
        .iter()
        .any(|s| super::surfaces::vertex_inside_or_on_paths_eo(x, y, s))
}

/// Calculate the interior region of a layer where solid surfaces and sparse
/// infill should be printed (i.e. the area enclosed by the **innermost** wall
/// of every island, optionally shrunk by a configured overlap).
///
/// # Strategy
///
/// Use the `OuterWall` centerline paths directly as the outer extent of each
/// island.  Arachne's outermost bead sits at inward depth `d/2` from the raw
/// mesh contour, so these paths are already a well-formed Clipper2 `Paths`
/// with correct winding (CCW for solid islands, CW for holes).  **Winding is
/// preserved — not normalised** — so that holes are correctly represented as
/// void regions rather than being flipped into solid material.
///
/// From that outer extent we deflate inward by:
///   `(walls_per_island − 0.5) × nozzle_diameter − overlap_distance`
/// where `walls_per_island = ceil(total_wall_beads / outer_island_count)`.
/// The `−0.5 × d` term accounts for the half-bead depth that the `OuterWall`
/// centerline is already shifted inward from the true model boundary.
///
/// Returns an empty `Paths` when the interior collapses, signalling that
/// walls alone fill the cross-section (the "smart-skip" outcome).
pub(crate) fn calculate_interior_region(
    layer: &SliceLayer,
    overlap_percent: f64,
    nozzle_diameter: f64,
    max_walls_per_island: usize,
) -> Paths {
    // Use OuterWall paths as the outer extent of each island.
    // Winding is preserved (CCW for solid islands, CW for holes) so that
    // Clipper2 inflate correctly treats holes as voids rather than solid material.
    let outer_paths = Paths::new(
        layer
            .paths
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                if layer.role_for_path(i) == ExtrusionRole::OuterWall {
                    Some(p.clone())
                } else {
                    None
                }
            })
            .collect(),
    );

    if outer_paths.is_empty() {
        return Paths::new(vec![]);
    }

    // Count total wall bead paths (outer + inner) and outer island paths so we
    // can estimate how many beads deep each island is.
    let total_wall_count = layer
        .paths
        .iter()
        .enumerate()
        .filter(|(i, _)| {
            matches!(
                layer.role_for_path(*i),
                ExtrusionRole::OuterWall | ExtrusionRole::InnerWall
            )
        })
        .count();
    let outer_count = outer_paths.len().max(1);
    // Estimate beads per island from the ratio of total wall paths to outer
    // island paths.  For complex cross-sections (multi-contour, portholes,
    // etc.) the Arachne generator can produce many paths per bead-ring, making
    // the naive ceiling overflow far past the configured wall_count.  Cap the
    // result at max_walls_per_island (= params.wall_count from the pipeline)
    // so the interior deflation never exceeds what the actual bead geometry
    // dictates.
    let computed = total_wall_count.div_ceil(outer_count);
    let walls_per_island = if max_walls_per_island > 0 {
        computed.min(max_walls_per_island)
    } else {
        computed
    };

    // The OuterWall bead centerline is already at depth d/2 from the raw mesh
    // contour.  We need to deflate inward by the remaining wall band thickness:
    //   (walls_per_island × d) − (d/2 already accounted for by the bead offset)
    //   = (walls_per_island − 0.5) × d
    // Subtract the configured overlap so surfaces bond to the innermost wall.
    let overlap_distance = nozzle_diameter * overlap_percent;
    let total_inward = (walls_per_island as f64 - 0.5) * nozzle_diameter - overlap_distance;

    if total_inward < 0.01 {
        // Walls fill the entire cross-section; return outer_paths as the
        // interior (degenerate single-wall case).
        return outer_paths;
    }

    // Empty result = correct "smart-skip" signal (walls alone fill the layer).
    clipper2::inflate(
        outer_paths,
        -total_inward,
        JoinType::Round,
        EndType::Polygon,
        2.0,
    )
}

/// Configuration for sparse infill generation.
///
/// Mirrors [`crate::core::SurfaceConfig`]: one struct instead of a long
/// positional argument list, so the pipeline reads as a description of the
/// settings rather than a sequence of bare numbers.
#[derive(Debug, Clone, Copy)]
pub struct InfillConfig {
    /// Infill density as a fraction (0.0 = no infill, 1.0 = solid).
    pub density: f64,
    /// Pattern geometry to generate (rectilinear, grid, …).
    pub pattern: crate::infill::InfillPattern,
    /// Base angle in degrees; alternating layers rotate +90° on top of it.
    pub base_angle_deg: f64,
    /// Flow spacing of one sparse-infill bead in mm — `width − h·(1 − π/4)`.
    ///
    /// This is the density unit: lines are laid `spacing / density` apart and
    /// the G-code generator charges `spacing × layer_height` per mm for them.
    /// Resolve it with [`crate::core::sparse_infill_nominal_width_mm`] +
    /// [`crate::core::extrusion_flow_spacing_mm`].
    pub spacing_mm: f64,
    /// Nozzle diameter, used when computing infill regions on the fly.
    pub nozzle_diameter_mm: f64,
    /// Gap in mm between the innermost wall and the infill boundary. A positive
    /// value leaves a gap; `0.0` means infill starts exactly at the inner wall edge.
    pub perimeter_gap_mm: f64,
    /// Isolated sparse-infill segments shorter than this are dropped as splats:
    /// a sub-threshold dash in a narrow corner deposits a mechanically
    /// insignificant amount of material yet still costs a full
    /// retract → travel → un-retract to reach. `0` disables the filter.
    pub min_extrusion_mm: f64,
    /// How far a lone infill line end may run along the boundary to anchor
    /// itself, in mm. `0` disables open anchors.
    pub anchor_length_mm: f64,
    /// Longest boundary stretch, in mm, that may join two infill lines into one
    /// continuous path. `0` disables anchoring entirely.
    pub anchor_length_max_mm: f64,
    /// Print sparse infill only every N layers, at N× the height. `1` = off.
    pub every_layers: u32,
    /// Tallest combined sparse-infill layer in mm; `0` = use the nozzle diameter.
    pub combination_max_layer_height_mm: f64,
    /// Print layer height in mm, needed to work out how many layers fit under
    /// the combined-height cap.
    pub layer_height_mm: f64,
    /// Force a fully solid internal layer every N layers. `0` = off.
    pub solid_every_layers: u32,
    /// Flow spacing of one **solid** fill bead in mm, used for the forced-solid
    /// layers so their pitch matches the flow the G-code generator charges.
    pub solid_spacing_mm: f64,
    /// Fill pattern used for those forced-solid layers.
    pub solid_pattern: crate::infill::SurfacePattern,
}

/// One region to fill on one layer.
struct FillJob {
    area: Paths,
    /// Extrusion height override; `None` = the print's layer height.
    height: Option<f64>,
    /// Fill it densely as internal solid infill rather than sparsely.
    solid: bool,
}

/// Redistribute per-layer fill areas so that groups of layers print their shared
/// sparse infill **once**, at the stacked height.
///
/// Returns, per layer, the fill jobs to generate: an ordinary one at the layer's
/// own height (`None`), plus — on the top layer of each group — the combined
/// patch tagged with the group's total height.
///
/// Faithful to libslic3r's `PrintObject::combine_infill()`:
///
/// * Layer 0 is never combined; a first layer must always print.
/// * A group grows until its stacked height would reach the cap, which is
///   `min(every_layers × layer_height, max_layer_height, nozzle_diameter)` — a
///   bead cannot reliably be laid taller than the orifice extruding it.
/// * The combined patch is the **intersection** of the group's fill areas, so a
///   region only qualifies where sparse infill exists on *every* layer of the
///   group. Solid surfaces and bridges have already been subtracted from those
///   areas upstream, which is what keeps combining away from shells — the same
///   effect libslic3r gets by intersecting only `stInternal` surfaces.
/// * Lower layers lose the patch *eroded by a clearance*, so each keeps a thin
///   ring of its own infill that the taller bead above bonds into instead of
///   ending against bare air.
/// * Patches too small to be worth the trouble are dropped and left as ordinary
///   per-layer infill.
fn combine_fill_areas(
    areas: Vec<Option<Paths>>,
    layers: &[SliceLayer],
    config: &InfillConfig,
) -> Vec<Vec<FillJob>> {
    let mut jobs: Vec<Vec<FillJob>> = areas
        .iter()
        .enumerate()
        .map(|(i, a)| match a {
            Some(area) => vec![FillJob {
                area: area.clone(),
                height: None,
                solid: is_forced_solid(i, config.solid_every_layers),
            }],
            None => Vec::new(),
        })
        .collect();

    let layer_height = config.layer_height_mm;
    if config.every_layers <= 1 || layer_height <= 0.0 || layers.len() < 3 {
        return jobs;
    }

    // How tall the stacked bead may get, and therefore how many layers a group
    // may hold.
    let cap_mm = if config.combination_max_layer_height_mm > 0.0 {
        config
            .combination_max_layer_height_mm
            .min(config.nozzle_diameter_mm)
    } else {
        config.nozzle_diameter_mm
    };
    // The epsilon keeps a clean ratio from falling a layer short: 0.6 / 0.2 is
    // 2.9999999999999996 in binary floating point, which would silently combine
    // two layers where the user asked for three.
    let by_height = (cap_mm / layer_height + 1e-6).floor() as u32;
    let group_len = config.every_layers.min(by_height.max(1)) as usize;
    if group_len < 2 {
        return jobs;
    }

    let min_area =
        INFILL_MIN_REGION_AREA_NOZZLE_MULT * config.nozzle_diameter_mm * config.nozzle_diameter_mm;

    // The combined bead is as tall as the group it replaces, so it is pulled a
    // half-bead back from the edge of the shared area: a bead that tall pressed
    // against the wall band would have nowhere to go.  libslic3r applies the same
    // idea with a `0.5 × perimeter_width + 1.5 × solid_width` clearance.
    let clearance = 0.5 * config.nozzle_diameter_mm;

    // Layer 0 always prints its own infill, so groups start at layer 1.
    let mut start = 1;
    while start < layers.len() {
        // A forced-solid layer must print its own dense fill at its own height,
        // so it can neither join a group nor be spanned by one.
        if is_forced_solid(start, config.solid_every_layers) {
            start += 1;
            continue;
        }
        let mut end = (start + group_len).min(layers.len());
        if let Some(solid) = (start..end).find(|&i| is_forced_solid(i, config.solid_every_layers)) {
            end = solid;
        }
        if end.saturating_sub(start) < 2 {
            start = end.max(start + 1);
            continue;
        }

        let shared = intersect_fill_areas(&areas[start..end]);
        let shared = drop_small_islands(shared, min_area);
        if shared.is_empty() {
            start = end;
            continue;
        }

        let combined = clipper2::inflate(
            shared.clone(),
            -clearance,
            clipper2::JoinType::Round,
            clipper2::EndType::Polygon,
            2.0,
        );
        let combined = drop_small_islands(combined, min_area);
        if combined.is_empty() {
            start = end;
            continue;
        }

        // The shared area leaves **every** layer of the group — including the
        // top.  The tall bead physically occupies all of those layers, so any
        // sliver left behind would be extruded straight into it.
        for layer_jobs in jobs.iter_mut().take(end).skip(start) {
            if let Some(job) = layer_jobs.first_mut() {
                job.area = difference(job.area.clone(), shared.clone(), FillRule::NonZero)
                    .unwrap_or_default();
            }
        }
        // It reappears once, on top, as tall as the group it stands in for.
        jobs[end - 1].push(FillJob {
            area: combined,
            height: Some(layer_height * (end - start) as f64),
            solid: false,
        });

        start = end;
    }

    jobs
}

/// Whether layer `index` is one of the forced internal solid layers.
///
/// Mirrors PrusaSlicer's `discover_horizontal_shells` rule: every layer whose
/// index is a multiple of N is retyped from sparse to solid
/// (`PrintObject.cpp:2918-2925`). OrcaSlicer has no equivalent setting.
fn is_forced_solid(index: usize, every: u32) -> bool {
    every > 0 && index.is_multiple_of(every as usize)
}

/// Intersect a run of per-layer fill areas, treating a missing layer as empty.
fn intersect_fill_areas(areas: &[Option<Paths>]) -> Paths {
    let mut acc: Option<Paths> = None;
    for area in areas {
        let Some(area) = area else {
            return Paths::default();
        };
        acc = Some(match acc {
            None => area.clone(),
            Some(prev) => match intersect(prev, area.clone(), FillRule::NonZero) {
                Ok(next) if !next.is_empty() => next,
                _ => return Paths::default(),
            },
        });
    }
    acc.unwrap_or_default()
}

/// Drop islands whose net material is below `min_area`.
///
/// A patch that small is not worth splitting off from the layers that would
/// otherwise have printed it normally.
fn drop_small_islands(paths: Paths, min_area: f64) -> Paths {
    if paths.is_empty() || min_area <= 0.0 {
        return paths;
    }

    let mut kept: Vec<clipper2::Path> = Vec::new();
    for island in group_islands(&paths) {
        if island_net_area(&island) < min_area {
            continue;
        }
        kept.push(island.0);
        kept.extend(island.1);
    }
    Paths::new(kept)
}

/// Add infill paths to layers based on slicing parameters.
///
/// Takes a set of layers with perimeter paths and adds infill patterns within
/// the perimeter boundaries. Infill paths are assigned the [`ExtrusionRole::Infill`]
/// role for proper G-code annotation.
///
/// # Arguments
/// * `layers` - Slice layers with perimeter paths (will be modified in place)
/// * `config` - Density, pattern, bead spacing and clearance settings
/// * `precomputed_infill_regions` - Optional per-layer interior regions computed **before**
///   any single-wall restrictions were applied.  When provided, these regions are used
///   instead of calling [`calculate_interior_region`] on each layer, which prevents
///   [`apply_single_wall_restrictions`] from inadvertently expanding the infill area into
///   the space that was occupied by stripped inner walls.
///
///   Pass `None` when calling outside of the full pipeline (e.g. in tests), in which
///   case the regions are derived from the current layer state.
///
/// # Example
/// ```rust,no_run
/// use slicer_engine::core::{slice_mesh, add_infill_to_layers, InfillConfig};
/// use slicer_engine::infill::{InfillPattern, SurfacePattern};
/// # use slicer_engine::mesh::types::Mesh;
/// # let mesh = Mesh::new();
///
/// let mut layers = slice_mesh(&mesh, 0.2);
/// add_infill_to_layers(
///     &mut layers,
///     &InfillConfig {
///         density: 0.2,
///         pattern: InfillPattern::Rectilinear,
///         base_angle_deg: 45.0,
///         spacing_mm: 0.357,
///         nozzle_diameter_mm: 0.4,
///         perimeter_gap_mm: 0.0,
///         min_extrusion_mm: 0.4,
///         anchor_length_mm: 1.4,
///         anchor_length_max_mm: 20.0,
///         every_layers: 1,
///         combination_max_layer_height_mm: 0.0,
///         layer_height_mm: 0.2,
///         solid_every_layers: 0,
///         solid_spacing_mm: 0.357,
///         solid_pattern: SurfacePattern::Monotonic,
///     },
///     None,
/// );
/// ```
pub fn add_infill_to_layers(
    layers: &mut [SliceLayer],
    config: &InfillConfig,
    precomputed_infill_regions: Option<&[Paths]>,
) {
    use crate::infill::{connect_infill, generate_infill, FillParams};

    let InfillConfig {
        density: infill_density,
        pattern: infill_pattern,
        base_angle_deg: infill_base_angle,
        spacing_mm,
        nozzle_diameter_mm,
        perimeter_gap_mm: infill_perimeter_gap_mm,
        min_extrusion_mm: min_infill_extrusion_mm,
        anchor_length_mm,
        anchor_length_max_mm,
        every_layers: _,
        combination_max_layer_height_mm: _,
        layer_height_mm: _,
        solid_every_layers: _,
        solid_spacing_mm,
        solid_pattern,
    } = *config;

    if infill_density <= 0.0 {
        return;
    }

    // Convert the mm gap into the overlap_percent convention used by
    // calculate_interior_region.  A positive gap_mm means MORE inward
    // deflation (smaller interior → gap from wall), which corresponds to a
    // *negative* overlap_percent (since overlap_percent is subtracted from
    // total_inward in that function).
    let gap_overlap = if nozzle_diameter_mm > 0.0 {
        -(infill_perimeter_gap_mm / nozzle_diameter_mm)
    } else {
        0.0
    };

    // ── Parallel compute pass ─────────────────────────────────────────────────
    // Each layer's fill area is independent.  Compute them all in parallel, let
    // the (serial) combining pass redistribute them across layers, then generate
    // and apply.  `None` means "skip this layer" (empty perimeters / empty area).
    let area_of = |layer_idx: usize| -> Option<Paths> {
        let layer = &layers[layer_idx];
        if layer.paths.is_empty() {
            return None;
        }

        let infill_area = if let Some(regions) = precomputed_infill_regions {
            if layer_idx < regions.len() && !regions[layer_idx].is_empty() {
                // Pre-computed regions already account for the wall geometry;
                // apply the perimeter gap on top by deflating an extra step.
                if infill_perimeter_gap_mm > 1e-9 {
                    let deflated = clipper2::inflate(
                        regions[layer_idx].clone(),
                        -infill_perimeter_gap_mm,
                        clipper2::JoinType::Round,
                        clipper2::EndType::Polygon,
                        2.0,
                    );
                    if deflated.is_empty() {
                        return None;
                    }
                    deflated
                } else {
                    regions[layer_idx].clone()
                }
            } else {
                calculate_interior_region(layer, gap_overlap, nozzle_diameter_mm, 0)
            }
        } else {
            calculate_interior_region(layer, gap_overlap, nozzle_diameter_mm, 0)
        };

        if infill_area.is_empty() {
            return None;
        }

        let infill_area = if !layer.solid_regions.is_empty() {
            // Subtract the solid surface **grown by half a bead**, not its raw
            // outline.
            //
            // `solid_regions` is a nominal polygon, but the surface is actually
            // printed as a rectilinear serpentine whose stepped extent only
            // approximates it, and it was already trimmed back off the wall band
            // by the surface pass.  Subtracting the raw outline therefore leaves
            // a thin crescent **sliver** between the solid region and the wall
            // all along a curved perimeter.  The scanline shatters that sliver
            // into a swarm of sub-millimetre dashes (31 on 3DBenchy layer 41
            // alone), each an isolated dab costing a full retract → travel →
            // un-retract for no structural gain — the "infill produces tiny
            // extrudes" defect.  The space is already flanked by the solid
            // surface on one side and a wall bead on the other.
            //
            // Growing the subtracted region by `SOLID_MARGIN_NOZZLE_MULT × d`
            // absorbs that sliver into the (already solid-filled) surface.  It
            // is deliberately keyed to `solid_regions`, so it is an exact
            // **no-op on layers that have no solid surface** — a genuinely thin
            // wall-to-wall cavity, such as the hollow-box mid-height layers of
            // the filament caddy, keeps its full sparse lattice.  A blanket
            // morphological opening of the infill area cannot make that
            // distinction and erases those legitimate lattices.
            let margin = SOLID_MARGIN_NOZZLE_MULT * nozzle_diameter_mm;
            let blocked = if margin > 1e-9 {
                clipper2::inflate(
                    layer.solid_regions.clone(),
                    margin,
                    clipper2::JoinType::Round,
                    clipper2::EndType::Polygon,
                    2.0,
                )
            } else {
                layer.solid_regions.clone()
            };
            let blocked = if blocked.is_empty() {
                layer.solid_regions.clone()
            } else {
                blocked
            };
            let remaining =
                difference(infill_area, blocked, FillRule::Positive).unwrap_or_default();
            if remaining.is_empty() {
                return None;
            }
            remaining
        } else {
            infill_area
        };

        // Subtract the gap-fill bead footprint so sparse infill abuts — never
        // re-extrudes over — the variable-width Arachne gap fill.
        let gap_fp = super::surfaces::compute_gap_fill_footprint(layer, nozzle_diameter_mm);
        let infill_area = if gap_fp.is_empty() {
            infill_area
        } else {
            let remaining = difference(infill_area, gap_fp, FillRule::Positive).unwrap_or_default();
            if remaining.is_empty() {
                return None;
            }
            remaining
        };

        // Subtract the actual **wall bead footprint** (grown by the configured
        // perimeter gap) so sparse infill keeps its intended clearance from the
        // *real* innermost wall, regardless of how far the interior-region
        // estimate reached.  The interior inset assumes a uniform wall count per
        // island; the Arachne generator places a *variable* number of beads, so
        // on a layer whose islands differ in bead count the inset lands a whole
        // bead-width too far out and sparse infill is laid on top of the inner
        // wall (measurable "Inner wall × Sparse infill" double-extrusion).
        // Clipping against the physical bead footprint is count- and
        // width-agnostic and is a no-op where the inset was already correct
        // (classic), so it removes only the genuine over-print.
        //
        // `NonZero` is required (not `Positive`): the wall footprint is a
        // frame with CW hole sub-paths (the enclosed interior).  `Positive`
        // would ignore those holes and treat the frame as a solid block,
        // erasing the whole interior; `NonZero` respects the holes so only the
        // wall band itself is subtracted.
        let wall_fp = super::surfaces::compute_wall_bead_footprint(layer, nozzle_diameter_mm);
        let infill_area = if wall_fp.is_empty() {
            infill_area
        } else {
            let clearance = infill_perimeter_gap_mm.max(0.0);
            let blocked = if clearance > 1e-9 {
                clipper2::inflate(
                    wall_fp,
                    clearance,
                    clipper2::JoinType::Round,
                    clipper2::EndType::Polygon,
                    2.0,
                )
            } else {
                wall_fp
            };
            let remaining = difference(infill_area, blocked, FillRule::NonZero).unwrap_or_default();
            if remaining.is_empty() {
                return None;
            }
            remaining
        };

        Some(infill_area)
    };

    #[cfg(not(target_arch = "wasm32"))]
    let areas: Vec<Option<Paths>> = {
        use rayon::prelude::*;
        (0..layers.len()).into_par_iter().map(area_of).collect()
    };
    #[cfg(target_arch = "wasm32")]
    let areas: Vec<Option<Paths>> = (0..layers.len()).map(area_of).collect();

    // ── Layer combining ───────────────────────────────────────────────────────
    // Hand parts of the lower layers' fill areas up to the top of their group, to
    // be printed once at the stacked height.  A no-op unless the user asked for
    // it, in which case `jobs[i]` may carry a second entry with its own height.
    let jobs = combine_fill_areas(areas, layers, config);

    // ── Parallel generation pass ──────────────────────────────────────────────
    type Generated = Vec<(Paths, Option<f64>, ExtrusionRole)>;
    let generate_for = |layer_idx: usize| -> Generated {
        let layer = &layers[layer_idx];
        let base_angle_rad = infill_base_angle.to_radians();
        // Alternating +90° per layer is what makes successive sparse layers
        // cross-hatch. `aligned-rectilinear` opts out so every layer's lines
        // stack instead.
        let angle_offset = if infill_pattern.alternates_per_layer() && !layer_idx.is_multiple_of(2)
        {
            base_angle_rad + std::f64::consts::FRAC_PI_2
        } else {
            base_angle_rad
        };

        jobs[layer_idx]
            .iter()
            .filter_map(|job| {
                let infill_area = &job.area;
                if infill_area.is_empty() {
                    return None;
                }

                // A forced-solid layer fills the very same area densely instead
                // of sparsely, so it inherits every clip the sparse area already
                // carries — walls, gap fill and the real top/bottom surfaces are
                // all outside it and cannot be printed over.
                if job.solid {
                    let solid = super::surfaces::generate_solid_infill(
                        infill_area,
                        solid_spacing_mm,
                        angle_offset.to_degrees(),
                        min_infill_extrusion_mm,
                        solid_pattern,
                    );
                    return Some((solid, job.height, ExtrusionRole::InternalSolid));
                }

                // Identify regions too small to hold anything but an isolated
                // dash.  The generated paths inside them are dropped *after*
                // generation so the scanline phase (seeded from the full area's
                // bounding box) is unchanged.
                let splats = splat_infill_regions(infill_area, nozzle_diameter_mm);

                let generated = generate_infill(
                    infill_area,
                    &FillParams {
                        pattern: infill_pattern,
                        density: infill_density,
                        spacing_mm,
                        angle_offset,
                        z_height: layer.z,
                    },
                );

                // Weld the line ends to the perimeter they stop against, and
                // merge the pairs a short stretch of wall can join.  This runs
                // **before** the splat and minimum-length filters below: a line
                // that anchoring turns into part of a long continuous path must
                // not first be discarded as an isolated dash.
                let generated = connect_infill(
                    generated,
                    infill_area,
                    anchor_length_mm,
                    anchor_length_max_mm,
                );

                let kept = if splats.is_empty() {
                    generated
                } else {
                    Paths::new(
                        generated
                            .iter()
                            .filter(|p| !path_is_in_a_splat_region(p, &splats))
                            .cloned()
                            .collect::<Vec<_>>(),
                    )
                };
                Some((kept, job.height, ExtrusionRole::Infill))
            })
            .collect()
    };

    #[cfg(not(target_arch = "wasm32"))]
    let results: Vec<Generated> = {
        use rayon::prelude::*;
        (0..layers.len())
            .into_par_iter()
            .map(generate_for)
            .collect()
    };
    #[cfg(target_arch = "wasm32")]
    let results: Vec<Generated> = (0..layers.len()).map(generate_for).collect();

    // Forced-solid layers become solid regions in their own right, so the layer
    // data stays truthful for anything downstream that inspects it.
    let solid_areas: Vec<Paths> = jobs
        .iter()
        .map(|per_layer| {
            let solid: Vec<clipper2::Path> = per_layer
                .iter()
                .filter(|j| j.solid)
                .flat_map(|j| j.area.iter().cloned())
                .collect();
            Paths::new(solid)
        })
        .collect();

    // ── Serial apply pass ─────────────────────────────────────────────────────
    //
    // Drop isolated sparse-infill segments shorter than `min_infill_extrusion_mm`:
    // a sub-threshold dash in a narrow corner deposits a mechanically-
    // insignificant amount of material yet still costs a full retract → travel →
    // un-retract to reach — the "tiny inner-body splat".  This mirrors the guard
    // solid-surface infill already applies in `generate_rectilinear_infill`.
    let min_len_sq = if min_infill_extrusion_mm > 0.0 {
        min_infill_extrusion_mm * min_infill_extrusion_mm
    } else {
        0.0
    };
    let polyline_len_sq_ge = |path: &clipper2::Path, min_sq: f64| -> bool {
        if min_sq <= 0.0 {
            return true;
        }
        let pts: Vec<(f64, f64)> = path.iter().map(|p| (p.x(), p.y())).collect();
        let mut len = 0.0;
        for w in pts.windows(2) {
            len += ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt();
            if len * len >= min_sq {
                return true;
            }
        }
        len * len >= min_sq
    };
    for (layer_idx, per_layer) in results.into_iter().enumerate() {
        let layer = &mut layers[layer_idx];
        for (paths, height, role) in per_layer {
            for infill_path in paths.iter() {
                if !polyline_len_sq_ge(infill_path, min_len_sq) {
                    continue;
                }
                // A combined patch carries an explicit height; ordinary infill
                // leaves the vector empty so nothing downstream pays for it.
                if let Some(h) = height {
                    while layer.path_heights.len() < layer.paths.len() {
                        layer.path_heights.push(None);
                    }
                    layer.path_heights.push(Some(h));
                } else if !layer.path_heights.is_empty() {
                    layer.path_heights.push(None);
                }
                layer.paths.push(infill_path.clone());
                layer.path_roles.push(role);
            }
        }
        let forced_solid = &solid_areas[layer_idx];
        if !forced_solid.is_empty() {
            let merged = union(
                layer.solid_regions.clone(),
                forced_solid.clone(),
                FillRule::NonZero,
            )
            .unwrap_or_else(|_| forced_solid.clone());
            layer.solid_regions = merged;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `A ⊃ B ⊃ C ⊃ D`: a plate with a hole holding a hollow post.
    ///
    /// Clipper2 hands these back flat, so the nesting has to be recovered.
    fn nested_rings() -> Paths {
        let ring = |half: f64, ccw: bool| -> clipper2::Path {
            let pts = if ccw {
                vec![(-half, -half), (half, -half), (half, half), (-half, half)]
            } else {
                vec![(-half, -half), (-half, half), (half, half), (half, -half)]
            };
            pts.into()
        };
        Paths::new(vec![
            ring(50.0, true),  // A: plate outline
            ring(10.0, false), // B: hole in the plate
            ring(7.5, true),   // C: post inside the hole
            ring(5.0, false),  // D: the post's bore
        ])
    }

    #[test]
    fn each_hole_belongs_to_its_immediate_parent() {
        let islands = group_islands(&nested_rings());
        assert_eq!(islands.len(), 2, "one island per outer contour");

        // Smallest outer first is not guaranteed, so look them up by area.
        let plate = islands
            .iter()
            .find(|(o, _)| o.signed_area().abs() > 1000.0)
            .expect("plate island");
        let post = islands
            .iter()
            .find(|(o, _)| o.signed_area().abs() < 1000.0)
            .expect("post island");

        assert_eq!(plate.1.len(), 1, "the plate owns only its own hole");
        assert_eq!(post.1.len(), 1, "the post owns only its own bore");
        assert!(
            (plate.1[0].signed_area().abs() - 400.0).abs() < 1.0,
            "the plate's hole is the 20×20 one, not the post's 10×10 bore"
        );

        // Net material must not double-count the inner island's void.
        assert!(
            (island_net_area(plate) - (10000.0 - 400.0)).abs() < 1.0,
            "the post's bore must not be subtracted from the plate"
        );
    }

    #[test]
    fn drop_small_islands_emits_each_contour_once() {
        // A duplicated hole flips the even-odd parity the infill clipper counts
        // on, so an enclosed void would read as solid and be filled across.
        let kept = drop_small_islands(nested_rings(), 2.0);
        assert_eq!(kept.len(), 4, "every contour exactly once");
    }

    #[test]
    fn drop_small_islands_keeps_a_large_island_holding_a_smaller_one() {
        // Attributing the inner bore to the plate too would make the plate look
        // smaller than it is — and with deep nesting, small enough to drop.
        let kept = drop_small_islands(nested_rings(), 9000.0);
        assert!(
            kept.iter().any(|p| p.signed_area().abs() > 9000.0),
            "the plate is well over the threshold and must survive"
        );
    }
}
