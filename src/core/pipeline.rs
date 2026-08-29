use clipper2::Paths;

use crate::logging::{phases, PhaseTimer, ProcessLogger};
use crate::mesh::types::Mesh;
use crate::settings::params::{SeamPosition, SlicingParams};

use super::infill::{add_infill_to_layers, calculate_interior_region};
use super::slicer::slice_mesh;
use super::surfaces::{
    generate_top_bottom_surfaces_with_interior, prune_redundant_gap_fill, SurfaceConfig,
};
use super::types::{ExtrusionRole, SliceLayer};
use super::walls::{apply_single_wall_restrictions, classify_overhang_perimeters};

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
    // Spiral (vase) mode forces a consistent single-wall configuration
    // (no infill/top surfaces/retraction) for the whole pipeline. Applied here
    // so every entry point (CLI, WebSocket, WASM) observes the same rules.
    let normalized = params.spiral_vase_normalized();
    let params = normalized.as_ref();

    logger.log_info(&format!("processing mesh: {} triangles", mesh.faces.len()));

    let t_slicing = PhaseTimer::start(phases::SLICING, logger);
    logger.log_debug("slicing mesh…");
    let mut layers = slice_mesh(mesh, params.layer_height);
    logger.log_info(&format!("sliced into {} layers", layers.len()));
    t_slicing.finish();

    if logger.is_cancelled() {
        logger.log_info("slice cancelled after slicing phase");
        return layers;
    }

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
            classify_overhang_perimeters(&mut layers, params.nozzle_diameter_mm);
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
            params.infill_density,
            infill_pattern,
            params.infill_base_angle,
            params.nozzle_diameter_mm,
            params.infill_perimeter_gap_mm,
            params.min_infill_extrusion_mm,
            pre_strip_infill_regions.as_deref(),
        );
        t_infill.finish();
        logger.log_debug("infill generation complete");
    }

    // Optimize path order (Greedy TSP within role groups)
    let t_tsp = PhaseTimer::start("Path Ordering", logger);
    for layer in layers.iter_mut() {
        let path_count = layer.paths.len();
        if path_count <= 1 {
            continue;
        }

        let paths_vec: Vec<_> = layer.paths.iter().cloned().collect();
        let mut ordered_paths = clipper2::Paths::default();
        let mut ordered_roles = Vec::with_capacity(path_count);
        let mut ordered_widths = Vec::with_capacity(path_count);
        let mut ordered_vertex_widths: Vec<Option<Vec<f64>>> = Vec::with_capacity(path_count);
        let mut ordered_is_open = Vec::with_capacity(path_count);

        let mut current_pos = (0.0, 0.0);

        // Group into contiguous ranges of the same role to preserve wall/infill print order
        let mut groups = Vec::new();
        let mut current_group = Vec::new();
        let mut current_group_role = layer.role_for_path(0);

        for (i, _) in paths_vec.iter().enumerate() {
            let role = layer.role_for_path(i);
            if role != current_group_role && !current_group.is_empty() {
                groups.push((current_group_role, current_group.clone()));
                current_group.clear();
                current_group_role = role;
            }
            current_group.push(i);
        }
        if !current_group.is_empty() {
            groups.push((current_group_role, current_group));
        }

        for (role, mut remaining) in groups {
            // Wall/skirt roles are nominally "closed" for TSP purposes, but
            // individual paths may be open arcs (split sub-segments from
            // classify_overhang_perimeters).  Open arcs are treated like open
            // polylines: both endpoints are candidate starts and current_pos
            // is updated to the path *end* (not the start) after emission.
            let role_is_closed = matches!(
                role,
                crate::core::ExtrusionRole::OuterWall
                    | crate::core::ExtrusionRole::InnerWall
                    | crate::core::ExtrusionRole::OverhangPerimeter
                    | crate::core::ExtrusionRole::Skirt
            );

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
                        let seam_v = choose_seam_vertex(path, params.seam_position, current_pos);
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
                ordered_vertex_widths.push(layer.vertex_widths_for_path(best_path_idx).map(|vw| {
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
                }));
                ordered_is_open.push(layer.is_path_open(best_path_idx));
            }
        }

        layer.paths = ordered_paths;
        layer.path_roles = ordered_roles;
        layer.path_widths = ordered_widths;
        layer.path_vertex_widths = ordered_vertex_widths;
        layer.path_is_open = ordered_is_open;
    }
    t_tsp.finish();

    // Wall overlap flow compensation — scale extrusion down where wall beads
    // overlap (tight slots, hairpins, acute concave corners).  Runs last so the
    // per-vertex widths it writes align with the final ordered/seamed geometry.
    let t_flow = PhaseTimer::start("Flow Compensation", logger);
    crate::flow::compensate(&mut layers, params);
    t_flow.finish();

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

    let mut layers = slice_mesh(mesh, params.layer_height);
    logger.log_info(&format!("sliced into {} layers", layers.len()));

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

    // Surfaces.
    if params.top_layers > 0 || params.bottom_layers > 0 {
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
            },
            Some(&interior_regions),
        );
        if !params.spiral_vase {
            classify_overhang_perimeters(&mut layers, params.nozzle_diameter_mm);
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
        let infill_pattern = params.infill_pattern;
        add_infill_to_layers(
            &mut layers,
            params.infill_density,
            infill_pattern,
            params.infill_base_angle,
            params.nozzle_diameter_mm,
            params.infill_perimeter_gap_mm,
            params.min_infill_extrusion_mm,
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

    logger.log_debug(&format!(
        "debug geometry: {} records captured across {} layers",
        debug.len(),
        layers.len()
    ));

    crate::flow::compensate(&mut layers, params);

    layers
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
