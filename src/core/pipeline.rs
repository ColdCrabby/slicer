use clipper2::Paths;

use crate::logging::{phases, PhaseTimer, ProcessLogger};
use crate::mesh::paint::FacetPaint;
use crate::mesh::types::Mesh;
use crate::settings::params::{SeamPosition, SlicingParams};

use super::infill::{add_infill_to_layers, calculate_interior_region, InfillConfig};
use super::slicer::slice_mesh_with_first_layer;
use super::surfaces::{
    generate_top_bottom_surfaces_with_interior, perimeter_paths_of, prune_redundant_gap_fill,
    SurfaceConfig,
};
use super::types::{ExtrusionRole, OverhangClass, SliceLayer};
use super::walls::{apply_single_wall_restrictions, classify_overhang_perimeters, OverhangGrading};

/// True when `role` is a solid-surface role whose configured fill pattern is
/// monotonic, and whose emitted order and direction must therefore survive the
/// path-ordering pass untouched.
fn monotonic_surface_role(role: ExtrusionRole, params: &SlicingParams) -> bool {
    match role {
        ExtrusionRole::TopSurface => params.top_surface_pattern.is_monotonic(),
        ExtrusionRole::BottomSurface => params.bottom_surface_pattern.is_monotonic(),
        // Ironing is a uniform one-way sweep by construction — reversing any of
        // its lines would drag the hot nozzle back across a stroke it has just
        // flattened, which is the whole thing the pass exists to avoid.
        ExtrusionRole::Ironing => true,
        _ => false,
    }
}

/// Resolve the sparse-infill settings for a slice.
///
/// The bead spacing is derived once, here, from the same nominal width the
/// G-code generator charges flow at (`sparse_infill_nominal_width_mm`), so the
/// pitch the pattern lays lines at and the material deposited on them always
/// agree — that identity is what makes "20 % density" mean 20 % of a solid
/// layer's volume.
fn sparse_infill_config(params: &SlicingParams) -> InfillConfig {
    let nominal = crate::core::sparse_infill_nominal_width_mm(params);
    let spacing_mm = crate::core::extrusion_flow_spacing_mm(nominal, params.layer_height);

    // `infill_anchor_percent` is relative to the bead, exactly as libslic3r
    // resolves its `infill_anchor` percentage against the fill spacing
    // (`Fill.cpp:212-228`), and can never exceed the two-line join cap.
    let anchor_length_max_mm = params.infill_anchor_max_mm.max(0.0);
    let anchor_length_mm =
        (params.infill_anchor_percent.max(0.0) * 0.01 * spacing_mm).min(anchor_length_max_mm);

    InfillConfig {
        density: params.infill_density,
        pattern: params.infill_pattern,
        base_angle_deg: params.infill_base_angle,
        spacing_mm,
        nozzle_diameter_mm: params.nozzle_diameter_mm,
        perimeter_gap_mm: params.infill_perimeter_gap_mm,
        min_extrusion_mm: params.min_infill_extrusion_mm,
        anchor_length_mm,
        anchor_length_max_mm,
        every_layers: params.infill_every_layers,
        combination_max_layer_height_mm: params.infill_combination_max_layer_height_mm,
        layer_height_mm: params.layer_height,
        solid_every_layers: params.solid_infill_every_layers,
        solid_spacing_mm: crate::core::extrusion_flow_spacing_mm(
            crate::core::solid_surface_nominal_width_mm(params),
            params.layer_height,
        ),
        solid_pattern: params.internal_solid_infill_pattern,
    }
}

/// True when a raft will be printed under the object.
///
/// A raft takes over bed contact entirely, which changes the answer to two
/// questions elsewhere in this file: the object's first layer is no longer the
/// one that has to cope with an imperfect bed, and it is no longer the layer a
/// first-layer height was chosen for.
fn raft_is_active(params: &SlicingParams) -> bool {
    params.adhesion_type == crate::settings::params::AdhesionType::Raft && params.raft_layers > 0
}

/// Thickness of the object's bottom layer, in mm.
///
/// `first_layer_height` is a bed-contact remedy, so it is deliberately ignored
/// when a raft is present: the object then starts on plastic, and the raft's own
/// layers are documented to print at `layer_height` (see the adhesion module).
/// Falls back to `layer_height` when unset.
///
/// Shared with the G-code generator, which needs the same answer to decide which
/// layer gets first-layer speeds and temperatures — the two must never disagree
/// about which layer is the first.
pub(crate) fn resolved_first_layer_height(params: &SlicingParams) -> f64 {
    if params.first_layer_height > 0.0 && !raft_is_active(params) {
        params.first_layer_height
    } else {
        params.layer_height
    }
}

/// Charge every path on the object's bottom layer at `first_h` rather than the
/// global layer height.
///
/// The slicer has already given that layer its own Z span; this is the flow half
/// of the same fact. It is applied **after** bed adhesion so a skirt or brim —
/// which sits on the bed at exactly that layer's Z, and is therefore exactly as
/// thick — is charged correctly too. `path_heights` is the established per-path
/// height override, so `extrusion_for_move`, the volumetric-speed cap and the
/// `;HEIGHT:` marker all pick it up with no further plumbing.
fn mark_first_layer_height(layers: &mut [SliceLayer], first_h: f64, layer_height: f64) {
    if (first_h - layer_height).abs() < 1e-9 {
        return;
    }
    let Some(layer) = layers.first_mut() else {
        return;
    };
    // Combined sparse infill never touches layer 0, so nothing else can have
    // written a height override here; a plain fill keeps the array aligned.
    layer.path_heights = vec![Some(first_h); layer.paths.len()];
}

/// Run dimensional compensation and report what it cost.
///
/// A no-op unless the user configured a delta, so the default pipeline is
/// untouched. Losses are logged rather than silently repaired: a compensation
/// large enough to consume a feature is a compensation the user needs to know
/// about.
fn apply_compensation(
    layers: &mut [SliceLayer],
    params: &SlicingParams,
    logger: &dyn ProcessLogger,
) {
    let report = super::compensation::apply_dimensional_compensation(
        layers,
        params.xy_size_compensation,
        params.xy_hole_compensation,
    );
    if report == super::compensation::CompensationReport::default() {
        return;
    }
    logger.log_debug(&format!(
        "applied dimensional compensation (size {:+.3}mm, hole {:+.3}mm)",
        params.xy_size_compensation, params.xy_hole_compensation
    ));
    if report.emptied_layers > 0 {
        logger.log_warn(&format!(
            "dimensional compensation removed every contour from {} layer(s) — the shrink is \
             larger than those cross-sections",
            report.emptied_layers
        ));
    }
}

/// Shrink the layers at the bed to undo the first layer's squish.
///
/// Runs immediately after [`apply_compensation`], in the same raw-contour
/// window. A no-op unless the user configured a shrink — and, unlike the XY
/// deltas, it is also skipped on a raft, where the first layer never meets the
/// plate.
fn apply_elephant_foot(
    layers: &mut [SliceLayer],
    params: &SlicingParams,
    logger: &dyn ProcessLogger,
) {
    let Some(config) = super::compensation::ElephantFootConfig::resolve(params) else {
        return;
    };
    logger.log_debug(&format!(
        "applying elephant-foot compensation ({:.3}mm over {} layer(s), \
         features kept at ≥{:.2}mm)",
        config.shrink_mm, config.layers, config.min_contour_width_mm
    ));
    let timer = PhaseTimer::start(phases::ELEPHANT_FOOT, logger);
    super::compensation::apply_elephant_foot(layers, &config);
    timer.finish();
}

/// Central entry point for the complete slicing pipeline.
///
/// This function processes a mesh through the entire slicing pipeline, including
/// basic slicing, top/bottom surface generation, and wall (perimeter) bead
/// generation.  All pipeline progress is reported through `logger`
/// so that CLI and WebSocket callers receive the same verbosity and information.
///
/// # Arguments
/// * `mesh` - The triangle mesh to process
/// * `params` - Slicing parameters controlling all aspects of the slicing process
/// * `logger` - Pipeline logger; use [`NullLogger`] when logging is not needed
///
/// # Returns
/// A `Vec<SliceLayer>` with all processing applied (walls, surfaces, etc.).
///
/// # Example
/// ```
/// use slicer_engine::logging::NullLogger;
/// use slicer_engine::mesh::types::Mesh;
/// use slicer_engine::settings::params::SlicingParams;
/// use slicer_engine::core::process_mesh;
///
/// let mesh = Mesh::new(); // Load your mesh
/// let params = SlicingParams::default();
/// let layers = process_mesh(&mesh, &params, &NullLogger);
/// ```
pub fn process_mesh(
    mesh: &Mesh,
    params: &SlicingParams,
    logger: &dyn ProcessLogger,
) -> Vec<SliceLayer> {
    process_mesh_with_paint(mesh, params, logger, &FacetPaint::new())
}

/// Slice a mesh that carries per-facet support paint.
///
/// Identical to [`process_mesh`] in every respect but one: the painted facets
/// are projected onto the layer stack and handed to support generation, so the
/// user's enforcers and blockers override the automatic overhang rule. An empty
/// annotation reproduces [`process_mesh`] exactly — the projection is skipped
/// entirely rather than producing empty masks — so an unpainted slice cannot
/// drift.
pub fn process_mesh_with_paint(
    mesh: &Mesh,
    params: &SlicingParams,
    logger: &dyn ProcessLogger,
    paint: &FacetPaint,
) -> Vec<SliceLayer> {
    // Spiral (vase) mode forces a consistent single-wall configuration
    // (no infill/top surfaces/retraction) for the whole pipeline. Applied here
    // so every entry point (CLI, WebSocket, WASM) observes the same rules.
    let normalized = params.spiral_vase_normalized();
    let params = normalized.as_ref();

    logger.log_info(&format!("processing mesh: {} triangles", mesh.faces.len()));

    let t_slicing = PhaseTimer::start(phases::SLICING, logger);
    logger.log_debug("slicing mesh…");
    let first_h = resolved_first_layer_height(params);
    let mut layers = slice_mesh_with_first_layer(mesh, params.layer_height, first_h);
    logger.log_info(&format!("sliced into {} layers", layers.len()));
    t_slicing.finish();

    if logger.is_cancelled() {
        logger.log_info("slice cancelled after slicing phase");
        return layers;
    }

    // Dimensional compensation, on the raw contours and nowhere else: every
    // later stage measures from the contour the wall generator consumed, so
    // correcting it here leaves all of those relations intact.
    apply_compensation(&mut layers, params, logger);
    apply_elephant_foot(&mut layers, params, logger);

    // Generate walls FIRST from the raw mesh contours
    logger.log_debug(&format!(
        "generating walls (generator: {}, wall_count: {}, nozzle: {}mm)",
        params.wall_generator.name(),
        params.wall_count,
        params.nozzle_diameter_mm
    ));
    let t_walls = PhaseTimer::start(phases::WALL_GENERATION, logger);
    let wall_timings = crate::walls::generate_walls(&mut layers, params);
    t_walls.finish();
    logger.log_debug(&format!(
        "wall sub-timings (CPU total across threads): collapse_depth {} ms, bead_shrinks {} ms",
        wall_timings.collapse_depth_ms, wall_timings.bead_shrink_ms,
    ));
    logger.log_debug("wall generation complete");

    if logger.is_cancelled() {
        logger.log_info("slice cancelled after wall generation phase");
        return layers;
    }

    // Pre-compute infill interior regions while all walls are still
    // present.  These are passed to add_infill_to_layers so that the
    // subsequent apply_single_wall_restrictions step (which strips inner walls
    // from certain layers) cannot accidentally expand the infill boundary into
    // the space the stripped walls occupied.
    //
    // Without this, a layer that has a small top-surface feature (e.g. the top
    // of an embossed letter on a calibration cube) loses inner walls for ALL
    // of its islands, causing calculate_interior_region to see walls_per_island
    // = 1 everywhere and place sparse infill far into the wall zone.
    let pre_strip_infill_regions: Option<Vec<Paths>> = if params.infill_density > 0.0
        && (params.only_one_wall_first_layer || params.only_one_wall_top)
    {
        let t_snapshot = PhaseTimer::start(phases::INFILL_REGION_SNAPSHOT, logger);
        #[cfg(not(target_arch = "wasm32"))]
        let result = {
            use rayon::prelude::*;
            Some(
                layers
                    .par_iter()
                    .map(|layer| {
                        calculate_interior_region(
                            layer,
                            0.0,
                            params.nozzle_diameter_mm,
                            params.wall_count,
                        )
                    })
                    .collect(),
            )
        };
        #[cfg(target_arch = "wasm32")]
        let result = Some(
            layers
                .iter()
                .map(|layer| {
                    calculate_interior_region(
                        layer,
                        0.0,
                        params.nozzle_diameter_mm,
                        params.wall_count,
                    )
                })
                .collect(),
        );
        t_snapshot.finish();
        result
    } else {
        None
    };

    // Apply single-wall restrictions to first/last-of-run layers if configured.
    //
    // Per-island detection runs inside apply_single_wall_restrictions so that
    // only the islands that actually end their top-surface run get stripped.
    // Previously the whole layer was stripped whenever any one island qualified,
    // which caused the infill boundary to over-expand into the wall zone for
    // the unaffected (continuing) islands on the same layer.
    if params.only_one_wall_first_layer || params.only_one_wall_top {
        logger.log_debug("applying single-wall restrictions (per-island)");
        let t_wall_restrictions = PhaseTimer::start(phases::WALL_RESTRICTIONS, logger);
        apply_single_wall_restrictions(&mut layers, params);
        t_wall_restrictions.finish();
    }

    // Calculate interior regions (inside walls) for each layer where surfaces will go
    let interior_regions: Vec<Paths> = if params.top_layers > 0 || params.bottom_layers > 0 {
        logger.log_debug("calculating interior regions for surfaces");
        let t_interior = PhaseTimer::start(phases::INTERIOR_REGIONS, logger);
        #[cfg(not(target_arch = "wasm32"))]
        let result = {
            use rayon::prelude::*;
            layers
                .par_iter()
                .map(|layer| {
                    calculate_interior_region(
                        layer,
                        params.infill_overlap_percent,
                        params.nozzle_diameter_mm,
                        params.wall_count,
                    )
                })
                .collect()
        };
        #[cfg(target_arch = "wasm32")]
        let result = layers
            .iter()
            .map(|layer| {
                calculate_interior_region(
                    layer,
                    params.infill_overlap_percent,
                    params.nozzle_diameter_mm,
                    params.wall_count,
                )
            })
            .collect();
        t_interior.finish();
        result
    } else {
        vec![]
    };

    // Snapshot pristine OuterWall perimeters for dynamic overhang-degree grading
    // *before* surface generation splits any walls via bridge clipping.  Layer
    // `i`'s support outline is `snapshot[i-1]`; the snapshot is consumed by
    // `classify_overhang_perimeters` to grade each wall segment's overhang
    // degree.  Only taken when the feature is enabled.
    let overhang_support: Option<Vec<Paths>> = snapshot_overhang_support(&layers, params);

    // Supports need the same un-split outlines, and for the same reason: the
    // classification pass below retags an overhanging wall as
    // `OverhangPerimeter` and splits its loop, so a steep slope keeps no
    // `OuterWall` path for `generate_supports` to measure. Taken here — before
    // that happens — rather than at the support step further down.
    let support_footprints: Option<Vec<Paths>> = if params.support_enabled {
        Some(snapshot_perimeters(&layers))
    } else {
        None
    };

    // Now generate top/bottom surfaces INSIDE the walls
    if params.top_layers > 0 || params.bottom_layers > 0 {
        let t_surfaces = PhaseTimer::start(phases::SURFACES, logger);
        logger.log_debug(&format!(
            "generating surfaces (top: {}, bottom: {}, angle: {}°)",
            params.top_layers, params.bottom_layers, params.surface_infill_angle
        ));
        let surface_timings = generate_top_bottom_surfaces_with_interior(
            &mut layers,
            &SurfaceConfig {
                top_layers: params.top_layers,
                bottom_layers: params.bottom_layers,
                layer_height: params.layer_height,
                infill_angle: params.surface_infill_angle,
                nozzle_diameter_mm: params.nozzle_diameter_mm,
                solid_surface_line_width_mm: crate::core::solid_surface_nominal_width_mm(params),
                min_infill_extrusion_mm: params.min_infill_extrusion_mm,
                bridge_flow_ratio: params.bridge_flow_ratio,
                bridge_min_area_mm2: params.bridge_min_area_mm2,
                bridge_noise_filter_mm: params.bridge_noise_filter_mm,
                bridge_anchor_mm: params.bridge_anchor_mm,
                infill_overlap_percent: params.infill_overlap_percent,
                ensure_vertical_shell_thickness: params.ensure_vertical_shell_thickness,
                bridge_angle_deg: params.bridge_angle,
                top_pattern: params.top_surface_pattern,
                bottom_pattern: params.bottom_surface_pattern,
                internal_solid_pattern: params.internal_solid_infill_pattern,
                ironing_enabled: params.ironing_enabled,
                ironing_type: params.ironing_type,
                ironing_spacing: params.ironing_spacing,
                ironing_angle: params.ironing_angle,
            },
            Some(&interior_regions),
        );
        logger.log_debug("surface generation complete");
        t_surfaces.finish();
        logger.log_debug(&format!(
            "surface sub-timings: perimeter_snapshot {} ms, detection {} ms, infill_gen {} ms",
            surface_timings.perimeter_snapshot_ms,
            surface_timings.detection_ms,
            surface_timings.infill_gen_ms,
        ));

        // Classify wall paths that lie mostly over unsupported air as
        // OverhangPerimeter so the G-code generator prints them with bridge
        // speed/flow/cooling.  Requires unsupported_regions populated by
        // the surface-generation pass above.
        //
        // Skipped in spiral (vase) mode: overhang classification splits closed
        // wall loops into open arcs, which would break the single continuous
        // contour the spiral emitter needs.
        if !params.spiral_vase {
            logger.log_debug("classifying overhang perimeters");
            let t_overhang = PhaseTimer::start("Overhang Perimeter Classification", logger);
            let grading = overhang_support.as_deref().map(|support| OverhangGrading {
                support,
                band_class: overhang_band_class(params),
            });
            classify_overhang_perimeters(&mut layers, params.nozzle_diameter_mm, grading);
            t_overhang.finish();
        }

        // Drop gap-fill beads that land inside a solid surface: the solid infill
        // covers them, so they'd otherwise sit as scattered variable-width
        // islands on the uniform surface.  Genuine gap fill in sparse zones and
        // thin ribs (outside solid_regions) is preserved.
        logger.log_debug("pruning redundant gap fill inside solid surfaces");
        prune_redundant_gap_fill(&mut layers, params.nozzle_diameter_mm);
    }

    // Add infill
    if params.infill_density > 0.0 {
        let infill_pattern = params.infill_pattern;

        logger.log_debug(&format!(
            "generating {} infill at {:.0}% density, {}° base angle…",
            infill_pattern.name(),
            params.infill_density * 100.0,
            params.infill_base_angle
        ));

        let t_infill = PhaseTimer::start(phases::INFILL, logger);
        add_infill_to_layers(
            &mut layers,
            &sparse_infill_config(params),
            pre_strip_infill_regions.as_deref(),
        );
        t_infill.finish();
        logger.log_debug("infill generation complete");
    }

    // Generate support structures for overhangs steeper than the threshold
    // angle.  Runs before path ordering so support strands are grouped and
    // ordered with the rest of the layer.
    if params.support_enabled {
        logger.log_debug(&format!(
            "generating supports (type: {:?}, threshold: {}°, density: {:.0}%)",
            params.support_type,
            params.support_threshold_angle,
            params.support_density * 100.0
        ));
        let t_support = PhaseTimer::start("Support Generation", logger);

        // Project painted facets onto the layer stack to get enforcer and
        // blocker masks. An empty annotation is fast-pathed, so unpainted
        // slices don't pay for this.
        let paint_masks = crate::core::project_support_paint(
            mesh,
            paint,
            &layers,
            params.layer_height,
            resolved_first_layer_height(params),
            params.nozzle_diameter_mm,
        );
        crate::core::generate_supports_with_paint(
            &mut layers,
            params,
            support_footprints.as_deref(),
            &paint_masks,
        );
        t_support.finish();
        logger.log_debug("support generation complete");
    }

    // Optimize path order: island by island, greedy TSP within each role run.
    let t_tsp = PhaseTimer::start("Path Ordering", logger);
    // The nozzle carries over from the layer below. Restarting every layer's
    // walk at the origin made each layer open with a hop toward whichever path
    // happened to sit nearest the bed's corner — a full crossing of the part,
    // every layer, on a plate that is nowhere near the origin.
    let mut current_pos = (0.0, 0.0);
    for layer in layers.iter_mut() {
        let path_count = layer.paths.len();
        if path_count == 0 {
            continue;
        }

        let paths_vec: Vec<_> = layer.paths.iter().cloned().collect();
        let mut ordered_paths = clipper2::Paths::default();
        let mut ordered_roles = Vec::with_capacity(path_count);
        let mut ordered_widths = Vec::with_capacity(path_count);
        let mut ordered_vertex_widths: Vec<Option<Vec<f64>>> = Vec::with_capacity(path_count);
        let mut ordered_is_open = Vec::with_capacity(path_count);
        let mut ordered_overhang = Vec::with_capacity(path_count);
        let mut ordered_heights = Vec::with_capacity(path_count);
        let mut ordered_objects = Vec::with_capacity(path_count);

        // Split the layer into islands and print one island out before moving
        // to the next. The fill generators emit per layer, not per island, so
        // without this every island's walls were laid first and the nozzle then
        // came back across the plate to fill each of them in turn.
        let islands = Islands::of(layer);

        for island in islands.visiting_order(current_pos) {
            // Contiguous runs of one role, in the order the generators emitted
            // them — that order is the wall sequence and the monotonic sweep.
            // Only the phase sort above moves anything.
            let mut groups: Vec<(ExtrusionRole, Vec<usize>)> = Vec::new();
            for &i in island {
                let role = layer.role_for_path(i);
                match groups.last_mut() {
                    Some((last_role, run)) if *last_role == role => run.push(i),
                    _ => groups.push((role, vec![i])),
                }
            }

            for (role, mut remaining) in groups {
                // A monotonic solid-surface group must keep the order and direction
                // the fill generator emitted. Re-optimising it with the greedy TSP —
                // which is free to reverse an open path — would scramble exactly the
                // uniform sweep the pattern exists to produce, and the surface would
                // look no different from a plain serpentine.
                if monotonic_surface_role(role, params) {
                    for path_idx in remaining.drain(..) {
                        let path = &paths_vec[path_idx];
                        if let Some(last) = path.iter().last() {
                            current_pos = (last.x(), last.y());
                        }
                        ordered_paths.push(path.clone());
                        ordered_roles.push(role);
                        ordered_widths.push(layer.width_for_path(path_idx));
                        ordered_vertex_widths.push(layer.vertex_widths_for_path(path_idx));
                        ordered_is_open.push(layer.is_path_open(path_idx));
                        ordered_overhang.push(layer.overhang_for_path(path_idx));
                        ordered_heights.push(layer.height_for_path(path_idx));
                        ordered_objects.push(layer.object_for_path(path_idx));
                    }
                    continue;
                }

                // Wall/skirt/support roles are nominally "closed" for TSP purposes,
                // but individual paths may be open arcs (split sub-segments from
                // classify_overhang_perimeters, or a support island's fill strands).
                // Open arcs are treated like open polylines: both endpoints are
                // candidate starts and current_pos is updated to the path *end*
                // (not the start) after emission.
                //
                // This must be the same predicate the G-code generator applies, or
                // the orderer's idea of where the nozzle ends up is wrong — hence
                // `ExtrusionRole::forms_closed_loops` rather than a second list.
                let role_is_closed = role.forms_closed_loops();

                while !remaining.is_empty() {
                    let mut best_i = 0;
                    let mut min_dist_sq = f64::MAX;
                    let mut best_reverse = false;
                    // For closed loops: the vertex index in the path to start at
                    // (= seam position).  Loops are cyclic, so any vertex can be
                    // the start; picking the one closest to `current_pos` minimises
                    // travel and consolidates seams ("nearest" seam policy used by
                    // PrusaSlicer/Orca).  For open paths this stays 0.
                    let mut best_seam_vertex: usize = 0;

                    for (i, &path_idx) in remaining.iter().enumerate() {
                        let path = &paths_vec[path_idx];
                        if path.is_empty() {
                            continue;
                        }

                        // A path that is nominally "closed" by role but flagged as
                        // an open arc is treated as open for path-ordering purposes.
                        let is_closed = role_is_closed && !layer.is_path_open(path_idx);

                        if is_closed {
                            // Choose this loop's seam vertex per the configured
                            // policy, then score the loop by the distance from
                            // current_pos to that seam vertex (= actual travel
                            // we'd incur if we picked this loop next).
                            let seam_v =
                                choose_seam_vertex(path, params.seam_position, current_pos);
                            let p = path.iter().nth(seam_v).unwrap();
                            let dx = p.x() - current_pos.0;
                            let dy = p.y() - current_pos.1;
                            let d = dx * dx + dy * dy;
                            if d < min_dist_sq {
                                min_dist_sq = d;
                                best_i = i;
                                best_reverse = false;
                                best_seam_vertex = seam_v;
                            }
                        } else {
                            // Open path: only the two endpoints are candidate starts.
                            let p_start = path.iter().next().unwrap();
                            let dx1 = p_start.x() - current_pos.0;
                            let dy1 = p_start.y() - current_pos.1;
                            let dist1 = dx1 * dx1 + dy1 * dy1;

                            if dist1 < min_dist_sq {
                                min_dist_sq = dist1;
                                best_i = i;
                                best_reverse = false;
                                best_seam_vertex = 0;
                            }

                            let p_end = path.iter().last().unwrap();
                            let dx2 = p_end.x() - current_pos.0;
                            let dy2 = p_end.y() - current_pos.1;
                            let dist2 = dx2 * dx2 + dy2 * dy2;
                            if dist2 < min_dist_sq {
                                min_dist_sq = dist2;
                                best_i = i;
                                best_reverse = true;
                                best_seam_vertex = 0;
                            }
                        }
                    }

                    let best_path_idx = remaining.remove(best_i);
                    let path = &paths_vec[best_path_idx];

                    // Per-path closed/open determination for current_pos update.
                    let best_is_closed = role_is_closed && !layer.is_path_open(best_path_idx);

                    let mut final_path = clipper2::Path::default();
                    if best_is_closed && best_seam_vertex != 0 {
                        // Rotate the closed loop so it starts at `best_seam_vertex`.
                        // The path's first vertex is preserved as the closing
                        // vertex by the G-code generator (which appends a move
                        // back to vertex[0] for closed loops).  After rotation,
                        // the loop reads: [v_seam, v_seam+1, …, v_n-1, v_0, v_1,
                        // …, v_seam-1].  Note: we do NOT duplicate v_seam at the
                        // end — the generator's "close contour" move handles the
                        // wrap-around.
                        let pts: Vec<_> = path.iter().copied().collect();
                        let n = pts.len();
                        for k in 0..n {
                            final_path.push(pts[(best_seam_vertex + k) % n]);
                        }
                    } else if best_reverse {
                        for p in path.iter().rev() {
                            final_path.push(*p);
                        }
                    } else {
                        for p in path.iter() {
                            final_path.push(*p);
                        }
                    }

                    if !final_path.is_empty() {
                        if best_is_closed {
                            // Closed loop: nozzle ends at the start vertex (the
                            // closing move in G-code returns to vertex[0]).
                            let p = final_path.iter().next().unwrap();
                            current_pos = (p.x(), p.y());
                        } else {
                            let p = final_path.iter().last().unwrap();
                            current_pos = (p.x(), p.y());
                        }
                    }

                    ordered_paths.push(final_path);
                    ordered_roles.push(layer.role_for_path(best_path_idx));
                    ordered_widths.push(layer.width_for_path(best_path_idx));
                    // Reorder any per-vertex widths with the same rotation/reversal
                    // applied to the path vertices above.
                    ordered_vertex_widths.push(layer.vertex_widths_for_path(best_path_idx).map(
                        |vw| {
                            let n = vw.len();
                            if best_is_closed && best_seam_vertex != 0 && n > 0 {
                                (0..n).map(|k| vw[(best_seam_vertex + k) % n]).collect()
                            } else if best_reverse {
                                let mut r = vw;
                                r.reverse();
                                r
                            } else {
                                vw
                            }
                        },
                    ));
                    ordered_is_open.push(layer.is_path_open(best_path_idx));
                    ordered_overhang.push(layer.overhang_for_path(best_path_idx));
                    ordered_heights.push(layer.height_for_path(best_path_idx));
                    ordered_objects.push(layer.object_for_path(best_path_idx));
                }
            }
        }

        layer.paths = ordered_paths;
        layer.path_roles = ordered_roles;
        layer.path_widths = ordered_widths;
        layer.path_vertex_widths = ordered_vertex_widths;
        layer.path_is_open = ordered_is_open;
        // Keep `path_overhang` populated only when it was graded; an all-`None`
        // (empty source) layer collapses back to empty.
        layer.path_overhang = if layer.path_overhang.is_empty() {
            Vec::new()
        } else {
            ordered_overhang
        };
        // Same treatment for the height overrides: empty unless infill combining
        // actually set any.
        layer.path_heights = if layer.path_heights.is_empty() {
            Vec::new()
        } else {
            ordered_heights
        };
        // And for the object tags, which only a plate sliced object by object
        // carries. Reordering the paths without them would hand every path the
        // previous occupant's identity and cancel the wrong part.
        layer.path_objects = if layer.path_objects.is_empty() {
            Vec::new()
        } else {
            ordered_objects
        };
    }
    t_tsp.finish();

    // Wall overlap flow compensation — scale extrusion down where wall beads
    // overlap (tight slots, hairpins, acute concave corners).  Runs last so the
    // per-vertex widths it writes align with the final ordered/seamed geometry.
    let t_flow = PhaseTimer::start("Flow Compensation", logger);
    crate::flow::compensate(&mut layers, params);
    t_flow.finish();

    // Fuzzy skin — cosmetic outer-wall texture.  Runs after ordering and flow
    // compensation (so it perturbs the final geometry and carries along any
    // per-vertex widths already written) and before adhesion, whose skirt/brim
    // trace the clean OuterWall centerlines and must not pick up the jitter.
    crate::walls::fuzzy_skin::apply(&mut layers, params);

    // Bed-adhesion helpers (skirt / brim / raft).  Runs after the object's own
    // toolpaths are fully ordered and flow-compensated so it never perturbs
    // them: skirt/brim loops are prepended to the first layer(s); raft prepends
    // sacrificial layers and shifts the object up.
    if params.adhesion_type != crate::settings::params::AdhesionType::None {
        let t_adhesion = PhaseTimer::start("Bed Adhesion", logger);
        logger.log_debug(&format!(
            "generating bed adhesion ({:?})",
            params.adhesion_type
        ));
        crate::adhesion::apply_adhesion(&mut layers, params);
        t_adhesion.finish();
    }

    // The flow half of a thicker first layer, applied last so the skirt or brim
    // sharing that layer's Z is charged at the same height the object is.
    mark_first_layer_height(&mut layers, first_h, params.layer_height);

    layers
}

/// Debug variant of [`process_mesh`].
///
/// Runs the full slicing pipeline and additionally collects geometry snapshots
/// at every major stage into `debug`.  The returned `Vec<SliceLayer>` is
/// identical to what `process_mesh` would produce.
///
/// Because walls are generated sequentially in debug mode (to allow in-order
/// snapshot capture), this function is **significantly slower** than
/// `process_mesh` on large models.  Use it only for debugging.
///
/// # Snapshots captured
///
/// | Stage | When |
/// |---|---|
/// | `RawContours` | After `slice_mesh`, before wall generation |
/// | `WallNormalisedInput` | Per layer: EvenOdd-union result fed to the generator |
/// | `WallOffsetStep { bead_k }` | Per layer / per bead: each `shrink` intermediate |
/// | `WallBeads` | Per layer: final bead centerlines |
/// | `InteriorRegion` | Per layer: inside-wall region for infill/surfaces |
/// | `SolidSurface` | Per layer: `layer.solid_regions` after surface generation |
/// | `Infill` | Per layer: infill + surface fill paths |
#[cfg(not(target_arch = "wasm32"))]
pub fn process_mesh_debug(
    mesh: &Mesh,
    params: &SlicingParams,
    logger: &dyn ProcessLogger,
    debug: &mut crate::debug::DebugGeometry,
) -> Vec<SliceLayer> {
    use crate::debug::DebugStage;

    // Match process_mesh: spiral (vase) mode forces a single-wall config.
    let normalized = params.spiral_vase_normalized();
    let params = normalized.as_ref();

    logger.log_info(&format!(
        "debug pipeline: processing mesh with {} triangles",
        mesh.faces.len()
    ));

    let first_h = resolved_first_layer_height(params);
    let mut layers = slice_mesh_with_first_layer(mesh, params.layer_height, first_h);
    logger.log_info(&format!("sliced into {} layers", layers.len()));

    // Both compensation passes run before the snapshot, so `RawContours` shows
    // exactly the shapes the wall generator is about to receive.
    apply_compensation(&mut layers, params, logger);
    apply_elephant_foot(&mut layers, params, logger);

    // Snapshot raw contours.
    for (i, layer) in layers.iter().enumerate() {
        debug.push(DebugStage::RawContours, i, layer.z, layer.paths.clone());
    }

    if logger.is_cancelled() {
        return layers;
    }

    // Walls — sequential, with debug snapshots captured per layer.
    logger.log_debug("generating walls (debug mode, sequential)");
    crate::walls::generate_walls_debug(&mut layers, params, debug);
    logger.log_debug("wall generation complete");

    if logger.is_cancelled() {
        return layers;
    }

    // Pre-strip infill region snapshot (same logic as process_mesh).
    let pre_strip_infill_regions: Option<Vec<Paths>> = if params.infill_density > 0.0
        && (params.only_one_wall_first_layer || params.only_one_wall_top)
    {
        Some(
            layers
                .iter()
                .map(|layer| {
                    calculate_interior_region(
                        layer,
                        0.0,
                        params.nozzle_diameter_mm,
                        params.wall_count,
                    )
                })
                .collect(),
        )
    } else {
        None
    };

    if params.only_one_wall_first_layer || params.only_one_wall_top {
        apply_single_wall_restrictions(&mut layers, params);
    }

    // Interior regions — snapshot each one.
    let interior_regions: Vec<Paths> = if params.top_layers > 0 || params.bottom_layers > 0 {
        let regions: Vec<Paths> = layers
            .iter()
            .map(|layer| {
                calculate_interior_region(
                    layer,
                    params.infill_overlap_percent,
                    params.nozzle_diameter_mm,
                    params.wall_count,
                )
            })
            .collect();
        for (i, (layer, region)) in layers.iter().zip(regions.iter()).enumerate() {
            debug.push(DebugStage::InteriorRegion, i, layer.z, region.clone());
        }
        regions
    } else {
        vec![]
    };

    // Support footprints must be snapshotted before `classify_overhang_perimeters`
    // splits and retags the overhanging walls — the same ordering constraint
    // `process_mesh` observes, and for the same reason.
    let support_footprints: Option<Vec<Paths>> = if params.support_enabled {
        Some(snapshot_perimeters(&layers))
    } else {
        None
    };

    // Surfaces.
    if params.top_layers > 0 || params.bottom_layers > 0 {
        let overhang_support = snapshot_overhang_support(&layers, params);
        generate_top_bottom_surfaces_with_interior(
            &mut layers,
            &SurfaceConfig {
                top_layers: params.top_layers,
                bottom_layers: params.bottom_layers,
                layer_height: params.layer_height,
                infill_angle: params.surface_infill_angle,
                nozzle_diameter_mm: params.nozzle_diameter_mm,
                solid_surface_line_width_mm: crate::core::solid_surface_nominal_width_mm(params),
                min_infill_extrusion_mm: params.min_infill_extrusion_mm,
                bridge_flow_ratio: params.bridge_flow_ratio,
                bridge_min_area_mm2: params.bridge_min_area_mm2,
                bridge_noise_filter_mm: params.bridge_noise_filter_mm,
                bridge_anchor_mm: params.bridge_anchor_mm,
                infill_overlap_percent: params.infill_overlap_percent,
                ensure_vertical_shell_thickness: params.ensure_vertical_shell_thickness,
                bridge_angle_deg: params.bridge_angle,
                top_pattern: params.top_surface_pattern,
                bottom_pattern: params.bottom_surface_pattern,
                internal_solid_pattern: params.internal_solid_infill_pattern,
                ironing_enabled: params.ironing_enabled,
                ironing_type: params.ironing_type,
                ironing_spacing: params.ironing_spacing,
                ironing_angle: params.ironing_angle,
            },
            Some(&interior_regions),
        );
        if !params.spiral_vase {
            classify_overhang_perimeters(
                &mut layers,
                params.nozzle_diameter_mm,
                overhang_support.as_deref().map(|support| OverhangGrading {
                    support,
                    band_class: overhang_band_class(params),
                }),
            );
        }

        // Snapshot solid surface regions.
        for (i, layer) in layers.iter().enumerate() {
            debug.push(
                DebugStage::SolidSurface,
                i,
                layer.z,
                layer.solid_regions.clone(),
            );
        }
    }

    // Infill.
    if params.infill_density > 0.0 {
        add_infill_to_layers(
            &mut layers,
            &sparse_infill_config(params),
            pre_strip_infill_regions.as_deref(),
        );

        // Snapshot infill paths (Infill, TopSurface, BottomSurface roles).
        for (layer_index, layer) in layers.iter().enumerate() {
            let infill_paths: Vec<clipper2::Path> = layer
                .paths
                .iter()
                .enumerate()
                .filter(|(i, _)| {
                    matches!(
                        layer.role_for_path(*i),
                        ExtrusionRole::Infill
                            | ExtrusionRole::TopSurface
                            | ExtrusionRole::BottomSurface
                            | ExtrusionRole::Bridge
                    )
                })
                .map(|(_, p)| p.clone())
                .collect();
            if !infill_paths.is_empty() {
                debug.push(
                    DebugStage::Infill,
                    layer_index,
                    layer.z,
                    clipper2::Paths::new(infill_paths),
                );
            }
        }
    }

    // Supports. Mirrors `process_mesh`: after infill, reading the pristine
    // perimeter snapshot taken above rather than the now-split walls.
    if params.support_enabled {
        crate::core::generate_supports(&mut layers, params, support_footprints.as_deref());

        for (layer_index, layer) in layers.iter().enumerate() {
            let support_paths: Vec<clipper2::Path> = layer
                .paths
                .iter()
                .enumerate()
                .filter(|(i, _)| layer.role_for_path(*i) == ExtrusionRole::Support)
                .map(|(_, p)| p.clone())
                .collect();
            if !support_paths.is_empty() {
                debug.push(
                    DebugStage::Support,
                    layer_index,
                    layer.z,
                    clipper2::Paths::new(support_paths),
                );
            }
        }
    }

    logger.log_debug(&format!(
        "debug geometry: {} records captured across {} layers",
        debug.len(),
        layers.len()
    ));

    crate::flow::compensate(&mut layers, params);

    crate::walls::fuzzy_skin::apply(&mut layers, params);

    mark_first_layer_height(&mut layers, first_h, params.layer_height);

    layers
}

/// Print phase of a role *within* its island.
///
/// Everything that is covered by something else prints first. The visible top
/// surface is laid after the sparse fill beside it, so the nozzle never crosses
/// a finished top to reach anything; ironing then sweeps the finished top.
/// Order inside a phase is the order the generators emitted — that order is the
/// wall sequence and the monotonic sweep, and neither may be disturbed.
fn print_phase(role: ExtrusionRole) -> u8 {
    match role {
        ExtrusionRole::Infill => 1,
        ExtrusionRole::TopSurface => 2,
        ExtrusionRole::Ironing => 3,
        _ => 0,
    }
}

/// One layer's path indices, split into the islands the nozzle should finish
/// one at a time.
///
/// An island is one outermost closed [`ExtrusionRole::OuterWall`] loop together
/// with everything printed inside it — its inner walls, its hole contours, its
/// fills. Surface and infill generation runs per *layer*, not per island, so
/// the paths arrive as "every island's walls, then every island's fill": the
/// nozzle used to lay all the walls on the plate, then cross back over all of
/// them again for the fill. Grouping first by island and only then by role is
/// what keeps a travel inside the island that is being printed.
struct Islands {
    /// Path indices per island, in print order.
    members: Vec<Vec<usize>>,
    /// A few approach points per island, for picking the nearest island next.
    anchors: Vec<Vec<(f64, f64)>>,
}

impl Islands {
    fn of(layer: &SliceLayer) -> Self {
        let bounds = |pts: &[(f64, f64)]| {
            pts.iter().fold(
                (f64::MAX, f64::MAX, f64::MIN, f64::MIN),
                |(x0, y0, x1, y1), &(x, y)| (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            )
        };

        // Candidate island outlines: every closed outer-wall loop. A hole's
        // boundary is one of these too, and is filtered out below.
        let mut loops: Vec<(usize, Vec<(f64, f64)>)> = Vec::new();
        for (i, path) in layer.paths.iter().enumerate() {
            if layer.role_for_path(i) == ExtrusionRole::OuterWall && !layer.is_path_open(i) {
                let pts: Vec<(f64, f64)> = path.iter().map(|p| (p.x(), p.y())).collect();
                if pts.len() >= 3 {
                    loops.push((i, pts));
                }
            }
        }

        // Outermost = whose first vertex lies inside no other loop. Separate
        // islands are disjoint, so only a hole is contained by anything.
        let outlines: Vec<Vec<(f64, f64)>> = loops
            .iter()
            .enumerate()
            .filter(|(k, (_, pts))| {
                !loops
                    .iter()
                    .enumerate()
                    .any(|(other, (_, o))| other != *k && point_inside(pts[0], o))
            })
            .map(|(_, (_, pts))| pts.clone())
            .collect();

        if outlines.len() <= 1 {
            // One island (or none to speak of): nothing to separate, but the
            // phase order still applies.
            let mut all: Vec<usize> = (0..layer.paths.len()).collect();
            all.sort_by_key(|&i| print_phase(layer.role_for_path(i)));
            let anchors = outlines.iter().map(|o| sample_anchors(o)).collect();
            return Self {
                members: vec![all],
                anchors,
            };
        }

        let boxes: Vec<(f64, f64, f64, f64)> = outlines.iter().map(|pts| bounds(pts)).collect();
        let mut members: Vec<Vec<usize>> = vec![Vec::new(); outlines.len()];
        for (i, path) in layer.paths.iter().enumerate() {
            let Some(first) = path.iter().next() else {
                continue;
            };
            let probe = (first.x(), first.y());
            members[Self::owner(probe, &outlines, &boxes)].push(i);
        }
        for island in members.iter_mut() {
            island.sort_by_key(|&i| print_phase(layer.role_for_path(i)));
        }

        let anchors = outlines.iter().map(|o| sample_anchors(o)).collect();
        Self { members, anchors }
    }

    /// The island a path belongs to: the outline that contains its first
    /// vertex, or — for a path that lies inside none of them, a support strand
    /// standing free of the part — the nearest one, so the nozzle still deals
    /// with one neighbourhood at a time.
    fn owner(
        probe: (f64, f64),
        outlines: &[Vec<(f64, f64)>],
        boxes: &[(f64, f64, f64, f64)],
    ) -> usize {
        for (i, outline) in outlines.iter().enumerate() {
            let (x0, y0, x1, y1) = boxes[i];
            if probe.0 < x0 || probe.0 > x1 || probe.1 < y0 || probe.1 > y1 {
                continue;
            }
            if point_inside_or_on(probe, outline) {
                return i;
            }
        }
        outlines
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                nearest_vertex_dist_sq(probe, a)
                    .partial_cmp(&nearest_vertex_dist_sq(probe, b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map_or(0, |(i, _)| i)
    }

    /// The islands in the order to print them: nearest first, then nearest to
    /// where that one was entered.
    ///
    /// Islands are scored against a handful of sampled outline vertices rather
    /// than all of them — picking the next island is quadratic in their count,
    /// and a lattice cross-section can carry hundreds per layer.
    fn visiting_order(&self, from: (f64, f64)) -> Vec<&[usize]> {
        if self.members.len() <= 1 {
            return self.members.iter().map(Vec::as_slice).collect();
        }
        let mut remaining: Vec<usize> = (0..self.members.len()).collect();
        let mut order = Vec::with_capacity(remaining.len());
        let mut pos = from;
        while !remaining.is_empty() {
            let mut best = (0usize, f64::MAX, pos);
            for (slot, &island) in remaining.iter().enumerate() {
                let Some(anchors) = self.anchors.get(island) else {
                    continue;
                };
                for &v in anchors {
                    let d = (v.0 - pos.0).powi(2) + (v.1 - pos.1).powi(2);
                    if d < best.1 {
                        best = (slot, d, v);
                    }
                }
            }
            let island = remaining.remove(best.0);
            pos = best.2;
            order.push(self.members[island].as_slice());
        }
        order
    }
}

/// At most this many outline vertices are kept as an island's approach points.
const ISLAND_ANCHORS: usize = 16;

/// Evenly sample up to [`ISLAND_ANCHORS`] vertices from a closed outline.
fn sample_anchors(outline: &[(f64, f64)]) -> Vec<(f64, f64)> {
    if outline.len() <= ISLAND_ANCHORS {
        return outline.to_vec();
    }
    let step = outline.len() as f64 / ISLAND_ANCHORS as f64;
    (0..ISLAND_ANCHORS)
        .map(|k| outline[((k as f64 * step) as usize).min(outline.len() - 1)])
        .collect()
}

fn nearest_vertex_dist_sq(probe: (f64, f64), pts: &[(f64, f64)]) -> f64 {
    pts.iter()
        .map(|v| (v.0 - probe.0).powi(2) + (v.1 - probe.1).powi(2))
        .fold(f64::MAX, f64::min)
}

fn point_inside(probe: (f64, f64), poly: &[(f64, f64)]) -> bool {
    matches!(ray_cast(probe, poly), Containment::Inside)
}

fn point_inside_or_on(probe: (f64, f64), poly: &[(f64, f64)]) -> bool {
    !matches!(ray_cast(probe, poly), Containment::Outside)
}

enum Containment {
    Inside,
    Outside,
    On,
}

/// Even-odd ray cast, winding-independent — the wall loops it is asked about
/// carry whatever orientation the generator gave them.
fn ray_cast(probe: (f64, f64), poly: &[(f64, f64)]) -> Containment {
    const ON_EDGE_TOLERANCE: f64 = 1e-9;
    let (px, py) = probe;
    let n = poly.len();
    if n < 3 {
        return Containment::Outside;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        // Collinear with this edge and within its span: on the boundary.
        let cross = (xj - xi) * (py - yi) - (yj - yi) * (px - xi);
        if cross.abs() <= ON_EDGE_TOLERANCE
            && px >= xi.min(xj) - ON_EDGE_TOLERANCE
            && px <= xi.max(xj) + ON_EDGE_TOLERANCE
            && py >= yi.min(yj) - ON_EDGE_TOLERANCE
            && py <= yi.max(yj) + ON_EDGE_TOLERANCE
        {
            return Containment::On;
        }
        if ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    if inside {
        Containment::Inside
    } else {
        Containment::Outside
    }
}

/// Snapshot each layer's pristine OuterWall perimeter outline for dynamic
/// overhang-degree grading, or `None` when the feature is disabled.
///
/// Must be called **before** surface generation, which splits walls via bridge
/// clipping — the grader needs the un-split centrelines so a layer's support
/// outline (`snapshot[i-1]`) matches the geometry `unsupported_regions` was
/// built from.
fn snapshot_overhang_support(layers: &[SliceLayer], params: &SlicingParams) -> Option<Vec<Paths>> {
    if !params.enable_overhang_speed {
        return None;
    }
    Some(snapshot_perimeters(layers))
}

/// Snapshot every layer's `OuterWall` centreline outline as it stands now.
///
/// Shared by overhang grading and support generation, both of which must read
/// the outlines *before* bridge clipping and overhang classification split and
/// retag them.
fn snapshot_perimeters(layers: &[SliceLayer]) -> Vec<Paths> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        layers.par_iter().map(perimeter_paths_of).collect()
    }
    #[cfg(target_arch = "wasm32")]
    {
        layers.iter().map(perimeter_paths_of).collect()
    }
}

/// Fold each raw overhang band `0..=4` to the [`OverhangClass`] the classifier
/// should emit, so a wall is only split where the degree actually changes the
/// printed speed or fan.
///
/// A supported-side band (Deg1/Deg2) whose per-degree speed is unset **and**
/// which is below the overhang-fan threshold behaves exactly like a plain wall,
/// so it folds to [`OverhangClass::None`] — avoiding thousands of pointless
/// wall-fragment splits (each an extra retract/travel) when the user only tuned
/// the steep degrees.  The steep bands (Deg3/Deg4) always stay distinct: they
/// carry the `OverhangPerimeter` role and its bridge speed differs from a wall.
fn overhang_band_class(params: &SlicingParams) -> [OverhangClass; 5] {
    // A degree's *upper* unsupported fraction, matched against the fan threshold
    // the same way the generator does.
    let fan_targets = |upper_fraction: f64| {
        params.overhang_fan_speed > 0.0
            && upper_fraction > params.overhang_fan_threshold + f64::EPSILON
    };
    let deg1 = if params.overhang_1_4_speed > 0.0 || fan_targets(0.25) {
        OverhangClass::Deg1
    } else {
        OverhangClass::None
    };
    let deg2 = if params.overhang_2_4_speed > 0.0 || fan_targets(0.5) {
        OverhangClass::Deg2
    } else {
        OverhangClass::None
    };
    [
        OverhangClass::None,
        deg1,
        deg2,
        OverhangClass::Deg3,
        OverhangClass::Deg4,
    ]
}

/// Pick the start vertex of a closed loop according to the configured
/// [`SeamPosition`] policy.  Returns an index into `path.iter()`.
///
/// All policies fall back to `0` for paths with fewer than 3 vertices (where
/// the seam choice is degenerate).
fn choose_seam_vertex(
    path: &clipper2::Path,
    policy: SeamPosition,
    current_pos: (f64, f64),
) -> usize {
    let n = path.len();
    if n < 3 {
        return 0;
    }

    match policy {
        SeamPosition::Nearest => {
            let mut best_v = 0;
            let mut best_d = f64::MAX;
            for (vi, p) in path.iter().enumerate() {
                let dx = p.x() - current_pos.0;
                let dy = p.y() - current_pos.1;
                let d = dx * dx + dy * dy;
                if d < best_d {
                    best_d = d;
                    best_v = vi;
                }
            }
            best_v
        }
        SeamPosition::Rear => {
            // Vertex with the largest Y coordinate.  Ties broken by smallest X
            // (left-back) so the choice is deterministic across runs.
            let mut best_v = 0;
            let mut best_y = f64::MIN;
            let mut best_x = f64::MAX;
            for (vi, p) in path.iter().enumerate() {
                let y = p.y();
                if y > best_y || (y == best_y && p.x() < best_x) {
                    best_y = y;
                    best_x = p.x();
                    best_v = vi;
                }
            }
            best_v
        }
        SeamPosition::Aligned => {
            // Vertex with the largest projection onto a fixed preferred
            // direction.  We use +Y (rear-aligned) by default — same as Rear
            // for a single loop, but the per-loop projection is consistent
            // across loops at different positions, so seams across multiple
            // islands form a parallel set of vertical lines instead of
            // tracking each island's bounding box independently.
            //
            // Future: expose `seam_aligned_direction_deg` to let users align
            // to e.g. -X for a side-facing seam.
            const DIR_X: f64 = 0.0;
            const DIR_Y: f64 = 1.0;
            let mut best_v = 0;
            let mut best_proj = f64::MIN;
            for (vi, p) in path.iter().enumerate() {
                let proj = p.x() * DIR_X + p.y() * DIR_Y;
                if proj > best_proj {
                    best_proj = proj;
                    best_v = vi;
                }
            }
            best_v
        }
        SeamPosition::SharpestCorner => {
            // Vertex with the largest *exterior* turn angle, biased toward
            // convex corners (positive cross product on a CCW loop).  The
            // signed turn angle θ_i ∈ (-π, π] at vertex i is the angle from
            // edge (i-1 → i) to edge (i → i+1).  Convex corners on a CCW
            // loop have θ > 0; concave corners have θ < 0.
            //
            // We use |θ| − k·max(0, −θ) (with k = 0.5) to score: sharp
            // convex corners win, sharp concave corners come second, smooth
            // arcs lose.  Falls back to Nearest for entirely-smooth loops
            // (max score below ~10°) so seams don't jump randomly.
            let pts: Vec<_> = path.iter().copied().collect();
            let mut best_v = 0_usize;
            let mut best_score = f64::MIN;
            const SMOOTH_THRESHOLD_RAD: f64 = 0.175; // ≈ 10°
            for i in 0..n {
                let prev = pts[(i + n - 1) % n];
                let here = pts[i];
                let next = pts[(i + 1) % n];
                let ax = here.x() - prev.x();
                let ay = here.y() - prev.y();
                let bx = next.x() - here.x();
                let by = next.y() - here.y();
                let cross = ax * by - ay * bx;
                let dot = ax * bx + ay * by;
                let theta = cross.atan2(dot); // signed turn angle
                let convex_bias = if theta < 0.0 { 0.5 * (-theta) } else { 0.0 };
                let score = theta.abs() - convex_bias;
                if score > best_score {
                    best_score = score;
                    best_v = i;
                }
            }
            if best_score < SMOOTH_THRESHOLD_RAD {
                // No meaningful corner — fall back to Nearest.
                return choose_seam_vertex(path, SeamPosition::Nearest, current_pos);
            }
            best_v
        }
        SeamPosition::Random => {
            // Deterministic per-loop pseudo-random: hash the loop's first
            // vertex coordinates so the same loop on the same layer always
            // picks the same vertex (consistent with multi-pass slicing and
            // reproducible builds).
            let p0 = path.iter().next().unwrap();
            let bits = (p0.x().to_bits()) ^ p0.y().to_bits().rotate_left(17);
            // splitmix64 finaliser — cheap, well-mixed.
            let mut z = bits.wrapping_add(0x9E37_79B9_7F4A_7C15);
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            (z as usize) % n
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clipper2::Path;

    fn square(x: f64, y: f64, side: f64) -> Path {
        vec![(x, y), (x + side, y), (x + side, y + side), (x, y + side)].into()
    }

    fn line(x0: f64, y0: f64, x1: f64, y1: f64) -> Path {
        vec![(x0, y0), (x1, y1)].into()
    }

    /// Add one path with its role, keeping the parallel vectors aligned.
    fn push(layer: &mut SliceLayer, path: Path, role: ExtrusionRole) {
        layer.paths.push(path);
        layer.path_roles.push(role);
        layer.path_is_open.push(role == ExtrusionRole::Infill);
    }

    /// Two islands, emitted the way the pipeline emits them: every island's
    /// walls first, then every island's fill.
    fn two_islands() -> SliceLayer {
        let mut layer = SliceLayer::new(0.2);
        push(&mut layer, square(0.0, 0.0, 10.0), ExtrusionRole::OuterWall);
        push(
            &mut layer,
            square(30.0, 0.0, 10.0),
            ExtrusionRole::OuterWall,
        );
        push(&mut layer, line(2.0, 5.0, 8.0, 5.0), ExtrusionRole::Infill);
        push(
            &mut layer,
            line(32.0, 5.0, 38.0, 5.0),
            ExtrusionRole::Infill,
        );
        layer
    }

    #[test]
    fn an_islands_fill_stays_with_its_own_walls() {
        let islands = Islands::of(&two_islands());
        assert_eq!(islands.members.len(), 2, "two separate islands");
        assert!(
            islands.members.contains(&vec![0, 2]) && islands.members.contains(&vec![1, 3]),
            "each island keeps its own wall and fill: {:?}",
            islands.members
        );
    }

    #[test]
    fn the_nearest_island_is_printed_first() {
        let islands = Islands::of(&two_islands());
        let near_right = islands.visiting_order((40.0, 5.0));
        assert_eq!(
            near_right[0],
            [1, 3],
            "start at the island under the nozzle"
        );
        let near_left = islands.visiting_order((-5.0, 5.0));
        assert_eq!(near_left[0], [0, 2]);
    }

    #[test]
    fn the_top_surface_is_laid_after_the_fill_beside_it() {
        let mut layer = SliceLayer::new(0.2);
        push(&mut layer, square(0.0, 0.0, 10.0), ExtrusionRole::OuterWall);
        push(
            &mut layer,
            line(2.0, 3.0, 8.0, 3.0),
            ExtrusionRole::TopSurface,
        );
        push(&mut layer, line(2.0, 7.0, 8.0, 7.0), ExtrusionRole::Infill);

        let islands = Islands::of(&layer);
        assert_eq!(
            islands.members[0],
            vec![0, 2, 1],
            "wall, then sparse fill, then the visible top"
        );
    }

    /// A hole's contour is an outer-wall loop too. Treating it as an island of
    /// its own would split the island that surrounds it in two.
    #[test]
    fn a_hole_belongs_to_the_island_around_it() {
        let mut layer = SliceLayer::new(0.2);
        push(&mut layer, square(0.0, 0.0, 20.0), ExtrusionRole::OuterWall);
        push(&mut layer, square(8.0, 8.0, 4.0), ExtrusionRole::OuterWall);

        let islands = Islands::of(&layer);
        assert_eq!(islands.members.len(), 1);
        assert_eq!(islands.members[0], vec![0, 1]);
    }
}
