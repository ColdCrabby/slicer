//! Filament (material) profile.
//!
//! Holds the material-specific *domain* fields used for display and estimation
//! (material family, colour, density, cost) plus a `params` bundle of sparse
//! [`SlicingParams`] overrides (temperatures, cooling, flow, filament
//! diameter). Slice parameters use the engine's own field names and units — no
//! renaming, no percent/fraction translation — so there is a single
//! representation and no mapping layer.
//!
//! [`SlicingParams`]: crate::settings::params::SlicingParams

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::meta::ProfileMeta;

/// Supported material families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub enum FilamentMaterial {
    /// Polylactic acid — easy, low-temp.
    #[default]
    PLA,
    /// Glycol-modified PET — tough, moderate-temp.
    PETG,
    /// ABS — high-temp, enclosure recommended.
    ABS,
    /// ASA — UV-stable ABS analogue.
    ASA,
    /// Thermoplastic polyurethane — flexible.
    TPU,
    /// Polycarbonate — very high-temp, strong.
    PC,
    /// Nylon (polyamide) — tough, hygroscopic.
    Nylon,
    /// Polyvinyl alcohol — water-soluble support.
    PVA,
}

impl FilamentMaterial {
    /// Typical starting `SlicingParams` overrides for this material, as a sparse
    /// JSON object. Fan speeds are engine-native fractions (0.0–1.0).
    pub fn default_params(self) -> serde_json::Value {
        let (nozzle, nozzle1, bed, bed1, fan_min, fan_max, vmax) = match self {
            Self::PLA => (210.0, 215.0, 60.0, 60.0, 1.0, 1.0, 15.0),
            Self::PETG => (240.0, 245.0, 80.0, 80.0, 0.4, 0.6, 12.0),
            Self::ABS => (250.0, 255.0, 100.0, 105.0, 0.0, 0.3, 11.0),
            Self::ASA => (250.0, 255.0, 100.0, 105.0, 0.0, 0.3, 11.0),
            Self::TPU => (230.0, 235.0, 40.0, 45.0, 0.5, 0.8, 4.0),
            Self::PC => (270.0, 275.0, 110.0, 110.0, 0.0, 0.2, 10.0),
            Self::Nylon => (260.0, 265.0, 90.0, 90.0, 0.0, 0.2, 10.0),
            Self::PVA => (215.0, 220.0, 60.0, 60.0, 0.3, 0.5, 6.0),
        };
        serde_json::json!({
            "nozzle_temp": nozzle,
            "nozzle_temp_first_layer": nozzle1,
            "bed_temp": bed,
            "bed_temp_first_layer": bed1,
            "first_layer_fan_speed": fan_min,
            "fan_speed": fan_max,
            "max_volumetric_speed": vmax,
            "disable_fan_first_layers": 1,
            "flow_ratio": 1.0,
            "pressure_advance": 0.04,
            "filament_diameter_mm": 1.75,
        })
    }
}

/// Typical density (g/cm³) for a material, used for weight / cost estimation.
pub fn material_density(material: FilamentMaterial) -> f64 {
    match material {
        FilamentMaterial::PLA => 1.24,
        FilamentMaterial::PETG => 1.27,
        FilamentMaterial::ABS => 1.04,
        FilamentMaterial::ASA => 1.07,
        FilamentMaterial::TPU => 1.21,
        FilamentMaterial::PC => 1.2,
        FilamentMaterial::Nylon => 1.14,
        FilamentMaterial::PVA => 1.23,
    }
}

/// A filament (material) profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FilamentProfile {
    /// Provenance (id, name, source, …).
    #[serde(flatten)]
    pub meta: ProfileMeta,

    /// Manufacturer / brand.
    pub vendor: String,
    /// Material family.
    pub material: FilamentMaterial,
    /// Display colour (hex string, e.g. `#e0730f`).
    pub color: String,
    /// Density (g/cm³) for weight / cost estimation.
    pub density_g_cm3: f64,
    /// Cost per kilogram (currency-agnostic) for estimation.
    pub cost_per_kg: f64,

    /// Sparse `SlicingParams` overrides this filament contributes
    /// (temperatures, cooling, flow, `filament_diameter_mm`).
    #[serde(default)]
    pub params: serde_json::Value,
}
