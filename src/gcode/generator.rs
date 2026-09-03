//! [`GcodeGenerator`] — per-layer extrusion logic and the `generate_gcode` convenience wrapper.

use std::borrow::Cow;

use crate::core::SliceLayer;
use crate::gcode::dialect::{GcodeDialect, WarnFn};
use crate::gcode::dialects::{KlipperDialect, MarlinDialect};
use crate::gcode::flavor::GcodeFlavor;
use crate::gcode::stats::SliceStatistics;
use crate::settings::params::{fan_index, LifecycleMarkerConfig, SlicingParams};

// ── Private helpers ────────────────────────────────────────────────────────────

/// Minimum width difference (mm) that triggers a new `;WIDTH:` annotation.
///
/// Changes smaller than this epsilon are treated as equal, preventing redundant
/// WIDTH comments for floating-point rounding differences between beads.
const WIDTH_EPSILON: f64 = 1e-6;

/// Per-vertex width deviation (mm) below which a vertex may be dropped by the
/// width-aware path simplification.  Keeps the taper of variable-width beads
/// (Arachne walk, gap fill, overlap compensation) while still collapsing their
/// long constant-width runs, so they no longer emit at full Voronoi/Clipper
/// resolution.
const WIDTH_SIMPLIFY_TOL_MM: f64 = 0.02;

/// Minimum lift, in mm, above the tallest already-printed object when
/// sequential printing hands over to the next one.
///
/// The nozzle has to reach the far side of a finished part without touching it,
/// and the layer block that follows will drop straight back down to the new
/// object's first layer — so the clearance is taken *before* the travel, not
/// as part of it.  A configured Z-hop larger than this wins.
const SEQUENTIAL_LIFT_MM: f64 = 1.0;

/// Width step (mm) at which a variable-width bead re-emits a `;WIDTH:` marker
/// mid-path, so viewers/post-processors render the actual (flow-compensated)
/// bead width rather than the nominal scalar.  Coarse enough (0.05 mm) to keep
/// the marker count \u2014 and G-code size \u2014 bounded, fine enough to show the taper.
const WIDTH_MARKER_STEP_MM: f64 = 0.05;

// ── Spiral (vase) mode helpers ─────────────────────────────────────────────────

/// Outcome of scanning a layer for a spiralizable outer contour.
enum SpiralDetect {
    /// No closed outer-wall loop to spiralize.
    None,
    /// Exactly one island — the given `SliceLayer::paths` index is its outer
    /// contour.
    Single(usize),
    /// More than one separate island; spiral mode cannot fuse them into a
    /// single continuous contour.
    Multi,
}

/// Even-odd ray-cast point-in-polygon test (winding-independent).
fn point_in_polygon(pt: (f64, f64), poly: &[(f64, f64)]) -> bool {
    let (px, py) = pt;
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Scan a layer for the single outermost closed outer-wall contour to
/// spiralize.
///
/// Only closed [`crate::core::ExtrusionRole::OuterWall`] paths are considered.
/// A loop whose first vertex lies inside another such loop is treated as a hole
/// and ignored, so a solid island with holes still spiralizes as one contour.
/// The count of *outermost* (non-contained) loops is the island count: exactly
/// one yields [`SpiralDetect::Single`]; more yields [`SpiralDetect::Multi`].
fn detect_spiral_loop(layer: &SliceLayer) -> SpiralDetect {
    // Gather (paths-index, points) for every closed outer-wall loop.
    let mut loops: Vec<(usize, Vec<(f64, f64)>)> = Vec::new();
    for (i, path) in layer.paths.iter().enumerate() {
        if layer.role_for_path(i) == crate::core::ExtrusionRole::OuterWall && !layer.is_path_open(i)
        {
            let pts: Vec<(f64, f64)> = path.iter().map(|p| (p.x(), p.y())).collect();
            if pts.len() >= 2 {
                loops.push((i, pts));
            }
        }
    }
    if loops.is_empty() {
        return SpiralDetect::None;
    }

    // Outermost = whose first vertex is not inside any other loop. A hole's
    // boundary vertex sits inside its outer contour; separate islands are
    // disjoint so neither contains the other.
    let mut outermost: Vec<usize> = Vec::new();
    for (slot, (_, pts)) in loops.iter().enumerate() {
        let probe = pts[0];
        let contained = loops
            .iter()
            .enumerate()
            .any(|(other, (_, o))| other != slot && point_in_polygon(probe, o));
        if !contained {
            outermost.push(slot);
        }
    }

    match outermost.len() {
        0 => SpiralDetect::None, // degenerate (mutually contained) — nothing to spiral
        1 => SpiralDetect::Single(loops[outermost[0]].0),
        _ => SpiralDetect::Multi,
    }
}

/// Rotate a closed loop so its first vertex is the one nearest `target`,
/// minimising the travel from the previous layer's end into the spiral and
/// keeping the (invisible) start line aligned across layers.
fn rotate_loop_nearest(pts: &[(f64, f64)], target: (f64, f64)) -> Vec<(f64, f64)> {
    let n = pts.len();
    if n == 0 {
        return Vec::new();
    }
    let mut best = 0;
    let mut best_d = f64::MAX;
    for (i, &(x, y)) in pts.iter().enumerate() {
        let d = (x - target.0).powi(2) + (y - target.1).powi(2);
        if d < best_d {
            best_d = d;
            best = i;
        }
    }
    (0..n).map(|k| pts[(best + k) % n]).collect()
}

/// Estimate the print time for a layer in seconds.
///
/// Sums the total XY move distance for all paths in the layer and divides by
/// `print_speed_mm_s`.  Travel moves are not modelled separately; this is a
/// deliberately cheap **pre-move** proxy used only for the adaptive fan-speed
/// decision (which must be emitted *before* the layer's moves are known, so the
/// accurate trapezoidal estimate is not yet available).  The user-facing ETA and
/// the `;LAYER_TIME:` markers are replaced afterwards with the
/// acceleration-aware figure from [`crate::gcode::time_estimate`].
pub(crate) fn estimate_layer_time(layer: &SliceLayer, print_speed_mm_s: f64) -> f64 {
    if print_speed_mm_s <= 0.0 {
        return 0.0;
    }
    let mut total_mm = 0.0_f64;
    for path in layer.paths.iter() {
        let pts: Vec<(f64, f64)> = path.iter().map(|p| (p.x(), p.y())).collect();
        for w in pts.windows(2) {
            let dx = w[1].0 - w[0].0;
            let dy = w[1].1 - w[0].1;
            total_mm += (dx * dx + dy * dy).sqrt();
        }
    }
    total_mm / print_speed_mm_s
}

/// Overwrite the value of each `;LAYER_TIME:` marker in `body` with the
/// acceleration-aware per-layer estimate.
///
/// The generator emits one `;LAYER_TIME:` marker per printed layer (in order)
/// as a cheap placeholder; this rewrites those values in place with the
/// trapezoidal figures so the viewer's Layer-Time colouring matches the ETA.
/// Markers with no corresponding estimate (should not happen — the estimator
/// keys off the same markers) are left untouched, so the pass is always safe.
fn patch_layer_time_markers(body: &mut String, per_layer_s: &[f64]) {
    if per_layer_s.is_empty() || !body.contains(";LAYER_TIME:") {
        return;
    }
    let mut patched = String::with_capacity(body.len());
    let mut idx = 0usize;
    // Preserve a trailing newline: `str::lines` drops it, so re-add per line.
    for line in body.lines() {
        if line.trim_start().starts_with(";LAYER_TIME:") {
            if let Some(&t) = per_layer_s.get(idx) {
                patched.push_str(&format!(";LAYER_TIME:{:.1}", t));
                idx += 1;
            } else {
                patched.push_str(line);
            }
        } else {
            patched.push_str(line);
        }
        patched.push('\n');
    }
    *body = patched;
}

/// Compute the extrusion length (mm of filament) needed to print a straight
/// line of length `move_len` at the given `layer_height` with the configured
/// nozzle and filament diameters.
///
/// Formula: E = line_length × (layer_height × line_width) / (π × filament_radius²) × flow_ratio
///
/// `flow_ratio` is the global volumetric flow multiplier (`1.0` = nominal); it
/// scales the deposited volume to correct material-specific under/over-extrusion.
/// A non-positive or non-finite ratio is treated as `1.0` so a malformed profile
/// can never silently zero out (or reverse) all extrusion.
pub(crate) fn extrusion_for_move(
    move_len: f64,
    layer_height: f64,
    width_mm: f64,
    filament_diameter_mm: f64,
    flow_ratio: f64,
) -> f64 {
    let filament_radius = filament_diameter_mm / 2.0;
    let cross_section = layer_height * width_mm;
    let filament_area = std::f64::consts::PI * filament_radius.powi(2);
    let flow = if flow_ratio.is_finite() && flow_ratio > 0.0 {
        flow_ratio
    } else {
        1.0
    };
    move_len * cross_section / filament_area * flow
}

/// Cap a linear feedrate (mm/min) so the volumetric extrusion rate stays at or
/// below `max_volumetric_speed` (mm³/s) — the hotend melt-rate ceiling.
///
/// The volumetric rate of an extrusion move is `cross_section × linear_speed`,
/// where `cross_section = layer_height × width_mm` (mm²).  When the requested
/// feedrate would exceed the ceiling this returns the highest feedrate that
/// still respects it (`max_volumetric_speed · 60 / cross_section`); otherwise
/// the input is returned unchanged.
///
/// A non-positive limit (the default `0`) disables the cap, and a degenerate
/// cross-section (zero height or width) is passed through untouched so a
/// malformed path can never divide-by-zero or stall the feedrate to zero.
pub(crate) fn volumetric_capped_speed_mm_min(
    speed_mm_min: f64,
    layer_height: f64,
    width_mm: f64,
    max_volumetric_speed: f64,
) -> f64 {
    if max_volumetric_speed <= 0.0 {
        return speed_mm_min;
    }
    let cross_section = layer_height * width_mm;
    if cross_section <= 0.0 {
        return speed_mm_min;
    }
    // mm³/s ÷ mm² = mm/s, ×60 → mm/min.
    let cap_mm_min = max_volumetric_speed * 60.0 / cross_section;
    speed_mm_min.min(cap_mm_min)
}

/// Convert a slice-layer Z into the Z actually written to the G-code, applying
/// the machine's `z_offset_mm` compensation (issue #102).
///
/// A negative offset lowers the nozzle (the endstop zeroes too high), a positive
/// one raises it.  The offset is a *machine* correction, so it is applied here —
/// at the emission boundary — and nowhere else: the slice layers, the print
/// statistics and the time estimate all keep the model's own Z.  This mirrors
/// PrusaSlicer / OrcaSlicer, where `z_offset` is added to every emitted Z
/// coordinate rather than being pushed into a firmware directive.
fn machine_z(z: f64, params: &SlicingParams) -> f64 {
    z + params.z_offset_mm
}

/// Slowest strictly-positive `overhang_*_speed` (mm/s), or `None` when no
/// per-degree overhang speed is configured.  Used by
/// [`GcodeGenerator::effective_speed_mm_min`] to clamp curl-prone steep
/// overhangs when `slowdown_for_curled_perimeters` is on.
fn slowest_overhang_speed_mm_s(params: &SlicingParams) -> Option<f64> {
    [
        params.overhang_1_4_speed,
        params.overhang_2_4_speed,
        params.overhang_3_4_speed,
        params.overhang_4_4_speed,
    ]
    .into_iter()
    .filter(|s| *s > 0.0)
    .min_by(|a, b| a.partial_cmp(b).unwrap())
}

/// `true` when an overhang class is severe enough to trigger the overhang fan.
///
/// The class's *upper* unsupported fraction (Deg1 → 25%, Deg2 → 50%, Deg3 →
/// 75%, Deg4 → 100%) must exceed `overhang_fan_threshold`, so the default 0.5
/// threshold engages the fan from Deg3 (50–75%) upward.
fn overhang_meets_fan_threshold(
    overhang: crate::core::OverhangClass,
    params: &SlicingParams,
) -> bool {
    let upper_fraction = overhang.band() as f64 * 0.25;
    upper_fraction > params.overhang_fan_threshold + f64::EPSILON
}

/// Resolve the extrusion width for a path.
///
/// Precedence (first match wins):
/// 0. **Fill roles** (solid top/bottom surface fill and sparse infill, with no
///    explicit/per-vertex width) are charged at their *line spacing*
///    (`extrusion_flow_spacing_mm` of their nominal width), not the nominal bead
///    width, so each line deposits `spacing × layer_height` and fills its strip
///    exactly instead of over-extruding it (see the inline note).
/// 1. A per-role width override (`outer_wall_line_width`, `inner_wall_line_width`,
///    `top_surface_line_width`, `sparse_infill_line_width`) when set (`> 0`) —
///    but only for **constant-width** paths (`has_vertex_widths == false`). This
///    is what lets a wall-width setting take effect even though the wall
///    generator stamps an explicit, nozzle-derived width on every wall path.
///    Variable-width Arachne beads (gap fill, tapered beads) are skipped here
///    because they carry their own authoritative per-segment widths.
/// 2. An explicit per-path width (Arachne bead width, bridge-flow reduction, …).
/// 3. The generic `line_width` setting — but only for **solid infill and
///    surfaces**, preserving the historical behaviour that walls ignore the
///    global line width (their width comes from the wall generator).
/// 4. The role's nozzle-derived default width.
pub(crate) fn resolve_width_mm(
    explicit: Option<f64>,
    has_vertex_widths: bool,
    role: crate::core::ExtrusionRole,
    params: &SlicingParams,
) -> f64 {
    use crate::core::ExtrusionRole;

    // Ironing is fully described by its own two settings and nothing may
    // override it. The pass sweeps a strip `ironing_spacing` wide but deposits
    // only `ironing_flow` percent of a full bead into it, and folding that
    // fraction into the width is what makes the reduction *arithmetic* rather
    // than a flow ratio: `extrusion_for_move` deliberately reads a non-positive
    // flow ratio as 1.0 so a malformed profile cannot zero out a print, which
    // would turn a legitimate "wipe only" ironing setting into a full-width
    // bead laid at 0.1 mm pitch.
    if role == ExtrusionRole::Ironing {
        let spacing = params.ironing_spacing.max(0.0);
        let flow = params.ironing_flow.clamp(0.0, 100.0) / 100.0;
        return spacing * flow;
    }

    // Fill roles are laid at their flow spacing (the libslic3r/Orca stadium
    // pitch, ≈ 0.357 mm at a 0.4 mm nozzle / 0.2 mm layers), so each line must
    // deposit `spacing × layer_height` of filament — the volume of the strip it
    // fills — *not* the full nominal bead width. Charging them at the wider
    // nominal width over-extrudes by `width / spacing` (≈ 13 % at nozzle width,
    // ≈ 23 % once `line_width` > nozzle): the raised / blobby top-surface
    // defect. Matching the flow to the spacing mirrors PrusaSlicer/Orca
    // (`mm³/mm = spacing × height`) and lays a flat surface.
    //
    // Sparse infill obeys the same identity: `add_infill_to_layers` pitches its
    // lines `spacing / density` apart, so charging them at `spacing` is what
    // makes the deposited volume equal the requested density. Bridges (their own
    // role, explicit width) are unaffected.
    if explicit.is_none() && !has_vertex_widths {
        match role {
            ExtrusionRole::TopSurface
            | ExtrusionRole::BottomSurface
            | ExtrusionRole::InternalSolid => {
                let nominal = crate::core::solid_surface_nominal_width_mm(params);
                return crate::core::extrusion_flow_spacing_mm(nominal, params.layer_height);
            }
            ExtrusionRole::Infill => {
                let nominal = crate::core::sparse_infill_nominal_width_mm(params);
                return crate::core::extrusion_flow_spacing_mm(nominal, params.layer_height);
            }
            _ => {}
        }
    }

    // A per-role override wins over the constant, generator-stamped width for
    // its role (walls included). Skipped for variable-width beads, whose
    // per-vertex widths are authoritative and applied separately.
    if !has_vertex_widths {
        let role_override = match role {
            ExtrusionRole::OuterWall | ExtrusionRole::OverhangPerimeter => {
                params.outer_wall_line_width
            }
            ExtrusionRole::InnerWall => params.inner_wall_line_width,
            ExtrusionRole::TopSurface
            | ExtrusionRole::BottomSurface
            | ExtrusionRole::InternalSolid => params.top_surface_line_width,
            ExtrusionRole::Infill => params.sparse_infill_line_width,
            _ => 0.0,
        };
        if role_override > 0.0 {
            return role_override;
        }
    }

    if let Some(w) = explicit {
        return w;
    }

    // Generic `line_width` still applies only to solid infill and surfaces.
    let line_width_role = matches!(
        role,
        ExtrusionRole::Infill
            | ExtrusionRole::TopSurface
            | ExtrusionRole::BottomSurface
            | ExtrusionRole::InternalSolid
    );
    if params.line_width > 0.0 && line_width_role {
        params.line_width
    } else {
        role.default_width_mm()
    }
}

/// Substitute template placeholders in a marker string.
///
/// Replaces `{z}`, `{height}`, `{type}`, and `{width}` with the supplied
/// values.  Placeholders that are not relevant to a particular marker are
/// simply left as-is when the corresponding value is an empty string.
pub(crate) fn render_marker(
    template: &str,
    z: &str,
    height: &str,
    type_name: &str,
    width: &str,
) -> String {
    template
        .replace("{z}", z)
        .replace("{height}", height)
        .replace("{type}", type_name)
        .replace("{width}", width)
}

/// Substitute print-parameter placeholders shared by custom start / end / layer
/// scripts: `{nozzle_temp}`, `{bed_temp}`, their `_first_layer` variants,
/// `{chamber_temp}` (and `{chamber_temp_first_layer}`), `{filament_type}`, plus
/// `{layer_height}` and `{first_layer_height}`.
///
/// For migration convenience, common Orca-style bracket placeholders are also
/// accepted as aliases (for example `[nozzle_temperature_initial_layer]`).
///
/// The `_first_layer` temperatures fall back to the general value when set to
/// `0` (the "use base value" sentinel), matching the slicer's own resolution.
/// Longer keys are replaced before their prefixes (e.g. `{nozzle_temp_first_layer}`
/// before `{nozzle_temp}`) so no partial substitution occurs. Dialect-default
/// scripts carry no placeholders, so this is a no-op for them.
pub(crate) fn render_script_placeholders(line: &str, params: &SlicingParams) -> String {
    let first_nozzle = if params.nozzle_temp_first_layer > 0.0 {
        params.nozzle_temp_first_layer
    } else {
        params.nozzle_temp
    };
    let first_bed = if params.bed_temp_first_layer > 0.0 {
        params.bed_temp_first_layer
    } else {
        params.bed_temp
    };
    let first_chamber = params.chamber_temp_first_layer_resolved();
    let first_height = if params.first_layer_height > 0.0 {
        params.first_layer_height
    } else {
        params.layer_height
    };
    line.replace("{nozzle_temp_first_layer}", &format!("{:.0}", first_nozzle))
        .replace("{bed_temp_first_layer}", &format!("{:.0}", first_bed))
        .replace(
            "{chamber_temp_first_layer}",
            &format!("{:.0}", first_chamber),
        )
        .replace("{nozzle_temp}", &format!("{:.0}", params.nozzle_temp))
        .replace("{bed_temp}", &format!("{:.0}", params.bed_temp))
        .replace("{chamber_temp}", &format!("{:.0}", params.chamber_temp))
        .replace("{filament_type}", &params.filament_type)
        .replace("{first_layer_height}", &format!("{:.3}", first_height))
        .replace("{layer_height}", &format!("{:.3}", params.layer_height))
        // Orca-style aliases
        .replace(
            "[nozzle_temperature_initial_layer]",
            &format!("{:.0}", first_nozzle),
        )
        .replace(
            "[bed_temperature_initial_layer_single]",
            &format!("{:.0}", first_bed),
        )
        .replace(
            "[chamber_temperature_initial_layer]",
            &format!("{:.0}", first_chamber),
        )
        .replace(
            "[nozzle_temperature]",
            &format!("{:.0}", params.nozzle_temp),
        )
        .replace("[bed_temperature]", &format!("{:.0}", params.bed_temp))
        .replace(
            "[chamber_temperature]",
            &format!("{:.0}", params.chamber_temp),
        )
        .replace("[filament_type]", &params.filament_type)
}

/// Tokens that mean a custom start script manages the chamber **heater** itself.
///
/// A `START_PRINT … CHAMBER={chamber_temp}` macro (Klippain and friends) already
/// heats and soaks the chamber, so the generator's own sequence would be a
/// second, conflicting heat-and-wait. Detecting any of these suppresses it.
///
/// Every token is uppercase and carries enough context to mean *heating*. A bare
/// `CHAMBER` would be wrong: enclosed printers routinely drive a chamber
/// circulation fan (`SET_FAN_SPEED FAN=chamber_fan …`, `M106 P2 S255 ; chamber
/// fan`) or mention the chamber in a comment, and matching those would silently
/// disable chamber heating altogether — the exact failure the feature prevents.
const CUSTOM_CHAMBER_TOKENS: &[&str] = &[
    // RepRap / Marlin set + wait
    "M141",
    "M191",
    // Macro argument: `START_PRINT … CHAMBER=50`
    "CHAMBER=",
    // Placeholders and their Orca aliases, plus `CHAMBER_TEMPERATURE=` macro args:
    // `{chamber_temp}`, `{chamber_temp_first_layer}`, `[chamber_temperature]`,
    // `[chamber_temperature_initial_layer]`
    "CHAMBER_TEMP",
    // Klipper native heater control
    "HEATER=CHAMBER",
    "HEATER_GENERIC CHAMBER",
];

/// Whether a custom start script already takes care of chamber heating.
///
/// Matched against the **raw** (pre-substitution) script so a `{chamber_temp}`
/// placeholder is still recognisable after it has been rendered to a number.
fn start_script_handles_chamber(script: &[String]) -> bool {
    script.iter().any(|line| {
        let upper = line.to_uppercase();
        CUSTOM_CHAMBER_TOKENS
            .iter()
            .any(|token| upper.contains(token))
    })
}

// ── GcodeGenerator ─────────────────────────────────────────────────────────────

/// High-level G-code generator that delegates all firmware-specific command
/// emission to a [`GcodeDialect`] implementation.
///
/// `GcodeGenerator` is the **façade** of the multi-flavor framework: it owns
/// the per-layer extrusion logic while the dialect handles the command syntax.
///
/// An optional **warn function** can be registered via [`GcodeGenerator::with_warn_fn`].
/// It is called when the active dialect advertises unsupported commands (see
/// [`GcodeDialect::unsupported_commands`]), so callers can surface those
/// warnings through the appropriate logging channel.
///
/// Custom start / end scripts set via [`GcodeGenerator::with_start_script`] and
/// [`GcodeGenerator::with_end_script`] take precedence over the dialect's
/// built-in defaults.  This supports the priority chain:
/// *CLI argument → global settings → dialect default*.
///
/// Per-flavor lifecycle marker overrides are applied via
/// [`GcodeGenerator::with_marker_config`].  When `marker_config.enabled` is
/// `true` (the default) each layer block is preceded by the full OrcaSlicer /
/// Klipper lifecycle marker block.
///
/// # Example
///
/// ```rust
/// use slicer_engine::gcode::{GcodeGenerator, GcodeFlavor};
/// use slicer_engine::settings::params::SlicingParams;
///
/// let gen = GcodeGenerator::new(GcodeFlavor::Marlin);
/// let gcode = gen.generate(&[], &SlicingParams::default());
/// assert!(gcode.contains("G28"));
/// ```
pub struct GcodeGenerator {
    dialect: Box<dyn GcodeDialect>,
    warn_fn: Option<WarnFn>,
    /// Per-flavor lifecycle marker configuration.
    marker_config: LifecycleMarkerConfig,
    /// Optional override for the start script (replaces dialect default).
    custom_start_script: Option<Vec<String>>,
    /// Optional override for the end script (replaces dialect default).
    custom_end_script: Option<Vec<String>>,
    /// Optional custom G-code emitted at every layer change (after the Z move).
    custom_layer_script: Option<Vec<String>>,
    /// Optional per-filament start script, emitted after the machine start
    /// script and before the first print move.
    custom_filament_start_script: Option<Vec<String>>,
    /// Optional per-filament end script, emitted after the last print move and
    /// before the machine end script.
    custom_filament_end_script: Option<Vec<String>>,
    /// Optional source model name, embedded in the metadata header (issue #15).
    model_name: Option<String>,
    /// Print objects on the plate, in tag order.
    ///
    /// Empty for a plate sliced without object identity; non-empty enables the
    /// firmware object markers (issue #22) and the sequential inter-object move
    /// (issue #112).
    objects: Vec<crate::core::ObjectIdentity>,
}

impl GcodeGenerator {
    /// Create a generator for the specified firmware flavor.
    pub fn new(flavor: GcodeFlavor) -> Self {
        let dialect: Box<dyn GcodeDialect> = match flavor {
            GcodeFlavor::Marlin => Box::new(MarlinDialect),
            GcodeFlavor::Klipper => Box::new(KlipperDialect),
        };
        Self {
            dialect,
            warn_fn: None,
            marker_config: LifecycleMarkerConfig::default(),
            custom_start_script: None,
            custom_end_script: None,
            custom_layer_script: None,
            custom_filament_start_script: None,
            custom_filament_end_script: None,
            model_name: None,
            objects: Vec::new(),
        }
    }

    /// Create a generator with a custom [`GcodeDialect`] implementation.
    ///
    /// Useful for testing or for dialects not covered by [`GcodeFlavor`].
    pub fn with_dialect(dialect: Box<dyn GcodeDialect>) -> Self {
        Self {
            dialect,
            warn_fn: None,
            marker_config: LifecycleMarkerConfig::default(),
            custom_start_script: None,
            custom_end_script: None,
            custom_layer_script: None,
            custom_filament_start_script: None,
            custom_filament_end_script: None,
            model_name: None,
            objects: Vec::new(),
        }
    }

    /// Register a warn callback invoked when the dialect signals unsupported commands.
    ///
    /// The function receives a human-readable warning message and is responsible
    /// for routing it to the appropriate output channel (e.g. [`crate::cli::emit::Emitter::log_warn`]).
    ///
    /// ```rust
    /// use slicer_engine::gcode::{GcodeGenerator, GcodeFlavor};
    ///
    /// let gen = GcodeGenerator::new(GcodeFlavor::Marlin)
    ///     .with_warn_fn(|msg| eprintln!("[warn] {}", msg));
    /// ```
    pub fn with_warn_fn(mut self, f: impl Fn(&str) + 'static) -> Self {
        self.warn_fn = Some(Box::new(f));
        self
    }

    /// Attach the plate's print objects so their moves can be attributed.
    ///
    /// With objects attached the generator declares them once at the top of the
    /// program and wraps each object's moves in the dialect's markers
    /// (`EXCLUDE_OBJECT_*` / `M486`), driven by the per-path tags in
    /// [`SliceLayer::path_objects`](crate::core::SliceLayer::path_objects).
    /// Passing an empty list — the default — leaves the output untouched.
    pub fn with_objects(mut self, objects: Vec<crate::core::ObjectIdentity>) -> Self {
        self.objects = objects;
        self
    }

    /// Configure whether layer lifecycle markers are emitted in the output.
    ///
    /// When `true` (the default) each layer block is preceded by:
    /// `;LAYER_CHANGE`, `;Z:`, `;HEIGHT:`, `;BEFORE_LAYER_CHANGE`, `G92 E0`,
    /// `;AFTER_LAYER_CHANGE` and `;TYPE:` / `;WIDTH:` annotations at each
    /// extrusion-role transition.
    ///
    /// Set to `false` to emit a minimal `; layer z=…` comment instead.
    ///
    /// For more fine-grained control use [`GcodeGenerator::with_marker_config`].
    pub fn with_lifecycle_markers(mut self, enabled: bool) -> Self {
        self.marker_config.enabled = enabled;
        self
    }

    /// Apply a full [`LifecycleMarkerConfig`] to this generator.
    ///
    /// This replaces the current marker configuration entirely, allowing callers
    /// to set per-flavor overrides loaded from the TOML config lifecycle_markers.
    pub fn with_marker_config(mut self, config: LifecycleMarkerConfig) -> Self {
        self.marker_config = config;
        self
    }

    /// Override the start script with custom G-code lines.
    ///
    /// When set, these lines are emitted instead of the dialect's built-in
    /// [`GcodeDialect::start_script`] output.
    ///
    /// ```rust
    /// use slicer_engine::gcode::{GcodeGenerator, GcodeFlavor};
    /// use slicer_engine::settings::params::SlicingParams;
    ///
    /// let gen = GcodeGenerator::new(GcodeFlavor::Klipper)
    ///     .with_start_script(vec!["START_PRINT BED_TEMP=65 EXTRUDER_TEMP=215".to_string()]);
    /// let gcode = gen.generate(&[], &SlicingParams::default());
    /// assert!(gcode.contains("BED_TEMP=65"));
    /// ```
    pub fn with_start_script(mut self, script: Vec<String>) -> Self {
        self.custom_start_script = Some(script);
        self
    }

    /// Override the end script with custom G-code lines.
    ///
    /// When set, these lines are emitted instead of the dialect's built-in
    /// [`GcodeDialect::end_script`] output.
    ///
    /// ```rust
    /// use slicer_engine::gcode::{GcodeGenerator, GcodeFlavor};
    /// use slicer_engine::settings::params::SlicingParams;
    ///
    /// let gen = GcodeGenerator::new(GcodeFlavor::Klipper)
    ///     .with_end_script(vec!["MY_END_PRINT".to_string()]);
    /// let gcode = gen.generate(&[], &SlicingParams::default());
    /// assert!(gcode.contains("MY_END_PRINT"));
    /// ```
    pub fn with_end_script(mut self, script: Vec<String>) -> Self {
        self.custom_end_script = Some(script);
        self
    }

    /// Set a custom G-code block emitted at every layer change, right after the
    /// Z move.
    ///
    /// Each line supports the `{z}`, `{height}` and `{layer_num}` (1-based)
    /// placeholders. This is the slicer analogue of PrusaSlicer's
    /// *Before/After layer change G-code* — useful for Klipper macros such as
    /// `_ON_LAYER_CHANGE` (Klippain) or timelapse triggers.
    ///
    /// ```rust
    /// use slicer_engine::gcode::{GcodeGenerator, GcodeFlavor};
    /// use slicer_engine::settings::params::SlicingParams;
    ///
    /// let gen = GcodeGenerator::new(GcodeFlavor::Klipper)
    ///     .with_layer_script(vec!["_ON_LAYER_CHANGE LAYER={layer_num} Z={z}".to_string()]);
    /// ```
    pub fn with_layer_script(mut self, script: Vec<String>) -> Self {
        self.custom_layer_script = Some(script);
        self
    }

    /// Set a per-filament start script, emitted after the machine start script
    /// (and the pressure-advance line) and before the first print move.
    ///
    /// This is the slicer analogue of SuperSlicer/PrusaSlicer's *filament start
    /// G-code*: material-scoped setup (purge lines, per-material pressure
    /// advance, temperature tweaks) that logically belongs to the filament
    /// profile rather than the machine. Each line supports the same
    /// temperature / material placeholders as the start script.
    pub fn with_filament_start_script(mut self, script: Vec<String>) -> Self {
        self.custom_filament_start_script = Some(script);
        self
    }

    /// Set a per-filament end script, emitted after the last print move and
    /// before the machine end script.
    ///
    /// The filament counterpart to [`GcodeGenerator::with_filament_start_script`]
    /// — material-scoped teardown that runs before the machine's own end
    /// sequence. Each line supports the same placeholders as the end script.
    pub fn with_filament_end_script(mut self, script: Vec<String>) -> Self {
        self.custom_filament_end_script = Some(script);
        self
    }

    /// Set the source model name embedded in the metadata header (issue #15).
    ///
    /// Typically the input file's stem (e.g. `benchy` for `benchy.stl`). When
    /// unset the header simply omits the `; model:` line.
    pub fn with_model_name(mut self, name: impl Into<String>) -> Self {
        self.model_name = Some(name.into());
        self
    }

    /// Return a reference to the active dialect.
    pub fn dialect(&self) -> &dyn GcodeDialect {
        self.dialect.as_ref()
    }

    /// Emit a warning through the registered warn function, if any.
    fn warn(&self, msg: &str) {
        if let Some(f) = &self.warn_fn {
            f(msg);
        }
    }

    /// Format an extruder-only move (retract / un-retract / prime), honouring
    /// the absolute-vs-relative E convention.
    ///
    /// `de` is the signed incremental filament length; `e_total` is the running
    /// absolute position *after* applying `de`. In relative mode (`M83`) the
    /// delta is emitted; in absolute mode (`M82`) the running total is.
    fn e_only_line(
        &self,
        de: f64,
        e_total: f64,
        speed_mm_min: f64,
        params: &SlicingParams,
    ) -> String {
        let value = if params.use_relative_e_distances {
            de
        } else {
            e_total
        };
        self.dialect.set_extruder_pos(value, speed_mm_min)
    }

    /// Format an extruding XY move, honouring the absolute-vs-relative E
    /// convention (see [`GcodeGenerator::e_only_line`]).
    fn xy_extrude_line(
        &self,
        x: f64,
        y: f64,
        de: f64,
        e_total: f64,
        speed_mm_min: f64,
        params: &SlicingParams,
    ) -> String {
        let value = if params.use_relative_e_distances {
            de
        } else {
            e_total
        };
        self.dialect.move_extrude(x, y, value, speed_mm_min)
    }

    /// Emit the wipe move: retrace `points` (the previous path's trajectory, in
    /// print order) backward from its end for up to `wipe_distance` mm.
    ///
    /// When `retract_during > 0` the retraction is distributed proportionally
    /// across the wiped length (a combined move-and-retract), smearing ooze onto
    /// already-printed material; when it is `0` the wipe is a pure travel drag
    /// (used with firmware retraction, or when the whole retraction happens
    /// before the wipe). Returns the retraction length actually applied during
    /// the wipe.
    fn emit_wipe(
        &self,
        out: &mut String,
        e_total: &mut f64,
        points: &[(f64, f64)],
        wipe_distance: f64,
        retract_during: f64,
        params: &SlicingParams,
    ) -> f64 {
        if points.len() < 2 || wipe_distance <= 0.0 {
            return 0.0;
        }

        // Length actually available walking backward from the path end.
        let mut avail = 0.0_f64;
        for w in points.windows(2).rev() {
            let (ax, ay) = w[1];
            let (bx, by) = w[0];
            avail += ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt();
            if avail >= wipe_distance {
                break;
            }
        }
        let wipe_len = avail.min(wipe_distance);
        if wipe_len <= 1e-9 {
            return 0.0;
        }

        let extruding = retract_during > 1e-9;
        let e_per_mm = if extruding {
            retract_during / wipe_len
        } else {
            0.0
        };
        // Wipe XY feedrate: the retraction speed is a safe, moderate rate for a
        // combined move-and-retract; a pure-travel wipe uses the travel speed.
        let feed = if extruding {
            params.retract_speed_mm_min.max(1.0)
        } else {
            params.travel_speed_mm_min.max(1.0)
        };

        let mut remaining = wipe_len;
        let mut applied = 0.0_f64;
        let mut idx = points.len() - 1;
        while idx > 0 && remaining > 1e-9 {
            let (ax, ay) = points[idx];
            let (bx, by) = points[idx - 1];
            let dx = bx - ax;
            let dy = by - ay;
            let seg = (dx * dx + dy * dy).sqrt();
            if seg < 1e-9 {
                idx -= 1;
                continue;
            }
            let step = seg.min(remaining);
            let (tx, ty) = if step >= seg {
                (bx, by)
            } else {
                let t = step / seg;
                (ax + t * dx, ay + t * dy)
            };
            if extruding {
                let de = -e_per_mm * step;
                *e_total += de;
                applied += -de;
                out.push_str(&format!(
                    "{} ; wipe\n",
                    self.xy_extrude_line(tx, ty, de, *e_total, feed, params)
                ));
            } else {
                out.push_str(&format!(
                    "{} ; wipe\n",
                    self.dialect.travel_xy(tx, ty, feed)
                ));
            }
            remaining -= step;
            if step >= seg {
                idx -= 1;
            } else {
                break;
            }
        }
        applied
    }

    /// Perform a retraction (if not already retracted), updating `e_total` and
    /// the `retracted` flag.
    ///
    /// Dispatches between firmware retraction (`G10`), a plain extruder-axis
    /// retract, and a wipe-while-retracting sequence, per the retraction
    /// settings. `last_path_points` is the previous path's trajectory used for
    /// the wipe; pass `None` to suppress wiping (e.g. the first path of a layer).
    fn do_retract(
        &self,
        out: &mut String,
        e_total: &mut f64,
        retracted: &mut bool,
        last_path_points: Option<&[(f64, f64)]>,
        params: &SlicingParams,
    ) {
        if *retracted {
            return;
        }

        let wipe_enabled = params.wipe && params.wipe_distance_mm > 0.0;

        if params.use_firmware_retraction {
            // Firmware retraction is atomic — the wipe can only precede it, as a
            // pure-travel drag over already-printed material.
            if wipe_enabled {
                if let Some(pts) = last_path_points {
                    self.emit_wipe(out, e_total, pts, params.wipe_distance_mm, 0.0, params);
                }
            }
            out.push_str(&format!(
                "{} ; firmware retract\n",
                self.dialect.firmware_retract()
            ));
            *retracted = true;
            return;
        }

        let retract_len = params.retract_mm;
        if retract_len <= 0.0 {
            // Nothing to retract; leave `retracted` false so the matching
            // un-retract is also a no-op.
            return;
        }
        let speed = params.retract_speed_mm_min.max(1.0);

        let wipe_pts = if wipe_enabled {
            last_path_points.filter(|p| p.len() >= 2)
        } else {
            None
        };

        if let Some(pts) = wipe_pts {
            let before_frac = params.retract_before_wipe_percent.clamp(0.0, 1.0);
            let pre = retract_len * before_frac;
            let during = retract_len - pre;
            let mut applied = 0.0_f64;
            if pre > 1e-9 {
                *e_total -= pre;
                out.push_str(&format!(
                    "{} ; retract before wipe\n",
                    self.e_only_line(-pre, *e_total, speed, params)
                ));
                applied += pre;
            }
            applied += self.emit_wipe(out, e_total, pts, params.wipe_distance_mm, during, params);
            let remainder = retract_len - applied;
            if remainder > 1e-9 {
                *e_total -= remainder;
                out.push_str(&format!(
                    "{} ; retract after wipe\n",
                    self.e_only_line(-remainder, *e_total, speed, params)
                ));
            }
        } else {
            *e_total -= retract_len;
            out.push_str(&format!(
                "{} ; retract\n",
                self.e_only_line(-retract_len, *e_total, speed, params)
            ));
        }
        *retracted = true;
    }

    /// Recover from a retraction (if currently retracted), updating `e_total`,
    /// the deposited-filament total, and the `retracted` flag.
    ///
    /// Emits `G11` under firmware retraction, otherwise primes the retracted
    /// length plus any configured restart-extra.
    fn do_unretract(
        &self,
        out: &mut String,
        e_total: &mut f64,
        retracted: &mut bool,
        total_filament_mm: &mut f64,
        params: &SlicingParams,
    ) {
        if !*retracted {
            return;
        }
        if params.use_firmware_retraction {
            out.push_str(&format!(
                "{} ; firmware recover\n",
                self.dialect.firmware_unretract()
            ));
            *retracted = false;
            return;
        }
        let speed = params.retract_speed_mm_min.max(1.0);
        let restart = params.retract_restart_extra_mm.max(0.0);
        let de = params.retract_mm + restart;
        *e_total += de;
        *total_filament_mm += restart;
        out.push_str(&format!(
            "{} ; un-retract\n",
            self.e_only_line(de, *e_total, speed, params)
        ));
        *retracted = false;
    }

    /// Resolve the effective print speed (in mm/min) for a given extrusion role
    /// and layer context.
    ///
    /// The priority order is:
    /// 1. First-layer speed (when `is_first_layer` is true)
    /// 2. **Dynamic overhang speed** (when `enable_overhang_speed` and the path
    ///    carries an overhang [`OverhangClass`]): the per-degree
    ///    `overhang_1_4_speed`…`overhang_4_4_speed` override, or the role's
    ///    normal speed when that degree is left at `0`.
    /// 3. Role-specific speed (perimeter, infill, bridge, top/bottom surface)
    /// 4. General `print_speed` fallback when a role-specific speed is ≤ 0
    fn effective_speed_mm_min(
        role: crate::core::ExtrusionRole,
        overhang: crate::core::OverhangClass,
        is_first_layer: bool,
        params: &SlicingParams,
    ) -> f64 {
        use crate::core::ExtrusionRole;
        let fallback = params.print_speed * 60.0;
        if is_first_layer {
            let s = params.first_layer_speed;
            return if s > 0.0 { s * 60.0 } else { fallback };
        }
        // Base role speed (also the fallback for an un-configured overhang
        // degree — perimeter speed for the mild Deg1/Deg2 walls, bridge speed
        // for the steep Deg3/Deg4 walls, which carry the OverhangPerimeter role).
        let base = match role {
            ExtrusionRole::OuterWall | ExtrusionRole::InnerWall => {
                let s = params.perimeter_speed;
                if s > 0.0 {
                    s * 60.0
                } else {
                    fallback
                }
            }
            ExtrusionRole::Infill => {
                let s = params.infill_speed;
                if s > 0.0 {
                    s * 60.0
                } else {
                    fallback
                }
            }
            ExtrusionRole::Bridge | ExtrusionRole::OverhangPerimeter => {
                let s = params.bridge_speed;
                if s > 0.0 {
                    s * 60.0
                } else {
                    fallback
                }
            }
            ExtrusionRole::TopSurface
            | ExtrusionRole::BottomSurface
            | ExtrusionRole::InternalSolid => {
                let s = params.top_surface_speed;
                if s > 0.0 {
                    s * 60.0
                } else {
                    fallback
                }
            }
            ExtrusionRole::Ironing => {
                // Slow is the point: the nozzle needs dwell time to re-melt the
                // surface it passes over. Falls back to the surface it follows.
                let s = if params.ironing_speed > 0.0 {
                    params.ironing_speed
                } else {
                    params.top_surface_speed
                };
                if s > 0.0 {
                    s * 60.0
                } else {
                    fallback
                }
            }
            ExtrusionRole::GapFill => {
                // Gap fill is a wall-family feature: fall back to perimeter
                // speed, then print speed.
                let s = if params.gap_fill_speed > 0.0 {
                    params.gap_fill_speed
                } else {
                    params.perimeter_speed
                };
                if s > 0.0 {
                    s * 60.0
                } else {
                    fallback
                }
            }
            _ => fallback,
        };

        // Dynamic overhang speed override.
        if params.enable_overhang_speed && overhang.is_overhang() {
            let cfg = match overhang {
                crate::core::OverhangClass::Deg1 => params.overhang_1_4_speed,
                crate::core::OverhangClass::Deg2 => params.overhang_2_4_speed,
                crate::core::OverhangClass::Deg3 => params.overhang_3_4_speed,
                crate::core::OverhangClass::Deg4 => params.overhang_4_4_speed,
                crate::core::OverhangClass::None => 0.0,
            };
            // `0` = keep the role's normal speed for this degree.
            let mut s = if cfg > 0.0 { cfg * 60.0 } else { base };
            // Slow curl-prone steep overhangs (Deg3/Deg4) to the most
            // conservative configured overhang speed.
            if params.slowdown_for_curled_perimeters && overhang.band() >= 3 {
                if let Some(slowest) = slowest_overhang_speed_mm_s(params) {
                    s = s.min(slowest * 60.0);
                }
            }
            return s;
        }

        base
    }

    /// Resolve the target acceleration (mm/s²) for a path, or `None` when
    /// acceleration control is disabled for it.
    ///
    /// Precedence (first match wins; each falls back to `acceleration`):
    /// 1. **First layer** → `first_layer_acceleration`, applied to every role
    ///    for adhesion.
    /// 2. **Bridge / overhang** → `bridge_acceleration` (Phase 3 — geometry
    ///    aware): strands printed into air get a low, steady acceleration.
    /// 3. **Top surface** → `top_surface_acceleration`.
    /// 4. **Outer wall** → `outer_wall_acceleration` (Phase 3): the visible
    ///    perimeter gets a dedicated limit to reduce ringing.
    /// 5. Everything else → `acceleration`.
    ///
    /// A resolved value of `0` (nothing configured) yields `None`, so no
    /// firmware command is emitted and existing output is unchanged.
    fn effective_acceleration(
        role: crate::core::ExtrusionRole,
        is_first_layer: bool,
        params: &SlicingParams,
    ) -> Option<f64> {
        use crate::core::ExtrusionRole;
        let normal = params.acceleration;
        // Pick a role-specific override, falling back to the normal value when
        // the override is unset (`0`).
        let or_normal = |override_val: f64| {
            if override_val > 0.0 {
                override_val
            } else {
                normal
            }
        };
        if is_first_layer {
            let a = or_normal(params.first_layer_acceleration);
            return (a > 0.0).then_some(a);
        }
        let a = match role {
            ExtrusionRole::Bridge | ExtrusionRole::OverhangPerimeter => {
                or_normal(params.bridge_acceleration)
            }
            ExtrusionRole::TopSurface => or_normal(params.top_surface_acceleration),
            // Ironing follows the surface it is smoothing.
            ExtrusionRole::Ironing => or_normal(params.top_surface_acceleration),
            ExtrusionRole::OuterWall => or_normal(params.outer_wall_acceleration),
            ExtrusionRole::InnerWall => or_normal(params.inner_wall_acceleration),
            ExtrusionRole::Infill => or_normal(params.sparse_infill_acceleration),
            // Internal solid layers and bottom surfaces share the solid-infill
            // limit (distinct from the visible top surface above).
            ExtrusionRole::BottomSurface | ExtrusionRole::InternalSolid => {
                or_normal(params.solid_infill_acceleration)
            }
            ExtrusionRole::GapFill => or_normal(params.gap_fill_acceleration),
            ExtrusionRole::Support => or_normal(params.support_acceleration),
            _ => normal,
        };
        (a > 0.0).then_some(a)
    }

    /// Resolve the target acceleration (mm/s²) for **travel** (non-printing)
    /// moves, or `None` when no dedicated travel acceleration is configured.
    ///
    /// Travel is not a printing role, so — unlike [`Self::effective_acceleration`]
    /// — it is a single global value that does not vary by role or layer. When it
    /// is unset (`0`) the caller leaves the printing acceleration in force, so
    /// output stays byte-identical to a profile that never touched it.
    fn effective_travel_acceleration(params: &SlicingParams) -> Option<f64> {
        (params.travel_acceleration > 0.0).then_some(params.travel_acceleration)
    }

    /// Emit one spiralized (vase-mode) outer contour with a continuous Z ramp.
    ///
    /// `pts` is the closed loop rotated so `pts[0]` is the start vertex. The
    /// nozzle is assumed to already be at `pts[0]` (in XY) and at height
    /// `z_bottom`. The loop is walked once — `pts[0] → pts[1] → … → pts[n-1] →
    /// pts[0]` — while Z ramps linearly from `z_bottom` to `z_top` in proportion
    /// to the distance travelled, so one full perimeter climbs exactly one layer
    /// height and no discrete Z step or seam remains.
    ///
    /// The per-move extrusion is scaled by a flow factor that ramps linearly
    /// from `flow_start` (at the seam) to `flow_end` (back at the seam), used to
    /// fade the very first spiral loop in and the very last one out.
    #[allow(clippy::too_many_arguments)]
    fn emit_spiral_loop(
        &self,
        out: &mut String,
        pts: &[(f64, f64)],
        z_bottom: f64,
        z_top: f64,
        width_mm: f64,
        speed_mm_min: f64,
        flow_start: f64,
        flow_end: f64,
        params: &SlicingParams,
        e_total: &mut f64,
        total_filament_mm: &mut f64,
    ) {
        let n = pts.len();
        if n < 2 {
            return;
        }

        // Total loop length including the closing segment back to pts[0].
        let mut total_len = 0.0_f64;
        for i in 0..n {
            let (ax, ay) = pts[i];
            let (bx, by) = pts[(i + 1) % n];
            total_len += ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt();
        }
        if total_len < 1e-9 {
            return;
        }

        let dz = z_top - z_bottom;
        let capped_speed = volumetric_capped_speed_mm_min(
            speed_mm_min,
            params.layer_height,
            width_mm,
            params.max_volumetric_speed,
        );

        let mut dist = 0.0_f64;
        for i in 0..n {
            let (ax, ay) = pts[i];
            let (bx, by) = pts[(i + 1) % n];
            let seg = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt();
            if seg < 1e-9 {
                continue;
            }
            dist += seg;
            let t = (dist / total_len).clamp(0.0, 1.0);
            let z = z_bottom + dz * t;
            let flow_scale = flow_start + (flow_end - flow_start) * t;
            // Apply the flow ramp *after* the flow-ratio correction: passing a
            // zero flow ratio into `extrusion_for_move` would trip its
            // "non-positive ratio → 1.0" safety guard, so the fade-out would
            // silently become full flow.
            let de = extrusion_for_move(
                seg,
                params.layer_height,
                width_mm,
                params.filament_diameter_mm,
                params.flow_ratio,
            ) * flow_scale;
            *e_total += de;
            *total_filament_mm += de;
            out.push_str(&format!(
                "{}\n",
                self.dialect
                    .move_extrude_z(bx, by, z, *e_total, capped_speed)
            ));
        }
    }

    /// Generate a complete G-code program from the given layers and parameters.
    ///
    /// The output is a single `String` with lines separated by `'\n'`.
    /// Returns a minimal (header + start + end) program when `layers` is empty.
    ///
    /// If any commands are listed in [`GcodeDialect::unsupported_commands`] the
    /// registered warn function is called once per unsupported command before
    /// generation begins.
    pub fn generate(&self, layers: &[SliceLayer], params: &SlicingParams) -> String {
        self.generate_with_stats(layers, params).0
    }

    /// Like [`GcodeGenerator::generate`], but also returns the aggregate
    /// [`SliceStatistics`] (layer count, filament usage, print-time estimate,
    /// bounding box) that is embedded in the metadata header (issue #15).
    ///
    /// The header is assembled *after* the body so its filament and print-time
    /// figures exactly match the emitted moves.
    pub fn generate_with_stats(
        &self,
        layers: &[SliceLayer],
        params: &SlicingParams,
    ) -> (String, SliceStatistics) {
        // Spiral (vase) mode forces the same single-wall configuration the
        // slicing pipeline used, so the header stats and emitted moves agree.
        let normalized = params.spiral_vase_normalized();
        let params = normalized.as_ref();

        // Warn about any commands the dialect doesn't natively support
        for cmd in self.dialect.unsupported_commands() {
            self.warn(&format!(
                "Command '{}' is not natively supported by the {} dialect; \
                 falling back to generic G-code",
                cmd,
                self.dialect.flavor_name()
            ));
        }

        // ── Print-object runs ─────────────────────────────────────────────────
        // Who owns each layer, and where that layer sits inside its owner's own
        // stack. In `by_layer` order there is one run and the two indices are
        // the same; sequential printing concatenates one stack per object, so
        // anything that counts "layers so far" — the spiral base, the flow
        // fade-in/out — has to count *within* the run or it only ever applies
        // to the first object.
        let sequential = !self.objects.is_empty()
            && params.print_sequence == crate::settings::params::PrintSequence::ByObject;
        let mut layer_owners: Vec<Option<usize>> = layers
            .iter()
            .map(|layer| (0..layer.paths.len()).find_map(|i| layer.object_for_path(i)))
            .collect();
        if sequential {
            // A layer with no paths carries no tag, so it would slip past the
            // hand-over — and its Z move would then descend to the *incoming*
            // object's first layer while the nozzle is still parked over the
            // one just finished. Attribute such a layer to the next object that
            // does have geometry (falling back to the previous one at the end
            // of the plate), so the hand-over always fires before the descent.
            // `slice_plate` already filters these out; this keeps a
            // hand-assembled plate safe too.
            let mut next: Option<usize> = None;
            for i in (0..layer_owners.len()).rev() {
                match layer_owners[i] {
                    Some(owner) => next = Some(owner),
                    None => layer_owners[i] = next,
                }
            }
            let mut previous: Option<usize> = None;
            for owner in layer_owners.iter_mut() {
                match owner {
                    Some(o) => previous = Some(*o),
                    None => *owner = previous,
                }
            }
        }
        let layer_index_in_run: Vec<usize> = if sequential {
            let mut indices = Vec::with_capacity(layers.len());
            let mut run_owner: Option<usize> = None;
            let mut index_in_run = 0usize;
            for (i, owner) in layer_owners.iter().enumerate() {
                // An untagged layer (bed adhesion only) continues the current
                // run rather than starting a new one.
                if let Some(owner) = owner {
                    if i == 0 || run_owner != Some(*owner) {
                        run_owner = Some(*owner);
                        index_in_run = 0;
                    } else {
                        index_in_run += 1;
                    }
                } else if i > 0 {
                    index_in_run += 1;
                }
                indices.push(index_in_run);
            }
            indices
        } else {
            (0..layers.len()).collect()
        };

        // ── Spiral (vase) mode plan ───────────────────────────────────────────
        // Spiralizable layers are those at or above `spiral_start_layer` that
        // expose exactly one closed outer-wall island. Layers below stay flat
        // (the solid base); layer 0 is always flat since a spiral cannot climb
        // from Z=0. Multi-island layers fall back to normal printing with a
        // single warning.
        let spiral_start_layer = params.bottom_layers.max(1);
        let spiral_path_indices: Vec<Option<usize>> = if params.spiral_vase {
            let mut warned = false;
            layers
                .iter()
                .enumerate()
                .map(|(i, layer)| {
                    if layer_index_in_run[i] < spiral_start_layer {
                        return None;
                    }
                    match detect_spiral_loop(layer) {
                        SpiralDetect::Single(idx) => Some(idx),
                        SpiralDetect::Multi => {
                            if !warned {
                                self.warn(
                                    "spiral vase mode: a layer has multiple islands — printing \
                                     those layers normally (spiral vase works on single-island \
                                     solid models)",
                                );
                                warned = true;
                            }
                            None
                        }
                        SpiralDetect::None => None,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        // First / last spiral layer **of each object**, so every vase on a
        // sequential plate fades its seam in and out — not just the first.
        let mut first_spiral_of_run: Vec<bool> = vec![false; spiral_path_indices.len()];
        let mut last_spiral_of_run: Vec<bool> = vec![false; spiral_path_indices.len()];
        {
            let mut seen: std::collections::HashMap<Option<usize>, (usize, usize)> =
                std::collections::HashMap::new();
            for (i, spiral) in spiral_path_indices.iter().enumerate() {
                if spiral.is_none() {
                    continue;
                }
                let owner = if sequential { layer_owners[i] } else { None };
                seen.entry(owner).and_modify(|e| e.1 = i).or_insert((i, i));
            }
            for (first, last) in seen.into_values() {
                first_spiral_of_run[first] = true;
                last_spiral_of_run[last] = true;
            }
        }

        // The metadata header is prepended once the filament total and geometry
        // are known (see the end of this method); the body starts here.
        let mut out = String::with_capacity(64 * 1024);

        // ── Object definitions (issue #22) ────────────────────────────────────
        // Declared before the start script: Klipper's `[exclude_object]` module
        // and Moonraker both expect to meet every object before the print
        // begins, so a front-end can list the plate's parts from the moment the
        // file is loaded rather than discovering them as they are reached.
        //
        // Markers are gated on `exclude_object` alone: sequential printing also
        // needs the plate segmented, but a user who only asked for one part at
        // a time did not ask for firmware object tracking.
        let object_markers = !self.objects.is_empty() && params.exclude_object;
        if object_markers {
            for line in self.dialect.object_definitions(&self.objects) {
                out.push_str(&line);
                out.push('\n');
            }
        }

        // ── Start script (custom override or flavor default) ──────────────────
        let start_script: Cow<[String]> = match &self.custom_start_script {
            Some(lines) => Cow::Borrowed(lines),
            None => Cow::Owned(self.dialect.start_script(params)),
        };

        // First-layer temperature targets ("0 = inherit the base value").
        let first_nozzle = if params.nozzle_temp_first_layer > 0.0 {
            params.nozzle_temp_first_layer
        } else {
            params.nozzle_temp
        };
        let first_bed = if params.bed_temp_first_layer > 0.0 {
            params.bed_temp_first_layer
        } else {
            params.bed_temp
        };

        // ── Chamber heating (opt-in via the printer's `heated_chamber`) ───────
        // The whole block runs *before* the start script, and the ordering is
        // load-bearing:
        //
        //   1. Set the bed target, without waiting. On the great majority of
        //      enclosed printers the **bed is the chamber's heat source**, so a
        //      chamber soak with a cold bed would never terminate.
        //   2. Set the chamber target (Klipper's `TEMPERATURE_WAIT` only waits —
        //      it does not set — so the target must be armed separately).
        //   3. Block until the chamber is soaked.
        //
        // The soak therefore overlaps the long pole of the warm-up (a 100–110 °C
        // bed) while the **nozzle is still cold** — waiting after the start
        // script would park molten filament in the hot end for the length of the
        // soak, oozing and degrading exactly the PC/Nylon/ABS materials this
        // feature exists for. Only the comparatively quick nozzle heat, which
        // the start script owns, is left running after the wait.
        //
        // A start script that manages the chamber itself owns the whole job —
        // emitting our sequence as well would heat and soak twice.
        let chamber_delegated =
            params.chamber_heating_active() && start_script_handles_chamber(&start_script);
        let emit_chamber = params.chamber_heating_active() && !chamber_delegated;
        let chamber_first_target = params.chamber_temp_first_layer_resolved();
        if chamber_delegated {
            out.push_str(&self.dialect.comment(
                "chamber temperature handled by the custom start G-code; \
                 slicer chamber directives suppressed",
            ));
            out.push('\n');
        }
        if emit_chamber {
            out.push_str(&format!(
                "{} ; bed target — the chamber's heat source on most enclosures\n",
                self.dialect.set_bed_temp(first_bed, false)
            ));
            out.push_str(&format!(
                "{} ; set chamber temperature\n",
                self.dialect.set_chamber_temp(chamber_first_target, false)
            ));
            out.push_str(&format!(
                "{} ; soak the chamber before the nozzle is heated\n",
                self.dialect.set_chamber_temp(chamber_first_target, true)
            ));
        }

        for line in start_script.iter() {
            out.push_str(&render_script_placeholders(line, params));
            out.push('\n');
        }

        // The generator tracks a running extruder position `e_total` and, by
        // default, emits **absolute** E positions (`M82`, `G92 E0` per layer).
        // A custom start script or a Klipper `START_PRINT` macro that primes /
        // uses firmware retraction can leave the extruder in an unexpected mode,
        // so the required mode is forced here to guarantee the invariant:
        //   - absolute (`M82`) — every `G1 … E<e_total>` is an absolute target;
        //   - relative (`M83`, when `use_relative_e_distances`) — every move
        //     carries its incremental filament length instead.
        if params.use_relative_e_distances {
            out.push_str(&format!("{}\n", self.dialect.extruder_relative_mode()));
        } else {
            out.push_str(&format!("{}\n", self.dialect.extruder_absolute_mode()));
        }
        out.push_str(&format!("{}\n", self.dialect.reset_extruder()));

        // ── Firmware retraction setup (opt-in) ────────────────────────────────
        // Sync the firmware's retraction length / speed / restart-extra to the
        // slicer settings so the `G10`/`G11` moves emitted below use them. The
        // Z-hop component is left to the slicer's explicit Z moves so behaviour
        // matches software retraction.
        if params.use_firmware_retraction {
            for line in self.dialect.firmware_retract_setup(
                params.retract_mm,
                params.retract_speed_mm_min,
                params.retract_restart_extra_mm,
            ) {
                out.push_str(&line);
                out.push('\n');
            }
        }

        // ── Pressure / linear advance (opt-in; `0` disables) ──────────────────
        // Emitted once, right after the start script, so it survives custom
        // `START_PRINT` macros and firmware retraction that may otherwise leave
        // the value at the printer's default.  The active dialect renders the
        // firmware-correct form (Klipper `SET_PRESSURE_ADVANCE`, Marlin
        // `M900 K`).
        if params.pressure_advance > 0.0 {
            out.push_str(&format!(
                "{} ; pressure advance\n",
                self.dialect.set_pressure_advance(params.pressure_advance)
            ));
        }

        // ── Machine kinematic limits (opt-in; each `0` leaves the default) ────
        // Emitted once, right after pressure advance, so the firmware corners
        // (square-corner velocity → junction deviation) and caps velocity
        // exactly as the acceleration-aware print-time estimate assumes. The
        // conversion needs an acceleration; use the normal target (falling back
        // to the estimator default when acceleration control is off).
        let kinematic_accel = if params.acceleration > 0.0 {
            params.acceleration
        } else {
            crate::gcode::time_estimate::DEFAULT_ACCELERATION_MM_S2
        };
        for line in self.dialect.set_kinematic_limits(
            params.square_corner_velocity,
            params.max_velocity,
            kinematic_accel,
        ) {
            out.push_str(&line);
            out.push('\n');
        }

        // ── Per-filament start script ─────────────────────────────────────────
        // Emitted after the machine start script (and pressure-advance / kinematic
        // setup) but before the first print move, so material-scoped setup (purge,
        // per-material PA / temperature tweaks) contributed by the filament profile
        // runs last before printing begins.
        if let Some(lines) = &self.custom_filament_start_script {
            for line in lines {
                out.push_str(&render_script_placeholders(line, params));
                out.push('\n');
            }
        }

        // ── Per-layer contours ────────────────────────────────────────────────
        let mut e_total = 0.0_f64;
        // Grand total of deposited filament (mm of feedstock) across every
        // layer — accumulated alongside `e_total` at each extrusion move and
        // used to compute filament weight/volume for the metadata header.
        let mut total_filament_mm = 0.0_f64;
        // Whether the extruder is currently retracted. Normally toggled back to
        // `false` after every travel, but `retract_on_layer_change` leaves it
        // set across the layer-change Z move.
        let mut retracted = false;
        // Trajectory (print-order XY) of the previous path, captured only when
        // wiping is enabled, so a retract can retrace it. Reset at each layer so
        // the first path never wipes across the layer-change Z move.
        let mut last_path_points: Option<Vec<(f64, f64)>> = None;
        // Track previous fan speed per config index for rate limiting (aux overrides).
        let mut prev_fan_speeds: Vec<Option<f64>> = vec![None; params.fan_configs.len()];
        // Track the last emitted acceleration so we only emit a firmware
        // command when the target changes (persists across layers).
        let mut last_accel: Option<f64> = None;
        // Persistent nozzle XY across layers — used by spiral mode to travel
        // into each spiral loop from the true previous position and to keep the
        // start line aligned. Updated at the end of every layer.
        let mut cur_xy: Option<(f64, f64)> = None;
        // ── Object attribution state (issues #22 / #112) ──────────────────────
        // The object whose marker block is currently open, the set already
        // introduced to the firmware (so `M486 A"name"` is spent once), and the
        // highest Z printed so far — the height the nozzle must clear when
        // sequential printing moves on to the next object.
        let mut current_object: Option<usize> = None;
        // Which object the *nozzle* is working on. Distinct from
        // `current_object` (which marker block is open) because a layer with no
        // paths never reaches the per-path attribution: tracking the hand-over
        // on the block state would re-fire it on every such layer.
        let mut handover_object: Option<usize> = layer_owners.first().copied().flatten();
        let mut introduced_objects: Vec<bool> = vec![false; self.objects.len()];
        let mut max_printed_z = 0.0_f64;
        let between_objects = gcode_block_lines(params.between_objects_gcode.as_deref());

        // If a custom start script heats for first layer (e.g.
        // `{nozzle_temp_first_layer}` / `{bed_temp_first_layer}`), restore the
        // normal temperatures when layer 2 starts unless a custom layer script
        // chooses to override that behavior afterward. `first_nozzle` /
        // `first_bed` were resolved above, where the chamber pre-heat needs the
        // bed target.
        let restore_nozzle_after_first_layer = params.nozzle_temp_first_layer > 0.0
            && (first_nozzle - params.nozzle_temp).abs() > 1e-9;
        let restore_bed_after_first_layer =
            params.bed_temp_first_layer > 0.0 && (first_bed - params.bed_temp).abs() > 1e-9;
        // The chamber soaked at `chamber_temp_first_layer`; drop it back to the
        // steady-state target once the first layer is down. Never blocks — the
        // chamber is already warm and the print must not stall mid-way.
        let restore_chamber_after_first_layer = emit_chamber
            && params.chamber_temp_first_layer > 0.0
            && (chamber_first_target - params.chamber_temp).abs() > 1e-9;

        for (layer_index, layer) in layers.iter().enumerate() {
            // ── Sequential printing: hand over to the next object (#112) ─────
            // Everything here has to happen *before* the layer's own Z move,
            // which drops the nozzle to the new object's first layer: doing it
            // afterwards would lower into the part just finished.  So: close
            // the outgoing marker block, retract, lift clear of the tallest
            // thing on the bed, then travel across.
            let layer_object = layer_owners[layer_index];
            if sequential
                && layer_index > 0
                && layer_object.is_some()
                && layer_object != handover_object
            {
                handover_object = layer_object;
                if object_markers {
                    if let Some(previous) = current_object.and_then(|i| self.objects.get(i)) {
                        out.push_str(&self.dialect.object_end(previous));
                        out.push('\n');
                    }
                }
                current_object = None;

                self.do_retract(
                    &mut out,
                    &mut e_total,
                    &mut retracted,
                    last_path_points.as_deref(),
                    params,
                );
                // `max_printed_z` tracks the model Z of the tallest finished
                // part, so the lift is computed in model space and converted
                // once — the part it has to clear physically sits at its own
                // machine Z too.
                let clearance_z = machine_z(
                    max_printed_z + params.z_hop_mm.max(SEQUENTIAL_LIFT_MM),
                    params,
                );
                out.push_str(&format!(
                    "{} ; clear the finished object\n",
                    self.dialect.move_z(clearance_z, params.travel_speed_mm_min)
                ));
                // Aim at the incoming object's first path, falling back to its
                // centre: an empty first layer would otherwise leave the nozzle
                // parked over the part just finished when the layer block below
                // drops back down to Z.
                let entry = layer
                    .paths
                    .iter()
                    .next()
                    .and_then(|p| p.iter().next())
                    .map(|p| (p.x(), p.y()))
                    .or_else(|| {
                        layer_object
                            .and_then(|i| self.objects.get(i))
                            .map(|object| object.center)
                    });
                if let Some((ex, ey)) = entry {
                    out.push_str(&format!(
                        "{} ; travel to the next object\n",
                        self.dialect.travel_xy(ex, ey, params.travel_speed_mm_min)
                    ));
                    cur_xy = Some((ex, ey));
                }
                if let Some(lines) = &between_objects {
                    for line in lines {
                        out.push_str(&render_script_placeholders(line, params));
                        out.push('\n');
                    }
                }
            }

            // ── Retract on layer change (opt-in) ─────────────────────────────
            // Retract *before* the layer-change Z move so the nozzle does not
            // ooze while lifting and travelling to the next layer's first path.
            // The wipe (when enabled) retraces the previous layer's last path,
            // which is correct because the Z move has not happened yet.
            if layer_index > 0 && params.retract_on_layer_change {
                self.do_retract(
                    &mut out,
                    &mut e_total,
                    &mut retracted,
                    last_path_points.as_deref(),
                    params,
                );
            }
            // A fresh layer starts a new set of paths: never wipe the first path
            // against the previous layer's geometry (that would drag at the new,
            // higher Z over material the nozzle is no longer touching).
            last_path_points = None;

            // Emitted (machine) Z carries the `z_offset_mm` compensation; the
            // model Z does not.  Lifecycle markers describe where the nozzle
            // actually is, so they use the machine Z (as PrusaSlicer does),
            // while the custom layer-change script sees the model Z — its
            // `{z}` is the analogue of PrusaSlicer's offset-free `layer_z`
            // placeholder, and a macro reasoning about the object must not be
            // handed an endstop correction.
            let z_str = format!("{:.3}", machine_z(layer.z, params));
            let model_z_str = format!("{:.3}", layer.z);
            // The height this layer *opens* at. A first layer of its own
            // thickness (or a combined-infill bead) carries a per-path override,
            // and announcing the global height here instead would have a viewer
            // size the opening beads wrong until the first re-announcement.
            let opening_height = layer
                .height_for_path(0)
                .filter(|h| *h > 0.0)
                .unwrap_or(params.layer_height);
            let height_str = format!("{:.3}", opening_height);
            // Last `;HEIGHT:` announced on this layer, so a taller combined-infill
            // bead re-announces it and the next ordinary path announces it back.
            let mut last_height: Option<String> = Some(height_str.clone());
            // The bed-contact layer. Compared by Z rather than by index so a
            // caller may generate a slab of mid-print layers on its own, and
            // bounded by the first layer's *own* thickness — which may be
            // thinner than the rest, in which case a `layer_height` bound would
            // sweep the second layer in with it.
            let is_first_layer = layer.z <= crate::core::resolved_first_layer_height(params) + 1e-6;

            // Per-layer travel router (opt-in).  Built once per layer so travel
            // hops can detour around outer walls instead of scarring the surface.
            let travel_planner = if params.avoid_crossing_perimeters {
                crate::gcode::travel::TravelPlanner::for_layer(layer)
            } else {
                None
            };

            // Spiral (vase) layer? If so, the single outer contour is emitted
            // with a continuous Z ramp and the usual discrete Z move is skipped
            // (the nozzle is already at the previous layer's top Z).
            let spiral_path_idx = spiral_path_indices.get(layer_index).copied().flatten();
            let is_spiral_layer = spiral_path_idx.is_some();

            if self.marker_config.enabled {
                // Lifecycle block: LAYER_CHANGE → BEFORE_LAYER_CHANGE → Z move → AFTER_LAYER_CHANGE
                let layer_change = self
                    .marker_config
                    .layer_change
                    .as_deref()
                    .unwrap_or(";LAYER_CHANGE");
                out.push_str(&render_marker(layer_change, &z_str, &height_str, "", ""));
                out.push('\n');

                let z_marker = self.marker_config.z_marker.as_deref().unwrap_or(";Z:{z}");
                out.push_str(&render_marker(z_marker, &z_str, &height_str, "", ""));
                out.push('\n');

                let height_marker = self
                    .marker_config
                    .height_marker
                    .as_deref()
                    .unwrap_or(";HEIGHT:{height}");
                out.push_str(&render_marker(height_marker, &z_str, &height_str, "", ""));
                out.push('\n');

                // Per-layer print-time estimate for the viewer's "Layer Time" mode.
                out.push_str(&format!(
                    ";LAYER_TIME:{:.1}\n",
                    estimate_layer_time(layer, params.print_speed)
                ));

                let before_lc = self
                    .marker_config
                    .before_layer_change
                    .as_deref()
                    .unwrap_or(";BEFORE_LAYER_CHANGE");
                out.push_str(&render_marker(before_lc, &z_str, &height_str, "", ""));
                out.push('\n');

                // Bare Z-value comment (`;0.200`) matches the OrcaSlicer / PrusaSlicer lifecycle
                // format; it intentionally differs from the `;Z:` label above and is used by
                // post-processing scripts that parse standalone numeric layer markers.
                out.push_str(&format!(";{}\n", z_str));

                // Reset extruder position at layer start
                out.push_str(&format!("{}\n", self.dialect.reset_extruder()));
                e_total = 0.0;

                // Spiral layers ramp Z along the perimeter, so no discrete Z
                // move here — the nozzle is already at the previous layer's top.
                if !is_spiral_layer {
                    out.push_str(&format!(
                        "{}\n",
                        self.dialect
                            .move_z(machine_z(layer.z, params), params.travel_speed_mm_min)
                    ));
                }

                let after_lc = self
                    .marker_config
                    .after_layer_change
                    .as_deref()
                    .unwrap_or(";AFTER_LAYER_CHANGE");
                out.push_str(&render_marker(after_lc, &z_str, &height_str, "", ""));
                out.push('\n');

                // Same bare Z-value convention after the layer change (see note above).
                out.push_str(&format!(";{}\n", z_str));
            } else {
                out.push_str(&format!("; layer z={}\n", z_str));
                if !is_spiral_layer {
                    out.push_str(&format!(
                        "{}\n",
                        self.dialect
                            .move_z(machine_z(layer.z, params), params.travel_speed_mm_min)
                    ));
                }
            }

            if layer_index == 1 {
                if restore_nozzle_after_first_layer {
                    out.push_str(&format!(
                        "{} ; restore normal nozzle temperature\n",
                        self.dialect.set_nozzle_temp(params.nozzle_temp, false)
                    ));
                }
                if restore_bed_after_first_layer {
                    out.push_str(&format!(
                        "{} ; restore normal bed temperature\n",
                        self.dialect.set_bed_temp(params.bed_temp, false)
                    ));
                }
                if restore_chamber_after_first_layer {
                    out.push_str(&format!(
                        "{} ; restore normal chamber temperature\n",
                        self.dialect.set_chamber_temp(params.chamber_temp, false)
                    ));
                }
            }

            // ── Custom layer-change G-code (Klipper macros, timelapse, …) ─────
            if let Some(script) = &self.custom_layer_script {
                let layer_num = (layer_index + 1).to_string();
                for line in script {
                    // `{z}` here is the *model* layer Z, free of the machine
                    // Z-offset (PrusaSlicer's `layer_z` placeholder semantics).
                    let rendered = render_marker(line, &model_z_str, &height_str, "", "")
                        .replace("{layer_num}", &layer_num);
                    out.push_str(&render_script_placeholders(&rendered, params));
                    out.push('\n');
                }
            }

            // ── Adaptive fan speed ───────────────────────────────────────────
            // The part-cooling fan's emitted base speed, captured for the
            // dynamic fan override so it can restore normal cooling when
            // leaving a bridge or overhang region.
            let mut part_cooling_base: Option<f64> = None;
            let mut part_cooling_klipper_name: Option<String> = None;
            if !params.fan_configs.is_empty() {
                let layer_time = estimate_layer_time(layer, params.print_speed);
                // Bridge detection: any path tagged Bridge or OverhangPerimeter
                // triggers bridge boost on aux fans (overhanging walls cool just
                // like bridge infill does).
                let has_bridges = layer.paths.iter().enumerate().any(|(i, _)| {
                    matches!(
                        layer.role_for_path(i),
                        crate::core::ExtrusionRole::Bridge
                            | crate::core::ExtrusionRole::OverhangPerimeter
                    )
                });

                for (fan_idx, fan) in params.fan_configs.iter().enumerate() {
                    let prev = prev_fan_speeds.get(fan_idx).copied().flatten();
                    let adaptive = fan.compute_speed(layer_time, has_bridges, prev);
                    // The filament-owned cooling policy (first-layer pinning and
                    // the `fan_speed` material ceiling) governs the part-cooling
                    // fan only; hotend / chamber / aux fans stay on the raw
                    // `fan_configs` curve plus their own aux overrides.
                    let speed = if fan.fan_index == fan_index::PART_COOLING {
                        params.part_cooling_speed(layer_index, adaptive)
                    } else {
                        adaptive
                    };
                    // Store what was actually emitted, which is what
                    // `max_speed_change_per_layer` rate-limits against: coming off
                    // a pinned first layer, the fan ramps up instead of slamming
                    // from off to full.
                    if let Some(slot) = prev_fan_speeds.get_mut(fan_idx) {
                        *slot = Some(speed);
                    }
                    if fan.fan_index == fan_index::PART_COOLING && part_cooling_base.is_none() {
                        part_cooling_base = Some(speed);
                        part_cooling_klipper_name = fan.klipper_name.clone();
                    }
                    out.push_str(&format!(
                        "{}\n",
                        self.dialect.set_fan_speed_indexed(
                            fan.fan_index,
                            fan.klipper_name.as_deref(),
                            speed
                        )
                    ));
                }
            }

            // ── Dynamic (per-segment) part-cooling override ──────────────────
            // Bridges and steep overhangs lay material over air and want a burst
            // of extra airflow for the length of those segments only.  The
            // override needs a part-cooling fan to drive and at least one of the
            // two triggers configured; it is suppressed entirely on the layers
            // where `disable_fan_first_layers` pins the fan, because a single
            // overhang there would otherwise defeat the adhesion gate.
            let dynamic_fan_enabled = !params.part_cooling_pinned(layer_index)
                && (params.bridge_fan_speed > 0.0
                    || (params.enable_overhang_speed && params.overhang_fan_speed > 0.0));
            let dynamic_base_fan: Option<f64> = if dynamic_fan_enabled {
                part_cooling_base
            } else {
                None
            };
            let dynamic_fan_klipper_name = part_cooling_klipper_name;
            let mut dynamic_fan_state: Option<f64> = dynamic_base_fan;

            let mut last_role: Option<crate::core::ExtrusionRole> = None;
            let mut last_width: Option<f64> = None;
            let mut last_pos: Option<(f64, f64)> = None;

            // ── Spiral (vase) mode: emit the single outer contour with a
            //    continuous Z ramp, then move to the next layer ───────────────
            if let Some(idx) = spiral_path_idx {
                // A spiral layer is one continuous loop rather than a list of
                // paths, so it never reaches the per-path attribution below —
                // it has to open its own object's marker block here.
                if !self.objects.is_empty() && layer_object != current_object {
                    if object_markers {
                        if let Some(previous) = current_object.and_then(|i| self.objects.get(i)) {
                            out.push_str(&self.dialect.object_end(previous));
                            out.push('\n');
                        }
                        if let Some(index) = layer_object {
                            if let Some(object) = self.objects.get(index) {
                                let first_use = !introduced_objects[index];
                                out.push_str(&self.dialect.object_start(object, first_use));
                                out.push('\n');
                                introduced_objects[index] = true;
                            }
                        }
                    }
                    current_object = layer_object;
                }

                let path = layer
                    .paths
                    .iter()
                    .nth(idx)
                    .expect("spiral path index is within the layer's paths");
                let pts: Vec<(f64, f64)> = path.iter().map(|p| (p.x(), p.y())).collect();

                let role = crate::core::ExtrusionRole::OuterWall;
                let width_mm = resolve_width_mm(layer.width_for_path(idx), false, role, params);
                // Spiral mode skips overhang classification (it would split the
                // single continuous contour), so there is never a degree here.
                let speed_mm_min = Self::effective_speed_mm_min(
                    role,
                    crate::core::OverhangClass::None,
                    is_first_layer,
                    params,
                );

                // Adaptive acceleration (opt-in), same policy as normal walls.
                if let Some(accel) = Self::effective_acceleration(role, is_first_layer, params) {
                    if last_accel != Some(accel) {
                        out.push_str(&format!(
                            "{} ; acceleration\n",
                            self.dialect.set_acceleration(accel)
                        ));
                        last_accel = Some(accel);
                    }
                }

                // ;TYPE: / ;WIDTH: annotations so viewers classify the wall.
                if self.marker_config.enabled {
                    let width_str = format!("{:.2}", width_mm);
                    let type_ann = self
                        .marker_config
                        .type_annotation
                        .as_deref()
                        .unwrap_or(";TYPE:{type}");
                    out.push_str(&render_marker(
                        type_ann,
                        &z_str,
                        &height_str,
                        role.type_name(),
                        &width_str,
                    ));
                    out.push('\n');
                    let width_ann = self
                        .marker_config
                        .width_annotation
                        .as_deref()
                        .unwrap_or(";WIDTH:{width}mm");
                    out.push_str(&render_marker(
                        width_ann,
                        &z_str,
                        &height_str,
                        role.type_name(),
                        &width_str,
                    ));
                    out.push('\n');
                }

                // Ramp from the previous layer's top Z to this layer's Z over
                // one perimeter. Start the loop nearest the previous nozzle
                // position to keep the (invisible) start line aligned.
                // Ramp up from the previous layer of **this** object. In
                // sequential order the layer before an object's first is the
                // previous object's *top*, and ramping from there would extrude
                // a continuous descent from that height straight through the
                // plate.
                //
                // The clamps below are deliberately evaluated in *model* space,
                // before `machine_z` is applied: a `max(0.0)` floor means "the
                // bed", which a negative Z-offset legitimately sits below.
                // Offsetting both ends afterwards shifts the ramp bodily while
                // preserving its exact one-layer-height rise.
                let z_bottom = machine_z(
                    if layer_index > 0 && layer_owners[layer_index - 1] == layer_object {
                        layers[layer_index - 1].z
                    } else {
                        (layer.z - params.layer_height).max(0.0)
                    }
                    // A spiral ramp only ever climbs. Capping at this layer's own Z
                    // makes the "descend while extruding" failure unrepresentable,
                    // whatever the previous layer turns out to be.
                    .min(layer.z)
                    .max(0.0),
                    params,
                );
                let z_top = machine_z(layer.z, params);
                let target = cur_xy.unwrap_or(pts[0]);
                let rotated = rotate_loop_nearest(&pts, target);
                let (sx, sy) = rotated[0];

                let need_travel = match cur_xy {
                    Some((cx, cy)) => ((sx - cx).powi(2) + (sy - cy).powi(2)).sqrt() > 0.05,
                    None => true,
                };
                if need_travel {
                    out.push_str(&format!(
                        "{} ; spiral travel\n",
                        self.dialect.travel_xy(sx, sy, params.travel_speed_mm_min)
                    ));
                }

                // Fade flow in on the first spiral loop and out on the last so
                // the seam disappears at both ends of the vase.
                let is_first_spiral = first_spiral_of_run[layer_index];
                let is_last_spiral = last_spiral_of_run[layer_index];
                let flow_start = if is_first_spiral && !is_last_spiral {
                    0.0
                } else {
                    1.0
                };
                let flow_end = if is_last_spiral && !is_first_spiral {
                    0.0
                } else {
                    1.0
                };

                self.emit_spiral_loop(
                    &mut out,
                    &rotated,
                    z_bottom,
                    z_top,
                    width_mm,
                    speed_mm_min,
                    flow_start,
                    flow_end,
                    params,
                    &mut e_total,
                    &mut total_filament_mm,
                );

                // The loop closes back to its start vertex; that's where the
                // next spiral layer begins.
                cur_xy = Some((sx, sy));
                // This branch skips the end-of-layer bookkeeping below, so the
                // clearance lift would otherwise never see a spiralised object
                // and would travel straight through it.
                if layer.z > max_printed_z {
                    max_printed_z = layer.z;
                }
                continue;
            }

            for (path_idx, path) in layer.paths.iter().enumerate() {
                let raw_points: Vec<(f64, f64)> = path.iter().map(|p| (p.x(), p.y())).collect();
                if raw_points.len() < 2 {
                    continue;
                }

                // ── Object attribution (issue #22) ───────────────────────────
                // Switch marker blocks *before* this path's travel, so the hop
                // between two parts is charged to the one it is heading for —
                // the convention PrusaSlicer and OrcaSlicer both follow, and
                // the one that lets a firmware skip a cancelled object's
                // approach moves along with its extrusions.  A `None` tag
                // (plate-wide adhesion) closes the block without opening one.
                let path_object = layer.object_for_path(path_idx);
                if !self.objects.is_empty() && path_object != current_object {
                    if object_markers {
                        if let Some(previous) = current_object.and_then(|i| self.objects.get(i)) {
                            out.push_str(&self.dialect.object_end(previous));
                            out.push('\n');
                        }
                        if let Some(index) = path_object {
                            if let Some(object) = self.objects.get(index) {
                                let first_use = !introduced_objects[index];
                                out.push_str(&self.dialect.object_start(object, first_use));
                                out.push('\n');
                                introduced_objects[index] = true;
                            }
                        }
                    }
                    current_object = path_object;
                }

                // Fetch the role and resolve the effective extrusion width.
                // Per-vertex widths (variable-width beads) taper the flow along
                // the path; `None` keeps the constant `width_mm`. They also gate
                // the per-role width override off, since such beads carry their
                // own authoritative widths.
                let role = layer.role_for_path(path_idx);
                let overhang = layer.overhang_for_path(path_idx);
                let raw_vertex_widths = layer.vertex_widths_for_path(path_idx);
                let width_mm = resolve_width_mm(
                    layer.width_for_path(path_idx),
                    raw_vertex_widths.is_some(),
                    role,
                    params,
                );
                // Combined sparse infill prints one bead for several layers, so
                // its flow, its `;HEIGHT:` annotation and its volumetric-speed
                // cap are all charged at the stacked height rather than the
                // layer height. Every other path leaves this `None`.
                let path_height = layer
                    .height_for_path(path_idx)
                    .filter(|h| *h > 0.0)
                    .unwrap_or(params.layer_height);
                let path_height_str = format!("{:.3}", path_height);

                // Combined infill prints taller than the layer it sits on, so
                // re-announce `;HEIGHT:` whenever it changes. Viewers and
                // post-processors read that marker to size the bead; without it a
                // 0.4 mm-tall infill bead would be drawn (and re-flowed) as
                // 0.2 mm. Emitted on change only, so an ordinary layer is
                // untouched.
                if self.marker_config.enabled
                    && last_height.as_deref() != Some(path_height_str.as_str())
                {
                    let height_marker = self
                        .marker_config
                        .height_marker
                        .as_deref()
                        .unwrap_or(";HEIGHT:{height}");
                    out.push_str(&render_marker(
                        height_marker,
                        &z_str,
                        &path_height_str,
                        "",
                        "",
                    ));
                    out.push('\n');
                    last_height = Some(path_height_str.clone());
                }

                // Resolve per-role print speed (with dynamic overhang override).
                let speed_mm_min =
                    Self::effective_speed_mm_min(role, overhang, is_first_layer, params);

                // ── Dynamic fan (bridges + overhangs) ────────────────────────
                // Raise the part-cooling fan for material laid over air and
                // restore the layer's normal cooling when leaving it.  Emitted
                // only on change, so a run of bridge lines or overhang arcs
                // (grouped by role) toggles the fan at most twice per layer.
                //
                // Bridges are unconditional — a bridge is a bridge regardless of
                // the dynamic-overhang *speed* feature — while the overhang
                // boost stays tied to `enable_overhang_speed` and its threshold.
                if let Some(base_fan) = dynamic_base_fan {
                    let bridge_target = (role == crate::core::ExtrusionRole::Bridge
                        && params.bridge_fan_speed > 0.0)
                        .then_some(params.bridge_fan_speed);
                    let overhang_target = (params.enable_overhang_speed
                        && params.overhang_fan_speed > 0.0
                        && overhang.is_overhang()
                        && overhang_meets_fan_threshold(overhang, params))
                    .then_some(params.overhang_fan_speed);
                    let target = match (bridge_target, overhang_target) {
                        (Some(a), Some(b)) => a.max(b),
                        (Some(a), None) | (None, Some(a)) => a,
                        (None, None) => base_fan,
                    };
                    if dynamic_fan_state != Some(target) {
                        out.push_str(&format!(
                            "{} ; dynamic fan\n",
                            self.dialect.set_fan_speed_indexed(
                                fan_index::PART_COOLING,
                                dynamic_fan_klipper_name.as_deref(),
                                target
                            )
                        ));
                        dynamic_fan_state = Some(target);
                    }
                }

                // Global volumetric-flow ceiling for constant-width emissions
                // (coasting splits and the close-contour move).  Variable-width
                // beads are capped per-segment in `seg_speed` below, where the
                // real per-segment width is known.
                let capped_speed_mm_min = volumetric_capped_speed_mm_min(
                    speed_mm_min,
                    path_height,
                    width_mm,
                    params.max_volumetric_speed,
                );

                // ── Adaptive acceleration (opt-in; emitted on change only) ────
                // Resolve this path's printing acceleration and any dedicated
                // travel acceleration. When a travel acceleration is configured
                // we defer the printing value until *after* the upcoming travel
                // (emitted just before the extrusion moves below) and set the
                // travel value for the hop instead — so travels ramp at their
                // own rate and extrusion at the role's. With no travel
                // acceleration set, the printing value is emitted here exactly
                // as before, keeping output byte-identical. Disabled roles
                // resolve to `None` and leave the previous limit in place.
                let print_accel = Self::effective_acceleration(role, is_first_layer, params);
                let travel_accel = Self::effective_travel_acceleration(params);
                if travel_accel.is_none() {
                    if let Some(accel) = print_accel {
                        if last_accel != Some(accel) {
                            out.push_str(&format!(
                                "{} ; acceleration\n",
                                self.dialect.set_acceleration(accel)
                            ));
                            last_accel = Some(accel);
                        }
                    }
                }

                // Apply Ramer-Douglas-Peucker simplification when a tolerance is
                // set.  Constant-width paths use the plain pass; variable-width
                // beads use the width-aware pass so `points` and their widths stay
                // aligned (and long constant-width runs still collapse), instead
                // of being emitted at full resolution.
                let (points, vertex_widths): (Vec<(f64, f64)>, Option<Vec<f64>>) =
                    match raw_vertex_widths {
                        Some(vw)
                            if params.path_tolerance > 0.0
                                && raw_points.len() > 2
                                && vw.len() == raw_points.len() =>
                        {
                            let (p, w) = crate::gcode::simplify::douglas_peucker_with_widths(
                                &raw_points,
                                &vw,
                                params.path_tolerance,
                                WIDTH_SIMPLIFY_TOL_MM,
                            );
                            (p, Some(w))
                        }
                        Some(vw) => (raw_points, Some(vw)),
                        None if params.path_tolerance > 0.0 && raw_points.len() > 2 => (
                            crate::gcode::simplify::douglas_peucker(
                                &raw_points,
                                params.path_tolerance,
                            ),
                            None,
                        ),
                        None => (raw_points, None),
                    };

                // Guard against future algorithm changes that might produce degenerate paths.
                debug_assert!(
                    points.len() >= 2,
                    "path should have >= 2 points after simplification"
                );

                // Emit ;TYPE: / ;WIDTH: annotation when the role OR extrusion
                // width changes.  This ensures slicers / post-processors always
                // see an up-to-date WIDTH comment before each wall bead.
                //
                // For variable-width beads the header advertises the *first*
                // segment's width (not the scalar mean), and the per-segment
                // loop below re-emits `;WIDTH:` as the width steps — so a viewer
                // renders the real, flow-compensated bead profile.
                let header_width = match &vertex_widths {
                    Some(vw) if vw.len() >= 2 => 0.5 * (vw[0] + vw[1]),
                    _ => width_mm,
                };
                if self.marker_config.enabled {
                    let role_changed = last_role != Some(role);
                    let width_changed =
                        last_width.is_none_or(|w| (w - header_width).abs() > WIDTH_EPSILON);

                    if role_changed || width_changed {
                        let type_name = role.type_name();
                        let width_str = format!("{:.2}", header_width);

                        let type_ann = self
                            .marker_config
                            .type_annotation
                            .as_deref()
                            .unwrap_or(";TYPE:{type}");
                        out.push_str(&render_marker(
                            type_ann,
                            &z_str,
                            &path_height_str,
                            type_name,
                            &width_str,
                        ));
                        out.push('\n');

                        let width_ann = self
                            .marker_config
                            .width_annotation
                            .as_deref()
                            .unwrap_or(";WIDTH:{width}mm");
                        out.push_str(&render_marker(
                            width_ann,
                            &z_str,
                            &path_height_str,
                            type_name,
                            &width_str,
                        ));
                        out.push('\n');

                        last_role = Some(role);
                        last_width = Some(header_width);
                    }
                }

                let (start_x, start_y) = points[0];

                let travel_dist = if let Some((lx, ly)) = last_pos {
                    let dx = start_x - lx;
                    let dy = start_y - ly;
                    (dx * dx + dy * dy).sqrt()
                } else {
                    f64::MAX
                };

                let role_changed = last_role != Some(role);
                // Retract policy (mirrors PrusaSlicer / Orca / Cura):
                //   - Never retract for travels at or under the configured
                //     `retract_before_travel_mm` minimum. A 0.4 mm hop from an
                //     inner-wall loop end to an outer-wall loop start does not
                //     ooze enough to justify the retract → z-hop → travel →
                //     lower → un-retract ceremony (which itself takes longer
                //     than the hop and pauses extrusion).
                //   - Long travels (> max(2 mm, the minimum)) always retract.
                //   - Travels between the minimum and that ceiling retract only
                //     when the extrusion role changes (e.g. infill → outer wall),
                //     where oozing would show on a visible surface.
                const ALWAYS_RETRACT_TRAVEL_MM: f64 = 2.0;
                let min_travel = params.retract_before_travel_mm.max(0.0);
                let always_retract = min_travel.max(ALWAYS_RETRACT_TRAVEL_MM);
                let needs_retract =
                    travel_dist > always_retract || (role_changed && travel_dist > min_travel);

                // Plan the travel path.  With `avoid_crossing_perimeters` the
                // planner may return intermediate waypoints that detour around
                // outer walls; otherwise this is a single straight hop to the
                // destination.  `from` is never included.
                let travel_route: Vec<(f64, f64)> = match (&travel_planner, last_pos) {
                    (Some(planner), Some(lp)) => planner.route(lp, (start_x, start_y)),
                    _ => vec![(start_x, start_y)],
                };

                // Switch to the travel acceleration for the upcoming hop (opt-in;
                // emitted on change only). The printing acceleration is restored
                // just before the extrusion moves below.
                if let Some(accel) = travel_accel {
                    if last_accel != Some(accel) {
                        out.push_str(&format!(
                            "{} ; travel acceleration\n",
                            self.dialect.set_acceleration(accel)
                        ));
                        last_accel = Some(accel);
                    }
                }

                if needs_retract {
                    // Retract [+ wipe], z-hop, travel (possibly via detour),
                    // lower, prime. The retract and prime dispatch on the
                    // retraction mode (software E move, firmware G10/G11,
                    // wipe-while-retracting); the z-hop is always slicer-driven
                    // so behaviour is mode-independent.
                    self.do_retract(
                        &mut out,
                        &mut e_total,
                        &mut retracted,
                        last_path_points.as_deref(),
                        params,
                    );
                    out.push_str(&format!(
                        "{} ; z-hop\n",
                        self.dialect.move_z(
                            machine_z(layer.z + params.z_hop_mm, params),
                            params.travel_speed_mm_min
                        )
                    ));
                    for (wi, &(wx, wy)) in travel_route.iter().enumerate() {
                        let tag = if wi + 1 == travel_route.len() {
                            "travel"
                        } else {
                            "travel (avoid crossing)"
                        };
                        out.push_str(&format!(
                            "{} ; {tag}\n",
                            self.dialect.travel_xy(wx, wy, params.travel_speed_mm_min)
                        ));
                    }
                    out.push_str(&format!(
                        "{} ; lower\n",
                        self.dialect
                            .move_z(machine_z(layer.z, params), params.travel_speed_mm_min)
                    ));
                    self.do_unretract(
                        &mut out,
                        &mut e_total,
                        &mut retracted,
                        &mut total_filament_mm,
                        params,
                    );
                } else if travel_dist > 0.05 {
                    // Short travel without stringing mitigation.  Travels under
                    // 0.05 mm are degenerate (floating-point rounding noise from
                    // path simplification) and are skipped entirely — emitting a
                    // G1 line for a sub-quantum hop just bloats the file and
                    // confuses motion planners on some firmwares.
                    for (wi, &(wx, wy)) in travel_route.iter().enumerate() {
                        let tag = if wi + 1 == travel_route.len() {
                            "short travel"
                        } else {
                            "short travel (avoid crossing)"
                        };
                        out.push_str(&format!(
                            "{} ; {tag}\n",
                            self.dialect.travel_xy(wx, wy, params.travel_speed_mm_min)
                        ));
                    }
                }

                // Restore the printing acceleration after the travel (opt-in;
                // emitted on change only). Only runs when a travel acceleration
                // was set above, so output stays byte-identical otherwise.
                if travel_accel.is_some() {
                    if let Some(accel) = print_accel {
                        if last_accel != Some(accel) {
                            out.push_str(&format!(
                                "{} ; acceleration\n",
                                self.dialect.set_acceleration(accel)
                            ));
                            last_accel = Some(accel);
                        }
                    }
                }

                // Determine if this is a closed-loop role.
                //
                // A path is a closed loop only when BOTH:
                //   1. Its role is one that normally forms closed contours, AND
                //   2. It is NOT marked as an open arc in `path_is_open`.
                //
                // `path_is_open` is set to `true` for sub-segments produced by
                // `classify_overhang_perimeters` when it splits a closed wall
                // loop at the air/support boundary.  Those sub-segments are
                // open polylines even though their role may still be `OuterWall`
                // or `InnerWall`.  Emitting a "close contour" G1 move for them
                // would create a phantom extrusion back through the model.
                let is_open_arc = layer.is_path_open(path_idx);
                let is_closed_loop = matches!(
                    role,
                    crate::core::ExtrusionRole::OuterWall
                        | crate::core::ExtrusionRole::InnerWall
                        | crate::core::ExtrusionRole::Skirt
                ) && !is_open_arc;

                // ── Coasting: stop extruding before end of perimeter ──────────
                // Coasting applies only to closed-loop perimeter paths and only
                // when a positive coasting distance is configured.  For the last
                // `coasting_distance_mm` of the path (including the close-loop
                // segment) the nozzle travels without extrusion, allowing nozzle
                // pressure to drop before reaching the seam.
                //
                // Implementation note: this requires two passes over the point array —
                // one to compute the total path length, one to emit moves.  For typical
                // perimeter paths (tens to hundreds of points) this is negligible; for
                // very detailed organic models with thousands of points per perimeter it
                // will add a linear pass per perimeter path. A future optimisation could
                // pre-compute cumulative lengths once if profiling shows this to be a
                // bottleneck.
                let apply_coasting = is_closed_loop && params.coasting_distance_mm > 0.0;

                // ── Print contour segments ────────────────────────────────────
                if apply_coasting {
                    // Pass 1: compute the total path length so we know when to
                    // start coasting.
                    let mut total_len = 0.0_f64;
                    let mut pp = points[0];
                    for &(x, y) in points.iter().skip(1) {
                        let dx = x - pp.0;
                        let dy = y - pp.1;
                        total_len += (dx * dx + dy * dy).sqrt();
                        pp = (x, y);
                    }
                    // Close-loop segment
                    let cx = start_x - pp.0;
                    let cy = start_y - pp.1;
                    total_len += (cx * cx + cy * cy).sqrt();

                    let coasting_start = (total_len - params.coasting_distance_mm).max(0.0);
                    let mut dist_traveled = 0.0_f64;
                    let mut prev = points[0];

                    // Pass 2: emit segments, switching to travel at coasting_start.
                    for &(x, y) in points.iter().skip(1) {
                        let dx = x - prev.0;
                        let dy = y - prev.1;
                        let seg_len = (dx * dx + dy * dy).sqrt();
                        if seg_len < 1e-6 {
                            prev = (x, y);
                            continue;
                        }
                        if dist_traveled + seg_len <= coasting_start {
                            // Entirely before coasting point → extrude normally
                            let de = extrusion_for_move(
                                seg_len,
                                path_height,
                                width_mm,
                                params.filament_diameter_mm,
                                params.flow_ratio,
                            );
                            e_total += de;
                            total_filament_mm += de;
                            out.push_str(&format!(
                                "{}\n",
                                self.xy_extrude_line(
                                    x,
                                    y,
                                    de,
                                    e_total,
                                    capped_speed_mm_min,
                                    params
                                )
                            ));
                        } else if dist_traveled < coasting_start {
                            // Segment straddles the coasting boundary → split it
                            let dist_to_coast = coasting_start - dist_traveled;
                            let t = dist_to_coast / seg_len;
                            let bx = prev.0 + t * dx;
                            let by = prev.1 + t * dy;
                            let de = extrusion_for_move(
                                dist_to_coast,
                                path_height,
                                width_mm,
                                params.filament_diameter_mm,
                                params.flow_ratio,
                            );
                            e_total += de;
                            total_filament_mm += de;
                            out.push_str(&format!(
                                "{}\n",
                                self.xy_extrude_line(
                                    bx,
                                    by,
                                    de,
                                    e_total,
                                    capped_speed_mm_min,
                                    params
                                )
                            ));
                            // Remainder is a travel move
                            out.push_str(&format!(
                                "{} ; coasting\n",
                                self.dialect.travel_xy(x, y, speed_mm_min)
                            ));
                        } else {
                            // Entirely in coasting zone → travel only
                            out.push_str(&format!(
                                "{} ; coasting\n",
                                self.dialect.travel_xy(x, y, speed_mm_min)
                            ));
                        }
                        dist_traveled += seg_len;
                        prev = (x, y);
                    }

                    // Close-loop segment
                    let dx = start_x - prev.0;
                    let dy = start_y - prev.1;
                    let seg_len = (dx * dx + dy * dy).sqrt();
                    if seg_len >= 1e-6 {
                        if dist_traveled + seg_len <= coasting_start {
                            let de = extrusion_for_move(
                                seg_len,
                                path_height,
                                width_mm,
                                params.filament_diameter_mm,
                                params.flow_ratio,
                            );
                            e_total += de;
                            total_filament_mm += de;
                            out.push_str(&format!(
                                "{} ; close contour\n",
                                self.xy_extrude_line(
                                    start_x,
                                    start_y,
                                    de,
                                    e_total,
                                    capped_speed_mm_min,
                                    params
                                )
                            ));
                        } else if dist_traveled < coasting_start {
                            let dist_to_coast = coasting_start - dist_traveled;
                            let t = dist_to_coast / seg_len;
                            let bx = prev.0 + t * dx;
                            let by = prev.1 + t * dy;
                            let de = extrusion_for_move(
                                dist_to_coast,
                                path_height,
                                width_mm,
                                params.filament_diameter_mm,
                                params.flow_ratio,
                            );
                            e_total += de;
                            total_filament_mm += de;
                            out.push_str(&format!(
                                "{}\n",
                                self.xy_extrude_line(
                                    bx,
                                    by,
                                    de,
                                    e_total,
                                    capped_speed_mm_min,
                                    params
                                )
                            ));
                            out.push_str(&format!(
                                "{} ; coasting close\n",
                                self.dialect.travel_xy(start_x, start_y, speed_mm_min)
                            ));
                        } else {
                            out.push_str(&format!(
                                "{} ; coasting close\n",
                                self.dialect.travel_xy(start_x, start_y, speed_mm_min)
                            ));
                        }
                    }
                    last_pos = Some((start_x, start_y));
                } else {
                    // Normal printing (no coasting)
                    // Per-segment width: variable-width beads taper between
                    // vertices; constant-width paths fall back to `width_mm`.
                    let seg_width = |j: usize| -> f64 {
                        match &vertex_widths {
                            Some(vw) if j + 1 < vw.len() => 0.5 * (vw[j] + vw[j + 1]),
                            _ => width_mm,
                        }
                    };
                    // Volumetric-flow cap for variable-width beads: a bead wider
                    // than the nozzle would over-run the hotend melt rate at the
                    // nominal feedrate and under-extrude, so it never squeezes
                    // into the gap.  Slow it in proportion to width so mm³/s
                    // holds at the nozzle-width rate — "extrude more, move slower".
                    //
                    // On top of that per-bead throttle, the global
                    // `max_volumetric_speed` ceiling is applied per-segment using
                    // the segment's real width, so both the constant- and
                    // variable-width cases honour the hotend melt-rate limit.
                    let nozzle = params.nozzle_diameter_mm;
                    let seg_speed = |sw: f64| -> f64 {
                        let base = if vertex_widths.is_some() && nozzle > 0.0 && sw > nozzle {
                            speed_mm_min * (nozzle / sw)
                        } else {
                            speed_mm_min
                        };
                        volumetric_capped_speed_mm_min(
                            base,
                            path_height,
                            sw,
                            params.max_volumetric_speed,
                        )
                    };
                    let mut prev = points[0];
                    for (i, &(x, y)) in points.iter().enumerate().skip(1) {
                        let dx = x - prev.0;
                        let dy = y - prev.1;
                        let len = (dx * dx + dy * dy).sqrt();
                        if len < 1e-6 {
                            prev = (x, y);
                            continue;
                        }
                        let sw = seg_width(i - 1);
                        // Variable-width beads: re-emit ;WIDTH: as the width
                        // steps so the viewer renders the compensated (thinner)
                        // bead where walls overlap, not the nominal width.
                        if self.marker_config.enabled
                            && vertex_widths.is_some()
                            && last_width.is_none_or(|w| (w - sw).abs() > WIDTH_MARKER_STEP_MM)
                        {
                            let width_str = format!("{:.2}", sw);
                            let width_ann = self
                                .marker_config
                                .width_annotation
                                .as_deref()
                                .unwrap_or(";WIDTH:{width}mm");
                            out.push_str(&render_marker(
                                width_ann,
                                &z_str,
                                &path_height_str,
                                role.type_name(),
                                &width_str,
                            ));
                            out.push('\n');
                            last_width = Some(sw);
                        }
                        let de = extrusion_for_move(
                            len,
                            path_height,
                            sw,
                            params.filament_diameter_mm,
                            params.flow_ratio,
                        );
                        e_total += de;
                        total_filament_mm += de;
                        out.push_str(&format!(
                            "{}\n",
                            self.xy_extrude_line(x, y, de, e_total, seg_speed(sw), params)
                        ));
                        prev = (x, y);
                    }

                    // Close the contour — only for inherently closed-loop roles such as
                    // perimeter walls and skirt/brim.  Open infill polylines (Infill,
                    // TopSurface, BottomSurface, Bridge, Support) must NOT be closed;
                    // doing so would add a long diagonal extrusion back to the path start,
                    // producing the "weird line crossing" artifact visible in gyroid infill.
                    if is_closed_loop {
                        let dx = start_x - prev.0;
                        let dy = start_y - prev.1;
                        let len = (dx * dx + dy * dy).sqrt();
                        if len >= 1e-6 {
                            let de = extrusion_for_move(
                                len,
                                path_height,
                                width_mm,
                                params.filament_diameter_mm,
                                params.flow_ratio,
                            );
                            e_total += de;
                            total_filament_mm += de;
                            out.push_str(&format!(
                                "{} ; close contour\n",
                                self.xy_extrude_line(
                                    start_x,
                                    start_y,
                                    de,
                                    e_total,
                                    capped_speed_mm_min,
                                    params
                                )
                            ));
                        }
                        last_pos = Some((start_x, start_y));
                    } else {
                        last_pos = Some(prev);
                    }
                }

                // Capture this path's trajectory so a subsequent retract can
                // wipe along it. Only done when wiping is enabled, to avoid the
                // per-path clone otherwise. Closed loops end back at their start,
                // so the closing vertex is appended; the wipe retraces this list
                // in reverse from the end.
                if params.wipe && params.wipe_distance_mm > 0.0 {
                    let mut traj = points.clone();
                    if is_closed_loop {
                        traj.push(points[0]);
                    }
                    last_path_points = Some(traj);
                }
            }

            // Remember where this (non-spiral) layer left the nozzle so a
            // following spiral layer can travel into its loop from the true
            // previous position.
            if last_pos.is_some() {
                cur_xy = last_pos;
            }

            // Highest material laid down so far — what the next object has to
            // clear when sequential printing hands over.
            if layer.z > max_printed_z {
                max_printed_z = layer.z;
            }
        }

        // The last object's marker block stays open until the print ends.
        if object_markers {
            if let Some(object) = current_object.and_then(|i| self.objects.get(i)) {
                out.push_str(&self.dialect.object_end(object));
                out.push('\n');
            }
        }

        // ── Per-filament end script ───────────────────────────────────────────
        // Material-scoped teardown, emitted after the last print move and before
        // the machine end script.
        if let Some(lines) = &self.custom_filament_end_script {
            for line in lines {
                out.push_str(&render_script_placeholders(line, params));
                out.push('\n');
            }
        }

        // ── End script (custom override or flavor default) ────────────────────
        let end_script: Cow<[String]> = match &self.custom_end_script {
            Some(lines) => Cow::Borrowed(lines),
            None => Cow::Owned(self.dialect.end_script()),
        };
        for line in end_script.iter() {
            out.push_str(&render_script_placeholders(line, params));
            out.push('\n');
        }

        // ── Acceleration-aware print-time estimate (issue #117) ───────────────
        // Measure the *emitted* moves — travel, Z lifts, retraction and every
        // per-role feedrate / acceleration the body above wrote — with the
        // trapezoidal planner model, then apply the user's estimate calibration
        // and splice the per-layer figures back into the `;LAYER_TIME:` markers
        // so the header/footer ETA and the viewer's Layer-Time colouring all read
        // the same numbers.
        //
        // Calibration (issue #117 follow-up): the toolpath physics from the
        // estimator is corrected by `time_estimate_scale` (a user fudge factor
        // for systematic error the model rounds off), then the fixed
        // `time_estimate_warmup_s` (homing / heat-soak / purge) and
        // `time_estimate_cooldown_s` (e.g. chamber cool-off) allowances — wall
        // clock the toolpath cannot show — are added. The per-layer markers get
        // the same scale so they stay consistent with the toolpath total, but
        // *not* the fixed allowances (those belong to no single layer).
        let est_cfg = crate::gcode::time_estimate::EstimatorConfig::from_params(params);
        let estimate = crate::gcode::time_estimate::estimate_print_time(&out, &est_cfg);
        let scale = if params.time_estimate_scale > 0.0 {
            params.time_estimate_scale
        } else {
            1.0
        };
        let scaled_per_layer: Vec<f64> = estimate.per_layer_s.iter().map(|t| t * scale).collect();
        patch_layer_time_markers(&mut out, &scaled_per_layer);
        let total_estimate_s = params.time_estimate_warmup_s.max(0.0)
            + estimate.total_s * scale
            + params.time_estimate_cooldown_s.max(0.0);

        // ── Metadata header (issue #15) ───────────────────────────────────────
        // Now that the body is emitted we know the measured filament total, the
        // geometry, and the print-time estimate; build the aggregate statistics
        // and prepend the flavor-specific header so its figures match the body
        // exactly.
        let stats = SliceStatistics::from_layers(
            layers,
            params,
            total_filament_mm,
            total_estimate_s,
            self.model_name.clone(),
        );
        let mut result = String::with_capacity(out.len() + 512);
        for line in self.dialect.header(params, &stats) {
            result.push_str(&line);
            result.push('\n');
        }
        append_thumbnail_block(&mut result, params);
        result.push_str(&out);

        // ── Metadata footer (Moonraker / PrusaSlicer-compatible config block) ─
        // Printer front-ends (Mainsail / Fluidd via Moonraker, OctoPrint) scan
        // the file *footer* — not the header above — for `; key = value`
        // metadata. Appending it here is what surfaces filament type / colour,
        // layer height, object height, and filament usage in the printer UI.
        for line in self.dialect.footer(params, &stats) {
            result.push_str(&line);
            result.push('\n');
        }

        (result, stats)
    }
}

// ── Convenience wrapper ────────────────────────────────────────────────────────

/// Generate a G-code string from a slice result using the default Marlin dialect.
///
/// This is a convenience wrapper around [`GcodeGenerator::new`] with
/// [`GcodeFlavor::Marlin`].  Prefer [`GcodeGenerator`] directly when you need
/// to select a specific firmware flavor.
///
/// # Arguments
/// * `layers` – ordered bottom-to-top slice layers produced by [`crate::core::slice_mesh`]
/// * `params` – slicing parameters (temperatures, speeds, layer height, …)
///
/// # Returns
/// A `String` containing the full G-code program.  Returns a minimal
/// (start + end only) program when `layers` is empty.
///
/// # Example
/// ```
/// use slicer_engine::gcode::generate_gcode;
/// use slicer_engine::settings::params::SlicingParams;
///
/// let gcode = generate_gcode(&[], &SlicingParams::default());
/// assert!(gcode.contains("G28"));
/// assert!(gcode.contains("M104 S0"));
/// ```
pub fn generate_gcode(layers: &[SliceLayer], params: &SlicingParams) -> String {
    GcodeGenerator::new(GcodeFlavor::Marlin).generate(layers, params)
}

/// Generate G-code honoring the firmware flavor and custom start/end/layer
/// scripts carried by `params`.
///
/// This is the entry point used by every *application* slice path (WS server,
/// WASM, desktop bridge). Unlike [`generate_gcode`] it respects
/// `params.gcode_flavor` and applies `params.start_gcode`, `params.end_gcode`,
/// `params.layer_gcode`, and the per-filament `params.start_filament_gcode` /
/// `params.end_filament_gcode` hooks when present (each split into lines on
/// `'\n'`).
///
/// A non-empty custom block wins over the dialect default; a blank/whitespace
/// block is ignored so an empty text field falls back to the flavor default.
pub fn generate_gcode_from_params(layers: &[SliceLayer], params: &SlicingParams) -> String {
    let mut generator = GcodeGenerator::new(params.gcode_flavor);
    if let Some(lines) = gcode_block_lines(params.start_gcode.as_deref()) {
        generator = generator.with_start_script(lines);
    }
    if let Some(lines) = gcode_block_lines(params.end_gcode.as_deref()) {
        generator = generator.with_end_script(lines);
    }
    if let Some(lines) = gcode_block_lines(params.layer_gcode.as_deref()) {
        generator = generator.with_layer_script(lines);
    }
    if let Some(lines) = gcode_block_lines(params.start_filament_gcode.as_deref()) {
        generator = generator.with_filament_start_script(lines);
    }
    if let Some(lines) = gcode_block_lines(params.end_filament_gcode.as_deref()) {
        generator = generator.with_filament_end_script(lines);
    }
    generator.generate(layers, params)
}

/// Generate G-code for a whole plate, including its per-object markers.
///
/// The object-aware sibling of [`generate_gcode_from_params`]: identical for a
/// plate sliced without object identity (`plate.objects` empty), and the one
/// call every runtime should make so exclude-object and sequential printing
/// work the same everywhere.
pub fn generate_gcode_for_plate(plate: &crate::core::PlateSlice, params: &SlicingParams) -> String {
    let mut generator =
        GcodeGenerator::new(params.gcode_flavor).with_objects(plate.objects.clone());
    if let Some(lines) = gcode_block_lines(params.start_gcode.as_deref()) {
        generator = generator.with_start_script(lines);
    }
    if let Some(lines) = gcode_block_lines(params.end_gcode.as_deref()) {
        generator = generator.with_end_script(lines);
    }
    if let Some(lines) = gcode_block_lines(params.layer_gcode.as_deref()) {
        generator = generator.with_layer_script(lines);
    }
    if let Some(lines) = gcode_block_lines(params.start_filament_gcode.as_deref()) {
        generator = generator.with_filament_start_script(lines);
    }
    if let Some(lines) = gcode_block_lines(params.end_filament_gcode.as_deref()) {
        generator = generator.with_filament_end_script(lines);
    }
    generator.generate(&plate.layers, params)
}

/// Split a multi-line custom G-code block into lines, returning `None` when the
/// block is absent or entirely blank (so the dialect default is kept).
fn gcode_block_lines(block: Option<&str>) -> Option<Vec<String>> {
    let block = block?;
    if block.trim().is_empty() {
        return None;
    }
    Some(block.lines().map(str::to_string).collect())
}

/// Emit a Prusa/Orca-style thumbnail comment block when the current request
/// carries a PNG payload.
fn append_thumbnail_block(out: &mut String, params: &SlicingParams) {
    if !params.thumbnail_enabled {
        return;
    }
    let Some(encoded) = normalize_thumbnail_base64(params.thumbnail_png_base64.as_deref()) else {
        return;
    };
    let size = params.thumbnail_size_px.clamp(64, 1024);
    out.push_str(&format!(
        "; thumbnail begin {}x{} {}\n",
        size,
        size,
        encoded.len()
    ));
    for chunk in encoded.as_bytes().chunks(78) {
        if let Ok(line) = std::str::from_utf8(chunk) {
            out.push_str("; ");
            out.push_str(line);
            out.push('\n');
        }
    }
    out.push_str("; thumbnail end\n");
}

fn normalize_thumbnail_base64(raw: Option<&str>) -> Option<String> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    let payload = raw.strip_prefix("data:image/png;base64,").unwrap_or(raw);
    let normalized: String = payload.chars().filter(|ch| !ch.is_whitespace()).collect();
    if normalized.is_empty() {
        return None;
    }
    if !normalized.bytes().all(|b| {
        matches!(
            b,
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' | b'='
        )
    }) {
        return None;
    }
    Some(normalized)
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::SliceLayer;

    #[test]
    fn test_generate_gcode_empty_layers_contains_header() {
        let gcode = generate_gcode(&[], &SlicingParams::default());
        assert!(gcode.contains("G28"), "missing G28 home");
        assert!(gcode.contains("G21"), "missing G21 mm mode");
        assert!(gcode.contains("M104 S210"), "missing nozzle temp");
        assert!(gcode.contains("M140 S60"), "missing bed temp");
    }

    #[test]
    fn test_generate_gcode_empty_layers_contains_footer() {
        let gcode = generate_gcode(&[], &SlicingParams::default());
        assert!(gcode.contains("M104 S0"), "missing nozzle off");
        assert!(gcode.contains("M140 S0"), "missing bed off");
        assert!(gcode.contains("M84"), "missing motors off");
    }

    #[test]
    fn test_generate_gcode_layer_z_appears() {
        let layer = SliceLayer::new(1.0);
        let gcode = generate_gcode(&[layer], &SlicingParams::default());
        // With lifecycle markers on by default, expect LAYER_CHANGE block
        assert!(
            gcode.contains(";LAYER_CHANGE"),
            "missing LAYER_CHANGE marker: {gcode}"
        );
        assert!(
            gcode.contains(";Z:1.000"),
            "missing ;Z: annotation: {gcode}"
        );
        assert!(gcode.contains("G1 Z1.000"), "missing Z move");
    }

    #[test]
    fn test_generate_gcode_with_contour() {
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.2);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(square);

        let gcode = generate_gcode(&[layer], &SlicingParams::default());
        assert!(gcode.contains(" E"), "no extrusion moves in gcode");
        assert!(gcode.contains("X0.000 Y0.000"), "missing start travel");
    }

    // ── Advanced retraction modes (issue #96) ─────────────────────────────────

    /// A closed square whose lower-left corner is `(x0, y0)`.
    fn square_at(x0: f64, y0: f64) -> clipper2::Path {
        vec![
            (x0, y0),
            (x0 + 10.0, y0),
            (x0 + 10.0, y0 + 10.0),
            (x0, y0 + 10.0),
        ]
        .into()
    }

    fn one_square_layer() -> SliceLayer {
        let mut layer = SliceLayer::new(0.2);
        layer.paths.push(square_at(0.0, 0.0));
        layer
    }

    /// The first line containing `needle`, or `""` if none.
    fn line_with<'a>(gcode: &'a str, needle: &str) -> &'a str {
        gcode.lines().find(|l| l.contains(needle)).unwrap_or("")
    }

    #[test]
    fn default_retraction_is_software_absolute() {
        // With every advanced mode off the output is a plain absolute-E software
        // retract — the baseline behaviour must be untouched.
        let gcode = generate_gcode(&[one_square_layer()], &SlicingParams::default());
        assert!(gcode.contains("M82"), "default must be absolute E: {gcode}");
        assert!(!gcode.contains("M83"), "default must not be relative E");
        assert!(
            !gcode.contains("G10"),
            "default must not use firmware retract"
        );
        assert!(
            line_with(&gcode, "; retract").contains("E-1.00000"),
            "default retract must be a software E move: {gcode}"
        );
        assert!(
            line_with(&gcode, "; un-retract").contains("E0.00000"),
            "default un-retract returns to the absolute datum: {gcode}"
        );
    }

    #[test]
    fn firmware_retraction_emits_g10_g11_and_setup() {
        let params = SlicingParams {
            use_firmware_retraction: true,
            ..SlicingParams::default()
        };
        let gcode = generate_gcode(&[one_square_layer()], &params);
        assert!(
            gcode.contains("M207"),
            "missing firmware retract setup: {gcode}"
        );
        assert!(
            gcode.contains("M208"),
            "missing firmware recover setup: {gcode}"
        );
        assert!(
            gcode.contains("G10 ; firmware retract"),
            "missing G10 firmware retract: {gcode}"
        );
        assert!(
            gcode.contains("G11 ; firmware recover"),
            "missing G11 firmware recover: {gcode}"
        );
        // Firmware retraction must not also move the extruder axis for the pull.
        assert!(
            !gcode.lines().any(|l| l.ends_with("; retract")),
            "firmware retraction must not emit a software retract: {gcode}"
        );
    }

    #[test]
    fn firmware_retraction_klipper_uses_set_retraction() {
        let params = SlicingParams {
            use_firmware_retraction: true,
            ..SlicingParams::default()
        };
        let gcode =
            GcodeGenerator::new(GcodeFlavor::Klipper).generate(&[one_square_layer()], &params);
        assert!(
            gcode.contains("SET_RETRACTION"),
            "Klipper firmware retraction setup missing: {gcode}"
        );
        assert!(
            gcode.contains("G10") && gcode.contains("G11"),
            "missing G10/G11: {gcode}"
        );
    }

    #[test]
    fn relative_e_distances_emit_m83_and_deltas() {
        let params = SlicingParams {
            use_relative_e_distances: true,
            ..SlicingParams::default()
        };
        let gcode = generate_gcode(&[one_square_layer()], &params);
        assert!(
            gcode.contains("M83"),
            "missing relative-E mode line: {gcode}"
        );
        // The un-retract is the raw retract length as a delta, not an absolute
        // datum return.
        assert!(
            line_with(&gcode, "; un-retract").contains("E1.00000"),
            "relative un-retract must prime the retract length: {gcode}"
        );
        // Every extrusion delta is a per-segment length (well under the running
        // absolute total an absolute stream would reach on a 40 mm perimeter).
        let max_e = gcode
            .lines()
            .filter(|l| l.contains(" X") && l.contains(" E"))
            .filter_map(|l| l.split_whitespace().find(|t| t.starts_with('E')))
            .filter_map(|t| t[1..].parse::<f64>().ok())
            .fold(0.0_f64, f64::max);
        assert!(
            max_e < 5.0,
            "relative extrusion deltas must stay small, got {max_e}: {gcode}"
        );
    }

    #[test]
    fn restart_extra_adds_prime_on_unretract() {
        let params = SlicingParams {
            use_relative_e_distances: true,
            retract_restart_extra_mm: 0.5,
            ..SlicingParams::default()
        };
        let gcode = generate_gcode(&[one_square_layer()], &params);
        // retract_mm (1.0) + restart_extra (0.5) = 1.5 mm primed on recover.
        assert!(
            line_with(&gcode, "; un-retract").contains("E1.50000"),
            "restart-extra must be added to the un-retract: {gcode}"
        );
    }

    #[test]
    fn retract_on_layer_change_retracts_before_z() {
        let mut l1 = SliceLayer::new(0.2);
        l1.paths.push(square_at(0.0, 0.0));
        let mut l2 = SliceLayer::new(0.4);
        l2.paths.push(square_at(0.0, 0.0));
        let layers = vec![l1, l2];

        let off = generate_gcode(&layers, &SlicingParams::default());
        let on = generate_gcode(
            &layers,
            &SlicingParams {
                retract_on_layer_change: true,
                ..SlicingParams::default()
            },
        );

        let second_lc = |g: &str| g.match_indices(";LAYER_CHANGE").nth(1).map(|(i, _)| i);
        let on_idx = second_lc(&on).expect("two layers -> two LAYER_CHANGE markers");
        let off_idx = second_lc(&off).expect("two layers -> two LAYER_CHANGE markers");
        assert!(
            on[..on_idx].trim_end().ends_with("; retract"),
            "layer-change retract must precede the layer-change marker: {on}"
        );
        assert!(
            !off[..off_idx].trim_end().ends_with("; retract"),
            "without the flag no retract precedes the layer-change marker: {off}"
        );
    }

    #[test]
    fn wipe_emits_wipe_moves_that_retract() {
        // Two squares 5 mm apart in one layer: the second path's retract wipes
        // along the first path's trajectory.
        let mut layer = SliceLayer::new(0.2);
        layer.paths.push(square_at(0.0, 0.0));
        layer.paths.push(square_at(0.0, 5.0));

        let off = generate_gcode(&[layer.clone()], &SlicingParams::default());
        assert!(!off.contains("; wipe"), "no wipe without the flag: {off}");

        let params = SlicingParams {
            wipe: true,
            wipe_distance_mm: 2.0,
            use_relative_e_distances: true,
            ..SlicingParams::default()
        };
        let on = generate_gcode(&[layer], &params);
        assert!(
            on.contains("; wipe"),
            "wipe flag must emit wipe moves: {on}"
        );
        // A wipe combines an XY move with a retraction (negative E delta).
        assert!(
            on.lines()
                .any(|l| l.contains("; wipe") && l.contains(" X") && l.contains("E-")),
            "wipe move must retract while moving: {on}"
        );
    }

    #[test]
    fn retract_before_travel_threshold_suppresses_short_hops() {
        // Two squares 5 mm apart. The default retracts on that 5 mm hop; a large
        // `retract_before_travel_mm` suppresses it (only the always-retract first
        // path of the layer remains).
        let mut layer = SliceLayer::new(0.2);
        layer.paths.push(square_at(0.0, 0.0));
        layer.paths.push(square_at(0.0, 5.0));

        let count = |g: &str| g.lines().filter(|l| l.ends_with("; retract")).count();

        let default = generate_gcode(&[layer.clone()], &SlicingParams::default());
        let high = generate_gcode(
            &[layer],
            &SlicingParams {
                retract_before_travel_mm: 20.0,
                ..SlicingParams::default()
            },
        );
        assert_eq!(
            count(&default),
            2,
            "default retracts on the 5 mm hop: {default}"
        );
        assert_eq!(
            count(&high),
            1,
            "a 20 mm minimum suppresses the 5 mm hop retract: {high}"
        );
    }

    #[test]
    fn avoid_crossing_perimeters_detours_travel_around_a_wall() {
        use crate::core::ExtrusionRole;
        use clipper2::Path;

        // Three outer-wall squares in a row.  Print order A, C, B forces a
        // travel from A to C whose straight line passes through B.
        let mut layer = SliceLayer::new(0.2);
        let push_square = |layer: &mut SliceLayer, x0: f64, y0: f64, x1: f64, y1: f64| {
            let sq: Path = vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)].into();
            layer.paths.push(sq);
            layer.path_roles.push(ExtrusionRole::OuterWall);
            layer.path_widths.push(Some(0.4));
            layer.path_is_open.push(false);
        };
        push_square(&mut layer, 0.0, 0.0, 10.0, 10.0); // A
        push_square(&mut layer, 30.0, 0.0, 40.0, 10.0); // C
        push_square(&mut layer, 15.0, -5.0, 25.0, 15.0); // B (obstacle in between)

        let base = generate_gcode(&[layer.clone()], &SlicingParams::default());
        assert!(
            !base.contains("avoid crossing"),
            "control: default should not route around walls"
        );

        let params = SlicingParams {
            avoid_crossing_perimeters: true,
            ..SlicingParams::default()
        };
        let routed = generate_gcode(&[layer], &params);
        assert!(
            routed.contains("avoid crossing"),
            "avoid_crossing_perimeters should detour the A→C travel around B"
        );
    }

    #[test]
    fn flow_ratio_scales_extrusion_linearly() {
        let base = extrusion_for_move(10.0, 0.2, 0.4, 1.75, 1.0);
        let double = extrusion_for_move(10.0, 0.2, 0.4, 1.75, 2.0);
        let half = extrusion_for_move(10.0, 0.2, 0.4, 1.75, 0.5);
        assert!((double - 2.0 * base).abs() < 1e-9, "flow 2.0 must double E");
        assert!((half - 0.5 * base).abs() < 1e-9, "flow 0.5 must halve E");
    }

    #[test]
    fn flow_ratio_non_positive_or_nan_falls_back_to_unity() {
        let unity = extrusion_for_move(10.0, 0.2, 0.4, 1.75, 1.0);
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let e = extrusion_for_move(10.0, 0.2, 0.4, 1.75, bad);
            assert!(
                (e - unity).abs() < 1e-9,
                "flow_ratio {bad} must be treated as 1.0, got {e}"
            );
        }
    }

    #[test]
    fn flow_ratio_scales_total_gcode_extrusion() {
        use clipper2::Path;
        let p1 = SlicingParams {
            flow_ratio: 1.0,
            ..SlicingParams::default()
        };
        let p2 = SlicingParams {
            flow_ratio: 1.5,
            ..SlicingParams::default()
        };

        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        let mk = || {
            let mut layer = SliceLayer::new(0.2);
            layer.paths.push(square.clone());
            layer
        };
        let total_e = |gcode: &str| -> f64 {
            gcode
                .lines()
                .filter_map(|l| l.split_whitespace().find(|t| t.starts_with('E')))
                .filter_map(|t| t[1..].parse::<f64>().ok())
                .filter(|e| *e > 0.0)
                .fold(0.0, f64::max)
        };
        let e1 = total_e(&generate_gcode(&[mk()], &p1));
        let e2 = total_e(&generate_gcode(&[mk()], &p2));
        assert!(e1 > 0.0 && e2 > 0.0, "both prints must extrude");
        assert!(
            (e2 / e1 - 1.5).abs() < 1e-3,
            "1.5x flow_ratio must raise total extrusion 1.5x: {e1} -> {e2}"
        );
    }

    // ── Volumetric-flow ceiling (max_volumetric_speed) ─────────────────────────

    #[test]
    fn volumetric_cap_disabled_is_passthrough() {
        // A non-positive limit never touches the feedrate.
        for limit in [0.0, -1.0] {
            let s = volumetric_capped_speed_mm_min(3000.0, 0.2, 0.4, limit);
            assert!((s - 3000.0).abs() < 1e-9, "limit {limit} must pass through");
        }
    }

    #[test]
    fn volumetric_cap_leaves_slow_moves_untouched() {
        // 5 mm³/s ceiling, cross-section 0.2×0.4 = 0.08 mm² → cap = 3750 mm/min.
        // A 1000 mm/min request is already under the ceiling.
        let s = volumetric_capped_speed_mm_min(1000.0, 0.2, 0.4, 5.0);
        assert!(
            (s - 1000.0).abs() < 1e-9,
            "under-ceiling speed must be kept"
        );
    }

    #[test]
    fn volumetric_cap_clamps_fast_moves() {
        // cap = 5.0 * 60 / (0.2 * 0.4) = 3750 mm/min.
        let s = volumetric_capped_speed_mm_min(6000.0, 0.2, 0.4, 5.0);
        assert!(
            (s - 3750.0).abs() < 1e-6,
            "fast move must clamp to 3750: {s}"
        );
    }

    #[test]
    fn volumetric_cap_degenerate_cross_section_is_passthrough() {
        // Zero height or width must never divide-by-zero or stall the feedrate.
        assert!((volumetric_capped_speed_mm_min(6000.0, 0.0, 0.4, 5.0) - 6000.0).abs() < 1e-9);
        assert!((volumetric_capped_speed_mm_min(6000.0, 0.2, 0.0, 5.0) - 6000.0).abs() < 1e-9);
    }

    #[test]
    fn volumetric_cap_lowers_gcode_feedrate() {
        use clipper2::Path;
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        let mk = || {
            let mut layer = SliceLayer::new(0.2);
            layer.paths.push(square.clone());
            layer
        };
        // Highest extruding feedrate on a printing move (has both X and E).
        // Retract/un-retract moves carry an E but no X and must be excluded.
        let max_extrude_feedrate = |gcode: &str| -> f64 {
            gcode
                .lines()
                .filter(|l| l.contains(" X") && l.contains(" E"))
                .filter_map(|l| l.split_whitespace().find(|t| t.starts_with('F')))
                .filter_map(|t| t[1..].parse::<f64>().ok())
                .fold(0.0, f64::max)
        };

        let uncapped = SlicingParams {
            max_volumetric_speed: 0.0,
            ..SlicingParams::default()
        };
        let capped = SlicingParams {
            // Deliberately tiny ceiling so it bites at any sane print speed.
            max_volumetric_speed: 1.0,
            ..SlicingParams::default()
        };

        let f_uncapped = max_extrude_feedrate(&generate_gcode(&[mk()], &uncapped));
        let f_capped = max_extrude_feedrate(&generate_gcode(&[mk()], &capped));
        assert!(
            f_uncapped > 0.0 && f_capped > 0.0,
            "both prints must extrude"
        );
        assert!(
            f_capped < f_uncapped,
            "volumetric ceiling must lower the feedrate: {f_uncapped} -> {f_capped}"
        );
    }

    // ── Width resolution (line_width + per-role overrides) ─────────────────────

    /// A `SlicingParams` with a specific generic `line_width` and otherwise
    /// default (all per-role overrides `0.0` = "derive").
    fn params_with_line_width(line_width: f64) -> SlicingParams {
        SlicingParams {
            line_width,
            ..SlicingParams::default()
        }
    }

    #[test]
    fn resolve_width_explicit_wins_for_variable_width_beads() {
        // A variable-width Arachne bead carries authoritative per-vertex widths,
        // so its scalar explicit width is never overridden by a per-role setting.
        let mut params = params_with_line_width(0.44);
        params.outer_wall_line_width = 0.6;
        params.sparse_infill_line_width = 0.6;
        let w = resolve_width_mm(
            Some(0.55),
            true,
            crate::core::ExtrusionRole::Infill,
            &params,
        );
        assert_eq!(w, 0.55);
        let w = resolve_width_mm(
            Some(0.30),
            true,
            crate::core::ExtrusionRole::OuterWall,
            &params,
        );
        assert_eq!(w, 0.30);
    }

    #[test]
    fn resolve_width_per_role_override_beats_explicit_constant_width() {
        // Regression: the classic wall generator stamps every wall path with an
        // explicit, nozzle-derived constant width and no per-vertex widths. A
        // per-role wall override must still take effect — it is the whole point
        // of `outer_wall_line_width` / `inner_wall_line_width`.
        let mut params = params_with_line_width(0.0);
        params.outer_wall_line_width = 0.6;
        params.inner_wall_line_width = 0.3;
        // `explicit = Some(0.4)` mimics a classic wall bead; no vertex widths.
        assert_eq!(
            resolve_width_mm(
                Some(0.4),
                false,
                crate::core::ExtrusionRole::OuterWall,
                &params
            ),
            0.6
        );
        assert_eq!(
            resolve_width_mm(
                Some(0.4),
                false,
                crate::core::ExtrusionRole::InnerWall,
                &params
            ),
            0.3
        );
    }

    #[test]
    fn resolve_width_line_width_applies_to_infill_and_surfaces() {
        let params = params_with_line_width(0.44);
        // Every fill role is charged at the *line spacing* derived from the same
        // 0.44 nominal width, so the flow matches the pitch its lines are laid at
        // (`mm³/mm = spacing × height`): surfaces stay flat and sparse infill
        // deposits exactly the requested density.
        let expect = crate::core::extrusion_flow_spacing_mm(0.44, params.layer_height);
        for role in [
            crate::core::ExtrusionRole::Infill,
            crate::core::ExtrusionRole::TopSurface,
            crate::core::ExtrusionRole::BottomSurface,
        ] {
            assert_eq!(
                resolve_width_mm(None, false, role, &params),
                expect,
                "{role:?}"
            );
        }
    }

    #[test]
    fn resolve_width_line_width_ignored_for_walls_and_when_zero() {
        // Walls keep their role default regardless of the generic line_width.
        let params = params_with_line_width(0.44);
        assert_eq!(
            resolve_width_mm(None, false, crate::core::ExtrusionRole::OuterWall, &params),
            crate::core::ExtrusionRole::OuterWall.default_width_mm()
        );
        // line_width = 0 means "derive from nozzle" → the nozzle-derived spacing.
        let params = params_with_line_width(0.0);
        assert_eq!(
            resolve_width_mm(None, false, crate::core::ExtrusionRole::Infill, &params),
            crate::core::extrusion_flow_spacing_mm(params.nozzle_diameter_mm, params.layer_height)
        );
    }

    #[test]
    fn resolve_width_per_role_override_applies_to_walls() {
        // A per-role wall override wins over the role default even though the
        // generic line_width never applies to walls.
        let mut params = params_with_line_width(0.0);
        params.outer_wall_line_width = 0.5;
        params.inner_wall_line_width = 0.6;
        assert_eq!(
            resolve_width_mm(None, false, crate::core::ExtrusionRole::OuterWall, &params),
            0.5
        );
        assert_eq!(
            resolve_width_mm(None, false, crate::core::ExtrusionRole::InnerWall, &params),
            0.6
        );
        // OverhangPerimeter follows the outer-wall override.
        assert_eq!(
            resolve_width_mm(
                None,
                false,
                crate::core::ExtrusionRole::OverhangPerimeter,
                &params
            ),
            0.5
        );
    }

    /// Ironing deposits `ironing_flow` percent of a bead across a strip
    /// `ironing_spacing` wide, and folding the fraction into the width is what
    /// keeps that arithmetic out of reach of `extrusion_for_move`'s flow guard.
    #[test]
    fn resolve_width_charges_ironing_at_spacing_times_flow() {
        let mut params = SlicingParams {
            ironing_spacing: 0.1,
            ironing_flow: 10.0,
            ..SlicingParams::default()
        };
        assert!(
            (resolve_width_mm(None, false, crate::core::ExtrusionRole::Ironing, &params) - 0.01)
                .abs()
                < 1e-12
        );

        // A per-role surface width must not leak into ironing: `resolve_width_mm`
        // returns `top_surface_line_width` before it ever reads an explicit
        // width, so sharing the TopSurface role would iron at full flow on any
        // profile that sets one.
        params.top_surface_line_width = 0.5;
        params.line_width = 0.6;
        assert!(
            (resolve_width_mm(None, false, crate::core::ExtrusionRole::Ironing, &params) - 0.01)
                .abs()
                < 1e-12,
            "no width setting may override the ironing flow calculation"
        );
    }

    /// `extrusion_for_move` reads a non-positive flow ratio as 1.0 so a
    /// malformed profile cannot zero out a print. A "wipe only" ironing setting
    /// must therefore never travel through that parameter — it would lay a
    /// full-width bead at 0.1mm pitch across the whole top surface.
    #[test]
    fn zero_ironing_flow_deposits_nothing_rather_than_everything() {
        let params = SlicingParams {
            ironing_spacing: 0.1,
            ironing_flow: 0.0,
            ..SlicingParams::default()
        };

        let width = resolve_width_mm(None, false, crate::core::ExtrusionRole::Ironing, &params);
        assert_eq!(width, 0.0);

        let e = extrusion_for_move(10.0, params.layer_height, width, 1.75, 1.0);
        assert_eq!(e, 0.0, "zero flow must extrude nothing at all");
    }

    /// Guards the same guard from the other side: a negative or absurd flow is
    /// clamped rather than inverted.
    #[test]
    fn out_of_range_ironing_flow_is_clamped() {
        let low = SlicingParams {
            ironing_spacing: 0.1,
            ironing_flow: -25.0,
            ..SlicingParams::default()
        };
        let high = SlicingParams {
            ironing_spacing: 0.1,
            ironing_flow: 400.0,
            ..SlicingParams::default()
        };
        assert_eq!(
            resolve_width_mm(None, false, crate::core::ExtrusionRole::Ironing, &low),
            0.0
        );
        assert!(
            (resolve_width_mm(None, false, crate::core::ExtrusionRole::Ironing, &high) - 0.1).abs()
                < 1e-12,
            "flow above 100% must cap at a full bead of the ironing strip"
        );
    }

    #[test]
    fn resolve_width_per_role_override_beats_generic_line_width() {
        // For infill, the per-role override supplies the nominal width the
        // spacing is derived from, winning over the generic line_width.
        let mut params = params_with_line_width(0.44);
        params.sparse_infill_line_width = 0.7;
        params.top_surface_line_width = 0.35;
        assert_eq!(
            resolve_width_mm(None, false, crate::core::ExtrusionRole::Infill, &params),
            crate::core::extrusion_flow_spacing_mm(0.7, params.layer_height)
        );
        // Solid surfaces honour the per-role override for their nominal width,
        // but are charged at the line spacing derived from it so the deposited
        // volume matches the fill (no over-extrusion).
        let expect = crate::core::extrusion_flow_spacing_mm(0.35, params.layer_height);
        assert_eq!(
            resolve_width_mm(None, false, crate::core::ExtrusionRole::TopSurface, &params),
            expect
        );
        // Bottom surfaces share the top-surface override field.
        assert_eq!(
            resolve_width_mm(
                None,
                false,
                crate::core::ExtrusionRole::BottomSurface,
                &params
            ),
            expect
        );
    }

    // ── Absolute extrusion mode guarantee ──────────────────────────────────────

    #[test]
    fn klipper_forces_absolute_extrusion_after_start_print() {
        let layer = SliceLayer::new(0.2);
        let gcode =
            GcodeGenerator::new(GcodeFlavor::Klipper).generate(&[layer], &SlicingParams::default());
        let start = gcode.find("START_PRINT").expect("START_PRINT missing");
        let m82 = gcode.find("M82").expect("M82 absolute mode missing");
        assert!(
            m82 > start,
            "M82 must follow START_PRINT so a macro's M83 can't leave the extruder relative:\n{gcode}"
        );
        // And the counter is zeroed right after, before any extrusion.
        assert!(
            gcode.contains("G92 E0"),
            "G92 E0 missing after start: {gcode}"
        );
    }

    #[test]
    fn marlin_output_is_in_absolute_extrusion_mode() {
        let layer = SliceLayer::new(0.2);
        let gcode =
            GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[layer], &SlicingParams::default());
        assert!(
            gcode.contains("M82"),
            "Marlin output must set absolute E: {gcode}"
        );
    }

    // ── Flavor enum ────────────────────────────────────────────────────────────

    #[test]
    fn test_gcode_flavor_from_str() {
        assert_eq!(
            "marlin".parse::<GcodeFlavor>().unwrap(),
            GcodeFlavor::Marlin
        );
        assert_eq!(
            "klipper".parse::<GcodeFlavor>().unwrap(),
            GcodeFlavor::Klipper
        );
        assert_eq!(
            "Marlin".parse::<GcodeFlavor>().unwrap(),
            GcodeFlavor::Marlin
        );
        assert_eq!(
            "KLIPPER".parse::<GcodeFlavor>().unwrap(),
            GcodeFlavor::Klipper
        );
    }

    #[test]
    fn test_gcode_flavor_from_str_invalid() {
        let err = "reprap".parse::<GcodeFlavor>().unwrap_err();
        assert!(err.contains("reprap"), "error should mention the bad value");
        assert!(
            err.contains("marlin") && err.contains("klipper"),
            "error should list supported flavors"
        );
    }

    #[test]
    fn test_gcode_flavor_display() {
        assert_eq!(GcodeFlavor::Marlin.to_string(), "marlin");
        assert_eq!(GcodeFlavor::Klipper.to_string(), "klipper");
    }

    #[test]
    fn test_gcode_flavor_default_is_marlin() {
        assert_eq!(GcodeFlavor::default(), GcodeFlavor::Marlin);
    }

    // ── GcodeGenerator ─────────────────────────────────────────────────────────

    #[test]
    fn test_generator_marlin_contains_standard_header() {
        let gcode =
            GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[], &SlicingParams::default());
        assert!(gcode.contains("G21"), "missing unit mode");
        assert!(gcode.contains("G28"), "missing home");
        assert!(gcode.contains("M104 S210"), "missing nozzle temp");
        assert!(gcode.contains("M140 S60"), "missing bed temp");
    }

    #[test]
    fn test_generator_marlin_contains_standard_footer() {
        let gcode =
            GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[], &SlicingParams::default());
        assert!(gcode.contains("M104 S0"), "missing nozzle off");
        assert!(gcode.contains("M140 S0"), "missing bed off");
        assert!(gcode.contains("M84"), "missing motors off");
    }

    #[test]
    fn test_generator_klipper_uses_start_print_macro() {
        let gcode =
            GcodeGenerator::new(GcodeFlavor::Klipper).generate(&[], &SlicingParams::default());
        assert!(
            gcode.contains("START_PRINT"),
            "Klipper gcode missing START_PRINT macro: {gcode}"
        );
        assert!(
            gcode.contains("BED_TEMP=60"),
            "Klipper START_PRINT missing BED_TEMP: {gcode}"
        );
        assert!(
            gcode.contains("EXTRUDER_TEMP=210"),
            "Klipper START_PRINT missing EXTRUDER_TEMP: {gcode}"
        );
        assert!(
            gcode.contains("BED=60"),
            "Klipper START_PRINT missing Klippain BED alias: {gcode}"
        );
        assert!(
            gcode.contains("EXTRUDER=210"),
            "Klipper START_PRINT missing Klippain EXTRUDER alias: {gcode}"
        );
    }

    #[test]
    fn test_generator_klipper_uses_end_print_macro() {
        let gcode =
            GcodeGenerator::new(GcodeFlavor::Klipper).generate(&[], &SlicingParams::default());
        assert!(
            gcode.contains("END_PRINT"),
            "Klipper gcode missing END_PRINT macro: {gcode}"
        );
    }

    #[test]
    fn test_generator_klipper_flavor_name_in_header() {
        let gcode =
            GcodeGenerator::new(GcodeFlavor::Klipper).generate(&[], &SlicingParams::default());
        assert!(
            gcode.contains("Klipper"),
            "header should mention Klipper flavor"
        );
    }

    #[test]
    fn test_generator_marlin_flavor_name_in_header() {
        let gcode =
            GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[], &SlicingParams::default());
        assert!(
            gcode.contains("Marlin"),
            "header should mention Marlin flavor"
        );
    }

    #[test]
    fn test_generator_klipper_layer_and_contour() {
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.2);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(square);

        let gcode =
            GcodeGenerator::new(GcodeFlavor::Klipper).generate(&[layer], &SlicingParams::default());
        // Lifecycle markers are on by default
        assert!(
            gcode.contains(";LAYER_CHANGE"),
            "missing LAYER_CHANGE marker"
        );
        assert!(gcode.contains(";Z:0.200"), "missing ;Z: annotation");
        assert!(gcode.contains(" E"), "no extrusion moves");
        assert!(gcode.contains("X0.000 Y0.000"), "missing start travel");
    }

    // ── KlipperDialect extras ──────────────────────────────────────────────────

    #[test]
    fn test_klipper_dialect_set_pressure_advance() {
        let d = KlipperDialect;
        let cmd = d.set_pressure_advance(0.05);
        assert_eq!(cmd, "SET_PRESSURE_ADVANCE ADVANCE=0.0500");
    }

    #[test]
    fn test_marlin_dialect_set_pressure_advance_default() {
        let d = MarlinDialect;
        // Marlin uses the linear-advance `M900 K` form via the trait default.
        assert_eq!(d.set_pressure_advance(0.05), "M900 K0.0500");
    }

    #[test]
    fn test_generator_emits_pressure_advance_klipper() {
        let params = SlicingParams {
            pressure_advance: 0.035,
            ..SlicingParams::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Klipper).generate(&[], &params);
        assert!(
            gcode.contains("SET_PRESSURE_ADVANCE ADVANCE=0.0350 ; pressure advance"),
            "Klipper pressure advance not emitted:\n{gcode}"
        );
    }

    #[test]
    fn test_generator_emits_pressure_advance_marlin() {
        let params = SlicingParams {
            pressure_advance: 0.06,
            ..SlicingParams::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[], &params);
        assert!(
            gcode.contains("M900 K0.0600 ; pressure advance"),
            "Marlin pressure advance not emitted:\n{gcode}"
        );
    }

    #[test]
    fn test_generator_omits_pressure_advance_when_zero() {
        let params = SlicingParams::default(); // pressure_advance defaults to 0.0
        let klipper = GcodeGenerator::new(GcodeFlavor::Klipper).generate(&[], &params);
        let marlin = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[], &params);
        assert!(
            !klipper.contains("SET_PRESSURE_ADVANCE"),
            "Klipper emitted pressure advance when disabled:\n{klipper}"
        );
        assert!(
            !marlin.contains("M900 K"),
            "Marlin emitted pressure advance when disabled:\n{marlin}"
        );
    }

    #[test]
    fn test_pressure_advance_emitted_after_start_script() {
        let params = SlicingParams {
            pressure_advance: 0.04,
            ..SlicingParams::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Klipper).generate(&[], &params);
        let pa = gcode
            .find("SET_PRESSURE_ADVANCE")
            .expect("pressure advance present");
        let start = gcode.find("START_PRINT").expect("start script present");
        assert!(
            pa > start,
            "pressure advance must be emitted after the start script"
        );
    }

    // ── Acceleration (Phase 2 — layer-type) ─────────────────────────────────────

    /// Build a single-path layer at height `z` with the given role.
    fn layer_with_role(z: f64, role: crate::core::ExtrusionRole) -> SliceLayer {
        use clipper2::Path;
        let mut layer = SliceLayer::new(z);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(square);
        layer.path_roles.push(role);
        layer
    }

    #[test]
    fn test_marlin_dialect_set_acceleration_default() {
        let d = MarlinDialect;
        assert_eq!(d.set_acceleration(6000.0), "M204 P6000");
    }

    #[test]
    fn test_klipper_dialect_set_acceleration() {
        let d = KlipperDialect;
        assert_eq!(d.set_acceleration(6000.0), "SET_VELOCITY_LIMIT ACCEL=6000");
    }

    #[test]
    fn test_generator_emits_first_layer_then_normal_acceleration() {
        use crate::core::ExtrusionRole;
        let params = SlicingParams {
            acceleration: 6000.0,
            first_layer_acceleration: 2000.0,
            ..SlicingParams::default()
        };
        // z=0.2 → first layer (layer_height 0.2); z=0.4 → subsequent.
        let layers = [
            layer_with_role(0.2, ExtrusionRole::OuterWall),
            layer_with_role(0.4, ExtrusionRole::OuterWall),
        ];
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&layers, &params);
        let first = gcode.find("M204 P2000").expect("first-layer accel");
        let normal = gcode.find("M204 P6000").expect("normal accel");
        assert!(
            first < normal,
            "first-layer acceleration must precede the normal acceleration:\n{gcode}"
        );
    }

    #[test]
    fn test_generator_emits_top_surface_acceleration() {
        use crate::core::ExtrusionRole;
        let params = SlicingParams {
            acceleration: 6000.0,
            top_surface_acceleration: 9000.0,
            ..SlicingParams::default()
        };
        // Non-first layer with a wall followed by a top surface.
        let mut layer = SliceLayer::new(0.4);
        let sq1: clipper2::Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        let sq2: clipper2::Path = vec![(1.0, 1.0), (9.0, 1.0), (9.0, 9.0), (1.0, 9.0)].into();
        layer.paths.push(sq1);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.paths.push(sq2);
        layer.path_roles.push(ExtrusionRole::TopSurface);
        let gcode = GcodeGenerator::new(GcodeFlavor::Klipper).generate(&[layer], &params);
        let wall = gcode
            .find("SET_VELOCITY_LIMIT ACCEL=6000")
            .expect("normal accel");
        let top = gcode
            .find("SET_VELOCITY_LIMIT ACCEL=9000")
            .expect("top-surface accel");
        assert!(
            wall < top,
            "top-surface acceleration must follow the wall acceleration:\n{gcode}"
        );
    }

    #[test]
    fn test_generator_omits_acceleration_when_zero() {
        use crate::core::ExtrusionRole;
        let params = SlicingParams::default(); // all acceleration fields default to 0
        let layers = [layer_with_role(0.4, ExtrusionRole::OuterWall)];
        let marlin = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&layers, &params);
        let klipper = GcodeGenerator::new(GcodeFlavor::Klipper).generate(&layers, &params);
        assert!(
            !marlin.contains("M204 P"),
            "Marlin emitted acceleration when disabled:\n{marlin}"
        );
        assert!(
            !klipper.contains("SET_VELOCITY_LIMIT ACCEL="),
            "Klipper emitted acceleration when disabled:\n{klipper}"
        );
    }

    #[test]
    fn test_generator_no_redundant_acceleration() {
        use crate::core::ExtrusionRole;
        let params = SlicingParams {
            acceleration: 6000.0,
            ..SlicingParams::default()
        };
        // Two non-first layers, same role → the accel command must appear once.
        let layers = [
            layer_with_role(0.4, ExtrusionRole::OuterWall),
            layer_with_role(0.6, ExtrusionRole::OuterWall),
        ];
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&layers, &params);
        let count = gcode.matches("M204 P6000").count();
        assert_eq!(
            count, 1,
            "acceleration should only be emitted on change:\n{gcode}"
        );
    }

    // ── Acceleration (Phase 3 — geometry aware) ─────────────────────────────────

    #[test]
    fn test_bridge_acceleration_applies_to_bridge_and_overhang() {
        use crate::core::ExtrusionRole;
        let params = SlicingParams {
            acceleration: 6000.0,
            bridge_acceleration: 1500.0,
            ..SlicingParams::default()
        };
        for role in [ExtrusionRole::Bridge, ExtrusionRole::OverhangPerimeter] {
            let layers = [layer_with_role(0.4, role)];
            let gcode = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&layers, &params);
            assert!(
                gcode.contains("M204 P1500"),
                "bridge acceleration not applied to {role:?}:\n{gcode}"
            );
        }
    }

    #[test]
    fn test_outer_wall_acceleration_applies_to_outer_wall_only() {
        use crate::core::ExtrusionRole;
        let params = SlicingParams {
            acceleration: 6000.0,
            outer_wall_acceleration: 3000.0,
            ..SlicingParams::default()
        };
        // Outer wall → dedicated accel; inner wall → normal accel.
        let mut layer = SliceLayer::new(0.4);
        let sq1: clipper2::Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        let sq2: clipper2::Path = vec![(1.0, 1.0), (9.0, 1.0), (9.0, 9.0), (1.0, 9.0)].into();
        layer.paths.push(sq1);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.paths.push(sq2);
        layer.path_roles.push(ExtrusionRole::InnerWall);
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[layer], &params);
        let outer = gcode.find("M204 P3000").expect("outer-wall accel");
        let inner = gcode.find("M204 P6000").expect("inner-wall normal accel");
        assert!(
            outer < inner,
            "outer-wall accel must precede inner-wall normal accel:\n{gcode}"
        );
    }

    #[test]
    fn test_first_layer_acceleration_overrides_bridge() {
        use crate::core::ExtrusionRole;
        let params = SlicingParams {
            acceleration: 6000.0,
            bridge_acceleration: 1500.0,
            first_layer_acceleration: 2000.0,
            ..SlicingParams::default()
        };
        // A bridge on the first layer must still use the first-layer accel.
        let layers = [layer_with_role(0.2, ExtrusionRole::Bridge)];
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&layers, &params);
        assert!(
            gcode.contains("M204 P2000") && !gcode.contains("M204 P1500"),
            "first-layer accel must win over bridge accel on the first layer:\n{gcode}"
        );
    }

    #[test]
    fn test_bridge_acceleration_falls_back_to_normal() {
        use crate::core::ExtrusionRole;
        let params = SlicingParams {
            acceleration: 6000.0, // bridge_acceleration left at 0
            ..SlicingParams::default()
        };
        let layers = [layer_with_role(0.4, ExtrusionRole::Bridge)];
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&layers, &params);
        assert!(
            gcode.contains("M204 P6000"),
            "bridge role should fall back to the normal acceleration:\n{gcode}"
        );
    }

    #[test]
    fn test_klipper_dialect_set_velocity_limit() {
        let d = KlipperDialect;
        let cmd = d.set_velocity_limit(200.0, 3000.0);
        assert_eq!(cmd, "SET_VELOCITY_LIMIT VELOCITY=200 ACCEL=3000");
    }

    #[test]
    fn test_klipper_dialect_call_macro() {
        let d = KlipperDialect;
        assert_eq!(d.call_macro("print_start"), "PRINT_START");
        assert_eq!(d.call_macro("PRINT_END"), "PRINT_END");
    }

    // ── GcodeDialect default methods ───────────────────────────────────────────

    #[test]
    fn test_dialect_default_comment() {
        let d = MarlinDialect;
        assert_eq!(d.comment("hello"), "; hello");
    }

    #[test]
    fn test_dialect_default_set_nozzle_temp() {
        let d = MarlinDialect;
        assert_eq!(d.set_nozzle_temp(210.0, false), "M104 S210");
        assert_eq!(d.set_nozzle_temp(210.0, true), "M109 S210");
    }

    #[test]
    fn test_dialect_default_set_bed_temp() {
        let d = MarlinDialect;
        assert_eq!(d.set_bed_temp(60.0, false), "M140 S60");
        assert_eq!(d.set_bed_temp(60.0, true), "M190 S60");
    }

    #[test]
    fn test_dialect_default_set_fan_speed() {
        let d = MarlinDialect;
        assert_eq!(d.set_fan_speed(0.0), "M107");
        assert_eq!(d.set_fan_speed(1.0), "M106 S255");
        assert_eq!(d.set_fan_speed(0.5), "M106 S128");
    }

    #[test]
    fn test_dialect_default_home_axes() {
        let d = MarlinDialect;
        assert_eq!(d.home_axes(), "G28");
    }

    #[test]
    fn test_dialect_default_reset_extruder() {
        let d = MarlinDialect;
        assert_eq!(d.reset_extruder(), "G92 E0");
    }

    // ── with_dialect (custom dialect) ─────────────────────────────────────────

    #[test]
    fn test_generator_with_custom_dialect() {
        let gen = GcodeGenerator::with_dialect(Box::new(KlipperDialect));
        let gcode = gen.generate(&[], &SlicingParams::default());
        assert!(gcode.contains("START_PRINT"));
    }

    // ── warn_fn mechanism ──────────────────────────────────────────────────────

    #[test]
    fn test_warn_fn_called_for_unsupported_commands() {
        use std::sync::{Arc, Mutex};

        // A test dialect that advertises one unsupported command
        struct LimitedDialect;
        impl GcodeDialect for LimitedDialect {
            fn flavor_name(&self) -> &'static str {
                "Limited"
            }
            fn start_script(&self, _: &SlicingParams) -> Vec<String> {
                vec![]
            }
            fn end_script(&self) -> Vec<String> {
                vec![]
            }
            fn unsupported_commands(&self) -> &'static [&'static str] {
                &["set_fan_speed"]
            }
        }

        let warnings: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
        let warnings_clone = Arc::clone(&warnings);
        let gen = GcodeGenerator::with_dialect(Box::new(LimitedDialect))
            .with_warn_fn(move |msg| warnings_clone.lock().unwrap().push(msg.to_string()));

        gen.generate(&[], &SlicingParams::default());

        let warnings = warnings.lock().unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("set_fan_speed"),
            "warning should mention the unsupported command"
        );
        assert!(
            warnings[0].contains("Limited"),
            "warning should mention the dialect name"
        );
    }

    #[test]
    fn test_no_warn_fn_is_silent() {
        // Verify the generator doesn't panic when no warn_fn is set
        // even if the dialect lists unsupported commands
        struct NoFanDialect;
        impl GcodeDialect for NoFanDialect {
            fn flavor_name(&self) -> &'static str {
                "NoFan"
            }
            fn start_script(&self, _: &SlicingParams) -> Vec<String> {
                vec![]
            }
            fn end_script(&self) -> Vec<String> {
                vec![]
            }
            fn unsupported_commands(&self) -> &'static [&'static str] {
                &["set_fan_speed"]
            }
        }

        // Should not panic
        let gen = GcodeGenerator::with_dialect(Box::new(NoFanDialect));
        let gcode = gen.generate(&[], &SlicingParams::default());
        assert!(gcode.contains("; generated by Cold Crabby"));
    }

    // ── Lifecycle markers ──────────────────────────────────────────────────────

    #[test]
    fn test_lifecycle_markers_enabled_by_default() {
        let layer = SliceLayer::new(0.2);
        let gcode =
            GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[layer], &SlicingParams::default());
        assert!(
            gcode.contains(";LAYER_CHANGE"),
            "LAYER_CHANGE must be present"
        );
        assert!(gcode.contains(";Z:0.200"), ";Z: annotation must be present");
        assert!(
            gcode.contains(";HEIGHT:0.200"),
            ";HEIGHT: annotation must be present"
        );
        assert!(
            gcode.contains(";BEFORE_LAYER_CHANGE"),
            ";BEFORE_LAYER_CHANGE must be present"
        );
        assert!(
            gcode.contains(";AFTER_LAYER_CHANGE"),
            ";AFTER_LAYER_CHANGE must be present"
        );
        assert!(gcode.contains("G92 E0"), "extruder reset must be present");
    }

    #[test]
    fn test_layer_time_marker_emitted() {
        let layer = SliceLayer::new(0.2);
        let gcode =
            GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[layer], &SlicingParams::default());
        assert!(
            gcode.contains(";LAYER_TIME:"),
            ";LAYER_TIME: marker must be present for the viewer's Layer Time mode"
        );
    }

    #[test]
    fn test_acceleration_aware_estimate_patches_markers_and_stats() {
        use clipper2::Path;

        // Three square-wall layers so travel, Z lifts and ramps all contribute.
        let mut layers = Vec::new();
        for i in 0..3 {
            let z = 0.2 * (i as f64 + 1.0);
            let mut layer = SliceLayer::new(z);
            let square: Path = vec![(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)].into();
            layer.paths.push(square);
            layer.path_roles.push(crate::core::ExtrusionRole::OuterWall);
            layers.push(layer);
        }
        let params = SlicingParams::default();
        let (gcode, stats) =
            GcodeGenerator::new(GcodeFlavor::Marlin).generate_with_stats(&layers, &params);

        // Collect the patched per-layer marker values.
        let marker_times: Vec<f64> = gcode
            .lines()
            .filter_map(|l| l.trim().strip_prefix(";LAYER_TIME:"))
            .filter_map(|v| v.trim().parse::<f64>().ok())
            .collect();
        assert_eq!(marker_times.len(), 3, "one marker per printed layer");
        assert!(
            marker_times.iter().all(|&t| t > 0.0),
            "every layer must report a positive time: {marker_times:?}"
        );

        // The acceleration-aware total must exceed the old naive length ÷ speed
        // sum: ramps, travel, Z lifts and retraction all add real time the naive
        // model ignored.
        let naive_total: f64 = layers
            .iter()
            .map(|l| estimate_layer_time(l, params.print_speed))
            .sum();
        assert!(
            stats.estimated_print_time_s > naive_total,
            "accel-aware total {} must exceed naive {naive_total}",
            stats.estimated_print_time_s
        );

        // Every marker must have been rewritten away from the naive placeholder.
        for (i, l) in layers.iter().enumerate() {
            let naive = estimate_layer_time(l, params.print_speed);
            assert!(
                (marker_times[i] - naive).abs() > 1e-6,
                "layer {i} marker {} must be patched off the naive value {naive}",
                marker_times[i]
            );
        }

        // Per-layer marker sum is a subset of the total (which also counts the
        // start/end script), so it must not exceed it.
        let marker_sum: f64 = marker_times.iter().sum();
        assert!(
            marker_sum <= stats.estimated_print_time_s + 1e-6,
            "layer sum {marker_sum} must not exceed total {}",
            stats.estimated_print_time_s
        );

        // Header/footer print the same human-formatted figure the stats carry.
        let human = stats.estimated_time_human();
        assert!(
            gcode.contains(&format!("; estimated printing time = {human}")),
            "header ETA must match stats: {human}"
        );
        assert!(
            gcode.contains(&format!(
                "; estimated printing time (normal mode) = {human}"
            )),
            "footer ETA must match stats: {human}"
        );
    }

    #[test]
    fn test_kinematic_limits_emitted_per_flavor() {
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.2);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(square);
        layer.path_roles.push(crate::core::ExtrusionRole::OuterWall);

        let params = SlicingParams {
            square_corner_velocity: 8.0,
            max_velocity: 300.0,
            acceleration: 3000.0,
            ..SlicingParams::default()
        };

        // Marlin: M203 velocity cap + M205 J junction deviation.
        let marlin = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[layer.clone()], &params);
        assert!(
            marlin.contains("M203 X300 Y300"),
            "Marlin must emit the max-feedrate cap: {marlin}"
        );
        assert!(
            marlin.contains("M205 J"),
            "Marlin must emit a junction-deviation limit: {marlin}"
        );

        // Klipper: a single SET_VELOCITY_LIMIT with both native fields.
        let klipper = GcodeGenerator::new(GcodeFlavor::Klipper).generate(&[layer], &params);
        assert!(
            klipper.contains("VELOCITY=300"),
            "Klipper must set the velocity cap: {klipper}"
        );
        assert!(
            klipper.contains("SQUARE_CORNER_VELOCITY=8.00"),
            "Klipper must set the square-corner velocity: {klipper}"
        );
    }

    #[test]
    fn test_kinematic_limits_absent_when_unset() {
        let layer = SliceLayer::new(0.2);
        // Defaults: square_corner_velocity = 0, max_velocity = 0 → no emission.
        let gcode =
            GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[layer], &SlicingParams::default());
        assert!(
            !gcode.contains("M203"),
            "no velocity cap must be emitted by default"
        );
        assert!(
            !gcode.contains("M205 J"),
            "no junction-deviation limit must be emitted by default"
        );
    }

    #[test]
    fn test_time_estimate_calibration_scales_and_offsets_total() {
        use clipper2::Path;
        let mk_layers = || {
            let mut v = Vec::new();
            for i in 0..2 {
                let mut l = SliceLayer::new(0.2 * (i as f64 + 1.0));
                let square: Path = vec![(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)].into();
                l.paths.push(square);
                l.path_roles.push(crate::core::ExtrusionRole::OuterWall);
                v.push(l);
            }
            v
        };

        let base_params = SlicingParams::default();
        let (_g, base) = GcodeGenerator::new(GcodeFlavor::Marlin)
            .generate_with_stats(&mk_layers(), &base_params);

        // scale = 2.0 doubles the toolpath portion; warmup/cooldown add on top.
        let cal_params = SlicingParams {
            time_estimate_scale: 2.0,
            time_estimate_warmup_s: 100.0,
            time_estimate_cooldown_s: 50.0,
            ..SlicingParams::default()
        };
        let (gcode, cal) =
            GcodeGenerator::new(GcodeFlavor::Marlin).generate_with_stats(&mk_layers(), &cal_params);

        let expected = base.estimated_print_time_s * 2.0 + 150.0;
        assert!(
            (cal.estimated_print_time_s - expected).abs() < 1e-6,
            "calibrated total {} must equal toolpath×2 + 150 = {expected}",
            cal.estimated_print_time_s
        );

        // Per-layer markers are scaled (×2) but carry no fixed allowance. They
        // are formatted to 0.1 s, so compare with rounding slack rather than
        // exactly. The fixed 150 s allowance must be absent from the markers.
        let marker_sum: f64 = gcode
            .lines()
            .filter_map(|l| l.trim().strip_prefix(";LAYER_TIME:"))
            .filter_map(|v| v.trim().parse::<f64>().ok())
            .sum();
        assert!(
            marker_sum < cal.estimated_print_time_s - 100.0,
            "marker sum {marker_sum} must exclude the 150s fixed allowance (total {})",
            cal.estimated_print_time_s
        );
        assert!(
            marker_sum > base.estimated_print_time_s,
            "scaled markers {marker_sum} should exceed the unscaled toolpath total {}",
            base.estimated_print_time_s
        );
    }

    #[test]
    fn test_lifecycle_markers_disabled_emits_legacy_comment() {
        let layer = SliceLayer::new(0.2);
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin)
            .with_lifecycle_markers(false)
            .generate(&[layer], &SlicingParams::default());
        assert!(
            gcode.contains("; layer z=0.200"),
            "legacy comment must appear when markers disabled"
        );
        assert!(
            !gcode.contains(";LAYER_CHANGE"),
            "LAYER_CHANGE must NOT appear when markers disabled"
        );
    }

    #[test]
    fn test_lifecycle_markers_type_annotation_emitted() {
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.2);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(square);
        layer.path_roles.push(crate::core::ExtrusionRole::OuterWall);

        let gcode =
            GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[layer], &SlicingParams::default());
        assert!(
            gcode.contains(";TYPE:Outer wall"),
            ";TYPE: annotation must be present"
        );
        assert!(
            gcode.contains(";WIDTH:0.40mm"),
            ";WIDTH: annotation must be present"
        );
    }

    #[test]
    fn per_vertex_widths_taper_extrusion_flow() {
        use clipper2::Path;
        // An open bead with an asymmetric width taper: the second segment is
        // 0.6 mm wide vs 0.4 mm for the first, so it must extrude 1.5x the
        // filament of the first over the same 5 mm length.
        let mut layer = SliceLayer::new(0.2);
        let bead: Path = vec![(0.0, 0.0), (5.0, 0.0), (10.0, 0.0)].into();
        layer.paths.push(bead);
        layer.path_roles.push(crate::core::ExtrusionRole::InnerWall);
        layer.path_widths.push(Some(0.5));
        layer.path_vertex_widths.push(Some(vec![0.4, 0.4, 0.8]));
        layer.path_is_open.push(true);

        let gcode =
            GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[layer], &SlicingParams::default());

        let e: Vec<f64> = gcode
            .lines()
            .filter(|l| l.starts_with("G1") && l.contains('X') && l.contains('E'))
            .filter_map(|l| {
                l.split_whitespace()
                    .find_map(|t| t.strip_prefix('E').and_then(|v| v.parse::<f64>().ok()))
            })
            .collect();
        assert_eq!(e.len(), 2, "expected two extrude moves, got {e:?}");
        let d0 = e[0];
        let d1 = e[1] - e[0];
        assert!(d0 > 0.0 && d1 > 0.0, "both segments must extrude");
        let ratio = d1 / d0;
        assert!(
            (ratio - 1.5).abs() < 1e-3,
            "tapered segment should extrude 1.5x the first (got {ratio:.4})"
        );
    }

    #[test]
    fn gap_fill_role_emits_orca_type_label() {
        use clipper2::Path;
        // A GapFill path must annotate as OrcaSlicer's `;TYPE:Gap infill` so
        // previews and post-processors classify it correctly.
        let mut layer = SliceLayer::new(0.2);
        let bead: Path = vec![(0.0, 0.0), (5.0, 0.0)].into();
        layer.paths.push(bead);
        layer.path_roles.push(crate::core::ExtrusionRole::GapFill);
        layer.path_widths.push(Some(0.45));
        layer.path_is_open.push(true);

        let gcode =
            GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[layer], &SlicingParams::default());
        assert!(
            gcode.contains(";TYPE:Gap infill"),
            "gap fill must emit the OrcaSlicer ;TYPE:Gap infill label:\n{gcode}"
        );
    }

    #[test]
    fn per_role_line_width_flows_into_width_annotation() {
        use clipper2::Path;
        // A classic (non-Arachne) outer wall — the wall generator stamps an
        // explicit, nozzle-derived constant width (0.40 mm) and no per-vertex
        // widths. The configured `outer_wall_line_width` must still win and
        // drive the `;WIDTH:` annotation. This is the regression: previously the
        // explicit width shadowed the override, so walls printed at 0.40 mm.
        let mut layer = SliceLayer::new(0.2);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(square);
        layer.path_roles.push(crate::core::ExtrusionRole::OuterWall);
        layer.path_widths.push(Some(0.40));

        let params = SlicingParams {
            outer_wall_line_width: 0.55,
            ..SlicingParams::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[layer], &params);
        assert!(
            gcode.contains(";TYPE:Outer wall"),
            "outer wall TYPE must be present:\n{gcode}"
        );
        assert!(
            gcode.contains(";WIDTH:0.55mm"),
            "per-role outer_wall_line_width must override the stamped 0.40 mm wall width:\n{gcode}"
        );
        assert!(
            !gcode.contains(";WIDTH:0.40mm"),
            "the stamped classic wall width must not survive the override:\n{gcode}"
        );
    }

    #[test]
    fn skirt_role_emits_type_and_width_annotation() {
        use clipper2::Path;
        // A Skirt path (adhesion helper) must annotate as `;TYPE:Skirt` with a
        // `;WIDTH:` so previews classify it correctly once skirt geometry is
        // generated.
        let mut layer = SliceLayer::new(0.2);
        let loop_path: Path = vec![(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)].into();
        layer.paths.push(loop_path);
        layer.path_roles.push(crate::core::ExtrusionRole::Skirt);

        let gcode =
            GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[layer], &SlicingParams::default());
        assert!(
            gcode.contains(";TYPE:Skirt"),
            "skirt must emit ;TYPE:Skirt:\n{gcode}"
        );
        assert!(
            gcode.contains(";WIDTH:0.40mm"),
            "skirt must emit a ;WIDTH: annotation:\n{gcode}"
        );
    }

    #[test]
    fn test_lifecycle_markers_type_transition_emitted_once_per_role() {
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.2);
        let sq: Path = vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].into();
        // Two Perimeter paths followed by one Infill path
        layer.paths.push(sq.clone());
        layer.paths.push(sq.clone());
        layer.paths.push(sq);
        layer.path_roles.push(crate::core::ExtrusionRole::OuterWall);
        layer.path_roles.push(crate::core::ExtrusionRole::OuterWall);
        layer.path_roles.push(crate::core::ExtrusionRole::Infill);

        let gcode =
            GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[layer], &SlicingParams::default());

        // OuterWall TYPE should appear exactly once (no duplicate at role boundary)
        let outer_wall_count = gcode.matches(";TYPE:Outer wall").count();
        assert_eq!(
            outer_wall_count, 1,
            "Outer wall TYPE emitted {} times",
            outer_wall_count
        );

        // Infill TYPE should appear exactly once
        let infill_count = gcode.matches(";TYPE:Sparse infill").count();
        assert_eq!(
            infill_count, 1,
            "Infill TYPE emitted {} times",
            infill_count
        );
    }

    #[test]
    fn test_lifecycle_markers_no_type_when_disabled() {
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.2);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(square);

        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin)
            .with_lifecycle_markers(false)
            .generate(&[layer], &SlicingParams::default());
        assert!(
            !gcode.contains(";TYPE:"),
            ";TYPE: must NOT appear when markers disabled"
        );
    }

    #[test]
    fn test_lifecycle_markers_custom_layer_change_template() {
        let layer = SliceLayer::new(0.4);
        let config = LifecycleMarkerConfig {
            layer_change: Some(";CUSTOM_LAYER z={z} h={height}".to_string()),
            ..LifecycleMarkerConfig::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin)
            .with_marker_config(config)
            .generate(&[layer], &SlicingParams::default());
        assert!(
            gcode.contains(";CUSTOM_LAYER z=0.400 h=0.200"),
            "custom layer_change template not rendered: {gcode}"
        );
    }

    #[test]
    fn test_lifecycle_markers_custom_type_annotation() {
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.2);
        let sq: Path = vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].into();
        layer.paths.push(sq);
        layer.path_roles.push(crate::core::ExtrusionRole::Infill);

        let config = LifecycleMarkerConfig {
            type_annotation: Some(";FEATURE {type}".to_string()),
            ..LifecycleMarkerConfig::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin)
            .with_marker_config(config)
            .generate(&[layer], &SlicingParams::default());
        assert!(
            gcode.contains(";FEATURE Sparse infill"),
            "custom type annotation not rendered: {gcode}"
        );
    }

    #[test]
    fn test_with_marker_config_disabled() {
        let layer = SliceLayer::new(0.2);
        let config = LifecycleMarkerConfig {
            enabled: false,
            ..LifecycleMarkerConfig::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin)
            .with_marker_config(config)
            .generate(&[layer], &SlicingParams::default());
        assert!(!gcode.contains(";LAYER_CHANGE"));
        assert!(gcode.contains("; layer z=0.200"));
    }

    // ── Custom start / end scripts ─────────────────────────────────────────────

    #[test]
    fn test_custom_start_script_overrides_dialect() {
        let gen = GcodeGenerator::new(GcodeFlavor::Marlin)
            .with_start_script(vec!["MY_CUSTOM_START".to_string()]);
        let gcode = gen.generate(&[], &SlicingParams::default());
        assert!(
            gcode.contains("MY_CUSTOM_START"),
            "custom start script not emitted"
        );
        // G21 (mm mode) is only in the Marlin start script, not the end script —
        // it should be absent when the start script is fully overridden.
        assert!(
            !gcode.contains("G21"),
            "dialect default start should be suppressed by custom script"
        );
    }

    #[test]
    fn test_custom_end_script_overrides_dialect() {
        let gen = GcodeGenerator::new(GcodeFlavor::Marlin)
            .with_end_script(vec!["MY_CUSTOM_END".to_string()]);
        let gcode = gen.generate(&[], &SlicingParams::default());
        assert!(
            gcode.contains("MY_CUSTOM_END"),
            "custom end script not emitted"
        );
        // Marlin's M84 should NOT be present when custom script overrides it
        assert!(
            !gcode.contains("M84"),
            "dialect default should be suppressed by custom end script"
        );
    }

    #[test]
    fn test_custom_start_script_klipper_override() {
        let gen = GcodeGenerator::new(GcodeFlavor::Klipper)
            .with_start_script(vec!["START_PRINT BED_TEMP=65 EXTRUDER_TEMP=215".to_string()]);
        let gcode = gen.generate(&[], &SlicingParams::default());
        assert!(gcode.contains("BED_TEMP=65"));
        assert!(gcode.contains("EXTRUDER_TEMP=215"));
    }

    #[test]
    fn test_custom_scripts_multiline() {
        let gen = GcodeGenerator::new(GcodeFlavor::Marlin)
            .with_start_script(vec![
                "G28 ; custom home".to_string(),
                "M190 S65 ; bed".to_string(),
            ])
            .with_end_script(vec!["M84 ; motors off".to_string()]);
        let gcode = gen.generate(&[], &SlicingParams::default());
        assert!(gcode.contains("G28 ; custom home"));
        assert!(gcode.contains("M190 S65"));
        assert!(gcode.contains("M84 ; motors off"));
    }

    #[test]
    fn test_start_script_substitutes_temperature_placeholders() {
        let params = SlicingParams {
            nozzle_temp: 215.0,
            bed_temp: 65.0,
            nozzle_temp_first_layer: 220.0,
            bed_temp_first_layer: 0.0, // falls back to bed_temp
            ..SlicingParams::default()
        };
        let gen = GcodeGenerator::new(GcodeFlavor::Klipper).with_start_script(vec![
            "START_PRINT EXTRUDER_TEMP={nozzle_temp_first_layer} BED_TEMP={bed_temp_first_layer}"
                .to_string(),
        ]);
        let gcode = gen.generate(&[], &params);
        assert!(
            gcode.contains("EXTRUDER_TEMP=220"),
            "first-layer nozzle temp not substituted: {gcode}"
        );
        assert!(
            gcode.contains("BED_TEMP=65"),
            "bed temp fallback not substituted: {gcode}"
        );
    }

    #[test]
    fn test_start_script_substitutes_chamber_and_material_placeholders() {
        let params = SlicingParams {
            nozzle_temp_first_layer: 255.0,
            bed_temp_first_layer: 105.0,
            chamber_temp: 50.0,
            filament_type: "ABS".to_string(),
            ..SlicingParams::default()
        };
        let gen = GcodeGenerator::new(GcodeFlavor::Klipper).with_start_script(vec![
            "START_PRINT EXTRUDER={nozzle_temp_first_layer} BED={bed_temp_first_layer} \
CHAMBER={chamber_temp} MATERIAL={filament_type}"
                .to_string(),
        ]);
        let gcode = gen.generate(&[], &params);
        assert!(
            gcode.contains("EXTRUDER=255 BED=105 CHAMBER=50 MATERIAL=ABS"),
            "Klippain start line not fully substituted: {gcode}"
        );
    }

    #[test]
    fn test_start_script_substitutes_orca_bracket_placeholders() {
        let params = SlicingParams {
            nozzle_temp: 210.0,
            bed_temp: 60.0,
            nozzle_temp_first_layer: 215.0,
            bed_temp_first_layer: 65.0,
            chamber_temp: 45.0,
            filament_type: "PLA".to_string(),
            ..SlicingParams::default()
        };
        let gen = GcodeGenerator::new(GcodeFlavor::Klipper).with_start_script(vec![
            "START_PRINT EXTRUDER=[nozzle_temperature_initial_layer] BED=[bed_temperature_initial_layer_single] CHAMBER=[chamber_temperature] MATERIAL=[filament_type]"
                .to_string(),
        ]);
        let gcode = gen.generate(&[], &params);
        assert!(
            gcode.contains("START_PRINT EXTRUDER=215 BED=65 CHAMBER=45 MATERIAL=PLA"),
            "Orca-style bracket placeholders were not substituted: {gcode}"
        );
    }

    // ── Chamber temperature management ───────────────────────────────────────

    #[test]
    fn test_dialect_default_set_chamber_temp() {
        let d = MarlinDialect;
        assert_eq!(d.set_chamber_temp(50.0, false), "M141 S50");
        assert_eq!(d.set_chamber_temp(50.0, true), "M191 S50");
    }

    #[test]
    fn test_klipper_dialect_set_chamber_temp_is_native() {
        // Klipper has no built-in M141/M191 and aborts on unknown commands.
        let d = KlipperDialect;
        assert_eq!(
            d.set_chamber_temp(50.0, false),
            "SET_HEATER_TEMPERATURE HEATER=chamber TARGET=50"
        );
        assert_eq!(
            d.set_chamber_temp(50.0, true),
            "TEMPERATURE_WAIT SENSOR=\"heater_generic chamber\" MINIMUM=50"
        );
    }

    #[test]
    fn test_chamber_soak_precedes_the_start_script_and_the_bed_leads_it() {
        let params = SlicingParams {
            heated_chamber: true,
            chamber_temp: 50.0,
            bed_temp: 100.0,
            bed_temp_first_layer: 105.0,
            ..SlicingParams::default()
        };
        let gen = GcodeGenerator::new(GcodeFlavor::Marlin)
            .with_start_script(vec!["G28 ; home".to_string()]);
        let gcode = gen.generate(&[], &params);

        let bed = gcode
            .find("M140 S105 ; bed target")
            .expect("expected the bed to be armed before the soak");
        let set = gcode.find("M141 S50").expect("expected M141");
        let wait = gcode.find("M191 S50").expect("expected M191");
        let home = gcode.find("G28 ; home").expect("expected start script");
        assert!(
            bed < set && set < wait,
            "the bed — the chamber's heat source — must be armed before the soak:\n{gcode}"
        );
        assert!(
            wait < home,
            "the soak must finish before the start script heats the nozzle:\n{gcode}"
        );
    }

    #[test]
    fn test_no_chamber_directives_without_heated_chamber() {
        // The filament asks for a chamber; the printer has no chamber heater.
        let params = SlicingParams {
            heated_chamber: false,
            chamber_temp: 50.0,
            ..SlicingParams::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[], &params);
        assert!(
            !gcode.contains("M141") && !gcode.contains("M191"),
            "chamber directives need the printer's heated_chamber capability:\n{gcode}"
        );
    }

    #[test]
    fn test_no_chamber_directives_when_temp_is_zero() {
        let params = SlicingParams {
            heated_chamber: true,
            chamber_temp: 0.0,
            ..SlicingParams::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[], &params);
        assert!(
            !gcode.contains("M141") && !gcode.contains("M191"),
            "a chamber target of 0 means 'don't manage the chamber':\n{gcode}"
        );
    }

    #[test]
    fn test_custom_start_script_owns_chamber_heating() {
        let params = SlicingParams {
            heated_chamber: true,
            chamber_temp: 50.0,
            ..SlicingParams::default()
        };
        let gen = GcodeGenerator::new(GcodeFlavor::Klipper).with_start_script(vec![
            "START_PRINT BED=105 CHAMBER={chamber_temp}".to_string(),
        ]);
        let gcode = gen.generate(&[], &params);
        assert!(
            gcode.contains("CHAMBER=50"),
            "the macro must still get its substituted value: {gcode}"
        );
        assert!(
            !gcode.contains("SET_HEATER_TEMPERATURE HEATER=chamber")
                && !gcode.contains("TEMPERATURE_WAIT"),
            "a start script that heats the chamber must not be doubled up:\n{gcode}"
        );
        assert!(
            gcode.contains("chamber temperature handled by the custom start G-code"),
            "the suppression should be visible in the output:\n{gcode}"
        );
    }

    #[test]
    fn test_bed_heater_wait_does_not_suppress_chamber_heating() {
        // A Klipper macro that waits on the *bed* must not be mistaken for one
        // that manages the chamber.
        let params = SlicingParams {
            heated_chamber: true,
            chamber_temp: 50.0,
            ..SlicingParams::default()
        };
        let gen = GcodeGenerator::new(GcodeFlavor::Klipper).with_start_script(vec![
            "SET_HEATER_TEMPERATURE HEATER=heater_bed TARGET=105".to_string(),
            "TEMPERATURE_WAIT SENSOR=\"heater_bed\" MINIMUM=105".to_string(),
        ]);
        let gcode = gen.generate(&[], &params);
        assert!(
            gcode.contains("SET_HEATER_TEMPERATURE HEATER=chamber TARGET=50"),
            "bed heater commands must not suppress chamber heating:\n{gcode}"
        );
    }

    #[test]
    fn test_chamber_fan_does_not_suppress_chamber_heating() {
        // Enclosed printers routinely drive a chamber circulation fan. Matching
        // a bare "chamber" would silently disable chamber *heating* — the exact
        // failure this feature prevents.
        let params = SlicingParams {
            heated_chamber: true,
            chamber_temp: 50.0,
            ..SlicingParams::default()
        };
        for script in [
            "SET_FAN_SPEED FAN=chamber_fan SPEED=1.0",
            "M106 P2 S255 ; chamber fan",
            "; keep the chamber door closed",
        ] {
            let gen = GcodeGenerator::new(GcodeFlavor::Marlin)
                .with_start_script(vec![script.to_string()]);
            let gcode = gen.generate(&[], &params);
            assert!(
                gcode.contains("M141 S50") && gcode.contains("M191 S50"),
                "'{script}' must not be read as chamber heating:\n{gcode}"
            );
        }
    }

    #[test]
    fn test_klipper_chamber_heater_script_suppresses_chamber_heating() {
        // The native Klipper form, written by hand, *is* chamber management.
        let params = SlicingParams {
            heated_chamber: true,
            chamber_temp: 50.0,
            ..SlicingParams::default()
        };
        for script in [
            "SET_HEATER_TEMPERATURE HEATER=chamber TARGET=50",
            "TEMPERATURE_WAIT SENSOR=\"heater_generic chamber\" MINIMUM=50",
        ] {
            let gen = GcodeGenerator::new(GcodeFlavor::Klipper)
                .with_start_script(vec![script.to_string()]);
            let gcode = gen.generate(&[], &params);
            assert!(
                gcode.contains("chamber temperature handled by the custom start G-code"),
                "'{script}' should hand chamber management to the script:\n{gcode}"
            );
        }
    }

    #[test]
    fn test_chamber_soaks_hotter_for_the_first_layer_then_restores() {
        let params = SlicingParams {
            heated_chamber: true,
            chamber_temp: 45.0,
            chamber_temp_first_layer: 60.0,
            ..SlicingParams::default()
        };
        let gen = GcodeGenerator::new(GcodeFlavor::Marlin)
            .with_start_script(vec!["G28 ; home".to_string()])
            .with_lifecycle_markers(false);
        let layers = vec![SliceLayer::new(0.2), SliceLayer::new(0.4)];
        let gcode = gen.generate(&layers, &params);

        assert!(
            gcode.contains("M141 S60"),
            "first-layer soak target: {gcode}"
        );
        assert!(gcode.contains("M191 S60"), "first-layer soak wait: {gcode}");
        assert!(
            gcode.contains("M141 S45 ; restore normal chamber temperature"),
            "chamber must drop back to the steady-state target: {gcode}"
        );
        assert!(
            !gcode.contains("M191 S45"),
            "the layer-2 restore must never block:\n{gcode}"
        );
    }

    #[test]
    fn test_chamber_first_layer_placeholders_substitute() {
        let params = SlicingParams {
            chamber_temp: 45.0,
            chamber_temp_first_layer: 60.0,
            ..SlicingParams::default()
        };
        let gen = GcodeGenerator::new(GcodeFlavor::Klipper).with_start_script(vec![
            "START_PRINT SOAK={chamber_temp_first_layer} HOLD=[chamber_temperature] \
             ORCA=[chamber_temperature_initial_layer]"
                .to_string(),
        ]);
        let gcode = gen.generate(&[], &params);
        assert!(
            gcode.contains("SOAK=60 HOLD=45 ORCA=60"),
            "chamber first-layer placeholders not substituted: {gcode}"
        );
    }

    #[test]
    fn test_restores_normal_temperatures_on_second_layer_when_first_layer_overridden() {
        let params = SlicingParams {
            nozzle_temp: 210.0,
            bed_temp: 60.0,
            nozzle_temp_first_layer: 215.0,
            bed_temp_first_layer: 65.0,
            ..SlicingParams::default()
        };
        let gen = GcodeGenerator::new(GcodeFlavor::Klipper)
            .with_start_script(vec![
                "START_PRINT EXTRUDER={nozzle_temp_first_layer} BED={bed_temp_first_layer}"
                    .to_string(),
            ])
            .with_lifecycle_markers(false);

        let layers = vec![SliceLayer::new(0.2), SliceLayer::new(0.4)];
        let gcode = gen.generate(&layers, &params);

        assert!(
            gcode.contains("START_PRINT EXTRUDER=215 BED=65"),
            "first-layer start temperatures missing: {gcode}"
        );
        assert!(
            gcode.contains("M104 S210 ; restore normal nozzle temperature"),
            "normal nozzle temperature was not restored on layer 2: {gcode}"
        );
        assert!(
            gcode.contains("M140 S60 ; restore normal bed temperature"),
            "normal bed temperature was not restored on layer 2: {gcode}"
        );
    }

    #[test]
    fn test_layer_script_emitted_with_placeholders() {
        let layer = SliceLayer::new(0.2);
        let gen = GcodeGenerator::new(GcodeFlavor::Klipper)
            .with_layer_script(vec!["_ON_LAYER_CHANGE LAYER={layer_num} Z={z}".to_string()]);
        let gcode = gen.generate(&[layer], &SlicingParams::default());
        assert!(
            gcode.contains("_ON_LAYER_CHANGE LAYER=1 Z=0.200"),
            "layer script not rendered: {gcode}"
        );
    }

    #[test]
    fn test_generate_gcode_from_params_honors_flavor_and_scripts() {
        let params = SlicingParams {
            gcode_flavor: GcodeFlavor::Klipper,
            start_gcode: Some("START_PRINT BED_TEMP={bed_temp}".to_string()),
            end_gcode: Some("  \n  ".to_string()), // blank → keep dialect default
            ..SlicingParams::default()
        };
        let gcode = generate_gcode_from_params(&[], &params);
        assert!(
            gcode.contains("START_PRINT BED_TEMP=60"),
            "start not applied: {gcode}"
        );
        // Blank end block keeps the Klipper dialect default.
        assert!(
            gcode.contains("END_PRINT"),
            "blank end should fall back: {gcode}"
        );
    }

    #[test]
    fn test_filament_gcode_emitted_between_machine_scripts() {
        let params = SlicingParams {
            gcode_flavor: GcodeFlavor::Klipper,
            start_gcode: Some("START_PRINT BED_TEMP={bed_temp}".to_string()),
            end_gcode: Some("END_PRINT".to_string()),
            start_filament_gcode: Some("M117 LOADING {filament_type}".to_string()),
            end_filament_gcode: Some("M117 DONE {filament_type}".to_string()),
            filament_type: "PETG".to_string(),
            ..SlicingParams::default()
        };
        let gcode = generate_gcode_from_params(&[], &params);
        // Placeholders in the filament blocks are substituted.
        assert!(
            gcode.contains("M117 LOADING PETG"),
            "filament start not applied: {gcode}"
        );
        assert!(
            gcode.contains("M117 DONE PETG"),
            "filament end not applied: {gcode}"
        );
        // Ordering: machine start → filament start → filament end → machine end.
        let machine_start = gcode.find("START_PRINT").expect("machine start");
        let fil_start = gcode.find("M117 LOADING").expect("filament start");
        let fil_end = gcode.find("M117 DONE").expect("filament end");
        let machine_end = gcode.find("END_PRINT").expect("machine end");
        assert!(
            machine_start < fil_start && fil_start < fil_end && fil_end < machine_end,
            "unexpected ordering ({machine_start} {fil_start} {fil_end} {machine_end}): {gcode}"
        );
    }

    #[test]
    fn test_filament_gcode_blank_block_is_noop() {
        let params = SlicingParams {
            gcode_flavor: GcodeFlavor::Klipper,
            start_filament_gcode: Some("   \n\t ".to_string()),
            end_filament_gcode: None,
            ..SlicingParams::default()
        };
        // A blank filament block must not emit stray blank lines or panic.
        let gcode = generate_gcode_from_params(&[], &params);
        assert!(gcode.contains("START_PRINT") || gcode.contains("G28"));
    }

    #[test]
    fn test_generate_gcode_from_params_embeds_thumbnail_block() {
        let params = SlicingParams {
            thumbnail_enabled: true,
            thumbnail_size_px: 256,
            thumbnail_png_base64: Some(
                "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8Xw8AAoMBgM4n4VwAAAAASUVORK5CYII="
                    .to_string(),
            ),
            ..SlicingParams::default()
        };
        let gcode = generate_gcode_from_params(&[], &params);
        assert!(
            gcode.contains("; thumbnail begin 256x256 "),
            "missing thumbnail start marker: {gcode}"
        );
        assert!(
            gcode.contains("; thumbnail end"),
            "missing thumbnail end marker: {gcode}"
        );
    }

    #[test]
    fn test_generate_gcode_from_params_skips_invalid_thumbnail_payload() {
        let params = SlicingParams {
            thumbnail_enabled: true,
            thumbnail_png_base64: Some("not base64?!".to_string()),
            ..SlicingParams::default()
        };
        let gcode = generate_gcode_from_params(&[], &params);
        assert!(
            !gcode.contains("; thumbnail begin"),
            "invalid payload must be ignored"
        );
    }

    // ── Metadata header ────────────────────────────────────────────────────────

    #[test]
    fn test_metadata_header_contains_settings() {
        let gcode =
            GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[], &SlicingParams::default());
        assert!(
            gcode.contains("; layer_height: 0.2 mm"),
            "missing layer_height"
        );
        assert!(
            gcode.contains("; nozzle_temp: 210 °C"),
            "missing nozzle_temp"
        );
        assert!(gcode.contains("; bed_temp: 60 °C"), "missing bed_temp");
        assert!(
            gcode.contains("; print_speed: 60 mm/s"),
            "missing print_speed"
        );
        assert!(
            gcode.contains("; wall_count: 3 walls (arachne generator)"),
            "missing wall_count"
        );
        assert!(
            gcode.contains("; infill_density: 20%"),
            "missing infill_density"
        );
    }

    #[test]
    fn test_metadata_header_statistics_block_marlin() {
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.2);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(square);
        layer.path_roles.push(crate::core::ExtrusionRole::OuterWall);

        let (gcode, stats) = GcodeGenerator::new(GcodeFlavor::Marlin)
            .with_model_name("widget")
            .generate_with_stats(&[layer], &SlicingParams::default());

        // Orca/Marlin-style delimiters wrap the metadata block.
        assert!(
            gcode.contains("; HEADER_BLOCK_START"),
            "missing HEADER_BLOCK_START: {gcode}"
        );
        assert!(
            gcode.contains("; HEADER_BLOCK_END"),
            "missing HEADER_BLOCK_END"
        );
        // Provenance, model name, and per-slice statistics.
        assert!(
            gcode.contains("; generated by Cold Crabby"),
            "missing provenance line"
        );
        assert!(gcode.contains("; model: widget"), "missing model name");
        assert!(
            gcode.contains("; total layers count = 1"),
            "missing layer count"
        );
        assert!(
            gcode.contains("; max_z_height = 0.20 mm"),
            "missing max_z_height"
        );
        assert!(
            gcode.contains("; filament used [mm] ="),
            "missing filament length"
        );
        assert!(
            gcode.contains("; filament used [g] ="),
            "missing filament weight"
        );
        assert!(
            gcode.contains("; estimated printing time ="),
            "missing print time"
        );
        assert!(
            gcode.contains("; model bounding box [mm] ="),
            "missing bounding box"
        );

        // The returned statistics agree with the emitted header.
        assert_eq!(stats.layer_count, 1);
        assert!(stats.filament_mm > 0.0, "extruding layer must use filament");
        assert!(
            stats.filament_g > 0.0,
            "weight derives from filament + density"
        );
        assert!(
            gcode.contains(&format!("; filament used [mm] = {:.2}", stats.filament_mm)),
            "header filament length must match returned stats"
        );
    }

    #[test]
    fn test_klipper_header_uses_klipper_markers() {
        let gcode =
            GcodeGenerator::new(GcodeFlavor::Klipper).generate(&[], &SlicingParams::default());
        assert!(
            gcode.contains("; KLIPPER_HEADER_START"),
            "Klipper must use its own header markers: {gcode}"
        );
        assert!(
            gcode.contains("; KLIPPER_HEADER_END"),
            "missing Klipper end marker"
        );
        assert!(
            !gcode.contains("; HEADER_BLOCK_START"),
            "Klipper must not emit the Marlin header markers"
        );
        assert!(gcode.contains("; flavor: Klipper"), "missing flavor line");
    }

    #[test]
    fn test_metadata_footer_is_moonraker_parseable() {
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.2);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(square);
        layer.path_roles.push(crate::core::ExtrusionRole::OuterWall);

        let params = SlicingParams {
            filament_type: "PETG".to_string(),
            filament_name: "Generic PETG".to_string(),
            filament_color: "#1A9CE0".to_string(),
            nozzle_diameter_mm: 0.4,
            filament_diameter_mm: 1.75,
            ..SlicingParams::default()
        };

        let (gcode, _stats) =
            GcodeGenerator::new(GcodeFlavor::Klipper).generate_with_stats(&[layer], &params);

        // The PrusaSlicer-family provenance line Moonraker keys off to select
        // its (rich) parser: `; generated by <name> on <YYYY-MM-DD at HH:MM:SS>`.
        assert!(
            gcode.contains("; generated by Cold Crabby"),
            "missing provenance line Moonraker identifies us by"
        );

        // The config block that printer front-ends actually scan (in the footer).
        for needle in [
            "; prusaslicer_config = begin",
            "; prusaslicer_config = end",
            "; total filament used [g] = ",
            "; filament used [mm] = ",
            "; estimated printing time (normal mode) = ",
            "; total layers count = 1",
            "; layer_height = 0.2",
            "; first_layer_height = 0.2",
            "; nozzle_diameter = 0.4",
            "; filament_diameter = 1.75",
            "; filament_type = PETG",
            "; filament_settings_id = Generic PETG",
            "; filament_colour = #1A9CE0",
        ] {
            assert!(
                gcode.contains(needle),
                "footer missing Moonraker field `{needle}`:\n{gcode}"
            );
        }

        // The config block is a *footer*: it must come after the print body
        // (the Klipper `END_PRINT` macro), not in the header.
        let end_idx = gcode.find("END_PRINT").expect("end script present");
        let cfg_idx = gcode
            .find("; prusaslicer_config = begin")
            .expect("config block present");
        assert!(
            cfg_idx > end_idx,
            "metadata config block must be emitted at the end of the file"
        );
    }

    #[test]
    fn test_metadata_footer_omits_filament_identity_when_unset() {
        // A flag-only slice (no filament profile) must not advertise an empty
        // material / name / colour.
        let gcode =
            GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[], &SlicingParams::default());
        assert!(
            gcode.contains("; prusaslicer_config = begin"),
            "config block should always be present"
        );
        assert!(
            !gcode.contains("; filament_type ="),
            "empty filament type must be omitted"
        );
        assert!(
            !gcode.contains("; filament_settings_id ="),
            "empty filament name must be omitted"
        );
        assert!(
            !gcode.contains("; filament_colour ="),
            "empty filament colour must be omitted"
        );
        assert!(
            !gcode.contains("; extruder_colour ="),
            "empty extruder colour must be omitted"
        );
        assert!(
            !gcode.contains("; printer_vendor ="),
            "empty printer vendor must be omitted"
        );
        assert!(
            !gcode.contains("; printer_model ="),
            "empty printer model must be omitted"
        );
        assert!(
            !gcode.contains("; total filament cost ="),
            "unpriced filament must not report a cost"
        );
    }

    /// The machine-identity + cost fields printer front-ends display alongside
    /// a job (issue #23). All are omitted when unset — see the test above.
    #[test]
    fn test_metadata_footer_reports_machine_identity_and_cost() {
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.2);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(square);
        layer.path_roles.push(crate::core::ExtrusionRole::OuterWall);

        let params = SlicingParams {
            printer_vendor: "Voron".to_string(),
            printer_model: "Trident 300".to_string(),
            filament_color: "#1A9CE0".to_string(),
            filament_cost_per_kg: 25.0,
            ..SlicingParams::default()
        };

        let (gcode, stats) =
            GcodeGenerator::new(GcodeFlavor::Klipper).generate_with_stats(&[layer], &params);

        for needle in [
            "; printer_vendor = Voron",
            "; printer_model = Trident 300",
            "; extruder_colour = #1A9CE0",
        ] {
            assert!(
                gcode.contains(needle),
                "footer missing `{needle}`:\n{gcode}"
            );
        }

        assert!(stats.filament_cost > 0.0, "priced filament must cost money");
        assert!(
            gcode.contains(&format!(
                "; total filament cost = {:.2}",
                stats.filament_cost
            )),
            "footer cost must match the stats figure ({}):\n{gcode}",
            stats.filament_cost
        );
    }

    #[test]
    fn test_header_omits_model_line_when_unset() {
        let gcode =
            GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[], &SlicingParams::default());
        assert!(
            !gcode.contains("; model:"),
            "no model line should appear when name is unset: {gcode}"
        );
    }

    #[test]
    fn test_bed_type_tracked_in_header_when_set() {
        let params = SlicingParams {
            bed_type: "Textured PEI Plate".to_string(),
            ..SlicingParams::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[], &params);
        assert!(
            gcode.contains("; bed_type: Textured PEI Plate"),
            "missing bed_type line: {gcode}"
        );
    }

    #[test]
    fn test_bed_type_omitted_when_empty() {
        let gcode =
            GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[], &SlicingParams::default());
        assert!(
            !gcode.contains("; bed_type:"),
            "bed_type line must be absent when unset: {gcode}"
        );
    }

    // ── resolve_gcode_source ───────────────────────────────────────────────────

    #[test]
    fn test_resolve_gcode_source_inline_string() {
        use crate::gcode::source::resolve_gcode_source;
        let lines = resolve_gcode_source("G28\nM109 S210").unwrap();
        assert_eq!(lines, vec!["G28", "M109 S210"]);
    }

    #[test]
    fn test_resolve_gcode_source_single_line() {
        use crate::gcode::source::resolve_gcode_source;
        let lines = resolve_gcode_source("START_PRINT BED_TEMP=60 EXTRUDER_TEMP=210").unwrap();
        assert_eq!(
            lines,
            vec!["START_PRINT BED_TEMP=60 EXTRUDER_TEMP=210".to_string()]
        );
    }

    #[test]
    fn test_resolve_gcode_source_from_file() {
        use crate::gcode::source::resolve_gcode_source;
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "G28 ; home").unwrap();
        writeln!(tmp, "M109 S210 ; wait").unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        let lines = resolve_gcode_source(&path).unwrap();
        assert_eq!(lines, vec!["G28 ; home", "M109 S210 ; wait"]);
    }

    #[test]
    fn test_resolve_gcode_source_file_too_large() {
        use crate::gcode::source::resolve_gcode_source;
        use std::io::Write;
        // Create a file that exceeds the 1 MiB limit
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        let big_line = "G1 X0 Y0\n".repeat(200_000); // ~1.8 MiB
        tmp.write_all(big_line.as_bytes()).unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        let err = resolve_gcode_source(&path).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(
            err.to_string().contains("too large"),
            "error should mention file is too large: {err}"
        );
    }

    // ── render_marker ──────────────────────────────────────────────────────────

    #[test]
    fn test_render_marker_substitutes_all_placeholders() {
        let result = render_marker(
            ";z={z} h={height} t={type} w={width}",
            "0.200",
            "0.200",
            "Perimeter",
            "0.40",
        );
        assert_eq!(result, ";z=0.200 h=0.200 t=Perimeter w=0.40");
    }

    #[test]
    fn test_render_marker_no_placeholders() {
        let result = render_marker(";LAYER_CHANGE", "0.200", "0.200", "", "");
        assert_eq!(result, ";LAYER_CHANGE");
    }

    // ── Path simplification (Douglas-Peucker integration) ──────────────────────

    #[test]
    fn test_path_simplification_reduces_collinear_moves() {
        use clipper2::Path;

        // A path with collinear intermediate points — simplification at 0.05 mm
        // should collapse them to just the two endpoints.
        // Use a diagonal line: (0,0) → (1,1) → (2,2) → (3,3) → (4,4)
        // All intermediate points lie exactly on the chord.
        let mut layer = SliceLayer::new(0.2);
        let path: Path = vec![(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0), (4.0, 4.0)].into();
        layer.paths.push(path);

        let params_with_simplification = SlicingParams {
            path_tolerance: 0.05,
            ..SlicingParams::default()
        };
        let params_no_simplification = SlicingParams {
            path_tolerance: 0.0,
            ..SlicingParams::default()
        };

        let gcode_simplified = GcodeGenerator::new(GcodeFlavor::Marlin)
            .with_lifecycle_markers(false)
            .generate(&[layer.clone()], &params_with_simplification);
        let gcode_full = GcodeGenerator::new(GcodeFlavor::Marlin)
            .with_lifecycle_markers(false)
            .generate(&[layer], &params_no_simplification);

        // The simplified output must have fewer G1 extrusion moves.
        let count_moves = |s: &str| {
            s.lines()
                .filter(|l| l.contains("G1") && l.contains(" E"))
                .count()
        };
        assert!(
            count_moves(&gcode_simplified) < count_moves(&gcode_full),
            "simplified gcode should have fewer extrusion moves than full gcode"
        );
    }

    #[test]
    fn test_path_simplification_disabled_with_zero_tolerance() {
        use clipper2::Path;

        let mut layer = SliceLayer::new(0.2);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(square);

        let params = SlicingParams {
            path_tolerance: 0.0,
            ..SlicingParams::default()
        };
        // Should not panic and should produce valid G-code with all four corner moves.
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin)
            .with_lifecycle_markers(false)
            .generate(&[layer], &params);
        assert!(gcode.contains(" E"), "should contain extrusion moves");
    }

    #[test]
    fn test_path_simplification_preserves_corners() {
        use clipper2::Path;

        // A square has no collinear intermediate points — all corners are
        // significant and must survive simplification.
        let mut layer = SliceLayer::new(0.2);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(square);

        let params = SlicingParams {
            path_tolerance: 0.05,
            ..SlicingParams::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin)
            .with_lifecycle_markers(false)
            .generate(&[layer], &params);

        // All four corners should appear as extrusion destinations.
        assert!(gcode.contains("X0.000 Y0.000"), "missing (0,0)");
        assert!(gcode.contains("X10.000 Y0.000"), "missing (10,0)");
        assert!(gcode.contains("X10.000 Y10.000"), "missing (10,10)");
        assert!(gcode.contains("X0.000 Y10.000"), "missing (0,10)");
    }

    // ── Smart speed selection ──────────────────────────────────────────────────

    #[test]
    fn test_effective_speed_mm_min_fallback_when_zero() {
        use crate::core::ExtrusionRole;
        let params = SlicingParams {
            print_speed: 60.0,
            perimeter_speed: 0.0, // disabled → fallback
            ..SlicingParams::default()
        };
        // When role-specific speed is 0, falls back to print_speed
        let s = GcodeGenerator::effective_speed_mm_min(
            ExtrusionRole::OuterWall,
            crate::core::OverhangClass::None,
            false,
            &params,
        );
        assert!(
            (s - 60.0 * 60.0).abs() < 1e-6,
            "expected fallback to print_speed * 60"
        );
    }

    // ── Fan control ────────────────────────────────────────────────────────────

    #[test]
    fn test_fan_config_speed_for_layer_time_fast() {
        use crate::settings::params::FanConfig;
        let cfg = FanConfig::default_part_cooling();
        // Layer time at or below fast threshold → max speed
        assert_eq!(cfg.speed_for_layer_time(5.0), cfg.max_speed);
        assert_eq!(cfg.speed_for_layer_time(10.0), cfg.max_speed);
    }

    #[test]
    fn test_fan_config_speed_for_layer_time_slow() {
        use crate::settings::params::FanConfig;
        let cfg = FanConfig::default_part_cooling();
        // Layer time at or above slow threshold → min speed
        assert_eq!(cfg.speed_for_layer_time(30.0), cfg.min_speed);
        assert_eq!(cfg.speed_for_layer_time(60.0), cfg.min_speed);
    }

    #[test]
    fn test_fan_config_speed_for_layer_time_midpoint() {
        use crate::settings::params::FanConfig;
        let cfg = FanConfig::default_part_cooling();
        // At the midpoint between fast and slow thresholds (20 s) speed should
        // be the average of min and max.
        let mid_time = (cfg.layer_time_fast_s + cfg.layer_time_slow_s) / 2.0;
        let expected = (cfg.min_speed + cfg.max_speed) / 2.0;
        let got = cfg.speed_for_layer_time(mid_time);
        assert!(
            (got - expected).abs() < 1e-9,
            "midpoint speed {got} != expected {expected}"
        );
    }

    #[test]
    fn test_effective_speed_mm_min_perimeter_role() {
        use crate::core::ExtrusionRole;
        let params = SlicingParams {
            print_speed: 60.0,
            perimeter_speed: 45.0,
            ..SlicingParams::default()
        };
        let s = GcodeGenerator::effective_speed_mm_min(
            ExtrusionRole::OuterWall,
            crate::core::OverhangClass::None,
            false,
            &params,
        );
        assert!(
            (s - 45.0 * 60.0).abs() < 1e-6,
            "expected perimeter_speed * 60"
        );
        let s = GcodeGenerator::effective_speed_mm_min(
            ExtrusionRole::InnerWall,
            crate::core::OverhangClass::None,
            false,
            &params,
        );
        assert!(
            (s - 45.0 * 60.0).abs() < 1e-6,
            "inner wall should also use perimeter_speed"
        );
    }

    #[test]
    fn test_fan_config_speed_for_layer_time_degenerate() {
        // When fast >= slow (degenerate), always return max_speed (no panic).
        use crate::settings::params::FanConfig;
        let cfg = FanConfig {
            fan_index: 0,
            klipper_name: None,
            min_speed: 0.35,
            max_speed: 1.0,
            layer_time_fast_s: 20.0,
            layer_time_slow_s: 20.0, // equal → degenerate
            aux_overrides: None,
        };
        assert_eq!(cfg.speed_for_layer_time(0.0), cfg.max_speed);
        assert_eq!(cfg.speed_for_layer_time(20.0), cfg.max_speed);
        assert_eq!(cfg.speed_for_layer_time(100.0), cfg.max_speed);
    }

    #[test]
    fn test_marlin_dialect_set_fan_speed_indexed_p0() {
        let d = MarlinDialect;
        // P0 should use the M107 / M106 S<val> convention; name_hint is ignored
        assert_eq!(d.set_fan_speed_indexed(0, None, 0.0), "M107");
        assert_eq!(d.set_fan_speed_indexed(0, None, 1.0), "M106 S255");
        assert_eq!(d.set_fan_speed_indexed(0, Some("rscs"), 0.5), "M106 S128");
    }

    #[test]
    fn test_marlin_dialect_set_fan_speed_indexed_p2() {
        let d = MarlinDialect;
        // Indexed fans use M106 P<n> S<val>; name_hint is ignored by Marlin
        assert_eq!(d.set_fan_speed_indexed(2, None, 0.0), "M106 P2 S0");
        assert_eq!(
            d.set_fan_speed_indexed(2, Some("chamber"), 1.0),
            "M106 P2 S255"
        );
        assert_eq!(d.set_fan_speed_indexed(3, None, 0.6), "M106 P3 S153");
    }

    #[test]
    fn test_klipper_dialect_set_fan_speed_indexed_defaults() {
        let d = KlipperDialect;
        // P0 (part-cooling) → M106/M107; P1 → fan_hotend, P2 → fan_chamber, P3 → fan_aux
        assert_eq!(d.set_fan_speed_indexed(0, None, 1.0), "M106 S255");
        assert_eq!(
            d.set_fan_speed_indexed(1, None, 0.0),
            "SET_FAN_SPEED fan=fan_hotend speed=0.0000"
        );
        assert_eq!(
            d.set_fan_speed_indexed(2, None, 0.6),
            "SET_FAN_SPEED fan=fan_chamber speed=0.6000"
        );
        assert_eq!(
            d.set_fan_speed_indexed(3, None, 0.6),
            "SET_FAN_SPEED fan=fan_aux speed=0.6000"
        );
    }

    #[test]
    fn test_effective_speed_mm_min_infill_role() {
        use crate::core::ExtrusionRole;
        let params = SlicingParams {
            print_speed: 60.0,
            infill_speed: 70.0,
            ..SlicingParams::default()
        };
        let s = GcodeGenerator::effective_speed_mm_min(
            ExtrusionRole::Infill,
            crate::core::OverhangClass::None,
            false,
            &params,
        );
        assert!((s - 70.0 * 60.0).abs() < 1e-6, "expected infill_speed * 60");
    }

    #[test]
    fn test_effective_speed_mm_min_bridge_role() {
        use crate::core::ExtrusionRole;
        let params = SlicingParams {
            print_speed: 60.0,
            bridge_speed: 25.0,
            ..SlicingParams::default()
        };
        let s = GcodeGenerator::effective_speed_mm_min(
            ExtrusionRole::Bridge,
            crate::core::OverhangClass::None,
            false,
            &params,
        );
        assert!((s - 25.0 * 60.0).abs() < 1e-6, "expected bridge_speed * 60");
    }

    #[test]
    fn test_effective_speed_mm_min_first_layer_overrides_role() {
        use crate::core::ExtrusionRole;
        let params = SlicingParams {
            print_speed: 60.0,
            infill_speed: 70.0,
            first_layer_speed: 20.0,
            ..SlicingParams::default()
        };
        // On first layer, all roles get first_layer_speed
        let s = GcodeGenerator::effective_speed_mm_min(
            ExtrusionRole::Infill,
            crate::core::OverhangClass::None,
            true,
            &params,
        );
        assert!(
            (s - 20.0 * 60.0).abs() < 1e-6,
            "first_layer_speed should override infill_speed on first layer"
        );
    }

    #[test]
    fn test_effective_speed_dynamic_overhang_per_degree() {
        use crate::core::{ExtrusionRole, OverhangClass};
        let params = SlicingParams {
            print_speed: 60.0,
            perimeter_speed: 45.0,
            bridge_speed: 25.0,
            enable_overhang_speed: true,
            overhang_1_4_speed: 0.0, // no slowdown → perimeter_speed
            overhang_2_4_speed: 40.0,
            overhang_3_4_speed: 30.0,
            overhang_4_4_speed: 15.0,
            ..SlicingParams::default()
        };
        // Deg1 (0) falls back to the mild wall base (perimeter_speed).
        let s1 = GcodeGenerator::effective_speed_mm_min(
            ExtrusionRole::InnerWall,
            OverhangClass::Deg1,
            false,
            &params,
        );
        assert!((s1 - 45.0 * 60.0).abs() < 1e-6, "Deg1=0 → perimeter_speed");
        // Deg2 uses its configured speed even on a still-InnerWall segment.
        let s2 = GcodeGenerator::effective_speed_mm_min(
            ExtrusionRole::InnerWall,
            OverhangClass::Deg2,
            false,
            &params,
        );
        assert!((s2 - 40.0 * 60.0).abs() < 1e-6, "Deg2 → overhang_2_4_speed");
        // Deg3/Deg4 carry the OverhangPerimeter role and use their speeds.
        let s3 = GcodeGenerator::effective_speed_mm_min(
            ExtrusionRole::OverhangPerimeter,
            OverhangClass::Deg3,
            false,
            &params,
        );
        assert!((s3 - 30.0 * 60.0).abs() < 1e-6, "Deg3 → overhang_3_4_speed");
        let s4 = GcodeGenerator::effective_speed_mm_min(
            ExtrusionRole::OverhangPerimeter,
            OverhangClass::Deg4,
            false,
            &params,
        );
        assert!((s4 - 15.0 * 60.0).abs() < 1e-6, "Deg4 → overhang_4_4_speed");
    }

    #[test]
    fn test_effective_speed_overhang_disabled_ignores_degree() {
        use crate::core::{ExtrusionRole, OverhangClass};
        // Feature off: an OverhangPerimeter Deg4 path still prints at bridge_speed.
        let params = SlicingParams {
            print_speed: 60.0,
            bridge_speed: 25.0,
            enable_overhang_speed: false,
            overhang_4_4_speed: 5.0, // ignored while disabled
            ..SlicingParams::default()
        };
        let s = GcodeGenerator::effective_speed_mm_min(
            ExtrusionRole::OverhangPerimeter,
            OverhangClass::Deg4,
            false,
            &params,
        );
        assert!(
            (s - 25.0 * 60.0).abs() < 1e-6,
            "disabled overhang speed must fall back to bridge_speed"
        );
    }

    #[test]
    fn test_effective_speed_slowdown_for_curled_perimeters_clamps_steep() {
        use crate::core::{ExtrusionRole, OverhangClass};
        let params = SlicingParams {
            print_speed: 60.0,
            enable_overhang_speed: true,
            slowdown_for_curled_perimeters: true,
            overhang_2_4_speed: 12.0, // slowest positive → the curl clamp
            overhang_3_4_speed: 30.0,
            overhang_4_4_speed: 20.0,
            ..SlicingParams::default()
        };
        // Deg3's own speed (30) is clamped down to the slowest positive (12).
        let s3 = GcodeGenerator::effective_speed_mm_min(
            ExtrusionRole::OverhangPerimeter,
            OverhangClass::Deg3,
            false,
            &params,
        );
        assert!(
            (s3 - 12.0 * 60.0).abs() < 1e-6,
            "curl clamp → slowest speed"
        );
    }

    #[test]
    fn test_overhang_meets_fan_threshold_default_half() {
        use crate::core::OverhangClass;
        let params = SlicingParams {
            overhang_fan_threshold: 0.5,
            ..SlicingParams::default()
        };
        assert!(!overhang_meets_fan_threshold(OverhangClass::Deg1, &params));
        assert!(!overhang_meets_fan_threshold(OverhangClass::Deg2, &params));
        assert!(overhang_meets_fan_threshold(OverhangClass::Deg3, &params));
        assert!(overhang_meets_fan_threshold(OverhangClass::Deg4, &params));
    }

    /// A plain square wall layer at `z`, used to pad the layer stack so a test's
    /// interesting layer is not the bed-contact layer (where
    /// `disable_fan_first_layers` pins the part-cooling fan).
    fn plain_wall_layer(z: f64) -> SliceLayer {
        use crate::core::{ExtrusionRole, OverhangClass};
        use clipper2::Path;
        let mut layer = SliceLayer::new(z);
        let wall: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(wall);
        layer.path_roles.push(ExtrusionRole::InnerWall);
        layer.path_overhang.push(OverhangClass::None);
        layer.path_is_open = vec![false];
        layer
    }

    #[test]
    fn test_generator_emits_overhang_fan_command() {
        use crate::core::{ExtrusionRole, OverhangClass};
        use clipper2::Path;
        let mut layer = SliceLayer::new(1.0);
        // A supported inner wall then a steep overhang arc.
        let wall: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        let arc: Path = vec![(0.0, 20.0), (10.0, 20.0), (10.0, 30.0)].into();
        layer.paths.push(wall);
        layer.path_roles.push(ExtrusionRole::InnerWall);
        layer.path_overhang.push(OverhangClass::None);
        layer.paths.push(arc);
        layer.path_roles.push(ExtrusionRole::OverhangPerimeter);
        layer.path_overhang.push(OverhangClass::Deg4);
        layer.path_is_open = vec![false, true];

        let params = SlicingParams {
            enable_overhang_speed: true,
            overhang_fan_speed: 1.0,
            overhang_fan_threshold: 0.5,
            // Bridge cooling off so only the overhang trigger can fire here.
            bridge_fan_speed: 0.0,
            // Base part-cooling fan tops out at 50% so the overhang boost to
            // 100% is a genuine change the generator must emit.
            fan_configs: vec![crate::settings::params::FanConfig {
                fan_index: 0,
                klipper_name: None,
                min_speed: 0.3,
                max_speed: 0.5,
                layer_time_fast_s: 10.0,
                layer_time_slow_s: 30.0,
                aux_overrides: None,
            }],
            ..SlicingParams::default()
        };
        let gen = GcodeGenerator::new(GcodeFlavor::Marlin);
        // The overhang must sit above the layers `disable_fan_first_layers` pins.
        let gcode = gen.generate(&[plain_wall_layer(0.2), layer], &params);
        assert!(
            gcode.contains("; dynamic fan"),
            "expected an overhang fan command in:\n{gcode}"
        );
    }

    #[test]
    fn test_generator_no_overhang_fan_when_disabled() {
        use crate::core::{ExtrusionRole, OverhangClass};
        use clipper2::Path;
        let mut layer = SliceLayer::new(1.0);
        let arc: Path = vec![(0.0, 20.0), (10.0, 20.0), (10.0, 30.0)].into();
        layer.paths.push(arc);
        layer.path_roles.push(ExtrusionRole::OverhangPerimeter);
        layer.path_overhang.push(OverhangClass::Deg4);
        layer.path_is_open = vec![true];

        // Both dynamic-fan triggers off → no override.
        let params = SlicingParams {
            enable_overhang_speed: true,
            overhang_fan_speed: 0.0,
            bridge_fan_speed: 0.0,
            ..SlicingParams::default()
        };
        let gen = GcodeGenerator::new(GcodeFlavor::Marlin);
        let gcode = gen.generate(&[plain_wall_layer(0.2), layer], &params);
        assert!(
            !gcode.contains("; dynamic fan"),
            "no overhang fan command expected when overhang_fan_speed is 0:\n{gcode}"
        );
    }

    #[test]
    fn test_klipper_dialect_set_fan_speed_indexed_custom_name() {
        let d = KlipperDialect;
        // name_hint overrides default fan name derivation
        assert_eq!(
            d.set_fan_speed_indexed(3, Some("rscs"), 0.8),
            "SET_FAN_SPEED fan=rscs speed=0.8000"
        );
        assert_eq!(
            d.set_fan_speed_indexed(0, Some("side_blast"), 0.5),
            "SET_FAN_SPEED fan=side_blast speed=0.5000"
        );
    }

    #[test]
    fn test_klipper_dialect_part_cooling_fan_uses_m106() {
        // The default part-cooling fan (index 0, no name) must use M106/M107 —
        // Klipper's `[fan]` object rejects SET_FAN_SPEED.
        let d = KlipperDialect;
        assert_eq!(d.set_fan_speed_indexed(0, None, 0.0), "M107");
        assert_eq!(d.set_fan_speed_indexed(0, None, 1.0), "M106 S255");
        assert_eq!(d.set_fan_speed_indexed(0, None, 0.5), "M106 S128");
    }

    #[test]
    fn test_gcode_uses_perimeter_speed_for_outer_wall() {
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.2); // first layer
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(square);
        layer.path_roles.push(crate::core::ExtrusionRole::OuterWall);

        let params = SlicingParams {
            first_layer_speed: 25.0,
            coasting_distance_mm: 0.0, // disable coasting so all moves are extrusion
            ..SlicingParams::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin)
            .with_lifecycle_markers(false)
            .generate(&[layer], &params);

        // First layer → F1500 (25 mm/s × 60)
        assert!(
            gcode.contains("F1500"),
            "first layer outer wall should use first_layer_speed=25 mm/s (F1500): {gcode}"
        );
    }

    #[test]
    fn test_generator_emits_fan_command_per_layer() {
        // Default params include one part-cooling fan.
        // A layer with some paths should trigger an M106/M107 command.
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.2);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(square);

        let gcode =
            GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[layer], &SlicingParams::default());
        // The default fan config should emit either M106 or M107
        assert!(
            gcode.contains("M106") || gcode.contains("M107"),
            "expected fan speed command in gcode:\n{gcode}"
        );
    }

    #[test]
    fn test_gcode_uses_infill_speed_for_infill_on_upper_layers() {
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.4); // layer 2 (above first layer at 0.2)
        let line: Path = vec![(0.0, 0.0), (10.0, 0.0)].into();
        layer.paths.push(line);
        layer.path_roles.push(crate::core::ExtrusionRole::Infill);

        let params = SlicingParams {
            layer_height: 0.2,
            infill_speed: 70.0,
            coasting_distance_mm: 0.0,
            ..SlicingParams::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin)
            .with_lifecycle_markers(false)
            .generate(&[layer], &params);

        // Upper layer infill → F4200 (70 mm/s × 60)
        assert!(
            gcode.contains("F4200"),
            "infill on upper layer should use infill_speed=70 mm/s (F4200): {gcode}"
        );
    }

    #[test]
    fn test_generator_no_fan_command_when_fan_configs_empty() {
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.2);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(square);

        let params = SlicingParams {
            fan_configs: vec![],
            ..SlicingParams::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[layer], &params);
        // No fan config → no M106 / M107 inside the layer block.
        // The footer has M104 S0 / M140 S0 but no fan commands.
        assert!(
            !gcode.contains("M106") && !gcode.contains("M107"),
            "unexpected fan command when fan_configs is empty:\n{gcode}"
        );
    }

    // ── Part-cooling material policy ──────────────────────────────────────────

    #[test]
    fn test_part_cooling_speed_pins_first_layers() {
        let params = SlicingParams {
            disable_fan_first_layers: 2,
            first_layer_fan_speed: 0.0,
            fan_speed: 1.0,
            ..SlicingParams::default()
        };
        // Pinned layers ignore the adaptive curve entirely.
        assert_eq!(params.part_cooling_speed(0, 1.0), 0.0);
        assert_eq!(params.part_cooling_speed(1, 1.0), 0.0);
        assert!(params.part_cooling_pinned(1));
        // The first free layer gets the curve back.
        assert!(!params.part_cooling_pinned(2));
        assert_eq!(params.part_cooling_speed(2, 1.0), 1.0);
    }

    #[test]
    fn test_part_cooling_speed_clamps_to_material_ceiling() {
        // An ABS-style preset: the adaptive curve wants full cooling, the
        // material only tolerates 30%.
        let params = SlicingParams {
            disable_fan_first_layers: 1,
            fan_speed: 0.3,
            ..SlicingParams::default()
        };
        assert!((params.part_cooling_speed(5, 1.0) - 0.3).abs() < 1e-9);
        // The ceiling never *raises* a slower adaptive speed.
        assert!((params.part_cooling_speed(5, 0.1) - 0.1).abs() < 1e-9);
    }

    #[test]
    fn test_generator_part_cooling_fan_off_on_first_layer() {
        use clipper2::Path;
        let mut first = SliceLayer::new(0.2);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        first.paths.push(square);
        let second = plain_wall_layer(0.4);

        // Stock defaults: disable_fan_first_layers = 1, first_layer_fan_speed = 0.
        let params = SlicingParams::default();
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[first, second], &params);
        let fan_lines: Vec<&str> = gcode
            .lines()
            .filter(|l| l.starts_with("M106") || l.starts_with("M107"))
            .collect();
        assert_eq!(
            fan_lines.first().copied(),
            Some("M107"),
            "part cooling must be off on the bed-contact layer:\n{gcode}"
        );
        assert!(
            fan_lines.iter().skip(1).any(|l| l.starts_with("M106")),
            "part cooling must come back on above the pinned layers:\n{gcode}"
        );
    }

    #[test]
    fn test_generator_fan_speed_ceiling_caps_part_cooling() {
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.4);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(square);

        let params = SlicingParams {
            fan_speed: 0.2, // ABS-style ceiling: 0.2 × 255 ≈ 51
            ..SlicingParams::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin)
            .generate(&[plain_wall_layer(0.2), layer], &params);
        assert!(
            gcode.contains("M106 S51"),
            "expected the part-cooling fan clamped to the material ceiling in:\n{gcode}"
        );
    }

    #[test]
    fn test_generator_ceiling_does_not_touch_non_part_cooling_fans() {
        use crate::settings::params::FanConfig;
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.4);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(square);

        let params = SlicingParams {
            fan_speed: 0.2,
            fan_configs: vec![FanConfig {
                fan_index: crate::settings::params::fan_index::AUX,
                klipper_name: None,
                min_speed: 1.0,
                max_speed: 1.0,
                layer_time_fast_s: 10.0,
                layer_time_slow_s: 30.0,
                aux_overrides: None,
            }],
            ..SlicingParams::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin)
            .generate(&[plain_wall_layer(0.2), layer], &params);
        assert!(
            gcode.contains("M106 P3 S255"),
            "the material ceiling governs the part-cooling fan only:\n{gcode}"
        );
    }

    #[test]
    fn test_generator_emits_bridge_fan_override() {
        use crate::core::{ExtrusionRole, OverhangClass};
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.4);
        // Bridge first, then a supported wall, so both the boost *and* the
        // restore to the layer's normal cooling are exercised.
        let span: Path = vec![(0.0, 20.0), (10.0, 20.0)].into();
        let wall: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(span);
        layer.path_roles.push(ExtrusionRole::Bridge);
        layer.path_overhang.push(OverhangClass::None);
        layer.paths.push(wall);
        layer.path_roles.push(ExtrusionRole::InnerWall);
        layer.path_overhang.push(OverhangClass::None);
        layer.path_is_open = vec![true, false];

        let params = SlicingParams {
            bridge_fan_speed: 1.0,
            // Base cooling tops out at 40% so the bridge boost is a real change.
            fan_speed: 0.4,
            ..SlicingParams::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin)
            .generate(&[plain_wall_layer(0.2), layer], &params);
        assert!(
            gcode.contains("M106 S255 ; dynamic fan"),
            "expected the bridge fan boost in:\n{gcode}"
        );
        assert!(
            gcode.contains("M106 S102 ; dynamic fan"),
            "expected the fan restored to the layer's normal cooling in:\n{gcode}"
        );
    }

    #[test]
    fn test_generator_bridge_fan_suppressed_on_pinned_first_layer() {
        use crate::core::{ExtrusionRole, OverhangClass};
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.2);
        let span: Path = vec![(0.0, 20.0), (10.0, 20.0)].into();
        layer.paths.push(span);
        layer.path_roles.push(ExtrusionRole::Bridge);
        layer.path_overhang.push(OverhangClass::None);
        layer.path_is_open = vec![true];

        let params = SlicingParams {
            bridge_fan_speed: 1.0,
            ..SlicingParams::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[layer], &params);
        assert!(
            !gcode.contains("; dynamic fan"),
            "the first-layer adhesion gate must not be defeated by a bridge:\n{gcode}"
        );
    }

    #[test]
    fn test_coasting_emits_travel_near_perimeter_end() {
        use clipper2::Path;
        // Use a large square (100 mm sides) so the path is much longer than the
        // coasting distance.
        let mut layer = SliceLayer::new(0.2);
        let square: Path = vec![(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)].into();
        layer.paths.push(square);
        layer.path_roles.push(crate::core::ExtrusionRole::OuterWall);

        let params_coasting = SlicingParams {
            coasting_distance_mm: 1.0,
            perimeter_speed: 45.0,
            ..SlicingParams::default()
        };
        let params_no_coasting = SlicingParams {
            coasting_distance_mm: 0.0,
            perimeter_speed: 45.0,
            ..SlicingParams::default()
        };

        let gcode_coast = GcodeGenerator::new(GcodeFlavor::Marlin)
            .with_lifecycle_markers(false)
            .generate(&[layer.clone()], &params_coasting);
        let gcode_no_coast = GcodeGenerator::new(GcodeFlavor::Marlin)
            .with_lifecycle_markers(false)
            .generate(&[layer], &params_no_coasting);

        // With coasting, there must be at least one "coasting" travel move
        assert!(
            gcode_coast.contains("; coasting"),
            "coasting gcode should contain '; coasting' travel moves: {gcode_coast}"
        );
        // Without coasting, no coasting travel moves
        assert!(
            !gcode_no_coast.contains("; coasting"),
            "no-coasting gcode should NOT contain '; coasting' moves"
        );
    }

    #[test]
    fn test_coasting_disabled_produces_only_extrusion_moves() {
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.2);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(square);
        layer.path_roles.push(crate::core::ExtrusionRole::OuterWall);

        let params = SlicingParams {
            coasting_distance_mm: 0.0,
            ..SlicingParams::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin)
            .with_lifecycle_markers(false)
            .generate(&[layer], &params);

        assert!(
            !gcode.contains("; coasting"),
            "disabled coasting should produce no coasting moves"
        );
    }

    #[test]
    fn test_coasting_only_applies_to_closed_loop_roles() {
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.2);
        let line: Path = vec![(0.0, 0.0), (100.0, 0.0)].into();
        layer.paths.push(line);
        // Infill is an open path — coasting must NOT apply
        layer.path_roles.push(crate::core::ExtrusionRole::Infill);

        let params = SlicingParams {
            coasting_distance_mm: 1.0,
            ..SlicingParams::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin)
            .with_lifecycle_markers(false)
            .generate(&[layer], &params);

        assert!(
            !gcode.contains("; coasting"),
            "coasting must not apply to open infill paths"
        );
    }

    // ── Fan control: multi-fan & AuxFanOverrides ───────────────────────────────

    #[test]
    fn test_generator_multi_fan_marlin() {
        // Simulate a 3-fan printer (Bambu-like): P0 part-cooling, P2 chamber.
        use crate::settings::params::FanConfig;
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.2);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(square);

        let params = SlicingParams {
            fan_configs: vec![
                FanConfig {
                    fan_index: 0,
                    klipper_name: None,
                    min_speed: 0.0,
                    max_speed: 1.0,
                    layer_time_fast_s: 10.0,
                    layer_time_slow_s: 30.0,
                    aux_overrides: None,
                },
                FanConfig {
                    fan_index: 2,
                    klipper_name: None,
                    min_speed: 0.0,
                    max_speed: 0.6,
                    layer_time_fast_s: 10.0,
                    layer_time_slow_s: 30.0,
                    aux_overrides: None,
                },
            ],
            ..SlicingParams::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&[layer], &params);
        // Both fans should have commands
        assert!(
            gcode.contains("M106") || gcode.contains("M107"),
            "expected part-cooling fan command"
        );
        assert!(
            gcode.contains("M106 P2"),
            "expected chamber fan command M106 P2 in:\n{gcode}"
        );
    }

    #[test]
    fn test_generator_klipper_multi_fan() {
        use crate::settings::params::FanConfig;
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.2);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(square);

        let params = SlicingParams {
            fan_configs: vec![
                FanConfig {
                    fan_index: 0,
                    klipper_name: None,
                    min_speed: 0.0,
                    max_speed: 1.0,
                    layer_time_fast_s: 10.0,
                    layer_time_slow_s: 30.0,
                    aux_overrides: None,
                },
                FanConfig {
                    fan_index: 2,
                    klipper_name: None,
                    min_speed: 0.0,
                    max_speed: 0.6,
                    layer_time_fast_s: 10.0,
                    layer_time_slow_s: 30.0,
                    aux_overrides: None,
                },
            ],
            ..SlicingParams::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Klipper).generate(&[layer], &params);
        // Part-cooling fan uses M106/M107 (Klipper's `[fan]` rejects SET_FAN_SPEED);
        // named/auxiliary fans use SET_FAN_SPEED syntax.
        assert!(
            gcode.contains("M106") || gcode.contains("M107"),
            "expected part-cooling fan command in:\n{gcode}"
        );
        assert!(
            !gcode.contains("SET_FAN_SPEED fan=fan "),
            "part-cooling fan must not use SET_FAN_SPEED fan=fan in:\n{gcode}"
        );
        assert!(
            gcode.contains("SET_FAN_SPEED fan=fan_chamber "),
            "expected chamber fan command in:\n{gcode}"
        );
    }

    #[test]
    fn test_generator_klipper_custom_fan_name() {
        // RSCS with a custom klipper_name
        use crate::settings::params::FanConfig;
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.2);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(square);

        let params = SlicingParams {
            fan_configs: vec![FanConfig {
                fan_index: 3,
                klipper_name: Some("rscs".to_string()),
                min_speed: 0.3,
                max_speed: 1.0,
                layer_time_fast_s: 10.0,
                layer_time_slow_s: 30.0,
                aux_overrides: None,
            }],
            ..SlicingParams::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Klipper).generate(&[layer], &params);
        assert!(
            gcode.contains("SET_FAN_SPEED fan=rscs "),
            "expected custom fan name 'rscs' in:\n{gcode}"
        );
        assert!(
            !gcode.contains("fan_aux"),
            "should NOT use default fan_aux name when klipper_name is set"
        );
    }

    #[test]
    fn test_estimate_layer_time_empty_paths() {
        let layer = SliceLayer::new(0.2);
        let t = estimate_layer_time(&layer, 60.0);
        assert_eq!(t, 0.0);
    }

    #[test]
    fn test_estimate_layer_time_single_segment() {
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.2);
        // A path that covers exactly 60 mm at 60 mm/s → 1.0 s
        let path: Path = vec![(0.0, 0.0), (60.0, 0.0)].into();
        layer.paths.push(path);
        let t = estimate_layer_time(&layer, 60.0);
        assert!((t - 1.0).abs() < 1e-9, "expected ~1.0 s, got {t}");
    }

    // ── AuxFanOverrides ────────────────────────────────────────────────────────

    #[test]
    fn test_aux_fan_compute_speed_no_boost_no_bridge() {
        use crate::settings::params::{AuxFanOverrides, FanConfig};
        // Long layer → min speed, no bridge, no boost applied
        let cfg = FanConfig {
            fan_index: 3,
            klipper_name: None,
            min_speed: 0.2,
            max_speed: 0.8,
            layer_time_fast_s: 10.0,
            layer_time_slow_s: 30.0,
            aux_overrides: Some(AuxFanOverrides {
                bridge_boost: 0.3,
                short_layer_boost: 0.2,
                boost_max_speed: 0.95,
                speed_scale: 1.0,
                max_speed_limit: 1.0,
                max_speed_change_per_layer: 1.0, // effectively no rate limit
            }),
        };
        // 30 s → min_speed (0.2), no triggers → result stays 0.2
        let s = cfg.compute_speed(30.0, false, None);
        assert!((s - 0.2).abs() < 1e-9, "expected min_speed 0.2, got {s}");
    }

    #[test]
    fn test_aux_fan_bridge_boost_applied_and_capped() {
        use crate::settings::params::{AuxFanOverrides, FanConfig};
        let cfg = FanConfig {
            fan_index: 3,
            klipper_name: None,
            min_speed: 0.2,
            max_speed: 0.5,
            layer_time_fast_s: 10.0,
            layer_time_slow_s: 30.0,
            aux_overrides: Some(AuxFanOverrides {
                bridge_boost: 0.6, // would exceed cap alone
                short_layer_boost: 0.0,
                boost_max_speed: 0.8, // cap at 0.8
                speed_scale: 1.0,
                max_speed_limit: 1.0,
                max_speed_change_per_layer: 1.0,
            }),
        };
        // Layer time = 20 s (mid-range) → base ≈ 0.35
        // bridge boost → 0.35 + 0.6 = 0.95, capped at 0.8
        let base = cfg.speed_for_layer_time(20.0);
        let s = cfg.compute_speed(20.0, true, None);
        assert!(
            s <= 0.8 + 1e-9,
            "bridge-boosted speed {s} must not exceed boost_max_speed 0.8 (base was {base})"
        );
        assert!(
            s > base + 0.1,
            "bridge boost should visibly increase speed above base {base}"
        );
    }

    #[test]
    fn test_aux_fan_short_layer_boost() {
        use crate::settings::params::{AuxFanOverrides, FanConfig};
        let cfg = FanConfig {
            fan_index: 3,
            klipper_name: None,
            min_speed: 0.2,
            max_speed: 0.6,
            layer_time_fast_s: 10.0,
            layer_time_slow_s: 30.0,
            aux_overrides: Some(AuxFanOverrides {
                bridge_boost: 0.0,
                short_layer_boost: 0.3,
                boost_max_speed: 1.0,
                speed_scale: 1.0,
                max_speed_limit: 1.0,
                max_speed_change_per_layer: 1.0,
            }),
        };
        // Layer time ≤ fast threshold → base = max_speed (0.6); short-layer boost adds 0.3 → 0.9
        let s = cfg.compute_speed(5.0, false, None);
        assert!(
            (s - 0.9).abs() < 1e-9,
            "expected short-layer-boosted speed 0.9, got {s}"
        );
    }

    #[test]
    fn test_aux_fan_speed_scale_applied() {
        use crate::settings::params::{AuxFanOverrides, FanConfig};
        let cfg = FanConfig {
            fan_index: 3,
            klipper_name: None,
            min_speed: 0.0,
            max_speed: 1.0,
            layer_time_fast_s: 10.0,
            layer_time_slow_s: 30.0,
            aux_overrides: Some(AuxFanOverrides {
                bridge_boost: 0.0,
                short_layer_boost: 0.0,
                boost_max_speed: 1.0,
                speed_scale: 0.5, // halve the computed speed
                max_speed_limit: 1.0,
                max_speed_change_per_layer: 1.0,
            }),
        };
        // Short layer → base = 1.0 × 0.5 = 0.5
        let s = cfg.compute_speed(5.0, false, None);
        assert!(
            (s - 0.5).abs() < 1e-9,
            "expected 0.5 after speed_scale, got {s}"
        );
    }

    #[test]
    fn test_aux_fan_max_speed_limit_enforced() {
        use crate::settings::params::{AuxFanOverrides, FanConfig};
        let cfg = FanConfig {
            fan_index: 3,
            klipper_name: None,
            min_speed: 0.0,
            max_speed: 1.0,
            layer_time_fast_s: 10.0,
            layer_time_slow_s: 30.0,
            aux_overrides: Some(AuxFanOverrides {
                bridge_boost: 0.0,
                short_layer_boost: 0.0,
                boost_max_speed: 1.0,
                speed_scale: 1.0,
                max_speed_limit: 0.6, // material safety cap
                max_speed_change_per_layer: 1.0,
            }),
        };
        // max_speed = 1.0, but material safety cap at 0.6
        let s = cfg.compute_speed(5.0, false, None);
        assert!(
            (s - 0.6).abs() < 1e-9,
            "expected max_speed_limit 0.6 to be enforced, got {s}"
        );
    }

    #[test]
    fn test_aux_fan_rate_limiter_clamps_increase() {
        use crate::settings::params::{AuxFanOverrides, FanConfig};
        let cfg = FanConfig {
            fan_index: 3,
            klipper_name: None,
            min_speed: 0.0,
            max_speed: 1.0,
            layer_time_fast_s: 10.0,
            layer_time_slow_s: 30.0,
            aux_overrides: Some(AuxFanOverrides {
                bridge_boost: 0.0,
                short_layer_boost: 0.0,
                boost_max_speed: 1.0,
                speed_scale: 1.0,
                max_speed_limit: 1.0,
                max_speed_change_per_layer: 0.15, // max 15% change
            }),
        };
        // Prev = 0.0, target = 1.0. Rate limit → max 0.15.
        let s = cfg.compute_speed(5.0, false, Some(0.0));
        assert!(
            (s - 0.15).abs() < 1e-9,
            "expected rate-limited speed 0.15, got {s}"
        );
    }

    #[test]
    fn test_aux_fan_rate_limiter_clamps_decrease() {
        use crate::settings::params::{AuxFanOverrides, FanConfig};
        let cfg = FanConfig {
            fan_index: 3,
            klipper_name: None,
            min_speed: 0.0,
            max_speed: 0.1,
            layer_time_fast_s: 10.0,
            layer_time_slow_s: 30.0,
            aux_overrides: Some(AuxFanOverrides {
                bridge_boost: 0.0,
                short_layer_boost: 0.0,
                boost_max_speed: 1.0,
                speed_scale: 1.0,
                max_speed_limit: 1.0,
                max_speed_change_per_layer: 0.15,
            }),
        };
        // Prev = 0.9, target = 0.1 (slow layer). Rate limit → min 0.9 - 0.15 = 0.75.
        let s = cfg.compute_speed(30.0, false, Some(0.9));
        assert!(
            (s - 0.75).abs() < 1e-9,
            "expected rate-limited speed 0.75, got {s}"
        );
    }

    #[test]
    fn test_generator_aux_fan_bridge_boost_on_bridge_layer() {
        // A layer with Bridge paths should trigger bridge_boost on aux fan.
        use crate::settings::params::{AuxFanOverrides, FanConfig};
        use clipper2::Path;
        let mut layer = SliceLayer::new(0.2);
        let sq: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(sq);
        layer.path_roles.push(crate::core::ExtrusionRole::Bridge);

        // Aux fan with very distinctive bridge_boost speed so we can assert it's used
        let params = SlicingParams {
            fan_configs: vec![FanConfig {
                fan_index: 3,
                klipper_name: Some("rscs".to_string()),
                min_speed: 0.0,
                max_speed: 0.0, // base is 0 for very long layers
                layer_time_fast_s: 0.0,
                layer_time_slow_s: 0.001, // force min_speed regime
                aux_overrides: Some(AuxFanOverrides {
                    bridge_boost: 0.8, // distinctive value
                    short_layer_boost: 0.0,
                    boost_max_speed: 0.8,
                    speed_scale: 1.0,
                    max_speed_limit: 1.0,
                    max_speed_change_per_layer: 1.0,
                }),
            }],
            ..SlicingParams::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Klipper).generate(&[layer], &params);
        // The bridge boost should push the speed to 0.8 (204/255 ≈ S204 in Marlin,
        // but we're checking Klipper which uses fractional speed)
        assert!(
            gcode.contains("SET_FAN_SPEED fan=rscs"),
            "expected rscs fan command in:\n{gcode}"
        );
        // Speed should reflect the boost, not zero
        assert!(
            !gcode.contains("speed=0.0000"),
            "bridge boost should raise speed above 0 in:\n{gcode}"
        );
    }

    // ── Spiral (vase) mode ─────────────────────────────────────────────────

    /// Build `n` stacked single-square outer-wall layers at 0.2 mm pitch,
    /// modelling a solid prism suitable for spiralization.
    fn spiral_square_layers(n: usize) -> Vec<SliceLayer> {
        use clipper2::Path;
        (0..n)
            .map(|i| {
                let z = 0.2 * (i as f64 + 1.0);
                let mut layer = SliceLayer::new(z);
                let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
                layer.paths.push(square);
                layer.path_roles.push(crate::core::ExtrusionRole::OuterWall);
                layer
            })
            .collect()
    }

    /// Extract the `Z` value from a `G1 … Z… E…` extruding move (returns `None`
    /// for non-extruding / Z-less moves).
    fn extrude_move_z(line: &str) -> Option<f64> {
        if !line.starts_with("G1 ") || !line.contains(" E") {
            return None;
        }
        for tok in line.split_whitespace() {
            if let Some(rest) = tok.strip_prefix('Z') {
                return rest.parse::<f64>().ok();
            }
        }
        None
    }

    #[test]
    fn spiral_vase_normalized_forces_single_wall_config() {
        let params = SlicingParams {
            spiral_vase: true,
            wall_count: 4,
            infill_density: 0.35,
            top_layers: 5,
            bottom_layers: 3,
            retract_mm: 1.5,
            z_hop_mm: 0.4,
            ironing_enabled: true,
            ..SlicingParams::default()
        };
        let n = params.spiral_vase_normalized();
        assert_eq!(n.wall_count, 1);
        assert_eq!(n.infill_density, 0.0);
        assert_eq!(n.top_layers, 0);
        assert_eq!(n.retract_mm, 0.0);
        assert_eq!(n.z_hop_mm, 0.0);
        assert!(!n.ironing_enabled);
        // The base is preserved so the vase has a floor.
        assert_eq!(n.bottom_layers, 3);
    }

    #[test]
    fn spiral_vase_normalized_is_noop_when_disabled() {
        let params = SlicingParams {
            wall_count: 4,
            ..SlicingParams::default()
        };
        // Borrowed (unchanged) when the flag is off.
        assert_eq!(params.spiral_vase_normalized().wall_count, 4);
    }

    #[test]
    fn spiral_vase_ramps_z_continuously() {
        let params = SlicingParams {
            spiral_vase: true,
            bottom_layers: 3,
            ..SlicingParams::default()
        };
        let layers = spiral_square_layers(6);
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&layers, &params);

        // Collect the Z of every extruding move in the spiral body.
        let zs: Vec<f64> = gcode.lines().filter_map(extrude_move_z).collect();
        assert!(
            zs.len() > 10,
            "expected many ramped extrude moves, got {}:\n{gcode}",
            zs.len()
        );
        // The helix only ever climbs.
        for w in zs.windows(2) {
            assert!(
                w[1] >= w[0] - 1e-9,
                "spiral Z must be non-decreasing: {} -> {}",
                w[0],
                w[1]
            );
        }
        // At least one intermediate Z sits strictly between two layer heights,
        // proving the rise is distributed along the perimeter (not a step).
        let has_fractional = zs
            .iter()
            .any(|z| (z / 0.2 - (z / 0.2).round()).abs() > 1e-3);
        assert!(
            has_fractional,
            "expected a continuously-ramped (fractional) Z, got {zs:?}"
        );
        // The top of the spiral reaches the last layer's Z.
        let zmax = zs.iter().cloned().fold(f64::MIN, f64::max);
        assert!(
            (zmax - 1.2).abs() < 1e-6,
            "spiral top Z should be 1.2, got {zmax}"
        );
    }

    #[test]
    fn spiral_vase_keeps_flat_base_and_skips_move_z_on_spiral_layers() {
        let params = SlicingParams {
            spiral_vase: true,
            bottom_layers: 3,
            ..SlicingParams::default()
        };
        let layers = spiral_square_layers(6);
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&layers, &params);

        // Base layers (z = 0.2/0.4/0.6) print flat: a discrete Z move exists.
        assert!(
            gcode.contains("G1 Z0.200"),
            "missing base move to 0.2:\n{gcode}"
        );
        assert!(
            gcode.contains("G1 Z0.600"),
            "missing base move to 0.6:\n{gcode}"
        );
        // Spiral layers (z ≥ 0.8) must NOT get a discrete Z move — the ramp
        // carries Z inside the extrude moves instead.
        assert!(
            !gcode.contains("G1 Z0.800"),
            "spiral layer must not emit a discrete Z move:\n{gcode}"
        );
        // Continuous-Z extrude moves are present.
        assert!(
            gcode.lines().any(|l| extrude_move_z(l).is_some()),
            "expected ramped extrude moves:\n{gcode}"
        );
    }

    #[test]
    fn spiral_vase_warns_and_falls_back_on_multi_island_layer() {
        use clipper2::Path;
        use std::cell::RefCell;
        use std::rc::Rc;

        let params = SlicingParams {
            spiral_vase: true,
            bottom_layers: 3,
            ..SlicingParams::default()
        };
        // Layers 0-2 base, layer 3 has TWO disjoint islands, layer 4 single.
        let mut layers = spiral_square_layers(5);
        let second: Path = vec![(20.0, 0.0), (30.0, 0.0), (30.0, 10.0), (20.0, 10.0)].into();
        layers[3].paths.push(second);
        layers[3]
            .path_roles
            .push(crate::core::ExtrusionRole::OuterWall);

        let warnings = Rc::new(RefCell::new(Vec::<String>::new()));
        let sink = warnings.clone();
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin)
            .with_warn_fn(move |m| sink.borrow_mut().push(m.to_string()))
            .generate(&layers, &params);

        assert!(
            warnings
                .borrow()
                .iter()
                .any(|w| w.contains("multiple islands")),
            "expected a multi-island warning, got {:?}",
            warnings.borrow()
        );
        // The multi-island layer (z = 0.8) falls back to a normal flat print,
        // so its discrete Z move is present.
        assert!(
            gcode.contains("G1 Z0.800"),
            "multi-island layer should fall back to a flat print:\n{gcode}"
        );
        // The single-island layer above it still spiralizes (continuous Z).
        assert!(
            gcode.lines().filter_map(extrude_move_z).any(|z| z > 0.8),
            "layer above the multi-island one should still spiralize:\n{gcode}"
        );
    }

    /// Extract the `E` value from a `G1 … E…` move.
    fn move_e(line: &str) -> Option<f64> {
        for tok in line.split_whitespace() {
            if let Some(rest) = tok.strip_prefix('E') {
                return rest.parse::<f64>().ok();
            }
        }
        None
    }

    #[test]
    fn spiral_vase_fades_flow_in_at_start_and_out_at_end() {
        let params = SlicingParams {
            spiral_vase: true,
            bottom_layers: 3,
            ..SlicingParams::default()
        };
        // Spiral layers: 3 (first), 4, 5 (middle), 6 (last).
        let layers = spiral_square_layers(7);
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&layers, &params);

        // Group the E of every ramped (spiral) move by layer. E resets to 0 at
        // each layer, so a drop marks a new layer.
        let mut groups: Vec<Vec<f64>> = Vec::new();
        let mut cur: Vec<f64> = Vec::new();
        let mut last_e = f64::MAX;
        for line in gcode.lines() {
            if extrude_move_z(line).is_none() {
                continue;
            }
            let e = move_e(line).expect("ramped move carries E");
            if e < last_e - 1e-9 && !cur.is_empty() {
                groups.push(std::mem::take(&mut cur));
            }
            cur.push(e);
            last_e = e;
        }
        if !cur.is_empty() {
            groups.push(cur);
        }
        assert_eq!(groups.len(), 4, "expected 4 spiral layers, got {groups:?}");

        let deltas = |es: &[f64]| -> Vec<f64> {
            let mut d = vec![es[0]];
            for w in es.windows(2) {
                d.push(w[1] - w[0]);
            }
            d
        };
        let first = deltas(&groups[0]);
        let mid = deltas(&groups[2]);
        let last = deltas(&groups[3]);

        // First spiral loop fades in: strictly increasing deposition.
        assert!(
            first.windows(2).all(|w| w[1] > w[0] - 1e-9) && first[3] > first[0] + 1e-6,
            "first spiral loop should fade flow in: {first:?}"
        );
        // Middle spiral loops are steady full flow (a perfect square → equal
        // per-segment deposition).
        let mid_max = mid.iter().cloned().fold(f64::MIN, f64::max);
        let mid_min = mid.iter().cloned().fold(f64::MAX, f64::min);
        assert!(
            mid_min > 0.0 && (mid_max - mid_min) < 1e-3,
            "middle steady: {mid:?}"
        );
        // Last spiral loop fades out to ~zero.
        assert!(
            last.windows(2).all(|w| w[1] < w[0] + 1e-9) && last[3] < last[0] - 1e-6,
            "last spiral loop should fade flow out: {last:?}"
        );
        assert!(
            last.last().unwrap().abs() < 1e-6,
            "last spiral segment should deposit ~0 (seam fade-out): {last:?}"
        );
    }

    #[test]
    fn spiral_vase_body_has_no_retraction() {
        // With no base (open tube) every layer above 0 spiralizes; the spiral
        // body is one continuous extrusion with no retract ceremony.
        let params = SlicingParams {
            spiral_vase: true,
            bottom_layers: 0,
            ..SlicingParams::default()
        };
        let layers = spiral_square_layers(5);
        let gcode = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&layers, &params);

        // Once the spiral starts it is one uninterrupted extrusion: no retract
        // or z-hop ceremony appears after the first ramped move. (The initial
        // approach before the spiral may retract; that is expected.)
        let lines: Vec<&str> = gcode.lines().collect();
        let first_spiral = lines
            .iter()
            .position(|l| extrude_move_z(l).is_some())
            .expect("spiral body should contain ramped extrude moves");
        assert!(
            lines[first_spiral..]
                .iter()
                .all(|l| !l.contains("; retract") && !l.contains("; z-hop")),
            "spiral vase body must not retract or z-hop:\n{gcode}"
        );
    }

    // ── Machine Z offset (issue #102) ──────────────────────────────────────

    /// Every `Z` word on a `G1` line, extruding or not.
    fn all_g1_z(gcode: &str) -> Vec<f64> {
        gcode
            .lines()
            .filter(|l| l.starts_with("G1 "))
            .filter_map(|l| {
                l.split_whitespace()
                    .find_map(|tok| tok.strip_prefix('Z')?.parse::<f64>().ok())
            })
            .collect()
    }

    /// The print body: from the first layer marker through the last extruding
    /// move. Excludes the machine start/end scripts, which are passed through
    /// verbatim and must never be rewritten (the Marlin end script's
    /// `G1 Z5 F3000` is a *relative* `G91` lift — offsetting it would be wrong).
    fn print_body(gcode: &str) -> String {
        let lines: Vec<&str> = gcode.lines().collect();
        let start = lines
            .iter()
            .position(|l| l.contains(";LAYER_CHANGE") || l.starts_with("; layer z="))
            .expect("no layer marker");
        let end = lines
            .iter()
            .rposition(|l| l.starts_with("G1 ") && l.contains(" E"))
            .expect("no extruding move");
        lines[start..=end].join("\n")
    }

    /// Two stacked squares far enough apart to force a retract + z-hop travel
    /// between them, so a run exercises layer moves, hops and lowers.
    fn offset_test_layers() -> Vec<SliceLayer> {
        use clipper2::Path;
        (0..3)
            .map(|i| {
                let mut layer = SliceLayer::new(0.2 * (i as f64 + 1.0));
                let a: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
                let b: Path = vec![(50.0, 50.0), (60.0, 50.0), (60.0, 60.0), (50.0, 60.0)].into();
                layer.paths.push(a);
                layer.path_roles.push(crate::core::ExtrusionRole::OuterWall);
                layer.paths.push(b);
                layer.path_roles.push(crate::core::ExtrusionRole::OuterWall);
                layer
            })
            .collect()
    }

    #[test]
    fn z_offset_shifts_every_emitted_z_without_touching_extrusion() {
        for offset in [0.25_f64, -0.15] {
            let base = SlicingParams::default();
            let shifted = SlicingParams {
                z_offset_mm: offset,
                ..SlicingParams::default()
            };
            let layers = offset_test_layers();
            let gen = GcodeGenerator::new(GcodeFlavor::Marlin);
            let plain = gen.generate(&layers, &base);
            let moved = gen.generate(&layers, &shifted);

            let plain_z = all_g1_z(&print_body(&plain));
            let moved_z = all_g1_z(&print_body(&moved));
            assert!(!plain_z.is_empty(), "fixture emitted no Z moves:\n{plain}");
            assert_eq!(
                plain_z.len(),
                moved_z.len(),
                "the offset must not add or drop moves"
            );
            for (p, m) in plain_z.iter().zip(&moved_z) {
                assert!(
                    (m - (p + offset)).abs() < 1e-6,
                    "Z {p} should have become {} with offset {offset}, got {m}",
                    p + offset
                );
            }

            // The machine end script is passed through verbatim: its lift is a
            // relative (`G91`) 5 mm hop, not a bed-referenced coordinate.
            assert!(
                plain.contains("G1 Z5 F3000 ; lift nozzle")
                    && moved.contains("G1 Z5 F3000 ; lift nozzle"),
                "machine scripts must not be rewritten by the offset:\n{moved}"
            );

            // Extrusion is a machine-Z-independent quantity: every E word, and
            // the line count, must be untouched.
            let e_of = |g: &str| -> Vec<String> {
                g.lines()
                    .filter_map(|l| {
                        l.split_whitespace()
                            .find(|tok| tok.starts_with('E'))
                            .map(str::to_string)
                    })
                    .collect()
            };
            assert_eq!(e_of(&plain), e_of(&moved), "offset changed extrusion");
            assert_eq!(
                plain.lines().count(),
                moved.lines().count(),
                "offset changed the emitted line count"
            );
        }
    }

    #[test]
    fn z_offset_zero_is_byte_identical() {
        let layers = offset_test_layers();
        let gen = GcodeGenerator::new(GcodeFlavor::Marlin);
        let implicit = gen.generate(&layers, &SlicingParams::default());
        let explicit = gen.generate(
            &layers,
            &SlicingParams {
                z_offset_mm: 0.0,
                ..SlicingParams::default()
            },
        );
        assert_eq!(implicit, explicit, "a zero offset must be a pure no-op");
    }

    #[test]
    fn z_offset_applies_to_hop_and_lower_moves() {
        let params = SlicingParams {
            z_offset_mm: 0.5,
            z_hop_mm: 0.4,
            retract_mm: 1.0,
            ..SlicingParams::default()
        };
        let gcode =
            GcodeGenerator::new(GcodeFlavor::Marlin).generate(&offset_test_layers(), &params);
        let z_on = |tag: &str| -> Vec<f64> {
            gcode
                .lines()
                .filter(|l| l.ends_with(tag))
                .filter_map(|l| {
                    l.split_whitespace()
                        .find_map(|tok| tok.strip_prefix('Z')?.parse::<f64>().ok())
                })
                .collect()
        };
        let hops = z_on("; z-hop");
        let lowers = z_on("; lower");
        assert!(!hops.is_empty(), "fixture produced no z-hop:\n{gcode}");
        assert_eq!(hops.len(), lowers.len());
        // First layer sits at 0.2 → lower 0.7, hop 0.7 + 0.4 = 1.1.
        assert!(
            hops.iter()
                .zip(&lowers)
                .all(|(h, l)| (h - l - 0.4).abs() < 1e-6),
            "hop must stay exactly z_hop_mm above the lower move: {hops:?} / {lowers:?}"
        );
        assert!(
            lowers.iter().any(|l| (l - 0.7).abs() < 1e-6),
            "expected a lower move at 0.2 + 0.5 offset: {lowers:?}"
        );
    }

    #[test]
    fn z_offset_shifts_layer_markers_but_not_the_custom_layer_script() {
        // `;Z:` describes where the nozzle physically is (offset included);
        // the custom layer script's `{z}` is the model's layer Z (offset free),
        // mirroring PrusaSlicer's `layer_z` placeholder.
        let params = SlicingParams {
            z_offset_mm: 0.3,
            ..SlicingParams::default()
        };
        let gcode = GcodeGenerator::new(GcodeFlavor::Klipper)
            .with_layer_script(vec!["_ON_LAYER_CHANGE LAYER={layer_num} Z={z}".to_string()])
            .generate(&[SliceLayer::new(0.2)], &params);

        assert!(
            gcode.contains(";Z:0.500"),
            "layer marker should carry the offset:\n{gcode}"
        );
        assert!(
            !gcode.contains(";Z:0.200"),
            "un-offset layer marker leaked:\n{gcode}"
        );
        assert!(
            gcode.contains("_ON_LAYER_CHANGE LAYER=1 Z=0.200"),
            "custom layer script must see the model Z:\n{gcode}"
        );
    }

    #[test]
    fn z_offset_shifts_the_spiral_ramp_end_to_end() {
        let params = SlicingParams {
            spiral_vase: true,
            bottom_layers: 1,
            z_offset_mm: 1.0,
            ..SlicingParams::default()
        };
        let layers = spiral_square_layers(4);
        let gen = GcodeGenerator::new(GcodeFlavor::Marlin);
        let plain = gen.generate(
            &layers,
            &SlicingParams {
                z_offset_mm: 0.0,
                ..params.clone()
            },
        );
        let moved = gen.generate(&layers, &params);

        let plain_ramp: Vec<f64> = plain.lines().filter_map(extrude_move_z).collect();
        let moved_ramp: Vec<f64> = moved.lines().filter_map(extrude_move_z).collect();
        assert!(!plain_ramp.is_empty(), "no ramped moves:\n{plain}");
        assert_eq!(plain_ramp.len(), moved_ramp.len());
        for (p, m) in plain_ramp.iter().zip(&moved_ramp) {
            assert!(
                (m - (p + 1.0)).abs() < 1e-6,
                "spiral ramp Z {p} should shift to {}, got {m}",
                p + 1.0
            );
        }
    }

    #[test]
    fn z_offset_leaves_statistics_on_model_z() {
        // The offset is a machine correction: `max_z_mm` and the bounding box
        // describe the object, so they must not move with it.
        let layers = offset_test_layers();
        let gen = GcodeGenerator::new(GcodeFlavor::Marlin);
        let (_, plain) = gen.generate_with_stats(&layers, &SlicingParams::default());
        let (_, moved) = gen.generate_with_stats(
            &layers,
            &SlicingParams {
                z_offset_mm: 0.75,
                ..SlicingParams::default()
            },
        );
        assert_eq!(plain.max_z_mm, moved.max_z_mm);
        assert_eq!(plain.bbox_min, moved.bbox_min);
        assert_eq!(plain.bbox_max, moved.bbox_max);
    }
}

// ── Object exclusion & sequential printing (issues #22 / #112) ─────────────────

#[cfg(test)]
mod object_tests {
    use super::*;
    use crate::core::{ExtrusionRole, ObjectIdentity, PlateSlice, SliceLayer};
    use crate::settings::params::PrintSequence;

    fn identity(index: usize, name: &str, x: f64) -> ObjectIdentity {
        ObjectIdentity {
            index,
            name: name.to_string(),
            center: (x + 5.0, 5.0),
            polygon: vec![(x, 0.0), (x + 10.0, 0.0), (x + 10.0, 10.0), (x, 10.0)],
            bbox: (x, 0.0, x + 10.0, 10.0),
            height_mm: 4.0,
        }
    }

    /// One layer holding one square per object, tagged in object order.
    fn tagged_layer(z: f64, objects: &[usize]) -> SliceLayer {
        let mut layer = SliceLayer::new(z);
        for (slot, &object) in objects.iter().enumerate() {
            let x = slot as f64 * 30.0;
            let square: clipper2::Path =
                vec![(x, 0.0), (x + 10.0, 0.0), (x + 10.0, 10.0), (x, 10.0)].into();
            layer.paths.push(square);
            layer.path_roles.push(ExtrusionRole::OuterWall);
            layer.path_widths.push(Some(0.4));
            layer.path_vertex_widths.push(None);
            layer.path_is_open.push(false);
            layer.path_objects.push(Some(object));
        }
        layer
    }

    fn plate(layers: Vec<SliceLayer>, objects: Vec<ObjectIdentity>) -> PlateSlice {
        PlateSlice { layers, objects }
    }

    fn klipper_params() -> SlicingParams {
        SlicingParams {
            gcode_flavor: crate::gcode::GcodeFlavor::Klipper,
            exclude_object: true,
            ..Default::default()
        }
    }

    #[test]
    fn klipper_defines_every_object_before_the_start_script() {
        let objects = vec![identity(0, "cube_a", 0.0), identity(1, "cube_b", 30.0)];
        let gcode = generate_gcode_for_plate(
            &plate(vec![tagged_layer(0.2, &[0, 1])], objects),
            &klipper_params(),
        );

        assert!(gcode.contains("EXCLUDE_OBJECT_DEFINE RESET=1"));
        assert!(
            gcode.contains(
                "EXCLUDE_OBJECT_DEFINE NAME=cube_a CENTER=5.000,5.000 POLYGON=[[0.000,0.000],"
            ),
            "missing cube_a definition: {gcode}"
        );
        assert!(gcode.contains("EXCLUDE_OBJECT_DEFINE NAME=cube_b"));

        // The definitions must precede the start script — Moonraker and the
        // Klipper module both expect to meet the objects before printing.
        let define_at = gcode.find("EXCLUDE_OBJECT_DEFINE NAME=cube_a").unwrap();
        let start_at = gcode.find("START_PRINT").unwrap();
        assert!(define_at < start_at, "definitions must come first: {gcode}");
    }

    #[test]
    fn klipper_wraps_each_object_and_closes_the_last_one() {
        let objects = vec![identity(0, "cube_a", 0.0), identity(1, "cube_b", 30.0)];
        let gcode = generate_gcode_for_plate(
            &plate(
                vec![tagged_layer(0.2, &[0, 1]), tagged_layer(0.4, &[0, 1])],
                objects,
            ),
            &klipper_params(),
        );

        let markers: Vec<&str> = gcode
            .lines()
            .filter(|l| {
                l.starts_with("EXCLUDE_OBJECT_START") || l.starts_with("EXCLUDE_OBJECT_END")
            })
            .collect();
        assert_eq!(
            markers,
            vec![
                "EXCLUDE_OBJECT_START NAME=cube_a",
                "EXCLUDE_OBJECT_END NAME=cube_a",
                "EXCLUDE_OBJECT_START NAME=cube_b",
                "EXCLUDE_OBJECT_END NAME=cube_b",
                "EXCLUDE_OBJECT_START NAME=cube_a",
                "EXCLUDE_OBJECT_END NAME=cube_a",
                "EXCLUDE_OBJECT_START NAME=cube_b",
                "EXCLUDE_OBJECT_END NAME=cube_b",
            ],
            "every block must be opened and closed: {gcode}"
        );
    }

    #[test]
    fn marlin_uses_m486_and_names_each_object_once() {
        let objects = vec![identity(0, "cube_a", 0.0), identity(1, "cube_b", 30.0)];
        let params = SlicingParams {
            exclude_object: true,
            ..Default::default()
        };
        let gcode = generate_gcode_for_plate(
            &plate(
                vec![tagged_layer(0.2, &[0, 1]), tagged_layer(0.4, &[0, 1])],
                objects,
            ),
            &params,
        );

        assert!(gcode.contains("M486 T2 ; object count"));
        // The name rides along the first `S` only; repeating it on every layer
        // would bloat the file for no gain.
        assert_eq!(gcode.matches("M486 S0 A\"cube_a\"").count(), 1);
        assert_eq!(gcode.matches("M486 S1 A\"cube_b\"").count(), 1);
        assert_eq!(gcode.matches("\nM486 S0\n").count(), 1);
        assert_eq!(gcode.matches("M486 S-1").count(), 4);
    }

    #[test]
    fn untagged_paths_close_the_block_without_opening_one() {
        // A plate-wide skirt (tag `None`) printed before the objects.
        let mut layer = tagged_layer(0.2, &[0, 1]);
        layer.path_objects[0] = None;
        layer.path_roles[0] = ExtrusionRole::Skirt;

        let gcode = generate_gcode_for_plate(
            &plate(
                vec![layer],
                vec![identity(0, "cube_a", 0.0), identity(1, "cube_b", 30.0)],
            ),
            &klipper_params(),
        );
        let markers: Vec<&str> = gcode
            .lines()
            .filter(|l| l.starts_with("EXCLUDE_OBJECT_"))
            .filter(|l| !l.starts_with("EXCLUDE_OBJECT_DEFINE"))
            .collect();
        assert_eq!(
            markers,
            vec![
                "EXCLUDE_OBJECT_START NAME=cube_b",
                "EXCLUDE_OBJECT_END NAME=cube_b",
            ],
            "the skirt belongs to no object: {gcode}"
        );
    }

    #[test]
    fn sequential_alone_does_not_emit_object_markers() {
        // Printing one part at a time is a *motion* choice; firmware object
        // tracking is a separate opt-in and must not ride along with it.
        let params = SlicingParams {
            gcode_flavor: crate::gcode::GcodeFlavor::Klipper,
            print_sequence: PrintSequence::ByObject,
            exclude_object: false,
            ..Default::default()
        };
        let gcode = generate_gcode_for_plate(
            &plate(
                vec![tagged_layer(0.2, &[0]), tagged_layer(0.2, &[1])],
                vec![identity(0, "cube_a", 0.0), identity(1, "cube_b", 30.0)],
            ),
            &params,
        );
        assert!(!gcode.contains("EXCLUDE_OBJECT"), "{gcode}");
        assert!(!gcode.contains("M486"), "{gcode}");
        // The hand-over itself still happens.
        assert!(gcode.contains("; clear the finished object"), "{gcode}");
    }

    /// One object's worth of layers: `flat` solid base layers then `spiral`
    /// single-loop layers, starting at `z0` and stepping by 0.2 mm.
    fn spiral_stack(object: usize, x: f64, z0: f64, flat: usize, spiral: usize) -> Vec<SliceLayer> {
        (0..flat + spiral)
            .map(|i| {
                let mut layer = tagged_layer(z0 + i as f64 * 0.2, &[object]);
                // Re-place the square so each object sits at its own X.
                layer.paths = clipper2::Paths::new(vec![vec![
                    (x, 0.0),
                    (x + 10.0, 0.0),
                    (x + 10.0, 10.0),
                    (x, 10.0),
                ]
                .into()]);
                layer
            })
            .collect()
    }

    #[test]
    fn sequential_spiral_vase_never_ramps_down_into_the_previous_object() {
        // Two vases, one after the other. The regression: the second object's
        // first spiral layer used to take its ramp start from the *previous
        // object's top* — a continuous extruding descent straight through the
        // plate — and the clearance lift never saw a spiral layer at all, so it
        // travelled through the finished vase.
        let mut layers = spiral_stack(0, 0.0, 0.2, 1, 8);
        layers.extend(spiral_stack(1, 60.0, 0.2, 1, 4));
        let params = SlicingParams {
            gcode_flavor: crate::gcode::GcodeFlavor::Klipper,
            print_sequence: PrintSequence::ByObject,
            exclude_object: true,
            spiral_vase: true,
            bottom_layers: 1,
            layer_height: 0.2,
            ..Default::default()
        };
        let gcode = generate_gcode_for_plate(
            &plate(
                layers,
                vec![identity(0, "vase_a", 0.0), identity(1, "vase_b", 60.0)],
            ),
            &params,
        );

        // No move may lose height while extruding.
        let mut z = 0.0_f64;
        let mut e = 0.0_f64;
        for line in gcode.lines() {
            if line.starts_with("G92 E0") {
                e = 0.0;
                continue;
            }
            if !line.starts_with("G1 ") {
                continue;
            }
            let field = |tag: char| {
                line.split_whitespace()
                    .find_map(|t| t.strip_prefix(tag))
                    .and_then(|v| v.parse::<f64>().ok())
            };
            let (nz, ne) = (field('Z'), field('E'));
            if let (Some(nz), Some(ne)) = (nz, ne) {
                assert!(
                    !(nz < z - 1e-9 && ne > e + 1e-9),
                    "extruding descent from {z} to {nz}: {line}"
                );
            }
            z = nz.unwrap_or(z);
            e = ne.unwrap_or(e);
        }

        // Exactly one hand-over, and it clears the finished vase (top 1.6 mm).
        assert_eq!(
            gcode.matches("; clear the finished object").count(),
            1,
            "{gcode}"
        );
        let lift_line = gcode
            .lines()
            .find(|l| l.ends_with("; clear the finished object"))
            .unwrap();
        let lift_z: f64 = lift_line
            .split_whitespace()
            .find_map(|t| t.strip_prefix('Z'))
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            lift_z > 1.6,
            "lift must clear the spiralised object: {lift_line}"
        );

        assert_eq!(gcode.matches("EXCLUDE_OBJECT_START NAME=vase_b").count(), 1);
        assert_eq!(gcode.matches("EXCLUDE_OBJECT_END NAME=vase_b").count(), 1);

        // Each vase keeps its own solid base — `bottom_layers` counts within an
        // object's stack, not from the start of the plate. A flat layer's
        // extrusions carry no Z; a spiral layer's ramp always does.
        let vase_b = &gcode[gcode.find("EXCLUDE_OBJECT_START NAME=vase_b").unwrap()..];
        let first_extrusion = vase_b
            .lines()
            .find(|l| l.starts_with("G1 ") && l.contains(" E"))
            .expect("vase_b prints something");
        assert!(
            !first_extrusion.contains(" Z"),
            "vase_b must start on a flat base layer, not mid-ramp: {first_extrusion}"
        );
    }

    #[test]
    fn an_empty_first_layer_still_triggers_the_hand_over() {
        // A layer with no paths carries no object tag. If the hand-over keyed
        // on the tag alone, such a layer would slip through and its Z move
        // would descend to the new object's first layer while the nozzle was
        // still over the finished one.
        let layers = vec![
            tagged_layer(0.2, &[0]),
            tagged_layer(4.0, &[0]),
            SliceLayer::new(0.2),
            tagged_layer(0.4, &[1]),
        ];
        let params = SlicingParams {
            print_sequence: PrintSequence::ByObject,
            ..Default::default()
        };
        let gcode = generate_gcode_for_plate(
            &plate(
                layers,
                vec![identity(0, "cube_a", 0.0), identity(1, "cube_b", 80.0)],
            ),
            &params,
        );

        let lift = gcode
            .find("; clear the finished object")
            .expect("the hand-over must fire before the empty layer's Z move");
        let descent = gcode[lift..]
            .find("G1 Z0.200")
            .expect("the empty layer still moves Z");
        assert!(descent > 0, "the descent must follow the lift");
        assert_eq!(gcode.matches("; clear the finished object").count(), 1);
    }

    #[test]
    fn no_objects_means_no_markers_at_all() {
        let mut layer = SliceLayer::new(0.2);
        let square: clipper2::Path =
            vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer.paths.push(square);
        layer.path_roles.push(ExtrusionRole::OuterWall);

        let gcode = generate_gcode_for_plate(
            &PlateSlice::from_layers(vec![layer.clone()]),
            &SlicingParams::default(),
        );
        assert!(!gcode.contains("M486"));
        assert_eq!(gcode, generate_gcode(&[layer], &SlicingParams::default()));
    }

    #[test]
    fn sequential_lifts_clear_of_the_finished_object_before_travelling() {
        // Object 0 printed to z=4.0, then object 1 starts again at z=0.2.
        let layers = vec![
            tagged_layer(0.2, &[0]),
            tagged_layer(4.0, &[0]),
            {
                let mut l = tagged_layer(0.2, &[1]);
                // Put object 1 well away from object 0.
                l.paths = clipper2::Paths::new(vec![vec![
                    (80.0, 80.0),
                    (90.0, 80.0),
                    (90.0, 90.0),
                    (80.0, 90.0),
                ]
                .into()]);
                l
            },
            tagged_layer(2.0, &[1]),
        ];
        let params = SlicingParams {
            gcode_flavor: crate::gcode::GcodeFlavor::Klipper,
            print_sequence: PrintSequence::ByObject,
            exclude_object: true,
            between_objects_gcode: Some("M117 next object".to_string()),
            ..Default::default()
        };
        let gcode = generate_gcode_for_plate(
            &plate(
                layers,
                vec![identity(0, "cube_a", 0.0), identity(1, "cube_b", 80.0)],
            ),
            &params,
        );

        let lift = gcode
            .find("; clear the finished object")
            .expect("expected a clearance lift at the object boundary");
        let travel = gcode
            .find("; travel to the next object")
            .expect("expected a travel to the next object");
        let hook = gcode
            .find("M117 next object")
            .expect("between-objects hook");
        let drop_back = gcode[travel..]
            .find("G1 Z0.200")
            .map(|i| i + travel)
            .expect("the next object's first layer Z move");

        // Order is the whole point: lift → travel → hook → only then descend.
        assert!(
            lift < travel && travel < hook && hook < drop_back,
            "{gcode}"
        );

        // The lift clears the 4.0 mm object already on the bed.
        let lift_line = gcode[..lift].lines().last().unwrap();
        let z: f64 = lift_line
            .split_whitespace()
            .find_map(|t| t.strip_prefix('Z'))
            .unwrap()
            .parse()
            .unwrap();
        assert!(z > 4.0, "lift must clear the finished object: {lift_line}");

        // Each object's block is opened once and closed once.
        assert_eq!(gcode.matches("EXCLUDE_OBJECT_START NAME=cube_a").count(), 1);
        assert_eq!(gcode.matches("EXCLUDE_OBJECT_END NAME=cube_a").count(), 1);
        assert_eq!(gcode.matches("EXCLUDE_OBJECT_START NAME=cube_b").count(), 1);
        assert_eq!(gcode.matches("EXCLUDE_OBJECT_END NAME=cube_b").count(), 1);
    }

    #[test]
    fn sequential_clearance_lift_carries_the_machine_z_offset() {
        // The finished part physically sits at its own offset machine Z, so the
        // lift that clears it has to move by the same amount — otherwise a
        // negative offset would eat into the clearance the lift exists to
        // guarantee (issue #102 × #112).
        let layers = vec![
            tagged_layer(0.2, &[0]),
            tagged_layer(4.0, &[0]),
            {
                let mut l = tagged_layer(0.2, &[1]);
                l.paths = clipper2::Paths::new(vec![vec![
                    (80.0, 80.0),
                    (90.0, 80.0),
                    (90.0, 90.0),
                    (80.0, 90.0),
                ]
                .into()]);
                l
            },
            tagged_layer(2.0, &[1]),
        ];
        let base = SlicingParams {
            gcode_flavor: crate::gcode::GcodeFlavor::Klipper,
            print_sequence: PrintSequence::ByObject,
            ..Default::default()
        };
        let objects = vec![identity(0, "cube_a", 0.0), identity(1, "cube_b", 80.0)];

        let lift_z = |params: &SlicingParams| -> f64 {
            let gcode = generate_gcode_for_plate(&plate(layers.clone(), objects.clone()), params);
            let at = gcode
                .find("; clear the finished object")
                .expect("expected a clearance lift");
            gcode[..at]
                .lines()
                .last()
                .unwrap()
                .split_whitespace()
                .find_map(|t| t.strip_prefix('Z')?.parse::<f64>().ok())
                .expect("lift line should carry a Z")
        };

        let plain = lift_z(&base);
        for offset in [0.3_f64, -0.3] {
            let shifted = lift_z(&SlicingParams {
                z_offset_mm: offset,
                ..base.clone()
            });
            assert!(
                (shifted - (plain + offset)).abs() < 1e-6,
                "clearance lift {plain} should shift to {} with offset {offset}, got {shifted}",
                plain + offset
            );
        }
    }
}
