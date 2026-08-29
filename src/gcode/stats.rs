//! Aggregate slice statistics for the G-code metadata header (issue #15).
//!
//! [`SliceStatistics`] is a bundle of *measured* and *derived* quantities about
//! a finished slice — layer count, model height, filament usage, and an
//! estimated print time. It is **not** a settings snapshot: everything here is
//! computed from the sliced [`SliceLayer`]s plus the running filament total the
//! generator accumulates while it emits extrusion moves.
//!
//! The struct is populated by
//! [`crate::gcode::GcodeGenerator::generate_with_stats`] and rendered into the
//! flavor-specific header by [`crate::gcode::GcodeDialect::header`].

use crate::core::SliceLayer;
use crate::settings::params::SlicingParams;

/// Aggregate print statistics computed from a full set of sliced layers.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceStatistics {
    /// Number of printed layers.
    pub layer_count: usize,
    /// Highest layer Z in millimetres (the model's printed height).
    pub max_z_mm: f64,
    /// Total filament **length** fed through the extruder (mm of feedstock).
    pub filament_mm: f64,
    /// Total filament **volume** in cm³ (feedstock cross-section × length).
    pub filament_cm3: f64,
    /// Total filament **weight** in grams (`volume × density`).
    pub filament_g: f64,
    /// Total material **cost** in the filament profile's currency
    /// (`weight × price-per-kg`). `0.0` when the price is unknown.
    pub filament_cost: f64,
    /// Estimated print time in seconds (sum of per-layer XY travel time).
    pub estimated_print_time_s: f64,
    /// Axis-aligned bounding-box minimum `[x, y, z]` over all extrusion points.
    pub bbox_min: [f64; 3],
    /// Axis-aligned bounding-box maximum `[x, y, z]` over all extrusion points.
    pub bbox_max: [f64; 3],
    /// Source model name (file stem), when known.
    pub model_name: Option<String>,
}

impl SliceStatistics {
    /// Build statistics from the sliced layers, the resolved parameters, the
    /// running filament length the generator measured while emitting moves, the
    /// acceleration-aware print-time estimate, and an optional model name.
    ///
    /// Geometric fields (layer count, height, bounding box) are derived purely
    /// from `layers`; the filament weight is derived from `filament_mm` and
    /// `params.filament_density_g_cm3`, and the material cost from that weight
    /// and `params.filament_cost_per_kg`.  The print time is **not** recomputed
    /// here — it is measured from the emitted G-code by
    /// [`crate::gcode::time_estimate::estimate_print_time`] and passed in as
    /// `estimated_print_time_s`, so the header/footer figure matches the moves
    /// the printer will actually run.
    pub fn from_layers(
        layers: &[SliceLayer],
        params: &SlicingParams,
        filament_mm: f64,
        estimated_print_time_s: f64,
        model_name: Option<String>,
    ) -> Self {
        let filament_radius_mm = params.filament_diameter_mm / 2.0;
        let filament_area_mm2 = std::f64::consts::PI * filament_radius_mm * filament_radius_mm;
        let filament_mm3 = filament_area_mm2 * filament_mm;
        let filament_cm3 = filament_mm3 / 1000.0;
        let filament_g = filament_cm3 * params.filament_density_g_cm3;
        let filament_cost = filament_g / 1000.0 * params.filament_cost_per_kg.max(0.0);

        let mut max_z_mm = 0.0_f64;
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];

        for layer in layers {
            max_z_mm = max_z_mm.max(layer.z);
            for path in layer.paths.iter() {
                for pt in path.iter() {
                    let (x, y) = (pt.x(), pt.y());
                    min[0] = min[0].min(x);
                    min[1] = min[1].min(y);
                    min[2] = min[2].min(layer.z);
                    max[0] = max[0].max(x);
                    max[1] = max[1].max(y);
                    max[2] = max[2].max(layer.z);
                }
            }
        }

        // Collapse the sentinels to zero for an empty / point-free slice so the
        // header never prints `inf`.
        if !min[0].is_finite() {
            min = [0.0; 3];
            max = [0.0; 3];
        }

        Self {
            layer_count: layers.len(),
            max_z_mm,
            filament_mm,
            filament_cm3,
            filament_g,
            filament_cost,
            estimated_print_time_s,
            bbox_min: min,
            bbox_max: max,
            model_name,
        }
    }

    /// Format the estimated print time as a compact `` `1h 2m 3s` `` string.
    ///
    /// Mirrors the OrcaSlicer / PrusaSlicer `estimated printing time` style:
    /// the hours and minutes fields are omitted while zero, but seconds are
    /// always shown so a sub-minute print still reads as e.g. `6s`.
    pub fn estimated_time_human(&self) -> String {
        let total = self.estimated_print_time_s.max(0.0).round() as u64;
        let h = total / 3600;
        let m = (total % 3600) / 60;
        let s = total % 60;
        let mut out = String::new();
        if h > 0 {
            out.push_str(&format!("{h}h "));
        }
        if h > 0 || m > 0 {
            out.push_str(&format!("{m}m "));
        }
        out.push_str(&format!("{s}s"));
        out
    }
}

/// Local wall-clock timestamp for the header (`YYYY-MM-DD at HH:MM:SS`).
///
/// The `gcode` module also compiles into the `web-slicer` wasm bundle, where
/// there is no system clock, so the timestamp collapses to `unknown` there.
fn slice_timestamp() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    {
        chrono::Local::now()
            .format("%Y-%m-%d at %H:%M:%S")
            .to_string()
    }
    #[cfg(target_arch = "wasm32")]
    {
        "unknown".to_string()
    }
}

/// The measured/derived statistics block: provenance (slicer version +
/// timestamp), flavor, model name, layer count, height, filament usage, print
/// time, and bounding box. Dialects wrap this list with their own block markers
/// (see [`crate::gcode::GcodeDialect::header`]).
pub(crate) fn metadata_lines(flavor_name: &str, stats: &SliceStatistics) -> Vec<String> {
    let mut lines = vec![
        format!(
            "; generated by Cold Crabby {} on {}",
            crate::version::VERSION,
            slice_timestamp()
        ),
        format!("; flavor: {flavor_name}"),
    ];
    if let Some(name) = &stats.model_name {
        lines.push(format!("; model: {name}"));
    }
    lines.push(format!("; total layers count = {}", stats.layer_count));
    lines.push(format!("; max_z_height = {:.2} mm", stats.max_z_mm));
    lines.push(format!("; filament used [mm] = {:.2}", stats.filament_mm));
    lines.push(format!("; filament used [cm3] = {:.2}", stats.filament_cm3));
    lines.push(format!("; filament used [g] = {:.2}", stats.filament_g));
    lines.push(format!(
        "; estimated printing time = {}",
        stats.estimated_time_human()
    ));
    lines.push(format!(
        "; model bounding box [mm] = {:.2},{:.2},{:.2} -> {:.2},{:.2},{:.2}",
        stats.bbox_min[0],
        stats.bbox_min[1],
        stats.bbox_min[2],
        stats.bbox_max[0],
        stats.bbox_max[1],
        stats.bbox_max[2],
    ));
    lines
}

/// The human-readable print-settings summary that follows the metadata block.
///
/// Kept as plain `; key: value` comments (terminated by `; ---`) so existing
/// post-processors that grepped the original header keep working.
pub(crate) fn settings_summary_lines(params: &SlicingParams) -> Vec<String> {
    let mut lines = vec![
        format!("; layer_height: {} mm", params.layer_height),
        format!("; nozzle_temp: {} °C", params.nozzle_temp),
        format!("; bed_temp: {} °C", params.bed_temp),
    ];
    // Bed/plate surface is tracked for printer integration (issue #11); only
    // emitted when the user actually selected one.
    if !params.bed_type.trim().is_empty() {
        lines.push(format!("; bed_type: {}", params.bed_type));
    }
    lines.extend([
        format!(
            "; print_speed: {} mm/s | perimeter: {} | infill: {} | bridge: {} | first_layer: {}",
            params.print_speed,
            params.perimeter_speed,
            params.infill_speed,
            params.bridge_speed,
            params.first_layer_speed,
        ),
        format!(
            "; wall_count: {} walls ({} generator)",
            params.wall_count,
            params.wall_generator.name(),
        ),
        format!(
            "; perimeter_order: {} | extra_perimeters: {} | thin_walls: {} | ensure_vertical_shell: {} | avoid_crossing: {}",
            if params.external_perimeters_first {
                "outer-first"
            } else {
                "inner-first"
            },
            if params.extra_perimeters { "on" } else { "off" },
            if params.thin_walls { "on" } else { "off" },
            if params.ensure_vertical_shell_thickness {
                "on"
            } else {
                "off"
            },
            if params.avoid_crossing_perimeters {
                "on"
            } else {
                "off"
            },
        ),
        format!("; infill_density: {:.0}%", params.infill_density * 100.0),
        "; ---".to_string(),
    ]);
    lines
}

/// PrusaSlicer/OrcaSlicer-compatible metadata block emitted at the **end** of
/// the program (the file *footer*), where printer front-ends actually scan for
/// print metadata.
///
/// Moonraker (Mainsail / Fluidd) recognises us as a PrusaSlicer-family slicer
/// via the generic `; generated by <name> on <date>` provenance line, then runs
/// its `PrusaSlicer` parser — which reads **every** field from the last 1 MiB of
/// the file using `; key = value` comments, *not* the human-readable header
/// block above. Emitting this block is what makes filament type, colour, layer
/// height, object height, filament usage, and the machine the file was sliced
/// for show up in the printer UI. The same footer is understood by OctoPrint's
/// slicer parser.
///
/// **Scope rule: emit only what a printer front-end consumes.** This is
/// deliberately not a dump of all ~130 resolved settings — every line here is
/// matched by a parser in the wild. The human-readable
/// [`settings_summary_lines`] header covers the rest.
///
/// Object height is intentionally *not* emitted here: the default per-layer
/// `;BEFORE_LAYER_CHANGE` + bare `;<z>` markers the generator already writes are
/// exactly what the PrusaSlicer parser reads for it, and they stay correct even
/// when the end G-code raises Z.
pub(crate) fn config_block_lines(params: &SlicingParams, stats: &SliceStatistics) -> Vec<String> {
    // Resolve "0 = inherit base" fields to the value the printer will actually
    // use so the metadata reflects reality rather than the sentinel.
    let first_layer_height = if params.first_layer_height > 0.0 {
        params.first_layer_height
    } else {
        params.layer_height
    };
    let first_layer_temp = if params.nozzle_temp_first_layer > 0.0 {
        params.nozzle_temp_first_layer
    } else {
        params.nozzle_temp
    };
    let first_layer_bed_temp = if params.bed_temp_first_layer > 0.0 {
        params.bed_temp_first_layer
    } else {
        params.bed_temp
    };

    let mut lines = vec![
        ";".to_string(),
        // ── Print statistics (Moonraker reads filament usage + time here) ─────
        format!("; filament used [mm] = {:.2}", stats.filament_mm),
        format!("; filament used [cm3] = {:.2}", stats.filament_cm3),
        format!("; filament used [g] = {:.2}", stats.filament_g),
        format!("; total filament used [g] = {:.2}", stats.filament_g),
    ];

    // Material cost, in the PrusaSlicer slot right after the weight it derives
    // from. Only emitted when the active filament profile carries a price, so a
    // flag-only CLI slice doesn't report a free print.
    if stats.filament_cost > 0.0 {
        lines.push(format!(
            "; total filament cost = {:.2}",
            stats.filament_cost
        ));
    }

    lines.extend([
        format!(
            "; estimated printing time (normal mode) = {}",
            stats.estimated_time_human()
        ),
        format!("; total layers count = {}", stats.layer_count),
        ";".to_string(),
        // ── Slice configuration (PrusaSlicer `key = value` convention) ────────
        "; prusaslicer_config = begin".to_string(),
        format!("; layer_height = {}", params.layer_height),
        format!("; first_layer_height = {}", first_layer_height),
        format!("; nozzle_diameter = {}", params.nozzle_diameter_mm),
        format!("; filament_diameter = {}", params.filament_diameter_mm),
        format!("; filament_density = {}", params.filament_density_g_cm3),
        format!("; temperature = {:.0}", params.nozzle_temp),
        format!("; first_layer_temperature = {:.0}", first_layer_temp),
        format!("; bed_temperature = {:.0}", params.bed_temp),
        format!(
            "; first_layer_bed_temperature = {:.0}",
            first_layer_bed_temp
        ),
        format!("; chamber_temperature = {:.0}", params.chamber_temp),
        format!("; fill_density = {:.0}%", params.infill_density * 100.0),
    ]);

    // Filament identity — only emitted when the active profile supplied a value,
    // so a flag-only CLI slice (no profile) doesn't advertise an empty
    // material / name / colour.
    if !params.filament_type.trim().is_empty() {
        lines.push(format!("; filament_type = {}", params.filament_type.trim()));
    }
    if !params.filament_name.trim().is_empty() {
        lines.push(format!(
            "; filament_settings_id = {}",
            params.filament_name.trim()
        ));
    }
    if !params.filament_color.trim().is_empty() {
        lines.push(format!(
            "; filament_colour = {}",
            params.filament_color.trim()
        ));
        // Moonraker falls back to `extruder_colour` for the swatch when a
        // front-end asks for the tool colour rather than the filament colour;
        // single-extruder machines have nothing else to put there.
        lines.push(format!(
            "; extruder_colour = {}",
            params.filament_color.trim()
        ));
    }

    // Machine identity — printer front-ends display it alongside the job, and
    // some warn when a file was sliced for a different machine.
    if !params.printer_vendor.trim().is_empty() {
        lines.push(format!(
            "; printer_vendor = {}",
            params.printer_vendor.trim()
        ));
    }
    if !params.printer_model.trim().is_empty() {
        lines.push(format!("; printer_model = {}", params.printer_model.trim()));
    }

    lines.push("; prusaslicer_config = end".to_string());
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::SliceLayer;

    #[test]
    fn empty_slice_has_zeroed_stats() {
        let params = SlicingParams::default();
        let stats = SliceStatistics::from_layers(&[], &params, 0.0, 0.0, None);
        assert_eq!(stats.layer_count, 0);
        assert_eq!(stats.max_z_mm, 0.0);
        assert_eq!(stats.filament_mm, 0.0);
        assert_eq!(stats.filament_g, 0.0);
        // Sentinels must collapse to zero, never leak `inf` into the header.
        assert_eq!(stats.bbox_min, [0.0; 3]);
        assert_eq!(stats.bbox_max, [0.0; 3]);
    }

    #[test]
    fn filament_weight_follows_volume_times_density() {
        // 1.75 mm filament: area = π·(0.875)² ≈ 2.4053 mm².
        // 1000 mm feedstock → 2405.3 mm³ → 2.4053 cm³.
        // PLA default density 1.24 g/cm³ → ≈ 2.982 g.
        let params = SlicingParams {
            filament_diameter_mm: 1.75,
            filament_density_g_cm3: 1.24,
            ..SlicingParams::default()
        };
        let stats = SliceStatistics::from_layers(&[], &params, 1000.0, 0.0, None);
        assert!(
            (stats.filament_cm3 - 2.4053).abs() < 1e-3,
            "cm3 = {}",
            stats.filament_cm3
        );
        assert!(
            (stats.filament_g - 2.9826).abs() < 1e-3,
            "g = {}",
            stats.filament_g
        );
    }

    /// Cost is weight-derived, so an unpriced filament reports nothing rather
    /// than a misleading zero-cost print (issue #23).
    #[test]
    fn filament_cost_follows_weight_times_price() {
        let priced = SlicingParams {
            filament_diameter_mm: 1.75,
            filament_density_g_cm3: 1.24,
            filament_cost_per_kg: 25.0,
            ..SlicingParams::default()
        };
        // 1000 mm of 1.75 mm PLA ≈ 2.9826 g → 2.9826 g × 25 /kg ≈ 0.0746.
        let stats = SliceStatistics::from_layers(&[], &priced, 1000.0, 0.0, None);
        assert!(
            (stats.filament_cost - 0.07456).abs() < 1e-4,
            "cost = {}",
            stats.filament_cost
        );

        let unpriced = SlicingParams {
            filament_cost_per_kg: 0.0,
            ..priced
        };
        let stats = SliceStatistics::from_layers(&[], &unpriced, 1000.0, 0.0, None);
        assert_eq!(stats.filament_cost, 0.0);
    }

    #[test]
    fn bounding_box_and_height_track_geometry() {
        use clipper2::Path;
        let mut l0 = SliceLayer::new(0.2);
        let sq: Path = vec![(1.0, 2.0), (11.0, 2.0), (11.0, 8.0), (1.0, 8.0)].into();
        l0.paths.push(sq);
        let mut l1 = SliceLayer::new(0.4);
        let sq2: Path = vec![(0.0, 0.0), (5.0, 0.0), (5.0, 5.0), (0.0, 5.0)].into();
        l1.paths.push(sq2);

        let params = SlicingParams::default();
        let stats = SliceStatistics::from_layers(&[l0, l1], &params, 0.0, 0.0, None);
        assert_eq!(stats.layer_count, 2);
        assert!((stats.max_z_mm - 0.4).abs() < 1e-9);
        assert_eq!(stats.bbox_min, [0.0, 0.0, 0.2]);
        assert_eq!(stats.bbox_max, [11.0, 8.0, 0.4]);
    }

    #[test]
    fn estimated_time_human_formats_compactly() {
        let mk = |s: f64| SliceStatistics {
            layer_count: 0,
            max_z_mm: 0.0,
            filament_mm: 0.0,
            filament_cm3: 0.0,
            filament_g: 0.0,
            filament_cost: 0.0,
            estimated_print_time_s: s,
            bbox_min: [0.0; 3],
            bbox_max: [0.0; 3],
            model_name: None,
        };
        assert_eq!(mk(6.0).estimated_time_human(), "6s");
        assert_eq!(mk(65.0).estimated_time_human(), "1m 5s");
        assert_eq!(mk(3661.0).estimated_time_human(), "1h 1m 1s");
    }
}
