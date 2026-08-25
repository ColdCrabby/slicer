//! Print bed configuration for the scene.

#[cfg(not(target_arch = "wasm32"))]
use crate::config::types::MachineConfig;
use serde::{Deserialize, Serialize};

/// Shape of the printable bed area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BedShape {
    /// Rectangular bed spanning the full `width` × `depth`.
    #[default]
    Rectangular,
    /// Circular (delta) bed inscribed in the `width` × `depth` bounding box.
    Circular,
}

/// Print bed dimensions and origin offset.
///
/// All units are millimeters. The bed lies in the XY plane with its origin
/// (printer 0,0) at `(origin_offset_x, origin_offset_y)` in scene coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BedConfig {
    /// Width along the X axis (mm).
    pub width: f64,
    /// Depth along the Y axis (mm).
    pub depth: f64,
    /// Maximum print height along the Z axis (mm).
    pub height: f64,
    /// X offset of the printer origin from the scene origin (mm).
    pub origin_offset_x: f64,
    /// Y offset of the printer origin from the scene origin (mm).
    pub origin_offset_y: f64,
    /// Shape of the printable area.
    #[serde(default)]
    pub shape: BedShape,
}

impl Default for BedConfig {
    fn default() -> Self {
        Self {
            width: 220.0,
            depth: 220.0,
            height: 250.0,
            origin_offset_x: 0.0,
            origin_offset_y: 0.0,
            shape: BedShape::Rectangular,
        }
    }
}

impl BedConfig {
    /// Geometric center of the bed in scene coordinates.
    pub fn center_xy(&self) -> (f64, f64) {
        (
            self.origin_offset_x + self.width / 2.0,
            self.origin_offset_y + self.depth / 2.0,
        )
    }

    /// Axis-aligned footprint (width, depth) usable for packing, centered on
    /// the bed. For circular beds this is the largest inscribed square
    /// (`diameter / √2`) so any object packed within it is guaranteed to sit
    /// inside the disk; rectangular beds return the full extents.
    pub fn usable_footprint(&self) -> (f64, f64) {
        match self.shape {
            BedShape::Rectangular => (self.width, self.depth),
            BedShape::Circular => {
                let diameter = self.width.min(self.depth);
                let side = diameter / std::f64::consts::SQRT_2;
                (side, side)
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl From<&MachineConfig> for BedConfig {
    fn from(m: &MachineConfig) -> Self {
        Self {
            width: m.build_volume_x,
            depth: m.build_volume_y,
            height: m.build_volume_z,
            origin_offset_x: 0.0,
            origin_offset_y: 0.0,
            shape: BedShape::Rectangular,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn from_machine_config_copies_dimensions() {
        let mc = MachineConfig {
            build_volume_x: 256.0,
            build_volume_y: 256.0,
            build_volume_z: 256.0,
            ..MachineConfig::default()
        };
        let bed: BedConfig = (&mc).into();
        assert_eq!(bed.width, 256.0);
        assert_eq!(bed.depth, 256.0);
        assert_eq!(bed.height, 256.0);
    }

    #[test]
    fn center_xy_accounts_for_offset() {
        let bed = BedConfig {
            width: 200.0,
            depth: 100.0,
            height: 250.0,
            origin_offset_x: 10.0,
            origin_offset_y: 20.0,
            shape: BedShape::Rectangular,
        };
        let (cx, cy) = bed.center_xy();
        assert!((cx - 110.0).abs() < 1e-9);
        assert!((cy - 70.0).abs() < 1e-9);
    }

    #[test]
    fn circular_usable_footprint_is_inscribed_square() {
        let bed = BedConfig {
            width: 200.0,
            depth: 200.0,
            height: 250.0,
            origin_offset_x: 0.0,
            origin_offset_y: 0.0,
            shape: BedShape::Circular,
        };
        let (w, d) = bed.usable_footprint();
        // Inscribed square of a 200 mm circle has side 200/√2 ≈ 141.42 mm.
        assert!((w - 141.421).abs() < 1e-2, "w={w}");
        assert!((d - 141.421).abs() < 1e-2, "d={d}");
    }

    #[test]
    fn rectangular_usable_footprint_is_full_extent() {
        let bed = BedConfig {
            width: 250.0,
            depth: 210.0,
            height: 250.0,
            origin_offset_x: 0.0,
            origin_offset_y: 0.0,
            shape: BedShape::Rectangular,
        };
        assert_eq!(bed.usable_footprint(), (250.0, 210.0));
    }
}
