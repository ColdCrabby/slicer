//! The slicing pipeline, as an ordered list of named stages.
//!
//! Every step `process_mesh` used to run as a line of straight-line code is a
//! [`Stage`] here, and the sequence they run in is a [`StageRegistry`] — a
//! value, not control flow. That is the whole reason this module exists: a
//! plugin says *where* its work goes by naming a stage, so every stage the
//! engine grows becomes two new hook points (before it, after it) for free.
//!
//! Two properties are load-bearing and easy to break:
//!
//! - **Order.** The comments on each stage below record *why* it sits where it
//!   does — surfaces after walls so the bead geometry is real, infill after
//!   surfaces so it can subtract them, adhesion dead last so it cannot perturb
//!   the object's own toolpaths. Reordering the pushes in [`core_stages`]
//!   changes the output.
//! - **Inter-stage state.** What used to be local variables now lives in
//!   [`Artifacts`]. A stage that reads one must cope with it being absent:
//!   several are only populated when the feature that produces them is on.
//!
//! [`Artifacts`]: crate::plugin::Artifacts

use clipper2::Paths;

use crate::logging::ProcessLogger;
use crate::plugin::{FnStage, SliceContext, StageRegistry};
use crate::settings::params::{SeamPosition, SlicingParams};

use super::infill::{add_infill_to_layers, calculate_interior_region, InfillConfig};
use super::pipeline::resolved_first_layer_height;
use super::slicer::slice_mesh_with_first_layer;
use super::surfaces::{
    generate_top_bottom_surfaces_with_interior, perimeter_paths_of, prune_redundant_gap_fill,
    SurfaceConfig,
};
use super::types::{ExtrusionRole, OverhangClass, SliceLayer};
use super::walls::{apply_single_wall_restrictions, classify_overhang_perimeters, OverhangGrading};

/// The stage ids of the core pipeline, in execution order.
///
/// These are the [`phases`] constants, so the name a plugin targets and the
/// name the phase timings report are one string — there is no second catalog
/// to drift.
pub mod ids {
    use crate::logging::phases;

    /// Triangle-plane intersection: mesh in, raw layer contours out.
    pub const SLICING: &str = phases::SLICING;
    /// XY size and hole compensation, on the raw contours.
    pub const COMPENSATION: &str = phases::COMPENSATION;
    /// Medial-limited shrink of the layers resting on the bed.
    pub const ELEPHANT_FOOT: &str = phases::ELEPHANT_FOOT;
    /// Perimeter bead generation.
    pub const WALL_GENERATION: &str = phases::WALL_GENERATION;
    /// Interior-region snapshot taken while every wall is still present.
    pub const INFILL_REGION_SNAPSHOT: &str = phases::INFILL_REGION_SNAPSHOT;
    /// Stripping inner walls from first / end-of-run layers.
    pub const WALL_RESTRICTIONS: &str = phases::WALL_RESTRICTIONS;
    /// Per-layer interior regions used to place surfaces.
    pub const INTERIOR_REGIONS: &str = phases::INTERIOR_REGIONS;
    /// Pristine outer-wall snapshot for overhang-degree grading.
    pub const OVERHANG_SUPPORT_SNAPSHOT: &str = phases::OVERHANG_SUPPORT_SNAPSHOT;
    /// Top / bottom solid surfaces, bridges and ironing.
    pub const SURFACES: &str = phases::SURFACES;
    /// Grading of wall segments that cross unsupported air.
    pub const OVERHANG_CLASSIFICATION: &str = phases::OVERHANG_CLASSIFICATION;
    /// Removal of gap-fill beads a solid surface already covers.
    pub const GAP_FILL_PRUNE: &str = phases::GAP_FILL_PRUNE;
    /// Sparse infill.
    pub const INFILL: &str = phases::INFILL;
    /// Greedy-TSP ordering and seam placement.
    pub const PATH_ORDERING: &str = phases::PATH_ORDERING;
    /// Extrusion scaling where wall beads overlap.
    pub const FLOW_COMPENSATION: &str = phases::FLOW_COMPENSATION;
    /// Cosmetic outer-wall texture.
    pub const FUZZY_SKIN: &str = phases::FUZZY_SKIN;
    /// Skirt / brim / raft.
    pub const BED_ADHESION: &str = phases::BED_ADHESION;
    /// Charging the object's bottom layer at its own height.
    pub const FIRST_LAYER_HEIGHT: &str = phases::FIRST_LAYER_HEIGHT;
}

/// Build the core pipeline.
///
/// The push order **is** the execution order. Plugin registrations are folded
/// in afterwards by [`crate::plugin::install`].
pub fn core_stages() -> StageRegistry {
    let mut registry = StageRegistry::new();
    registry
        .push(FnStage::boxed(ids::SLICING, slice))
        .push(FnStage::boxed(ids::COMPENSATION, compensate_dimensions))
        .push(FnStage::boxed(ids::ELEPHANT_FOOT, elephant_foot))
        .push(FnStage::boxed(ids::WALL_GENERATION, generate_walls))
        .push(FnStage::boxed(
            ids::INFILL_REGION_SNAPSHOT,
            snapshot_infill_regions,
        ))
        .push(FnStage::boxed(ids::WALL_RESTRICTIONS, restrict_walls))
        .push(FnStage::boxed(ids::INTERIOR_REGIONS, interior_regions))
        .push(FnStage::boxed(
            ids::OVERHANG_SUPPORT_SNAPSHOT,
            snapshot_overhang_support,
        ))
        .push(FnStage::boxed(ids::SURFACES, surfaces))
        .push(FnStage::boxed(
            ids::OVERHANG_CLASSIFICATION,
            classify_overhangs,
        ))
        .push(FnStage::boxed(ids::GAP_FILL_PRUNE, prune_gap_fill))
        .push(FnStage::boxed(ids::INFILL, infill))
        .push(FnStage::boxed(ids::PATH_ORDERING, order_paths))
        .push(FnStage::boxed(ids::FLOW_COMPENSATION, compensate_flow))
        .push(FnStage::boxed(ids::FUZZY_SKIN, fuzzy_skin))
        .push(FnStage::boxed(ids::BED_ADHESION, bed_adhesion))
        .push(FnStage::boxed(ids::FIRST_LAYER_HEIGHT, first_layer_height));
    registry
}

// ── Stages ──────────────────────────────────────────────────────────────────

/// Cut the mesh into layer contours.
///
/// Layer 0 gets its own Z span so an explicit `first_layer_height` is honoured
/// by the geometry as well as by the flow; the resolved value is recorded in
/// [`Artifacts::first_layer_height`] for the stage at the far end of the
/// pipeline that charges it.
///
/// [`Artifacts::first_layer_height`]: crate::plugin::Artifacts::first_layer_height
fn slice(cx: &mut SliceContext<'_>) {
    cx.logger.log_debug("slicing mesh…");
    let first_h = resolved_first_layer_height(cx.params);
    cx.artifacts.first_layer_height = first_h;
    cx.layers = slice_mesh_with_first_layer(cx.mesh, cx.params.layer_height, first_h);
    cx.logger
        .log_info(&format!("sliced into {} layers", cx.layers.len()));
}

/// Apply the XY size and hole deltas, on the raw contours and nowhere else.
///
/// Every later stage measures relative to the contour the wall generator
/// consumed, so correcting it here leaves all of those relations true.
fn compensate_dimensions(cx: &mut SliceContext<'_>) {
    apply_compensation(&mut cx.layers, cx.params, cx.logger);
}

/// Undo the first layer's squish, in the same raw-contour window.
fn elephant_foot(cx: &mut SliceContext<'_>) {
    apply_elephant_foot(&mut cx.layers, cx.params, cx.logger);
}

/// Replace the raw contours with wall bead centrelines.
fn generate_walls(cx: &mut SliceContext<'_>) {
    let params = cx.params;
    cx.logger.log_debug(&format!(
        "generating walls (generator: {}, wall_count: {}, nozzle: {}mm)",
        params.wall_generator.name(),
        params.wall_count,
        params.nozzle_diameter_mm
    ));
    let wall_timings = crate::walls::generate_walls(&mut cx.layers, params);
    cx.logger.log_debug(&format!(
        "wall sub-timings (CPU total across threads): collapse_depth {} ms, bead_shrinks {} ms",
        wall_timings.collapse_depth_ms, wall_timings.bead_shrink_ms,
    ));
    cx.logger.log_debug("wall generation complete");
}

/// Snapshot the infill interior **while every wall is still present**.
///
/// The next stage strips inner walls from some islands; without this snapshot
/// `calculate_interior_region` would later see one wall where there were
/// several and push sparse infill far into the wall zone of islands that were
/// never stripped.
fn snapshot_infill_regions(cx: &mut SliceContext<'_>) {
    let params = cx.params;
    if !(params.infill_density > 0.0
        && (params.only_one_wall_first_layer || params.only_one_wall_top))
    {
        return;
    }
    cx.artifacts.pre_strip_infill_regions = Some(interior_regions_for(&cx.layers, params, 0.0));
}

/// Strip inner walls from the islands that end a top-surface run.
///
/// Per island, not per layer: the body island sharing a layer with a small
/// embossed feature keeps its walls.
fn restrict_walls(cx: &mut SliceContext<'_>) {
    let params = cx.params;
    if !(params.only_one_wall_first_layer || params.only_one_wall_top) {
        return;
    }
    cx.logger
        .log_debug("applying single-wall restrictions (per-island)");
    apply_single_wall_restrictions(&mut cx.layers, params);
}

/// Compute the region surfaces are placed inside.
fn interior_regions(cx: &mut SliceContext<'_>) {
    let params = cx.params;
    if params.top_layers == 0 && params.bottom_layers == 0 {
        cx.artifacts.interior_regions = Vec::new();
        return;
    }
    cx.logger
        .log_debug("calculating interior regions for surfaces");
    cx.artifacts.interior_regions =
        interior_regions_for(&cx.layers, params, params.infill_overlap_percent);
}

/// Snapshot pristine outer walls before surface generation splits any of them.
///
/// Layer `i`'s support outline is `snapshot[i - 1]`; the grading stage below
/// consumes it. Only taken when dynamic overhang speed is on.
fn snapshot_overhang_support(cx: &mut SliceContext<'_>) {
    cx.artifacts.overhang_support = overhang_support_snapshot(&cx.layers, cx.params);
}

/// Generate top / bottom solid surfaces, bridges and ironing inside the walls.
fn surfaces(cx: &mut SliceContext<'_>) {
    let params = cx.params;
    if params.top_layers == 0 && params.bottom_layers == 0 {
        return;
    }
    cx.logger.log_debug(&format!(
        "generating surfaces (top: {}, bottom: {}, angle: {}°)",
        params.top_layers, params.bottom_layers, params.surface_infill_angle
    ));
    let interior = std::mem::take(&mut cx.artifacts.interior_regions);
    let surface_timings = generate_top_bottom_surfaces_with_interior(
        &mut cx.layers,
        &surface_config(params),
        Some(&interior),
    );
    cx.artifacts.interior_regions = interior;
    cx.logger.log_debug("surface generation complete");
    cx.logger.log_debug(&format!(
        "surface sub-timings: perimeter_snapshot {} ms, detection {} ms, infill_gen {} ms",
        surface_timings.perimeter_snapshot_ms,
        surface_timings.detection_ms,
        surface_timings.infill_gen_ms,
    ));
}

/// Grade wall segments that lie over air so the generator prints them with
/// bridge speed, flow and cooling.
///
/// Skipped in spiral mode, where splitting a closed loop into open arcs would
/// break the single continuous contour the spiral emitter needs. Requires
/// `unsupported_regions`, which only surface generation populates — hence the
/// same guard as the stage above.
fn classify_overhangs(cx: &mut SliceContext<'_>) {
    let params = cx.params;
    if (params.top_layers == 0 && params.bottom_layers == 0) || params.spiral_vase {
        return;
    }
    cx.logger.log_debug("classifying overhang perimeters");
    let support = std::mem::take(&mut cx.artifacts.overhang_support);
    let grading = support.as_deref().map(|support| OverhangGrading {
        support,
        band_class: overhang_band_class(params),
    });
    classify_overhang_perimeters(&mut cx.layers, params.nozzle_diameter_mm, grading);
    cx.artifacts.overhang_support = support;
}

/// Drop gap-fill beads a solid surface already covers.
///
/// They would otherwise sit as scattered variable-width islands under a
/// uniform surface. Genuine gap fill in sparse zones and thin ribs is kept.
fn prune_gap_fill(cx: &mut SliceContext<'_>) {
    let params = cx.params;
    if params.top_layers == 0 && params.bottom_layers == 0 {
        return;
    }
    cx.logger
        .log_debug("pruning redundant gap fill inside solid surfaces");
    prune_redundant_gap_fill(&mut cx.layers, params.nozzle_diameter_mm);
}

/// Lay sparse infill inside the interior, minus the solid regions.
fn infill(cx: &mut SliceContext<'_>) {
    let params = cx.params;
    if params.infill_density <= 0.0 {
        return;
    }
    cx.logger.log_debug(&format!(
        "generating {} infill at {:.0}% density, {}° base angle…",
        params.infill_pattern.name(),
        params.infill_density * 100.0,
        params.infill_base_angle
    ));
    let pre_strip = std::mem::take(&mut cx.artifacts.pre_strip_infill_regions);
    add_infill_to_layers(
        &mut cx.layers,
        &sparse_infill_config(params),
        pre_strip.as_deref(),
    );
    cx.artifacts.pre_strip_infill_regions = pre_strip;
    cx.logger.log_debug("infill generation complete");
}

/// Order each layer's paths with a greedy TSP inside role groups, and place
/// the seam.
fn order_paths(cx: &mut SliceContext<'_>) {
    order_layer_paths(&mut cx.layers, cx.params);
}

/// Scale extrusion down where wall beads overlap.
///
/// Runs after ordering so the per-vertex widths it writes align with the final
/// geometry.
fn compensate_flow(cx: &mut SliceContext<'_>) {
    crate::flow::compensate(&mut cx.layers, cx.params);
}

/// Perturb the outer wall into a rough texture.
///
/// After ordering and flow compensation, so it carries along the per-vertex
/// widths already written; before adhesion, whose skirt and brim trace the
/// clean outer-wall centrelines and must not pick up the jitter.
fn fuzzy_skin(cx: &mut SliceContext<'_>) {
    crate::walls::fuzzy_skin::apply(&mut cx.layers, cx.params);
}

/// Prepend skirt / brim, or a raft and the Z shift that goes with it.
///
/// Dead last among the geometry stages so the object's own toolpaths are
/// provably unperturbed by it.
fn bed_adhesion(cx: &mut SliceContext<'_>) {
    let params = cx.params;
    if params.adhesion_type == crate::settings::params::AdhesionType::None {
        return;
    }
    cx.logger.log_debug(&format!(
        "generating bed adhesion ({:?})",
        params.adhesion_type
    ));
    crate::adhesion::apply_adhesion(&mut cx.layers, params);
}

/// Charge the object's bottom layer at the first-layer height.
///
/// After adhesion, so a skirt sharing that layer's Z is charged at the same
/// thickness the object is.
fn first_layer_height(cx: &mut SliceContext<'_>) {
    mark_first_layer_height(
        &mut cx.layers,
        cx.artifacts.first_layer_height,
        cx.params.layer_height,
    );
}

// ── Shared helpers ──

/// Per-layer interior regions, in parallel where the target has threads.
fn interior_regions_for(
    layers: &[SliceLayer],
    params: &SlicingParams,
    overlap_percent: f64,
) -> Vec<Paths> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        layers
            .par_iter()
            .map(|layer| {
                calculate_interior_region(
                    layer,
                    overlap_percent,
                    params.nozzle_diameter_mm,
                    params.wall_count,
                )
            })
            .collect()
    }
    #[cfg(target_arch = "wasm32")]
    {
        layers
            .iter()
            .map(|layer| {
                calculate_interior_region(
                    layer,
                    overlap_percent,
                    params.nozzle_diameter_mm,
                    params.wall_count,
                )
            })
            .collect()
    }
}

/// The surface-generation configuration, derived once so the production and
/// debug paths cannot disagree about it.
pub(crate) fn surface_config(params: &SlicingParams) -> SurfaceConfig {
    SurfaceConfig {
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
    }
}

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
    // No timer here: the stage runner already times this under
    // `phases::ELEPHANT_FOOT`, and a second one would emit the phase twice
    // under the same name.
    super::compensation::apply_elephant_foot(layers, &config);
}

/// Snapshot each layer's pristine OuterWall perimeter outline for dynamic
/// overhang-degree grading, or `None` when the feature is disabled.
///
/// Must be called **before** surface generation, which splits walls via bridge
/// clipping — the grader needs the un-split centrelines so a layer's support
/// outline (`snapshot[i-1]`) matches the geometry `unsupported_regions` was
/// built from.
fn overhang_support_snapshot(layers: &[SliceLayer], params: &SlicingParams) -> Option<Vec<Paths>> {
    if !params.enable_overhang_speed {
        return None;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let snapshot = {
        use rayon::prelude::*;
        layers.par_iter().map(perimeter_paths_of).collect()
    };
    #[cfg(target_arch = "wasm32")]
    let snapshot = layers.iter().map(perimeter_paths_of).collect();
    Some(snapshot)
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

/// The greedy-TSP ordering pass, verbatim from the pipeline it was
/// extracted out of.
fn order_layer_paths(layers: &mut [SliceLayer], params: &SlicingParams) {
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
        let mut ordered_overhang = Vec::with_capacity(path_count);
        let mut ordered_heights = Vec::with_capacity(path_count);

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
                }
                continue;
            }

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
                ordered_overhang.push(layer.overhang_for_path(best_path_idx));
                ordered_heights.push(layer.height_for_path(best_path_idx));
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
    }
}
