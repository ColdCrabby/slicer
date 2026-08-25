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
/// - [`KlipperDialect::set_pressure_advance`] — pressure advance tuning
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

    /// Emit a `SET_PRESSURE_ADVANCE` command.
    ///
    /// Pressure advance compensates for filament compression in the hotend,
    /// improving corner quality at high speeds.
    pub fn set_pressure_advance(&self, value: f64) -> String {
        format!("SET_PRESSURE_ADVANCE ADVANCE={:.4}", value)
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
    /// Print temperatures are forwarded as macro arguments so the user's
    /// Klipper `START_PRINT` macro can use them for pre-heat, bed levelling,
    /// purge routines, etc.  This follows the OrcaSlicer / SuperSlicer
    /// convention for Klipper start G-code.
    fn start_script(&self, params: &SlicingParams) -> Vec<String> {
        vec![format!(
            "START_PRINT BED_TEMP={:.0} EXTRUDER_TEMP={:.0}",
            params.bed_temp, params.nozzle_temp
        )]
    }

    /// Default Klipper end script: delegates to the `END_PRINT` macro.
    fn end_script(&self) -> Vec<String> {
        vec!["END_PRINT".to_string()]
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
}
