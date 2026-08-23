//! Bare-minimum blank-slate defaults — the *only* profile data the engine
//! ships.
//!
//! The engine deliberately carries **no product/vendor profiles**. It provides
//! exactly one generic `builtin` default per category so an offline install can
//! always create, edit, and slice; the vendor catalog (Prusa, Bambu, …) is a
//! cloud concern and lives entirely outside this crate.

use serde_json::json;

use super::filament::{material_density, FilamentMaterial, FilamentProfile};
use super::meta::ProfileMeta;
use super::printer::{BedShape, PrinterConnection, PrinterProfile};
use super::process::{PrintQuality, ProcessProfile};

const DEFAULT_START_GCODE: &str =
    "; --- start ---\nG28 ; home all axes\nG92 E0 ; reset extruder\nG1 Z2.0 F3000 ; lift nozzle";
const DEFAULT_END_GCODE: &str = "; --- end ---\nG91 ; relative positioning\nG1 E-2 F2700 ; retract\nG1 Z10 F3000 ; lift\nG90 ; absolute positioning\nM104 S0 ; nozzle off\nM140 S0 ; bed off\nM84 ; disable steppers";

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
            "infill_density": 0.2,
            "infill_pattern": "Gyroid",
            "infill_base_angle": 45.0,
            "print_speed": 120.0,
            "perimeter_speed": 80.0,
            "infill_speed": 150.0,
            "top_surface_speed": 60.0,
            "first_layer_speed": 30.0,
            "support_threshold_angle": 55.0,
            "adhesion_type": "skirt",
            "skirt_loops": 1,
        }),
    }
}

/// The single offline default printer.
pub fn default_printer() -> PrinterProfile {
    let mut p = base_printer(ProfileMeta::builtin(
        "builtin-generic-printer",
        "Generic 220 mm printer",
    ));
    p.vendor = "Generic".to_string();
    p.model = "FDM 220".to_string();
    p
}

/// The single offline default filament (a generic PLA).
pub fn default_filament() -> FilamentProfile {
    let mut f = base_filament(
        ProfileMeta::builtin("builtin-generic-pla", "Generic PLA"),
        FilamentMaterial::PLA,
    );
    f.vendor = "Generic".to_string();
    f.color = "#d8d8dc".to_string();
    f
}

/// The single offline default process profile.
pub fn default_process() -> ProcessProfile {
    base_process(ProfileMeta::builtin(
        "builtin-standard-02",
        "Standard — 0.20 mm",
    ))
}
