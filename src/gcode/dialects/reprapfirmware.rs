//! RepRapFirmware G-code dialect.

use crate::gcode::stats::{self, SliceStatistics};
use crate::gcode::GcodeDialect;
use crate::settings::params::{BedMeshMode, SlicingParams};

/// RepRapFirmware G-code dialect — Duet and compatible boards, RRF 3.
///
/// RepRapFirmware reads most of Marlin's command set, which is what makes it
/// easy to get wrong: much of the Marlin output a generic dialect would write
/// is accepted and then does something else. Every override here exists for
/// that reason, not for style:
///
/// | Concern            | Marlin form   | What RRF does with it                  | Emitted here         |
/// | ------------------ | ------------- | -------------------------------------- | -------------------- |
/// | Pause              | `M0`          | **ends the job** and runs `stop.g`     | `M226`               |
/// | Max feedrate       | `M203` mm/s   | reads mm/min — a 60× slower cap        | `M203` in mm/min     |
/// | Cornering          | `M205 J`      | no junction deviation                  | `M205 X Y` jerk      |
/// | Recover settings   | `M208`        | sets the **axis travel limits**        | `M207 … R T`         |
/// | Pressure advance   | `M900 K`      | unknown command                        | `M572 D0 S`          |
/// | Mesh               | `M420 S1`     | unknown command                        | `G29 S1` / `G29 S0`  |
/// | Fan                | `M106 S0–255` | `S1` means **full** speed (0–1 scale)  | `M106 S0.00–1.00`    |
/// | Tool               | implicit      | no selected tool ⇒ no extrusion        | `T0`                 |
///
/// Everything in the default output exists in RRF 3.1 and later (`M486` object
/// labels being the newest).
pub struct RepRapFirmwareDialect;

/// Narrowest edge an adaptive probe grid is given. A print whose footprint is
/// a single line would otherwise define a zero-width grid, which `M557`
/// rejects.
const MIN_GRID_SPAN_MM: f64 = 10.0;

/// Spacing aimed for between adaptive probe points, before the per-axis count
/// is clamped to [`GRID_POINTS`].
const GRID_SPACING_MM: f64 = 40.0;

/// Probe points per axis on an adaptive grid: at least three, so the mesh can
/// bend as well as tilt; at most nine, so a full-plate print probes for a minute
/// or two rather than ten.
const GRID_POINTS: (u32, u32) = (3, 9);

impl RepRapFirmwareDialect {
    /// The `M557` grid that bounds probing to the print's footprint.
    ///
    /// `M557` takes bed coordinates for the points to probe, and RRF skips any
    /// point its probe cannot reach, so the footprint is passed as-is rather
    /// than shifted by the probe offset.
    fn adaptive_grid((min_x, min_y, max_x, max_y): (f64, f64, f64, f64)) -> String {
        let (x0, x1) = widen(min_x, max_x);
        let (y0, y1) = widen(min_y, max_y);
        format!(
            "M557 X{:.1}:{:.1} Y{:.1}:{:.1} P{}:{} ; probe grid over the print footprint",
            x0,
            x1,
            y0,
            y1,
            grid_points(x1 - x0),
            grid_points(y1 - y0)
        )
    }
}

/// Grow a span symmetrically to at least [`MIN_GRID_SPAN_MM`].
fn widen(min: f64, max: f64) -> (f64, f64) {
    let pad = ((MIN_GRID_SPAN_MM - (max - min)) / 2.0).max(0.0);
    (min - pad, max + pad)
}

fn grid_points(span_mm: f64) -> u32 {
    let points = (span_mm / GRID_SPACING_MM).ceil() as u32 + 1;
    points.clamp(GRID_POINTS.0, GRID_POINTS.1)
}

/// The height-map file a named mesh lives in. RRF names saved maps by file, so
/// a bare name gets the `.csv` its maps are written with.
fn height_map_file(name: &str) -> String {
    // A quote would end the G-code string early; no real file name has one.
    let name = name.trim().replace('"', "");
    if name.contains('.') {
        name
    } else {
        format!("{name}.csv")
    }
}

/// `M106` speed on RRF's own 0–1 scale.
fn fan_speed_value(speed: f64) -> Option<String> {
    let s = speed.clamp(0.0, 1.0);
    if s < 0.005 {
        None
    } else {
        Some(format!("{:.2}", s))
    }
}

impl GcodeDialect for RepRapFirmwareDialect {
    fn flavor_name(&self) -> &'static str {
        "RepRapFirmware"
    }

    /// The metadata block is delimited by `; HEADER_BLOCK_START` /
    /// `; HEADER_BLOCK_END`, the convention downstream tools (print farms,
    /// analytics) parse.
    fn header(&self, params: &SlicingParams, stats: &SliceStatistics) -> Vec<String> {
        let mut lines = vec!["; HEADER_BLOCK_START".to_string()];
        lines.extend(stats::metadata_lines(self.flavor_name(), stats));
        lines.push("; HEADER_BLOCK_END".to_string());
        lines.extend(stats::settings_summary_lines(params));
        lines
    }

    /// RRF runs `sys/start.g` itself before the first line of the file, so this
    /// only heats, homes and waits.
    ///
    /// `T0` comes first because RepRapFirmware refuses to extrude with no tool
    /// selected; `M116` then waits for the bed and the tool together, RRF's own
    /// idiom for what Marlin spells as `M190` + `M109`.
    fn start_script(&self, params: &SlicingParams) -> Vec<String> {
        vec![
            "G21 ; millimetres".to_string(),
            "G90 ; absolute positioning".to_string(),
            "M82 ; extruder absolute mode".to_string(),
            format!("M140 S{:.0} ; set bed temperature", params.bed_temp),
            "T0 ; select the tool, without which RepRapFirmware will not extrude".to_string(),
            format!("M104 S{:.0} ; set tool temperature", params.nozzle_temp),
            "G28 ; home all axes".to_string(),
            "M116 ; wait for the bed and tool temperatures".to_string(),
            "G92 E0 ; reset extruder".to_string(),
        ]
    }

    /// RRF 3.5 and later run `sys/stop.g` after the last line on their own; this
    /// leaves the machine safe on earlier versions too. `M18` rather than `M84`,
    /// which RRF 3.6 deprecates.
    fn end_script(&self) -> Vec<String> {
        vec![
            "; end of print".to_string(),
            "G91 ; relative positioning".to_string(),
            "G1 E-2 F3000 ; final retract".to_string(),
            "G1 Z5 F3000 ; lift nozzle".to_string(),
            "G90 ; absolute positioning".to_string(),
            "G28 X Y ; park".to_string(),
            "M104 S0 ; tool heater off".to_string(),
            "M140 S0 ; bed off".to_string(),
            "M18 ; disable motors".to_string(),
        ]
    }

    fn set_fan_speed(&self, speed: f64) -> String {
        self.set_fan_speed_indexed(0, None, speed)
    }

    /// RRF reads `M106 S` as a 0–1 fraction whenever the value is 1 or less, so
    /// Marlin's integer `S1` (≈ 0.4 %) would run the fan flat out. Speeds are
    /// written as fractions instead, and off is `S0` because `M107` is
    /// deprecated in RRF.
    fn set_fan_speed_indexed(&self, fan_index: u8, name_hint: Option<&str>, speed: f64) -> String {
        let _ = name_hint; // RRF addresses fans by number only
        let value = fan_speed_value(speed).unwrap_or_else(|| "0".to_string());
        if fan_index == 0 {
            format!("M106 S{value}")
        } else {
            format!("M106 P{fan_index} S{value}")
        }
    }

    /// `M572 D<extruder> S<seconds>` — RRF's pressure advance, in seconds of
    /// advance like Klipper's, for extruder drive 0.
    fn set_pressure_advance(&self, value: f64) -> String {
        format!("M572 D0 S{:.4}", value)
    }

    /// `M203` is in **mm/min** on RRF (a bare Marlin-style mm/s value would cap
    /// the machine at a sixtieth of the intended speed), and cornering is a
    /// jerk limit rather than junction deviation.
    ///
    /// Jerk is the per-axis speed change allowed at a corner, so a right-angle
    /// corner taken at the square-corner velocity `v` changes each axis by `v`:
    /// the square-corner velocity *is* the jerk for X and Y. It is emitted with
    /// `M205`, in mm/s, rather than `M566`: from RRF 3.6 `M205` sets limits for
    /// the current job only, never above the machine limits `config.g` sets
    /// with `M566`, which is exactly the scope a slicer should have.
    fn set_kinematic_limits(
        &self,
        square_corner_velocity_mm_s: f64,
        max_velocity_mm_s: f64,
        _accel_mm_s2: f64,
    ) -> Vec<String> {
        let mut lines = Vec::new();
        if max_velocity_mm_s > 0.0 {
            lines.push(format!(
                "M203 X{0:.0} Y{0:.0} ; max feedrate (mm/min)",
                max_velocity_mm_s * 60.0
            ));
        }
        if square_corner_velocity_mm_s > 0.0 {
            lines.push(format!(
                "M205 X{0:.2} Y{0:.2} ; jerk (mm/s)",
                square_corner_velocity_mm_s
            ));
        }
        lines
    }

    /// `M207` carries everything on RRF — length, extra un-retract (`R`),
    /// retract and un-retract speeds (`F`/`T`) and Z-hop — in one command.
    /// Marlin's `M208` must not be sent: on RRF it sets the axis travel limits.
    fn firmware_retract_setup(
        &self,
        retract_mm: f64,
        retract_speed_mm_min: f64,
        restart_extra_mm: f64,
    ) -> Vec<String> {
        vec![format!(
            "M207 S{:.3} R{:.3} F{:.0} T{:.0} Z0 ; firmware retraction",
            retract_mm, restart_extra_mm, retract_speed_mm_min, retract_speed_mm_min
        )]
    }

    /// RRF has no `M420`; its mesh is a height-map file.
    ///
    /// - **Load:** `G29 S1` loads `sys/heightmap.csv`, or the named map with
    ///   `P"<name>.csv"`, and enables compensation.
    /// - **Calibrate:** `G29 S0` probes the `M557` grid, saves the map and
    ///   enables it. `S0` rather than a bare `G29`, which would run the user's
    ///   `mesh.g` instead when one exists.
    ///
    /// An adaptive `area` redefines the grid with `M557` first. RRF keeps that
    /// grid until the next `M557` or a restart, so a later non-adaptive
    /// calibration probes the last footprint rather than the grid `config.g`
    /// set — the price of the only way RRF offers to probe part of the bed.
    fn bed_mesh_lines(
        &self,
        mode: BedMeshMode,
        profile_name: Option<&str>,
        area: Option<(f64, f64, f64, f64)>,
    ) -> Vec<String> {
        match mode {
            BedMeshMode::Off => Vec::new(),
            BedMeshMode::LoadProfile => {
                vec![match profile_name.filter(|name| !name.trim().is_empty()) {
                    Some(name) => format!(
                        "G29 S1 P\"{}\" ; load saved height map",
                        height_map_file(name)
                    ),
                    None => "G29 S1 ; load saved height map".to_string(),
                }]
            }
            BedMeshMode::Calibrate => {
                let mut lines: Vec<String> = area.map(Self::adaptive_grid).into_iter().collect();
                lines.push("G29 S0 ; probe the bed and enable mesh compensation".to_string());
                lines
            }
        }
    }

    /// `M226` pauses after every earlier command has run and calls `pause.g`.
    /// The Marlin default, `M0`, would **end the print** on RRF.
    fn pause_gcode(&self) -> Vec<String> {
        vec!["M226 ; pause".to_string()]
    }

    /// `M600` behaves like `M226` but runs `filament-change.g` when it exists,
    /// falling back to `pause.g` — so it is never worse than a plain pause.
    fn color_change_gcode(&self) -> Vec<String> {
        vec!["M600 ; color change".to_string()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: RepRapFirmwareDialect = RepRapFirmwareDialect;

    #[test]
    fn pause_never_ends_the_job() {
        assert_eq!(D.pause_gcode(), vec!["M226 ; pause"]);
        assert_eq!(D.color_change_gcode(), vec!["M600 ; color change"]);
    }

    #[test]
    fn max_feedrate_is_written_in_mm_per_minute() {
        let lines = D.set_kinematic_limits(0.0, 300.0, 3000.0);
        assert_eq!(lines, vec!["M203 X18000 Y18000 ; max feedrate (mm/min)"]);
    }

    #[test]
    fn square_corner_velocity_becomes_jerk_not_junction_deviation() {
        let lines = D.set_kinematic_limits(8.0, 0.0, 3000.0);
        assert_eq!(lines, vec!["M205 X8.00 Y8.00 ; jerk (mm/s)"]);
        assert!(D.set_kinematic_limits(0.0, 0.0, 3000.0).is_empty());
    }

    #[test]
    fn firmware_retraction_is_one_m207_and_never_m208() {
        let lines = D.firmware_retract_setup(0.8, 2400.0, 0.1);
        assert_eq!(
            lines,
            vec!["M207 S0.800 R0.100 F2400 T2400 Z0 ; firmware retraction"]
        );
        assert!(lines.iter().all(|l| !l.contains("M208")));
    }

    #[test]
    fn pressure_advance_is_m572() {
        assert_eq!(D.set_pressure_advance(0.045), "M572 D0 S0.0450");
    }

    #[test]
    fn fans_are_fractions_so_a_whisper_is_not_full_speed() {
        assert_eq!(D.set_fan_speed_indexed(0, None, 0.004), "M106 S0");
        assert_eq!(D.set_fan_speed_indexed(0, None, 1.0 / 255.0), "M106 S0");
        assert_eq!(D.set_fan_speed_indexed(0, None, 0.5), "M106 S0.50");
        assert_eq!(D.set_fan_speed_indexed(0, None, 1.0), "M106 S1.00");
        assert_eq!(
            D.set_fan_speed_indexed(2, Some("chamber"), 0.3),
            "M106 P2 S0.30"
        );
        assert_eq!(D.set_fan_speed_indexed(3, None, 0.0), "M106 P3 S0");
        assert_eq!(D.set_fan_speed(0.0), "M106 S0");
    }

    #[test]
    fn mesh_load_uses_height_map_files_not_m420() {
        assert_eq!(
            D.bed_mesh_lines(BedMeshMode::LoadProfile, None, None),
            vec!["G29 S1 ; load saved height map"]
        );
        assert_eq!(
            D.bed_mesh_lines(BedMeshMode::LoadProfile, Some("pei"), None),
            vec!["G29 S1 P\"pei.csv\" ; load saved height map"]
        );
        assert_eq!(
            D.bed_mesh_lines(BedMeshMode::LoadProfile, Some("textured.csv"), None),
            vec!["G29 S1 P\"textured.csv\" ; load saved height map"]
        );
        assert_eq!(
            D.bed_mesh_lines(BedMeshMode::LoadProfile, Some("  "), None),
            vec!["G29 S1 ; load saved height map"]
        );
    }

    #[test]
    fn calibrate_probes_with_s0_and_bounds_the_grid_when_adaptive() {
        assert_eq!(
            D.bed_mesh_lines(BedMeshMode::Calibrate, None, None),
            vec!["G29 S0 ; probe the bed and enable mesh compensation"]
        );
        let lines = D.bed_mesh_lines(
            BedMeshMode::Calibrate,
            None,
            Some((50.0, 60.0, 150.0, 80.0)),
        );
        assert_eq!(
            lines,
            vec![
                "M557 X50.0:150.0 Y60.0:80.0 P4:3 ; probe grid over the print footprint",
                "G29 S0 ; probe the bed and enable mesh compensation",
            ]
        );
        assert!(D.bed_mesh_lines(BedMeshMode::Off, None, None).is_empty());
    }

    #[test]
    fn a_line_shaped_footprint_still_gets_a_grid_m557_accepts() {
        let grid = RepRapFirmwareDialect::adaptive_grid((100.0, 20.0, 100.0, 220.0));
        assert!(
            grid.starts_with("M557 X95.0:105.0 Y20.0:220.0 P3:6"),
            "{grid}"
        );
    }

    #[test]
    fn start_script_selects_the_tool_and_waits_with_m116() {
        let script = D.start_script(&SlicingParams::default());
        let t0 = script.iter().position(|l| l.starts_with("T0")).unwrap();
        let heat = script.iter().position(|l| l.starts_with("M104")).unwrap();
        assert!(t0 < heat, "tool selected before its heater is set");
        assert!(script.iter().any(|l| l.starts_with("M116")));
        assert!(!script.iter().any(|l| l.starts_with("M109")));
    }

    #[test]
    fn end_script_disables_motors_with_m18() {
        let script = D.end_script();
        assert!(script.iter().any(|l| l.starts_with("M18")));
        assert!(!script
            .iter()
            .any(|l| l.starts_with("M84") || l.starts_with("M0")));
    }
}
