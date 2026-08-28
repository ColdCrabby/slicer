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

/// Add infill paths to layers based on slicing parameters.
///
/// Takes a set of layers with perimeter paths and adds infill patterns within
/// the perimeter boundaries. Infill paths are assigned the [`ExtrusionRole::Infill`]
/// role for proper G-code annotation.
///
/// # Arguments
/// * `layers` - Slice layers with perimeter paths (will be modified in place)
/// * `infill_density` - Infill density as a fraction (0.0 = no infill, 1.0 = solid)
/// * `infill_pattern` - The pattern type to generate (rectilinear, grid, etc.)
/// * `infill_base_angle` - Base angle in degrees (alternating layers rotate +90° on top of this)
/// * `nozzle_diameter_mm` - Nozzle diameter used when computing infill regions on the fly
/// * `infill_perimeter_gap_mm` - Gap in mm between the innermost wall and the infill boundary.
///   A positive value leaves a gap; `0.0` means infill starts exactly at the inner wall edge.
/// * `min_infill_extrusion_mm` - Isolated sparse-infill segments shorter than this are dropped
///   as splats: a sub-threshold dash in a narrow corner deposits a mechanically-insignificant
///   amount of material yet still costs a full retract → travel → un-retract to reach.  `0`
///   disables the filter.  Mirrors the identical guard on solid-surface infill.
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
/// use slicer_engine::core::{slice_mesh, add_infill_to_layers};
/// use slicer_engine::infill::InfillPattern;
/// # use slicer_engine::mesh::types::Mesh;
/// # let mesh = Mesh::new();
///
/// let mut layers = slice_mesh(&mesh, 0.2);
/// add_infill_to_layers(&mut layers, 0.2, InfillPattern::Rectilinear, 45.0, 0.4, 0.0, 0.4, None);
/// ```
#[allow(clippy::too_many_arguments)]
pub fn add_infill_to_layers(
    layers: &mut [SliceLayer],
    infill_density: f64,
    infill_pattern: crate::infill::InfillPattern,
    infill_base_angle: f64,
    nozzle_diameter_mm: f64,
    infill_perimeter_gap_mm: f64,
    min_infill_extrusion_mm: f64,
    precomputed_infill_regions: Option<&[Paths]>,
) {
    use crate::infill::generate_infill;

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
    // Each layer's infill is independent.  Compute all infill path sets in
    // parallel, then apply them to `layers` in a serial pass.
    // `None` entry means "skip this layer" (empty perimeters / empty area).
    let compute = |layer_idx: usize| -> Option<Paths> {
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

        let base_angle_rad = infill_base_angle.to_radians();
        let angle_offset = if layer_idx.is_multiple_of(2) {
            base_angle_rad
        } else {
            base_angle_rad + std::f64::consts::FRAC_PI_2
        };

        Some(generate_infill(
            &infill_area,
            infill_pattern,
            infill_density,
            angle_offset,
            layer.z,
        ))
    };

    #[cfg(not(target_arch = "wasm32"))]
    let results: Vec<Option<Paths>> = {
        use rayon::prelude::*;
        (0..layers.len()).into_par_iter().map(compute).collect()
    };
    #[cfg(target_arch = "wasm32")]
    let results: Vec<Option<Paths>> = (0..layers.len()).map(compute).collect();

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
    for (layer_idx, infill_paths) in results.into_iter().enumerate() {
        if let Some(paths) = infill_paths {
            let layer = &mut layers[layer_idx];
            for infill_path in paths.iter() {
                if !polyline_len_sq_ge(infill_path, min_len_sq) {
                    continue;
                }
                layer.paths.push(infill_path.clone());
                layer.path_roles.push(ExtrusionRole::Infill);
            }
        }
    }
}
