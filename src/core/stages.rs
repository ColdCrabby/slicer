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
use super::support_paint::project_support_paint;
use super::supports::generate_supports_with_paint;
use super::surfaces::{
    generate_top_bottom_surfaces_with_interior, perimeter_paths_of, prune_redundant_gap_fill,
    SurfaceConfig,
};
use super::types::{ExtrusionRole, OverhangClass, PathPick, SliceLayer, VertexOrder};
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
    /// Snapshot of the raw contours as the material footprint, before walls.
    pub const SLICE_OUTLINE_SNAPSHOT: &str = phases::SLICE_OUTLINE_SNAPSHOT;
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
    /// Pristine outer-wall snapshot for support generation.
    pub const OVERHANG_SUPPORT_SNAPSHOT: &str = phases::OVERHANG_SUPPORT_SNAPSHOT;
    /// Top / bottom solid surfaces, bridges and ironing.
    pub const SURFACES: &str = phases::SURFACES;
    /// Grading of wall segments that cross unsupported air.
    pub const OVERHANG_CLASSIFICATION: &str = phases::OVERHANG_CLASSIFICATION;
    /// Removal of gap-fill beads a solid surface already covers.
    pub const GAP_FILL_PRUNE: &str = phases::GAP_FILL_PRUNE;
    /// Sparse infill.
    pub const INFILL: &str = phases::INFILL;
    /// Support strands under overhangs steeper than the threshold angle.
    pub const SUPPORT_GENERATION: &str = phases::SUPPORT_GENERATION;
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
        .push(FnStage::boxed(
            ids::SLICE_OUTLINE_SNAPSHOT,
            snapshot_slice_outlines,
        ))
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
        .push(FnStage::boxed(ids::SUPPORT_GENERATION, generate_supports))
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

/// Snapshot the raw contours as the material footprint, before walls exist.
///
/// This is the outline the overhang classifier measures against: the material
/// of layer `i` fills its slice outline, so layer `i + 1`'s walls hang over
/// `slice_outlines[i]`, not over the centrelines the wall generator is about
/// to replace `paths` with. The timing is the whole subtlety — after
/// dimensional compensation (a deliberate resize the printed part really has)
/// and before elephant foot, which is *not* a step the next layer hangs over:
/// it shrinks the first layers precisely so the squashed bead spreads back out
/// to the model's width, so the material still reaches the uncompensated
/// outline. Taken unconditionally: whether a wall hangs in air is geometry and
/// cannot depend on a speed setting.
fn snapshot_slice_outlines(cx: &mut SliceContext<'_>) {
    cx.artifacts.slice_outlines = cx.layers.iter().map(|l| l.paths.clone()).collect();
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
/// Supports need the same un-split outlines, and for the same reason: the
/// classification pass below retags an overhanging wall as `OverhangPerimeter`
/// and splits its loop, so a steep slope keeps no `OuterWall` path for
/// [`generate_supports_with_paint`] to measure. Layer `i`'s snapshot is the
/// footprint the strands of layer `i + 1` are grown from. Only taken when
/// support generation is on.
///
/// [`generate_supports_with_paint`]: super::supports::generate_supports_with_paint
fn snapshot_overhang_support(cx: &mut SliceContext<'_>) {
    if !cx.params.support_enabled {
        return;
    }
    cx.artifacts.overhang_support = Some(perimeter_snapshot(&cx.layers));
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
///
/// Measures against [`Artifacts::slice_outlines`], the layer below's material
/// footprint — not against its wall centrelines, which understate the
/// footprint by half a bead and turn a supported bead into an "airborne" one.
/// Only the *degrees* are gated on `enable_overhang_speed`; whether a wall
/// hangs in air is geometry and is graded unconditionally.
///
/// [`Artifacts::slice_outlines`]: crate::plugin::Artifacts::slice_outlines
fn classify_overhangs(cx: &mut SliceContext<'_>) {
    let params = cx.params;
    if (params.top_layers == 0 && params.bottom_layers == 0) || params.spiral_vase {
        return;
    }
    cx.logger.log_debug("classifying overhang perimeters");
    let outlines = std::mem::take(&mut cx.artifacts.slice_outlines);
    let grading = params.enable_overhang_speed.then(|| OverhangGrading {
        band_class: overhang_band_class(params),
    });
    classify_overhang_perimeters(
        &mut cx.layers,
        params.nozzle_diameter_mm,
        Some(&outlines),
        grading,
    );
    cx.artifacts.slice_outlines = outlines;
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

/// Generate support structures for overhangs steeper than the threshold angle.
///
/// Runs before path ordering so support strands are grouped and ordered with
/// the rest of the layer. Reads the pristine outer-wall snapshot taken before
/// surface generation split any wall — the live `OuterWall` paths of a steep
/// slope are already gone by now, retagged and split by the classification
/// stage above.
///
/// The user's painted facets are projected onto the layer stack here, so
/// enforcers and blockers override the automatic overhang rule. An empty
/// annotation skips the projection entirely, so an unpainted slice cannot
/// drift.
fn generate_supports(cx: &mut SliceContext<'_>) {
    let params = cx.params;
    if !params.support_enabled {
        return;
    }
    cx.logger.log_debug(&format!(
        "generating supports (type: {:?}, threshold: {}°, density: {:.0}%)",
        params.support_type,
        params.support_threshold_angle,
        params.support_density * 100.0
    ));
    let paint_masks = project_support_paint(
        cx.mesh,
        cx.paint,
        &cx.layers,
        cx.artifacts.first_layer_height,
        params.nozzle_diameter_mm,
    );
    let pristine = std::mem::take(&mut cx.artifacts.overhang_support);
    generate_supports_with_paint(&mut cx.layers, params, pristine.as_deref(), &paint_masks);
    cx.artifacts.overhang_support = pristine;
    cx.logger.log_debug("support generation complete");
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
        elephant_foot: super::compensation::ElephantFootConfig::resolve(params),
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

/// Snapshot each layer's pristine `OuterWall` perimeter outline.
///
/// The caller decides whether the snapshot is wanted; this only gathers it.
fn perimeter_snapshot(layers: &[SliceLayer]) -> Vec<Paths> {
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
    let deg1 = if params
        .overhang_1_4_speed
        .resolve(params.perimeter_speed)
        .is_some()
        || fan_targets(0.25)
    {
        OverhangClass::Deg1
    } else {
        OverhangClass::None
    };
    let deg2 = if params
        .overhang_2_4_speed
        .resolve(params.perimeter_speed)
        .is_some()
        || fan_targets(0.5)
    {
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

/// Order each layer's paths: island by island, a greedy TSP within each run of
/// one role, with closed loops started at the seam the configured policy picks.
///
/// The nozzle carries over from the layer below. Restarting every layer's walk
/// at the origin made each layer open with a hop toward whichever path happened
/// to sit nearest the bed's corner — a full crossing of the part, every layer,
/// on a plate that is nowhere near the origin.
fn order_layer_paths(layers: &mut [SliceLayer], params: &SlicingParams) {
    let mut current_pos = (0.0, 0.0);
    for layer in layers.iter_mut() {
        let path_count = layer.paths.len();
        if path_count == 0 {
            continue;
        }

        let paths_vec: Vec<_> = layer.paths.iter().cloned().collect();
        // The pass decides an *order*, not new geometry: it records which path
        // to take next and how to walk its vertices, and the layer is rebuilt
        // from that once at the end.
        let mut picks: Vec<PathPick> = Vec::with_capacity(path_count);

        // Split the layer into islands and print one island out before moving
        // to the next. The fill generators emit per layer, not per island, so
        // without this every island's walls were laid first and the nozzle then
        // came back across the plate to fill each of them in turn.
        let islands = Islands::of(layer);

        for island in islands.visiting_order(current_pos) {
            // Contiguous runs of one role, in the order the generators emitted
            // them — that order is the wall sequence and the monotonic sweep.
            // Only the phase sort in `Islands::of` moves anything.
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
                        picks.push(PathPick::keep(path_idx));
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

                    // Rotating a closed loop moves its seam to `best_seam_vertex`.
                    // The first vertex stays the closing vertex — the generator
                    // appends the move back to it — so the loop reads
                    // [v_seam, …, v_n-1, v_0, …, v_seam-1] with no duplicate.
                    let order = if best_is_closed && best_seam_vertex != 0 {
                        VertexOrder::RotatedTo(best_seam_vertex)
                    } else if best_reverse {
                        VertexOrder::Reversed
                    } else {
                        VertexOrder::AsIs
                    };

                    let points: Vec<_> = path.iter().copied().collect();
                    if !points.is_empty() {
                        let first = match order {
                            VertexOrder::AsIs => points[0],
                            VertexOrder::Reversed => points[points.len() - 1],
                            VertexOrder::RotatedTo(seam) => points[seam % points.len()],
                        };
                        let last = match order {
                            VertexOrder::AsIs => points[points.len() - 1],
                            VertexOrder::Reversed => points[0],
                            VertexOrder::RotatedTo(seam) => {
                                points[(seam + points.len() - 1) % points.len()]
                            }
                        };
                        // A closed loop ends where it started; an open one ends at
                        // its far end.
                        let end = if best_is_closed { first } else { last };
                        current_pos = (end.x(), end.y());
                    }

                    picks.push(PathPick {
                        index: best_path_idx,
                        order,
                    });
                }
            }
        }

        // One rebuild for the whole layer: paths, every per-path array, and
        // every per-vertex array re-walked with the same order as the vertices
        // they describe. Doing it by hand is how an array gets forgotten and
        // somebody's tags end up on the wrong path.
        layer.rebuild_paths(&picks);
    }
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
