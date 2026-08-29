//! [`GcodeDialect`] trait — abstraction over firmware-specific command syntax.

use crate::gcode::stats::{self, SliceStatistics};
use crate::settings::params::SlicingParams;

/// Boxed warning callback type used by [`crate::gcode::GcodeGenerator`].
///
/// Receives a human-readable message whenever the active dialect signals that
/// a command is not natively supported.
pub type WarnFn = Box<dyn Fn(&str)>;

/// Abstraction over firmware-specific G-code command emission.
///
/// Implement this trait to add support for a new printer firmware flavor.
/// The three **required** methods — [`GcodeDialect::flavor_name`],
/// [`GcodeDialect::start_script`], and [`GcodeDialect::end_script`] — are the
/// minimum needed to produce valid output, because every other method has a
/// default implementation based on standard G-code syntax.
///
/// However, firmware-specific features (e.g. `SET_VELOCITY_LIMIT` for Klipper,
/// or custom fan-speed curves for specialty firmware) should be added either by
/// overriding the relevant default methods or by exposing extra methods directly
/// on the concrete struct.
///
/// Use [`GcodeDialect::unsupported_commands`] to advertise commands that this
/// dialect cannot handle natively; the [`crate::gcode::GcodeGenerator`] will
/// emit a warning via its registered warn function before falling back to the
/// standard implementation.
///
/// All dimensional values use millimetres; speeds use **mm/min** (the native
/// unit for G-code `F` parameters).
pub trait GcodeDialect: Send + Sync {
    /// Human-readable name of this dialect (used in header comments).
    fn flavor_name(&self) -> &'static str;

    /// Emit the complete start sequence for the given slicing parameters.
    ///
    /// Typically includes unit mode, positioning mode, temperature targets,
    /// homing, and any firmware-specific preamble.
    fn start_script(&self, params: &SlicingParams) -> Vec<String>;

    /// Emit the complete end sequence.
    ///
    /// Typically includes cooling, final retract, nozzle park, and motor-off.
    fn end_script(&self) -> Vec<String>;

    /// Emit the metadata header block for a finished slice.
    ///
    /// Rendered once at the very top of the program (before the start script),
    /// this carries the slicer version + timestamp, model name, layer count,
    /// height, filament usage (mm / cm³ / g), estimated print time, and model
    /// bounding box, followed by a human-readable print-settings summary
    /// (issue #15).
    ///
    /// The default implementation produces a portable, flavor-tagged block.
    /// Firmware dialects override this to match their ecosystem's expected
    /// header format (e.g. Marlin/OrcaSlicer's `HEADER_BLOCK_START` … `END`).
    fn header(&self, params: &SlicingParams, stats: &SliceStatistics) -> Vec<String> {
        let mut lines = vec!["; ---- slicer-engine metadata ----".to_string()];
        lines.extend(stats::metadata_lines(self.flavor_name(), stats));
        lines.extend(stats::settings_summary_lines(params));
        lines
    }

    /// Emit the metadata **footer** block for a finished slice.
    ///
    /// Rendered once at the very end of the program (after the end script), this
    /// carries a PrusaSlicer/OrcaSlicer-compatible `; key = value` configuration
    /// block plus the print statistics (filament usage, estimated time, layer
    /// count). Printer front-ends — Moonraker (Mainsail / Fluidd) and OctoPrint —
    /// scan the file *footer* for this exact format to populate filament type,
    /// colour, layer height, object height, and filament usage; the
    /// human-readable [`GcodeDialect::header`] block is not parsed by them.
    ///
    /// The default implementation is flavor-agnostic (the same block is parsed
    /// identically regardless of firmware), so dialects rarely need to override
    /// it.
    fn footer(&self, params: &SlicingParams, stats: &SliceStatistics) -> Vec<String> {
        stats::config_block_lines(params, stats)
    }

    /// List of command identifiers not natively supported by this dialect.
    ///
    /// When [`crate::gcode::GcodeGenerator`] encounters a command in this list
    /// it emits a warning via the registered warn function before falling back
    /// to the default standard G-code implementation.
    ///
    /// Command names should correspond to the method names on this trait
    /// (e.g. `"set_fan_speed"`, `"set_nozzle_temp"`).  Returns an empty slice
    /// by default — i.e. all commands are assumed supported.
    fn unsupported_commands(&self) -> &'static [&'static str] {
        &[]
    }

    /// Format a standalone comment line.
    fn comment(&self, text: &str) -> String {
        format!("; {}", text)
    }

    /// Set extruder (nozzle) temperature.
    ///
    /// When `wait` is `true` the firmware blocks until target is reached
    /// (`M109`); otherwise it sets the target and returns immediately (`M104`).
    fn set_nozzle_temp(&self, temp: f64, wait: bool) -> String {
        if wait {
            format!("M109 S{:.0}", temp)
        } else {
            format!("M104 S{:.0}", temp)
        }
    }

    /// Set heated-bed temperature.
    ///
    /// When `wait` is `true` the firmware blocks until target is reached
    /// (`M190`); otherwise it sets the target and returns immediately (`M140`).
    fn set_bed_temp(&self, temp: f64, wait: bool) -> String {
        if wait {
            format!("M190 S{:.0}", temp)
        } else {
            format!("M140 S{:.0}", temp)
        }
    }

    /// Move to `(x, y)` while extruding filament to absolute E position `e`
    /// at `speed_mm_min` mm/min.
    fn move_extrude(&self, x: f64, y: f64, e: f64, speed_mm_min: f64) -> String {
        format!("G1 X{:.3} Y{:.3} E{:.5} F{:.0}", x, y, e, speed_mm_min)
    }

    /// Move to `(x, y, z)` while extruding filament to absolute E position `e`
    /// at `speed_mm_min` mm/min.
    ///
    /// Used by spiral (vase) mode, where the Z height ramps continuously along
    /// the perimeter instead of stepping once per layer.
    fn move_extrude_z(&self, x: f64, y: f64, z: f64, e: f64, speed_mm_min: f64) -> String {
        format!(
            "G1 X{:.3} Y{:.3} Z{:.3} E{:.5} F{:.0}",
            x, y, z, e, speed_mm_min
        )
    }

    /// Move the Z axis to `z` at `speed_mm_min` mm/min (no extrusion).
    fn move_z(&self, z: f64, speed_mm_min: f64) -> String {
        format!("G1 Z{:.3} F{:.0}", z, speed_mm_min)
    }

    /// Travel (non-extrusion) move in XY at `speed_mm_min` mm/min.
    fn travel_xy(&self, x: f64, y: f64, speed_mm_min: f64) -> String {
        format!("G1 X{:.3} Y{:.3} F{:.0}", x, y, speed_mm_min)
    }

    /// Set part-cooling fan speed.
    ///
    /// `speed` is a normalised fraction `0.0` (off) to `1.0` (full).
    /// Emits `M107` when speed rounds to zero, `M106 S<value>` otherwise.
    fn set_fan_speed(&self, speed: f64) -> String {
        let s = (speed.clamp(0.0, 1.0) * 255.0).round() as u8;
        if s == 0 {
            "M107".to_string()
        } else {
            format!("M106 S{}", s)
        }
    }

    /// Set fan speed for an indexed fan (`M106 P<n> S<value>`).
    ///
    /// `fan_index` selects the physical fan (0 = part-cooling, 1 = hotend,
    /// 2 = chamber, 3 = auxiliary).  `speed` is a normalised fraction
    /// `0.0`–`1.0`.
    ///
    /// `name_hint` is an optional dialect-specific fan name override.  The
    /// Klipper dialect uses it to emit `SET_FAN_SPEED fan=<name_hint> …`
    /// instead of the default name derived from `fan_index`; Marlin ignores it.
    ///
    /// The default implementation emits Marlin-style `M106 P<n> S<value>`.
    /// Fan index 0 follows the `M107`/`M106 S<value>` convention (no explicit
    /// `P0`) to maximise firmware compatibility.  Other indices always include
    /// the explicit `P<n>`.
    fn set_fan_speed_indexed(&self, fan_index: u8, name_hint: Option<&str>, speed: f64) -> String {
        let _ = name_hint; // Marlin does not use fan names
        let s = (speed.clamp(0.0, 1.0) * 255.0).round() as u8;
        if fan_index == 0 {
            // P0 uses the conventional M107 / M106 S<val> form
            if s == 0 {
                "M107".to_string()
            } else {
                format!("M106 S{}", s)
            }
        } else if s == 0 {
            format!("M106 P{} S0", fan_index)
        } else {
            format!("M106 P{} S{}", fan_index, s)
        }
    }

    /// Set linear / pressure advance (the *K* factor).
    ///
    /// Pressure advance compensates for the pressure lag in the hotend, keeping
    /// corners crisp and preventing bulging / gaps at speed changes.  The
    /// default implementation targets Marlin's `M900 K<value>` linear-advance
    /// command; Klipper overrides this with `SET_PRESSURE_ADVANCE ADVANCE=…`.
    ///
    /// `value` is the firmware K factor (typically `0.02`–`0.08`).
    fn set_pressure_advance(&self, value: f64) -> String {
        format!("M900 K{:.4}", value)
    }

    /// Set the printing acceleration in mm/s².
    ///
    /// Emitted whenever the slicer's target acceleration changes (per layer /
    /// per role) so motion can be tuned for adhesion, surface finish, or speed.
    /// The default implementation targets Marlin's `M204 P<accel>` (printing
    /// acceleration); Klipper overrides this with `SET_VELOCITY_LIMIT ACCEL=…`.
    fn set_acceleration(&self, accel: f64) -> String {
        format!("M204 P{:.0}", accel)
    }

    /// Home all axes (`G28`).
    fn home_axes(&self) -> String {
        "G28".to_string()
    }

    /// Set the extruder to an absolute `e` position at `speed_mm_min` mm/min.
    ///
    /// Used for both retraction (negative delta) and priming (positive delta).
    fn set_extruder_pos(&self, e: f64, speed_mm_min: f64) -> String {
        format!("G1 E{:.5} F{:.0}", e, speed_mm_min)
    }

    /// Reset the extruder position counter to zero (`G92 E0`).
    fn reset_extruder(&self) -> String {
        "G92 E0".to_string()
    }

    /// Force **absolute** extrusion mode (`M82`).
    ///
    /// The generator emits absolute E positions (an accumulating `e_total`
    /// reset with `G92 E0` per layer), so absolute extrusion mode is a hard
    /// invariant.  Custom start scripts and Klipper `START_PRINT` macros that
    /// prime or use firmware retraction frequently leave the extruder in
    /// **relative** mode (`M83`); the generator emits this after the start
    /// script to guarantee the invariant regardless of what the macro left set.
    fn extruder_absolute_mode(&self) -> String {
        "M82 ; extruder absolute mode".to_string()
    }
}
