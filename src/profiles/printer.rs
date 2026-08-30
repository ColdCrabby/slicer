//! Printer (machine) profile.
//!
//! Holds the hardware *domain* fields (build volume, bed shape, network
//! connection) plus a `params` bundle of sparse [`SlicingParams`] overrides
//! (nozzle diameter, retraction, travel speed, firmware flavor, start/end
//! G-code, …). Bed geometry feeds the scene's bed config, not `SlicingParams`.
//! Slice parameters use the engine's own field names and units, so there is a
//! single representation and no mapping layer.
//!
//! [`SlicingParams`]: crate::settings::params::SlicingParams

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::meta::ProfileMeta;

/// Bed geometry. Circular beds (deltas) use `bed_width` as the diameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BedShape {
    /// Rectangular bed of `bed_width` × `bed_depth`.
    #[default]
    Rectangular,
    /// Circular bed; `bed_width` is the diameter, `bed_depth` is ignored.
    Circular,
}

/// Kind of network connection a printer can use to receive prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PrinterConnectionKind {
    /// No network connection configured.
    #[default]
    None,
    /// OctoPrint host.
    Octoprint,
    /// Moonraker (Klipper) host.
    Moonraker,
    /// Bambu Lab cloud/LAN.
    Bambu,
    /// PrusaLink host.
    Prusalink,
}

/// Network connection settings for a printer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PrinterConnection {
    /// Transport kind.
    #[serde(default)]
    pub kind: PrinterConnectionKind,
    /// Host / address, when applicable. May include a scheme (`http://…`) and
    /// an explicit `:port`; the transport fills in sensible defaults otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Explicit TCP port override. Ignored when `host` already carries a port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// API key / token for authenticated hosts (Moonraker `X-Api-Key`,
    /// OctoPrint `X-Api-Key`). Never surfaced back to the UI once stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Whether the last connection attempt succeeded (UI-owned status).
    #[serde(default)]
    pub connected: bool,
}

/// Bed dimension defaults keep an incomplete profile (e.g. one persisted before
/// these fields existed) deserialising instead of rejecting the whole message.
fn default_bed_width() -> f64 {
    220.0
}
fn default_bed_depth() -> f64 {
    220.0
}
fn default_bed_height() -> f64 {
    250.0
}

/// A printer (machine) profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PrinterProfile {
    /// Provenance (id, name, source, …).
    #[serde(flatten)]
    pub meta: ProfileMeta,

    /// Manufacturer / brand.
    pub vendor: String,
    /// Model designation.
    pub model: String,

    /// Bed shape.
    #[serde(default)]
    pub bed_shape: BedShape,
    /// Width (mm) along +X. For circular beds this is the diameter.
    #[serde(default = "default_bed_width")]
    pub bed_width: f64,
    /// Depth (mm) along +Y. Ignored for circular beds.
    #[serde(default = "default_bed_depth")]
    pub bed_depth: f64,
    /// Max Z height (mm).
    #[serde(default = "default_bed_height")]
    pub bed_height: f64,
    /// True for delta / origin-at-center machines.
    #[serde(default)]
    pub origin_at_center: bool,

    /// Preferred Z-rotation (degrees) applied on top of the orientation
    /// auto-orient picks, when placing parts on this machine.
    ///
    /// A machine characteristic rather than a print setting: CoreXY printers
    /// move fastest along their diagonals, so many users print everything
    /// rotated `45°` to keep long walls off the belt axes. `0.0` (default)
    /// leaves the auto-orient result untouched.
    #[serde(default)]
    pub preferred_orientation_deg: f64,

    /// Network connection settings.
    #[serde(default)]
    pub connection: PrinterConnection,

    /// Sparse `SlicingParams` overrides this printer contributes
    /// (`nozzle_diameter_mm`, `filament_diameter_mm`, `print_speed`,
    /// `travel_speed_mm_min`, `retract_mm`, `retract_speed_mm_min`, `z_hop_mm`,
    /// `gcode_flavor`, `start_gcode`, `end_gcode`, `extruder_count`).
    #[schemars(schema_with = "crate::settings::params::slicing_params_schema")]
    #[serde(default)]
    pub params: serde_json::Value,
}

impl PrinterProfile {
    /// Printable-area dimensions `(width, depth)` in mm for the bed config.
    /// Circular beds report the diameter for both axes.
    pub fn bed_dimensions(&self) -> (f64, f64) {
        match self.bed_shape {
            BedShape::Circular => (self.bed_width, self.bed_width),
            BedShape::Rectangular => (self.bed_width, self.bed_depth),
        }
    }

    /// Auto-orient options this machine prefers, ready to hand to
    /// [`crate::orient::auto_orient`] or [`crate::orient::ArrangeOptions`].
    pub fn orient_options(&self) -> crate::orient::AutoOrientOptions {
        crate::orient::AutoOrientOptions {
            preferred_z_rotation_deg: self.preferred_orientation_deg,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferred_orientation_defaults_to_zero_for_older_profiles() {
        // Profiles persisted before the field existed must still load — a
        // rejected library would lose every printer the user owns.
        let stored = serde_json::json!({
            "id": "p1",
            "name": "Old printer",
            "source": "user",
            "vendor": "Custom",
            "model": "Generic",
        });

        let printer: PrinterProfile = serde_json::from_value(stored).expect("legacy profile loads");
        assert_eq!(printer.preferred_orientation_deg, 0.0);
        assert_eq!(printer.orient_options().preferred_z_rotation_deg, 0.0);
    }

    #[test]
    fn preferred_orientation_reaches_the_orient_options() {
        let mut printer: PrinterProfile = serde_json::from_value(serde_json::json!({
            "id": "p1",
            "name": "CoreXY",
            "source": "user",
            "vendor": "Custom",
            "model": "Generic",
            "preferred_orientation_deg": 45.0,
        }))
        .expect("profile loads");

        assert_eq!(printer.orient_options().preferred_z_rotation_deg, 45.0);

        printer.preferred_orientation_deg = 0.0;
        assert_eq!(printer.orient_options().preferred_z_rotation_deg, 0.0);
    }
}
