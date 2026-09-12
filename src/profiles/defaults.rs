//! Bare-minimum blank-slate defaults — the *only* profile data the engine
//! ships.
//!
//! The engine deliberately carries **no product/vendor profiles**. What it does
//! ship is a small set of generic `builtin` presets — enough that an offline
//! install can create, edit and slice without inventing anything; the vendor
//! catalog is a cloud concern and lives entirely outside this crate.
//!
//! "Generic" is the rule that decides what belongs here. A preset named after a
//! machine, or carrying one machine's calibration, is catalog data. A preset
//! named after a *class* of machine — a 220 mm bedslinger, a 350 mm CoreXY — is
//! a starting point any owner of that class can recognise and correct, and that
//! is what these are.

use serde_json::json;

use super::filament::{material_density, FilamentMaterial, FilamentProfile};
use super::meta::ProfileMeta;
use super::printer::{BedShape, PrinterConnection, PrinterProfile};
use super::process::{PrintQuality, ProcessProfile};

const DEFAULT_START_GCODE: &str = "; Cold Crabby standard Marlin start\nG21 ; millimetres\nG90 ; absolute positioning\nM82 ; extruder absolute mode\nM140 S{bed_temp_first_layer} ; set bed temperature\nM104 S{nozzle_temp_first_layer} ; set nozzle temperature\nG28 ; home all axes\nM190 S{bed_temp_first_layer} ; wait for bed temperature\nM109 S{nozzle_temp_first_layer} ; wait for nozzle temperature\nG92 E0 ; reset extruder\nG1 Z2.0 F3000 ; lift nozzle";
const DEFAULT_END_GCODE: &str = "; Cold Crabby standard Marlin end\nG91 ; relative positioning\nG1 E-2 F2700 ; retract\nG1 Z10 F3000 ; lift\nG90 ; absolute positioning\nM104 S0 ; nozzle off\nM140 S0 ; bed off\nM84 ; disable steppers";

/// A blank-slate printer with sensible defaults, tagged with `meta`.
pub fn base_printer(meta: ProfileMeta) -> PrinterProfile {
    PrinterProfile {
        meta,
        vendor: "Custom".to_string(),
        model: "Generic".to_string(),
        bed_shape: BedShape::Rectangular,
        bed_width: 220.0,
        bed_depth: 220.0,
        bed_height: 250.0,
        origin_at_center: false,
        preferred_orientation_deg: 0.0,
        connection: PrinterConnection::default(),
        params: json!({
            "nozzle_diameter_mm": 0.4,
            "filament_diameter_mm": 1.75,
            "extruder_count": 1,
            "print_speed": 150.0,
            "travel_speed_mm_min": 15000.0,
            "retract_mm": 0.8,
            "retract_speed_mm_min": 2400.0,
            "z_hop_mm": 0.2,
            "gcode_flavor": "marlin",
            "start_gcode": DEFAULT_START_GCODE,
            "end_gcode": DEFAULT_END_GCODE,
        }),
    }
}

/// A blank-slate filament for `material`, tagged with `meta`.
pub fn base_filament(meta: ProfileMeta, material: FilamentMaterial) -> FilamentProfile {
    FilamentProfile {
        meta,
        vendor: "Custom".to_string(),
        material,
        color: "#e0730f".to_string(),
        density_g_cm3: material_density(material),
        cost_per_kg: 25.0,
        params: material.default_params(),
    }
}

/// A blank-slate process profile, tagged with `meta`.
///
/// Everything here is the engine's own default, spelled out so the preset reads
/// as a complete recipe, except what the preset genuinely decides: the thicker
/// first layer and wider bead that go with a 0.20 mm layer, and the skirt. The
/// rest is pinned to agree by
/// `the_shipped_process_profile_agrees_with_the_engine_defaults` — a profile
/// that quietly disagrees means a slice from the app and one from the command
/// line print differently.
pub fn base_process(meta: ProfileMeta) -> ProcessProfile {
    ProcessProfile {
        meta,
        quality: PrintQuality::Standard,
        params: json!({
            "layer_height": 0.2,
            "first_layer_height": 0.24,
            "line_width": 0.44,
            "wall_generator": "arachne",
            "wall_count": 3,
            "top_layers": 4,
            "bottom_layers": 3,
            "seam_position": "aligned",
            "infill_density": 0.15,
            "infill_pattern": "TpmsD",
            "infill_base_angle": 45.0,
            "print_speed": 120.0,
            "perimeter_speed": 80.0,
            "infill_speed": 150.0,
            "top_surface_speed": 60.0,
            "first_layer_speed": 30.0,
            "support_threshold_angle": 45.0,
            "adhesion_type": "skirt",
            "skirt_loops": 1,
        }),
    }
}

const KLIPPER_START_GCODE: &str = "; Cold Crabby Klipper start\nPRINT_START BED={bed_temp_first_layer} EXTRUDER={nozzle_temp_first_layer}";
const KLIPPER_END_GCODE: &str = "; Cold Crabby Klipper end\nPRINT_END";

/// The offline default printer.
pub fn default_printer() -> PrinterProfile {
    let mut p = base_printer(ProfileMeta::builtin(
        "builtin-generic-printer",
        "Generic 220 mm printer",
    ));
    p.vendor = "Generic".to_string();
    p.model = "FDM 220".to_string();
    p
}

/// A generic high-performance CoreXY, of the kind a 0.6 nozzle and firmware
/// retraction are ordinary on.
///
/// Not a model, a *class*: a 350 mm cube running Klipper, with the machine
/// limits such a printer is normally commissioned with. It exists because the
/// fast process presets are unusable behind a profile that travels at 150 mm/s
/// and retracts 0.8 mm on a bowden-length setting — the process can ask for
/// speed the printer profile then refuses to carry.
///
/// **Pressure advance stays off here, deliberately.** On these machines it is
/// tuned on the printer and lives in the firmware; a profile that shipped a
/// number would overwrite a calibration it knows nothing about. The same goes
/// for retraction, which is why firmware retraction is on: the printer's own
/// values win, and the lengths below are only what a slicer-side fallback would
/// use.
pub fn corexy_printer() -> PrinterProfile {
    let mut p = base_printer(ProfileMeta::builtin(
        "builtin-corexy-350",
        "Generic CoreXY 350 mm",
    ));
    p.vendor = "Generic".to_string();
    p.model = "CoreXY 350".to_string();
    p.bed_width = 350.0;
    p.bed_depth = 350.0;
    p.bed_height = 370.0;
    p.params = json!({
        "nozzle_diameter_mm": 0.6,
        "filament_diameter_mm": 1.75,
        "extruder_count": 1,
        "gcode_flavor": "klipper",
        // The machine's own retraction is the tuned one; these are the fallback
        // for a firmware build that does not answer `G10`/`G11`.
        "use_firmware_retraction": true,
        "retract_mm": 0.4,
        "retract_speed_mm_min": 1800.0,
        "z_hop_mm": 0.2,
        // A failed part can be skipped without losing the plate — standard on a
        // Klipper machine with object exclusion built in.
        "exclude_object": true,
        "start_gcode": KLIPPER_START_GCODE,
        "end_gcode": KLIPPER_END_GCODE,
    });
    p
}

/// The built-in offline printer presets: a 220 mm bedslinger and a 350 mm
/// CoreXY — the two shapes almost every desktop machine is one of.
pub fn default_printers() -> Vec<PrinterProfile> {
    vec![default_printer(), corexy_printer()]
}

/// The default offline filament (a generic PLA). Kept as the resolve fallback.
pub fn default_filament() -> FilamentProfile {
    let mut f = base_filament(
        ProfileMeta::builtin("builtin-generic-pla", "Generic PLA"),
        FilamentMaterial::PLA,
    );
    f.vendor = "Generic".to_string();
    f.color = "#d8d8dc".to_string();
    f
}

/// A generic PETG built-in preset.
pub fn default_petg() -> FilamentProfile {
    let mut f = base_filament(
        ProfileMeta::builtin("builtin-generic-petg", "Generic PETG"),
        FilamentMaterial::PETG,
    );
    f.vendor = "Generic".to_string();
    f.color = "#2f7fb8".to_string();
    f
}

/// A generic ABS built-in preset.
pub fn default_abs() -> FilamentProfile {
    let mut f = base_filament(
        ProfileMeta::builtin("builtin-generic-abs", "Generic ABS"),
        FilamentMaterial::ABS,
    );
    f.vendor = "Generic".to_string();
    f.color = "#3a3a3f".to_string();
    f
}

/// The built-in offline filament presets: PLA, PETG, ABS — the three most
/// common FDM materials.
pub fn default_filaments() -> Vec<FilamentProfile> {
    vec![default_filament(), default_petg(), default_abs()]
}

/// The offline default process profile — the one every fallback resolves to.
pub fn default_process() -> ProcessProfile {
    base_process(ProfileMeta::builtin(
        "builtin-standard-02",
        "Standard — 0.20 mm",
    ))
}

/// 0.20 mm tuned for a well-built CoreXY — roughly twice the standard preset.
///
/// The standard profile is written for a machine that may be a decade old and a
/// bedslinger; it is deliberately slow enough that it cannot embarrass itself.
/// A modern CoreXY with a high-flow hotend spends that margin doing nothing, so
/// this preset asks for what such a machine is actually commissioned to do —
/// 200 mm/s on walls-and-infill work, accelerations in the tens of thousands —
/// while keeping the outer wall and the top surface slow, because those two are
/// what the print is judged by and neither is where the time goes.
///
/// **What caps it is the filament, not this profile.** Flow is the real ceiling
/// at these speeds, and the volumetric limit that enforces it belongs to the
/// spool, not the recipe: set `Max Volumetric Speed` on the filament and every
/// speed here is held to whatever the hotend can actually melt.
pub fn high_speed_process() -> ProcessProfile {
    let mut p = base_process(ProfileMeta::builtin(
        "builtin-high-speed-02",
        "High Speed — 0.20 mm",
    ));
    // Tagged draft, not standard: the layer height is the same 0.20 mm, but a
    // preset that spends surface finish to save time is what the badge is for.
    p.quality = PrintQuality::Draft;
    p.params = json!({
        "layer_height": 0.2,
        "first_layer_height": 0.24,
        // Derived from the nozzle rather than pinned: this preset is for
        // machines that are often not on a 0.4, and a hard 0.44 would under-fill
        // a 0.6 by a third.
        "line_width": 0.0,
        "wall_generator": "arachne",
        "wall_count": 3,
        "top_layers": 4,
        "bottom_layers": 3,
        "seam_position": "aligned",
        "infill_density": 0.15,
        "infill_pattern": "TpmsD",
        "infill_base_angle": 45.0,
        "print_speed": 200.0,
        "perimeter_speed": 120.0,
        "infill_speed": 250.0,
        "top_surface_speed": 100.0,
        "first_layer_speed": 40.0,
        "travel_speed_mm_min": 24000.0,
        "acceleration": 15000.0,
        "first_layer_acceleration": 3000.0,
        "outer_wall_acceleration": 6000.0,
        "inner_wall_acceleration": 12000.0,
        "sparse_infill_acceleration": 18000.0,
        "solid_infill_acceleration": 12000.0,
        "top_surface_acceleration": 8000.0,
        "travel_acceleration": 25000.0,
        // Klipper's own default. Below this a fast machine rounds every corner
        // it is allowed to; above it, it rings.
        "square_corner_velocity": 5.0,
        "support_threshold_angle": 45.0,
        "adhesion_type": "skirt",
        "skirt_loops": 1,
    });
    p
}

/// 0.20 mm at the limits a well-tuned machine can actually hold.
///
/// The top of the sensible range, not past it: 300 mm/s and accelerations at
/// 30 000 mm/s² are what a commissioned CoreXY runs at, and the shape of the
/// profile is the same as the one below it — the outer wall and the top surface
/// are still held back, because the point of going fast on the inside is to
/// afford going slowly on the outside.
///
/// This asks more of the machine than of the slicer, so it is the preset most
/// likely to need correcting: a printer that cannot hold these limits will
/// simply not reach them, and one whose hotend cannot melt the flow will ring
/// or under-extrude. The flow ceiling belongs on the filament — around
/// 24 mm³/s is what a modern high-flow hotend sustains.
pub fn maximum_process() -> ProcessProfile {
    let mut p = high_speed_process();
    p.meta = ProfileMeta::builtin("builtin-maximum-02", "Maximum — 0.20 mm");
    p.quality = PrintQuality::Draft;
    p.params = json!({
        "layer_height": 0.2,
        "first_layer_height": 0.24,
        "line_width": 0.0,
        "wall_generator": "arachne",
        "wall_count": 3,
        "top_layers": 4,
        "bottom_layers": 3,
        "seam_position": "aligned",
        "infill_density": 0.15,
        "infill_pattern": "TpmsD",
        "infill_base_angle": 45.0,
        "print_speed": 300.0,
        "perimeter_speed": 200.0,
        "infill_speed": 300.0,
        "top_surface_speed": 150.0,
        "first_layer_speed": 50.0,
        "travel_speed_mm_min": 36000.0,
        "acceleration": 25000.0,
        "first_layer_acceleration": 5000.0,
        "outer_wall_acceleration": 10000.0,
        "inner_wall_acceleration": 20000.0,
        "sparse_infill_acceleration": 30000.0,
        "solid_infill_acceleration": 20000.0,
        "top_surface_acceleration": 10000.0,
        "gap_fill_acceleration": 5000.0,
        "support_acceleration": 20000.0,
        "travel_acceleration": 30000.0,
        "square_corner_velocity": 5.0,
        "support_threshold_angle": 45.0,
        "adhesion_type": "skirt",
        "skirt_loops": 1,
    });
    p
}

/// The built-in offline process presets, slowest first.
pub fn default_processes() -> Vec<ProcessProfile> {
    vec![default_process(), high_speed_process(), maximum_process()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::params::SlicingParams;

    /// What the preset is allowed to decide for itself: the two that define the
    /// quality (a 0.20 mm layer needs a thicker first layer and a wider bead
    /// than the nozzle), and the skirt — a product choice the engine's own
    /// default deliberately leaves off, so a programmatic slice produces the
    /// object and nothing else. Everything else it spells out must be what the
    /// engine would have used anyway.
    const PRESET_KEYS: [&str; 3] = ["first_layer_height", "line_width", "adhesion_type"];

    /// The `Speed` group — everything a "go faster" preset is allowed to touch.
    /// Kept in step with the `x-group = "Speed"` annotations in
    /// `settings::params`; a new speed parameter belongs in both.
    const SPEED_KEYS: [&str; 20] = [
        "print_speed",
        "perimeter_speed",
        "infill_speed",
        "top_surface_speed",
        "first_layer_speed",
        "gap_fill_speed",
        "support_speed",
        "bridge_speed",
        "travel_speed_mm_min",
        "acceleration",
        "first_layer_acceleration",
        "outer_wall_acceleration",
        "inner_wall_acceleration",
        "sparse_infill_acceleration",
        "solid_infill_acceleration",
        "top_surface_acceleration",
        "gap_fill_acceleration",
        "support_acceleration",
        "travel_acceleration",
        "square_corner_velocity",
    ];

    /// Every shipped process preset, and the speed keys each is allowed to
    /// disagree with the engine on.
    ///
    /// A fast preset exists precisely to disagree about speed, so the agreement
    /// check has to know that — but only about speed. It must still be caught
    /// if it quietly changes a wall count or an infill pattern, because a preset
    /// that differs in something it never advertises is the failure this test
    /// was written for.
    fn every_shipped_process() -> Vec<ProcessProfile> {
        default_processes()
    }

    #[test]
    fn the_fast_presets_only_disagree_about_speed() {
        let defaults = serde_json::to_value(SlicingParams::default()).expect("params serialize");
        let defaults = defaults.as_object().expect("params are an object");

        for profile in every_shipped_process().into_iter().skip(1) {
            let name = profile.meta.name.clone();
            let params = profile.params;
            let params = params.as_object().expect("profile params are an object");
            for (key, value) in params {
                let default = defaults
                    .get(key)
                    .unwrap_or_else(|| panic!("`{key}` is not a slicing parameter"));
                if value == default || PRESET_KEYS.contains(&key.as_str()) {
                    continue;
                }
                assert!(
                    SPEED_KEYS.contains(&key.as_str()),
                    "`{name}` changes `{key}`, which is not a speed setting — a speed preset that \
                     quietly reshapes the print is not what the user picked it for"
                );
            }
        }
    }

    #[test]
    fn every_preset_has_its_own_id_and_name() {
        let all = every_shipped_process();
        let ids: std::collections::HashSet<_> = all.iter().map(|p| &p.meta.id).collect();
        let names: std::collections::HashSet<_> = all.iter().map(|p| &p.meta.name).collect();
        assert_eq!(ids.len(), all.len(), "two process presets share an id");
        assert_eq!(names.len(), all.len(), "two process presets share a name");

        let printers = default_printers();
        let printer_ids: std::collections::HashSet<_> =
            printers.iter().map(|p| &p.meta.id).collect();
        assert_eq!(
            printer_ids.len(),
            printers.len(),
            "two printer presets share an id"
        );
    }

    /// The presets get faster in the order they are listed, so the picker reads
    /// as a scale rather than as three unrelated recipes.
    #[test]
    fn the_presets_are_listed_slowest_first() {
        let speeds: Vec<f64> = every_shipped_process()
            .iter()
            .map(|p| p.params["print_speed"].as_f64().expect("a print speed"))
            .collect();
        assert!(
            speeds.windows(2).all(|w| w[0] < w[1]),
            "process presets are not ordered slowest first: {speeds:?}"
        );
    }

    /// The badge has to say something. It may repeat — two presets can trade
    /// different amounts of the same thing — but it must fall as the list gets
    /// faster, and one tag across the whole list makes it decoration.
    #[test]
    fn the_quality_tag_falls_as_the_presets_get_faster() {
        let rank = |q: PrintQuality| match q {
            PrintQuality::Draft => 0,
            PrintQuality::Standard => 1,
            PrintQuality::Fine => 2,
        };
        let all = every_shipped_process();
        for pair in all.windows(2) {
            assert!(
                rank(pair[1].quality) <= rank(pair[0].quality),
                "`{}` is faster than `{}` but claims a better quality",
                pair[1].meta.name,
                pair[0].meta.name
            );
        }
        let tags: std::collections::HashSet<_> = all.iter().map(|p| rank(p.quality)).collect();
        assert!(
            tags.len() > 1,
            "every shipped preset carries the same quality tag, which makes the badge decoration"
        );
    }

    #[test]
    fn the_shipped_process_profile_agrees_with_the_engine_defaults() {
        let defaults = serde_json::to_value(SlicingParams::default()).expect("params serialize");
        let defaults = defaults.as_object().expect("params are an object");
        let profile = default_process().params;
        let profile = profile.as_object().expect("profile params are an object");

        for (key, value) in profile {
            if PRESET_KEYS.contains(&key.as_str()) {
                continue;
            }
            let default = defaults
                .get(key)
                .unwrap_or_else(|| panic!("`{key}` is not a slicing parameter"));
            assert_eq!(
                value, default,
                "the shipped profile sets `{key}` to {value}, but the engine defaults to {default}\
                 — a slice from the app and one from the CLI would print differently"
            );
        }
    }
}
