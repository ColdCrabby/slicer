//! TOML configuration data structures.

use crate::settings::params::{LifecycleMarkerConfig, SlicingParams};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Machine hardware specification embedded in the config file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct MachineConfig {
    /// Human-readable machine name.
    #[serde(default = "MachineConfig::default_name")]
    pub name: String,
    /// Nozzle diameter in mm.
    #[serde(default = "MachineConfig::default_nozzle_diameter")]
    pub nozzle_diameter: f64,
    /// Minimum supported layer height in mm.
    #[serde(default = "MachineConfig::default_min_layer_height")]
    pub min_layer_height: f64,
    /// Maximum supported layer height in mm.
    #[serde(default = "MachineConfig::default_max_layer_height")]
    pub max_layer_height: f64,
    /// Maximum print speed in mm/s.
    #[serde(default = "MachineConfig::default_max_print_speed")]
    pub max_print_speed: f64,
    /// Maximum acceleration in mm/s².
    #[serde(default = "MachineConfig::default_max_acceleration")]
    pub max_acceleration: f64,
    /// Build volume X in mm.
    #[serde(default = "MachineConfig::default_build_volume_x")]
    pub build_volume_x: f64,
    /// Build volume Y in mm.
    #[serde(default = "MachineConfig::default_build_volume_y")]
    pub build_volume_y: f64,
    /// Build volume Z in mm.
    #[serde(default = "MachineConfig::default_build_volume_z")]
    pub build_volume_z: f64,
    /// Preferred Z-rotation (degrees) applied after auto-orient finds the best
    /// face-down orientation.  Set to `45.0` for CoreXY printers to align the
    /// print seam with the stepper axes.  `0.0` = disabled (default).
    #[serde(default)]
    pub preferred_print_rotation_deg: f64,
}

impl MachineConfig {
    fn default_name() -> String {
        "Default 0.4mm Nozzle".to_string()
    }
    fn default_nozzle_diameter() -> f64 {
        0.4
    }
    fn default_min_layer_height() -> f64 {
        0.1
    }
    fn default_max_layer_height() -> f64 {
        0.3
    }
    fn default_max_print_speed() -> f64 {
        150.0
    }
    fn default_max_acceleration() -> f64 {
        1000.0
    }
    fn default_build_volume_x() -> f64 {
        220.0
    }
    fn default_build_volume_y() -> f64 {
        220.0
    }
    fn default_build_volume_z() -> f64 {
        250.0
    }
}

impl Default for MachineConfig {
    fn default() -> Self {
        Self {
            name: Self::default_name(),
            nozzle_diameter: Self::default_nozzle_diameter(),
            min_layer_height: Self::default_min_layer_height(),
            max_layer_height: Self::default_max_layer_height(),
            max_print_speed: Self::default_max_print_speed(),
            max_acceleration: Self::default_max_acceleration(),
            build_volume_x: Self::default_build_volume_x(),
            build_volume_y: Self::default_build_volume_y(),
            build_volume_z: Self::default_build_volume_z(),
            preferred_print_rotation_deg: 0.0,
        }
    }
}

/// Server runtime configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct ServerConfig {
    /// Host address to bind.
    #[serde(default = "ServerConfig::default_host")]
    pub host: String,
    /// TCP port to listen on.
    #[serde(default = "ServerConfig::default_port")]
    pub port: u16,
    /// Directory containing the built Angular app.
    #[serde(default = "ServerConfig::default_ui_dir")]
    pub ui_dir: String,
    /// Directory to store temporary session files.
    #[serde(default)]
    pub work_dir: Option<String>,
    /// Allowed CORS origins (empty = no restriction).
    #[serde(default)]
    pub cors_origins: Vec<String>,
}

impl ServerConfig {
    fn default_host() -> String {
        "127.0.0.1".to_string()
    }
    fn default_port() -> u16 {
        5201
    }
    fn default_ui_dir() -> String {
        "./ui/dist/slicer-ui/browser".to_string()
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: Self::default_host(),
            port: Self::default_port(),
            ui_dir: Self::default_ui_dir(),
            work_dir: None,
            cors_origins: Vec::new(),
        }
    }
}

/// Global application settings stored in the config file.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct GlobalConfig {
    /// Log level (e.g. "info", "debug", "warn", "error").
    #[serde(default)]
    pub log_level: Option<String>,
}

/// A named slicing preset.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct SlicingPreset {
    /// Human-readable description.
    #[serde(default)]
    pub description: Option<String>,
    /// Slicing parameters for this preset.
    #[serde(flatten)]
    pub params: SlicingParams,
}

/// A named material profile.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct MaterialProfile {
    /// Human-readable description.
    #[serde(default)]
    pub description: Option<String>,
    /// Recommended nozzle temperature in °C.
    #[serde(default)]
    pub nozzle_temp: Option<f64>,
    /// Recommended bed temperature in °C.
    #[serde(default)]
    pub bed_temp: Option<f64>,
    /// Recommended print speed in mm/s.
    #[serde(default)]
    pub print_speed: Option<f64>,
    /// Filament diameter in mm.
    #[serde(default)]
    pub filament_diameter_mm: Option<f64>,
}

/// Profile collection: presets, machine profiles, material profiles.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct ProfilesConfig {
    /// Named slicing presets (draft, standard, high-quality, etc.).
    #[serde(default)]
    pub presets: HashMap<String, SlicingPreset>,
    /// Named machine profiles.
    #[serde(default)]
    pub machines: HashMap<String, MachineConfig>,
    /// Named material profiles.
    #[serde(default)]
    pub materials: HashMap<String, MaterialProfile>,
}

/// Root application configuration persisted to `slicer.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct AppConfig {
    /// Global application settings.
    #[serde(default)]
    pub global: GlobalConfig,
    /// Default slicing parameters. When `None`, built-in defaults apply.
    #[serde(default)]
    pub slicing: Option<SlicingParams>,
    /// Server runtime configuration.
    #[serde(default)]
    pub server: ServerConfig,
    /// Active machine specification.
    #[serde(default)]
    pub machine: MachineConfig,
    /// Named profile collections.
    #[serde(default)]
    pub profiles: ProfilesConfig,
    /// Custom G-code inserted before all print moves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_print_gcode: Option<String>,
    /// Custom G-code inserted after all print moves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_print_gcode: Option<String>,
    /// Per-flavor lifecycle marker overrides (`[lifecycle_markers.marlin]`, etc.).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub lifecycle_markers: HashMap<String, LifecycleMarkerConfig>,
}
