//! Klipper firmware G-code dialect.

use crate::gcode::stats::{self, SliceStatistics};
use crate::gcode::GcodeDialect;
use crate::settings::params::SlicingParams;

/// Klipper firmware G-code dialect.
///
/// Targets Klipper firmware's macro-based print workflow.  The default start
/// and end scripts delegate to the user-defined `START_PRINT` / `END_PRINT`
/// macros (OrcaSlicer-style), passing print parameters as macro arguments.
///
/// Printer-specific setup (homing, bed levelling, purge lines, etc.) is
/// handled inside those Klipper macros, keeping the slicer output clean and
/// portable across different Klipper printer configurations.
///
/// Extra Klipper-specific commands are available as helper methods:
/// - [`KlipperDialect::set_velocity_limit`] — runtime velocity/acceleration cap
/// - [`GcodeDialect::set_pressure_advance`] — pressure advance tuning (trait override)
/// - [`KlipperDialect::call_macro`] — invoke a named Klipper macro
pub struct KlipperDialect;

impl KlipperDialect {
    /// Emit a `SET_VELOCITY_LIMIT` command.
    ///
    /// Klipper uses this to configure the printer's motion system at runtime,
    /// which is more flexible than compile-time Marlin firmware limits.
    pub fn set_velocity_limit(&self, velocity: f64, accel: f64) -> String {
        format!(
            "SET_VELOCITY_LIMIT VELOCITY={:.0} ACCEL={:.0}",
            velocity, accel
        )
    }

    /// Invoke a named Klipper macro (e.g. `PRINT_START`, `PRINT_END`).
    ///
    /// The name is upper-cased to match Klipper macro naming conventions.
    pub fn call_macro(&self, name: &str) -> String {
        name.to_uppercase()
    }

    /// Return the Klipper fan name for a given fan index.
    ///
    /// Klipper uses named fans rather than P-indexed M106 commands:
    /// - P0 → `fan` (part-cooling; note the default `[fan]` object is driven by
    ///   `M106`/`M107`, so [`KlipperDialect::set_fan_speed_indexed`] only uses
    ///   this name when an explicit override is not supplied for a non-zero index)
    /// - P1 → `fan_hotend`
    /// - P2 → `fan_chamber`
    /// - P3 and above → `fan_aux`
    ///
    /// All indices beyond 3 map to `fan_aux` on the assumption that a printer
    /// with more than 4 fans would require custom start/end scripts rather than
    /// generic indexed fan commands.
    pub fn fan_name_for_index(fan_index: u8) -> &'static str {
        match fan_index {
            0 => "fan",
            1 => "fan_hotend",
            2 => "fan_chamber",
            _ => "fan_aux",
        }
    }
}

impl GcodeDialect for KlipperDialect {
    fn flavor_name(&self) -> &'static str {
        "Klipper"
    }

    /// Klipper-flavored header: the metadata block is delimited by
    /// `; KLIPPER_HEADER_START` / `; KLIPPER_HEADER_END`, keeping it distinct
    /// from the Marlin/Orca `HEADER_BLOCK` convention while carrying the same
    /// slice statistics.
    fn header(&self, params: &SlicingParams, stats: &SliceStatistics) -> Vec<String> {
        let mut lines = vec!["; KLIPPER_HEADER_START".to_string()];
        lines.extend(stats::metadata_lines(self.flavor_name(), stats));
        lines.push("; KLIPPER_HEADER_END".to_string());
        lines.extend(stats::settings_summary_lines(params));
        lines
    }

    /// Default Klipper start script: delegates to the `START_PRINT` macro.
    ///
    /// Print temperatures are forwarded using both common parameter naming
    /// conventions:
    /// - Orca/SuperSlicer style: `BED_TEMP` / `EXTRUDER_TEMP`
    /// - Klippain style: `BED` / `EXTRUDER`
    ///
    /// Emitting both keeps the default robust across macro packs while still
    /// preserving user-configurable custom start G-code overrides.
    fn start_script(&self, params: &SlicingParams) -> Vec<String> {
        vec![format!(
            "START_PRINT BED_TEMP={:.0} EXTRUDER_TEMP={:.0} BED={:.0} EXTRUDER={:.0}",
            params.bed_temp, params.nozzle_temp, params.bed_temp, params.nozzle_temp
        )]
    }

    /// Default Klipper end script: delegates to the `END_PRINT` macro.
    fn end_script(&self) -> Vec<String> {
        vec!["END_PRINT".to_string()]
    }

    /// Klipper uses `SET_PRESSURE_ADVANCE ADVANCE=…` rather than Marlin's
    /// `M900 K…`.  Pressure advance compensates for filament compression in the
    /// hotend, improving corner quality at high speeds.
    fn set_pressure_advance(&self, value: f64) -> String {
        format!("SET_PRESSURE_ADVANCE ADVANCE={:.4}", value)
    }

    /// Klipper configures acceleration at runtime via `SET_VELOCITY_LIMIT`
    /// rather than Marlin's `M204`.  Only the `ACCEL` field is set here; the
    /// velocity cap is left to the printer's configured default.
    fn set_acceleration(&self, accel: f64) -> String {
        format!("SET_VELOCITY_LIMIT ACCEL={:.0}", accel)
    }

    /// Klipper expresses both kinematic limits with a single `SET_VELOCITY_LIMIT`
    /// carrying the native `VELOCITY` and `SQUARE_CORNER_VELOCITY` fields — no
    /// junction-deviation conversion needed (unlike Marlin's `M205 J`). `accel`
    /// is unused here because Klipper's acceleration is set separately per role.
    fn set_kinematic_limits(
        &self,
        square_corner_velocity_mm_s: f64,
        max_velocity_mm_s: f64,
        _accel_mm_s2: f64,
    ) -> Vec<String> {
        let mut parts = String::new();
        if max_velocity_mm_s > 0.0 {
            parts.push_str(&format!(" VELOCITY={:.0}", max_velocity_mm_s));
        }
        if square_corner_velocity_mm_s > 0.0 {
            parts.push_str(&format!(
                " SQUARE_CORNER_VELOCITY={:.2}",
                square_corner_velocity_mm_s
            ));
        }
        if parts.is_empty() {
            Vec::new()
        } else {
            vec![format!("SET_VELOCITY_LIMIT{parts}")]
        }
    }

    /// Named Klipper fans use `SET_FAN_SPEED fan=<name> speed=<0.0–1.0>`; the
    /// default part-cooling fan uses `M106`/`M107` instead.
    ///
    /// Klipper's primary `[fan]` object is *only* controllable via `M106`/`M107`
    /// — `SET_FAN_SPEED` targets `[fan_generic]` objects and errors on `[fan]`
    /// (`The value 'fan' is not valid for FAN`). So the part-cooling fan
    /// (index 0 with no explicit name) falls back to `M106`/`M107`; only fans
    /// with an explicit `name_hint`, or non-part-cooling indices, use
    /// `SET_FAN_SPEED`.
    fn set_fan_speed_indexed(&self, fan_index: u8, name_hint: Option<&str>, speed: f64) -> String {
        let s = speed.clamp(0.0, 1.0);
        match name_hint {
            Some(name) => format!("SET_FAN_SPEED fan={} speed={:.4}", name, s),
            None if fan_index == 0 => {
                let v = (s * 255.0).round() as u8;
                if v == 0 {
                    "M107".to_string()
                } else {
                    format!("M106 S{}", v)
                }
            }
            None => format!(
                "SET_FAN_SPEED fan={} speed={:.4}",
                Self::fan_name_for_index(fan_index),
                s
            ),
        }
    }

    /// Klipper configures firmware retraction at runtime with `SET_RETRACTION`
    /// (requires a `[firmware_retraction]` section in the printer config), so
    /// `G10`/`G11` use the slicer's length / speed / restart-extra. The Z-hop
    /// component is left to the slicer's explicit Z moves.
    fn firmware_retract_setup(
        &self,
        retract_mm: f64,
        retract_speed_mm_min: f64,
        restart_extra_mm: f64,
    ) -> Vec<String> {
        // Klipper SET_RETRACTION speeds are in mm/s.
        let speed_mm_s = retract_speed_mm_min / 60.0;
        vec![format!(
            "SET_RETRACTION RETRACT_LENGTH={:.3} RETRACT_SPEED={:.1} \
             UNRETRACT_EXTRA_LENGTH={:.3} UNRETRACT_SPEED={:.1} ; firmware retraction",
            retract_mm, speed_mm_s, restart_extra_mm, speed_mm_s
        )]
    }
}
