//! [`GcodeGenerator`] — per-layer extrusion logic and the `generate_gcode` convenience wrapper.

use std::borrow::Cow;

use crate::core::SliceLayer;
use crate::gcode::dialect::{GcodeDialect, WarnFn};
use crate::gcode::dialects::{KlipperDialect, MarlinDialect};
use crate::gcode::flavor::GcodeFlavor;
use crate::settings::params::{LifecycleMarkerConfig, SlicingParams};

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

/// Width step (mm) at which a variable-width bead re-emits a `;WIDTH:` marker
/// mid-path, so viewers/post-processors render the actual (flow-compensated)
/// bead width rather than the nominal scalar.  Coarse enough (0.05 mm) to keep
/// the marker count \u2014 and G-code size \u2014 bounded, fine enough to show the taper.
const WIDTH_MARKER_STEP_MM: f64 = 0.05;

/// Estimate the print time for a layer in seconds.
///
/// Sums the total XY move distance for all paths in the layer and divides by
/// `print_speed_mm_s`.  Travel moves are not modelled separately; this gives
/// a conservative lower-bound that is close enough for fan-speed decisions.
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

/// Resolve the extrusion width for a path.
///
/// Precedence: an explicit per-path width (Arachne bead width, bridge-flow
/// reduction, …) always wins.  Otherwise the `line_width` setting overrides the
/// nozzle-derived default for **solid infill and surfaces** when set (`> 0`);
/// every other role falls back to its role default.
pub(crate) fn resolve_width_mm(
    explicit: Option<f64>,
    role: crate::core::ExtrusionRole,
    line_width: f64,
) -> f64 {
    if let Some(w) = explicit {
        return w;
    }
    let line_width_role = matches!(
        role,
        crate::core::ExtrusionRole::Infill
            | crate::core::ExtrusionRole::TopSurface
            | crate::core::ExtrusionRole::BottomSurface
    );
    if line_width > 0.0 && line_width_role {
        line_width
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
/// `{chamber_temp}`, `{filament_type}`, plus `{layer_height}` and
/// `{first_layer_height}`.
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
    let first_height = if params.first_layer_height > 0.0 {
        params.first_layer_height
    } else {
        params.layer_height
    };
    line.replace("{nozzle_temp_first_layer}", &format!("{:.0}", first_nozzle))
        .replace("{bed_temp_first_layer}", &format!("{:.0}", first_bed))
        .replace("{nozzle_temp}", &format!("{:.0}", params.nozzle_temp))
        .replace("{bed_temp}", &format!("{:.0}", params.bed_temp))
        .replace("{chamber_temp}", &format!("{:.0}", params.chamber_temp))
        .replace("{filament_type}", &params.filament_type)
        .replace("{first_layer_height}", &format!("{:.3}", first_height))
        .replace("{layer_height}", &format!("{:.3}", params.layer_height))
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

    /// Resolve the effective print speed (in mm/min) for a given extrusion role
    /// and layer context.
    ///
    /// The priority order is:
    /// 1. First-layer speed (when `is_first_layer` is true)
    /// 2. Role-specific speed (perimeter, infill, bridge, top/bottom surface)
    /// 3. General `print_speed` fallback when a role-specific speed is ≤ 0
    fn effective_speed_mm_min(
        role: crate::core::ExtrusionRole,
        is_first_layer: bool,
        params: &SlicingParams,
    ) -> f64 {
        use crate::core::ExtrusionRole;
        let fallback = params.print_speed * 60.0;
        if is_first_layer {
            let s = params.first_layer_speed;
            return if s > 0.0 { s * 60.0 } else { fallback };
        }
        match role {
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
            ExtrusionRole::TopSurface | ExtrusionRole::BottomSurface => {
                let s = params.top_surface_speed;
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
        }
    }

    /// Generate a complete G-code program from the given layers and parameters.
    ///
    /// The output is a single `String` with lines separated by `'\n'`.
    /// Returns a minimal (start + end only) program when `layers` is empty.
    ///
    /// If any commands are listed in [`GcodeDialect::unsupported_commands`] the
    /// registered warn function is called once per unsupported command before
    /// generation begins.
    pub fn generate(&self, layers: &[SliceLayer], params: &SlicingParams) -> String {
        // Warn about any commands the dialect doesn't natively support
        for cmd in self.dialect.unsupported_commands() {
            self.warn(&format!(
                "Command '{}' is not natively supported by the {} dialect; \
                 falling back to generic G-code",
                cmd,
                self.dialect.flavor_name()
            ));
        }

        let mut out = String::with_capacity(64 * 1024);

        // ── Metadata header ──────────────────────────────────────────────────
        use std::fmt::Write as _;
        let _ = write!(
            out,
            "; Generated by slicer-engine | flavor: {}\n\
             ; layer_height: {} mm\n\
             ; nozzle_temp: {} °C\n\
             ; bed_temp: {} °C\n\
             ; print_speed: {} mm/s | perimeter: {} | infill: {} | bridge: {} | first_layer: {}\n\
             ; wall_count: {} walls ({} generator)\n\
             ; infill_density: {:.0}%\n\
             ; ---\n",
            self.dialect.flavor_name(),
            params.layer_height,
            params.nozzle_temp,
            params.bed_temp,
            params.print_speed,
            params.perimeter_speed,
            params.infill_speed,
            params.bridge_speed,
            params.first_layer_speed,
            params.wall_count,
            params.wall_generator.name(),
            params.infill_density * 100.0,
        );

        // ── Start script (custom override or flavor default) ──────────────────
        let start_script: Cow<[String]> = match &self.custom_start_script {
            Some(lines) => Cow::Borrowed(lines),
            None => Cow::Owned(self.dialect.start_script(params)),
        };
        for line in start_script.iter() {
            out.push_str(&render_script_placeholders(line, params));
            out.push('\n');
        }

        // The whole generator emits absolute E positions (accumulating `e_total`,
        // `G92 E0` per layer).  A custom start script or a Klipper `START_PRINT`
        // macro that primes / uses firmware retraction can leave the extruder in
        // relative mode (`M83`), which would make every `G1 … E<e_total>` a
        // relative extrusion of the full running total → gross over-extrusion.
        // Force absolute mode and zero the counter so the invariant always holds.
        out.push_str(&format!("{}\n", self.dialect.extruder_absolute_mode()));
        out.push_str(&format!("{}\n", self.dialect.reset_extruder()));

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

        // ── Per-layer contours ────────────────────────────────────────────────
        let mut e_total = 0.0_f64;
        // Track previous fan speed per config index for rate limiting (aux overrides).
        let mut prev_fan_speeds: Vec<Option<f64>> = vec![None; params.fan_configs.len()];

        for (layer_index, layer) in layers.iter().enumerate() {
            let z_str = format!("{:.3}", layer.z);
            let height_str = format!("{:.3}", params.layer_height);
            // Detect first layer: z within half a layer height of layer_height.
            let is_first_layer = layer.z <= params.layer_height + 1e-6;

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

                out.push_str(&format!(
                    "{}\n",
                    self.dialect.move_z(layer.z, params.travel_speed_mm_min)
                ));

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
                out.push_str(&format!(
                    "{}\n",
                    self.dialect.move_z(layer.z, params.travel_speed_mm_min)
                ));
            }

            // ── Custom layer-change G-code (Klipper macros, timelapse, …) ─────
            if let Some(script) = &self.custom_layer_script {
                let layer_num = (layer_index + 1).to_string();
                for line in script {
                    let rendered = render_marker(line, &z_str, &height_str, "", "")
                        .replace("{layer_num}", &layer_num);
                    out.push_str(&render_script_placeholders(&rendered, params));
                    out.push('\n');
                }
            }

            // ── Adaptive fan speed ───────────────────────────────────────────
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
                    let speed = fan.compute_speed(layer_time, has_bridges, prev);
                    // Store the emitted speed for the next layer's rate-limiting.
                    if let Some(slot) = prev_fan_speeds.get_mut(fan_idx) {
                        *slot = Some(speed);
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

            let mut last_role: Option<crate::core::ExtrusionRole> = None;
            let mut last_width: Option<f64> = None;
            let mut last_pos: Option<(f64, f64)> = None;

            for (path_idx, path) in layer.paths.iter().enumerate() {
                let raw_points: Vec<(f64, f64)> = path.iter().map(|p| (p.x(), p.y())).collect();
                if raw_points.len() < 2 {
                    continue;
                }

                // Fetch the role and resolve the effective extrusion width
                let role = layer.role_for_path(path_idx);
                let width_mm =
                    resolve_width_mm(layer.width_for_path(path_idx), role, params.line_width);
                // Per-vertex widths (variable-width beads) taper the flow along
                // the path; `None` keeps the constant `width_mm`.
                let raw_vertex_widths = layer.vertex_widths_for_path(path_idx);

                // Resolve per-role print speed.
                let speed_mm_min = Self::effective_speed_mm_min(role, is_first_layer, params);

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
                            &height_str,
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
                            &height_str,
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
                // Retract policy:
                //   - Skip retract for any travel under `MIN_TRAVEL_FOR_RETRACT_MM`,
                //     even on role change.  A 0.4 mm hop from the inner-wall
                //     loop end to the outer-wall loop start does not ooze
                //     enough to justify the 5-line retract+zhop+travel+lower+
                //     un-retract ceremony (which itself takes longer than the
                //     hop and pauses extrusion).
                //   - Long travels always retract, regardless of role.
                //
                // Other slicers (PrusaSlicer, Orca, Cura) all use a similar
                // "min travel for retract" cutoff (typically 1.0–2.0 mm).
                const MIN_TRAVEL_FOR_RETRACT_MM: f64 = 1.0;
                let needs_retract =
                    travel_dist > 2.0 || (role_changed && travel_dist > MIN_TRAVEL_FOR_RETRACT_MM);

                if needs_retract {
                    // Retract, z-hop, travel, lower, prime
                    e_total -= params.retract_mm;
                    out.push_str(&format!(
                        "{} ; retract\n",
                        self.dialect.set_extruder_pos(e_total, 3000.0)
                    ));
                    out.push_str(&format!(
                        "{} ; z-hop\n",
                        self.dialect
                            .move_z(layer.z + params.z_hop_mm, params.travel_speed_mm_min)
                    ));
                    out.push_str(&format!(
                        "{} ; travel\n",
                        self.dialect
                            .travel_xy(start_x, start_y, params.travel_speed_mm_min)
                    ));
                    out.push_str(&format!(
                        "{} ; lower\n",
                        self.dialect.move_z(layer.z, params.travel_speed_mm_min)
                    ));
                    e_total += params.retract_mm;
                    out.push_str(&format!(
                        "{} ; un-retract\n",
                        self.dialect.set_extruder_pos(e_total, 3000.0)
                    ));
                } else if travel_dist > 0.05 {
                    // Short travel without stringing mitigation.  Travels under
                    // 0.05 mm are degenerate (floating-point rounding noise from
                    // path simplification) and are skipped entirely — emitting a
                    // G1 line for a sub-quantum hop just bloats the file and
                    // confuses motion planners on some firmwares.
                    out.push_str(&format!(
                        "{} ; short travel\n",
                        self.dialect
                            .travel_xy(start_x, start_y, params.travel_speed_mm_min)
                    ));
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
                            e_total += extrusion_for_move(
                                seg_len,
                                params.layer_height,
                                width_mm,
                                params.filament_diameter_mm,
                                params.flow_ratio,
                            );
                            out.push_str(&format!(
                                "{}\n",
                                self.dialect.move_extrude(x, y, e_total, speed_mm_min)
                            ));
                        } else if dist_traveled < coasting_start {
                            // Segment straddles the coasting boundary → split it
                            let dist_to_coast = coasting_start - dist_traveled;
                            let t = dist_to_coast / seg_len;
                            let bx = prev.0 + t * dx;
                            let by = prev.1 + t * dy;
                            e_total += extrusion_for_move(
                                dist_to_coast,
                                params.layer_height,
                                width_mm,
                                params.filament_diameter_mm,
                                params.flow_ratio,
                            );
                            out.push_str(&format!(
                                "{}\n",
                                self.dialect.move_extrude(bx, by, e_total, speed_mm_min)
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
                            e_total += extrusion_for_move(
                                seg_len,
                                params.layer_height,
                                width_mm,
                                params.filament_diameter_mm,
                                params.flow_ratio,
                            );
                            out.push_str(&format!(
                                "{} ; close contour\n",
                                self.dialect
                                    .move_extrude(start_x, start_y, e_total, speed_mm_min)
                            ));
                        } else if dist_traveled < coasting_start {
                            let dist_to_coast = coasting_start - dist_traveled;
                            let t = dist_to_coast / seg_len;
                            let bx = prev.0 + t * dx;
                            let by = prev.1 + t * dy;
                            e_total += extrusion_for_move(
                                dist_to_coast,
                                params.layer_height,
                                width_mm,
                                params.filament_diameter_mm,
                                params.flow_ratio,
                            );
                            out.push_str(&format!(
                                "{}\n",
                                self.dialect.move_extrude(bx, by, e_total, speed_mm_min)
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
                    let nozzle = params.nozzle_diameter_mm;
                    let seg_speed = |sw: f64| -> f64 {
                        if vertex_widths.is_some() && nozzle > 0.0 && sw > nozzle {
                            speed_mm_min * (nozzle / sw)
                        } else {
                            speed_mm_min
                        }
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
                                &height_str,
                                role.type_name(),
                                &width_str,
                            ));
                            out.push('\n');
                            last_width = Some(sw);
                        }
                        e_total += extrusion_for_move(
                            len,
                            params.layer_height,
                            sw,
                            params.filament_diameter_mm,
                            params.flow_ratio,
                        );
                        out.push_str(&format!(
                            "{}\n",
                            self.dialect.move_extrude(x, y, e_total, seg_speed(sw))
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
                            e_total += extrusion_for_move(
                                len,
                                params.layer_height,
                                width_mm,
                                params.filament_diameter_mm,
                                params.flow_ratio,
                            );
                            out.push_str(&format!(
                                "{} ; close contour\n",
                                self.dialect
                                    .move_extrude(start_x, start_y, e_total, speed_mm_min)
                            ));
                        }
                        last_pos = Some((start_x, start_y));
                    } else {
                        last_pos = Some(prev);
                    }
                }
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

        out
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
/// `params.gcode_flavor` and applies `params.start_gcode`, `params.end_gcode`
/// and `params.layer_gcode` when present (each split into lines on `'\n'`).
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
    generator.generate(layers, params)
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

    #[test]
    fn test_extrusion_for_move_positive() {
        let e = extrusion_for_move(10.0, 0.2, 0.4, 1.75, 1.0);
        assert!(e > 0.0, "extrusion must be positive");
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

    // ── Width resolution (line_width override) ─────────────────────────────────

    #[test]
    fn resolve_width_explicit_always_wins() {
        // An explicit per-path width (Arachne/bridge) is never overridden.
        let w = resolve_width_mm(Some(0.55), crate::core::ExtrusionRole::Infill, 0.44);
        assert_eq!(w, 0.55);
        let w = resolve_width_mm(Some(0.30), crate::core::ExtrusionRole::OuterWall, 0.44);
        assert_eq!(w, 0.30);
    }

    #[test]
    fn resolve_width_line_width_applies_to_infill_and_surfaces() {
        for role in [
            crate::core::ExtrusionRole::Infill,
            crate::core::ExtrusionRole::TopSurface,
            crate::core::ExtrusionRole::BottomSurface,
        ] {
            assert_eq!(resolve_width_mm(None, role, 0.44), 0.44, "{role:?}");
        }
    }

    #[test]
    fn resolve_width_line_width_ignored_for_walls_and_when_zero() {
        // Walls keep their role default regardless of line_width.
        assert_eq!(
            resolve_width_mm(None, crate::core::ExtrusionRole::OuterWall, 0.44),
            crate::core::ExtrusionRole::OuterWall.default_width_mm()
        );
        // line_width = 0 means "derive from nozzle" → role default.
        assert_eq!(
            resolve_width_mm(None, crate::core::ExtrusionRole::Infill, 0.0),
            crate::core::ExtrusionRole::Infill.default_width_mm()
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
        let mut params = SlicingParams::default();
        params.pressure_advance = 0.035;
        let gcode = GcodeGenerator::new(GcodeFlavor::Klipper).generate(&[], &params);
        assert!(
            gcode.contains("SET_PRESSURE_ADVANCE ADVANCE=0.0350 ; pressure advance"),
            "Klipper pressure advance not emitted:\n{gcode}"
        );
    }

    #[test]
    fn test_generator_emits_pressure_advance_marlin() {
        let mut params = SlicingParams::default();
        params.pressure_advance = 0.06;
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
        let mut params = SlicingParams::default();
        params.pressure_advance = 0.04;
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
        assert!(gcode.contains("; Generated by slicer-engine"));
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
        let s = GcodeGenerator::effective_speed_mm_min(ExtrusionRole::OuterWall, false, &params);
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
        let s = GcodeGenerator::effective_speed_mm_min(ExtrusionRole::OuterWall, false, &params);
        assert!(
            (s - 45.0 * 60.0).abs() < 1e-6,
            "expected perimeter_speed * 60"
        );
        let s = GcodeGenerator::effective_speed_mm_min(ExtrusionRole::InnerWall, false, &params);
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
        let s = GcodeGenerator::effective_speed_mm_min(ExtrusionRole::Infill, false, &params);
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
        let s = GcodeGenerator::effective_speed_mm_min(ExtrusionRole::Bridge, false, &params);
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
        let s = GcodeGenerator::effective_speed_mm_min(ExtrusionRole::Infill, true, &params);
        assert!(
            (s - 20.0 * 60.0).abs() < 1e-6,
            "first_layer_speed should override infill_speed on first layer"
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
}
