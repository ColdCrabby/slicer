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

    /// Is this XY point on the printable bed?
    ///
    /// The one place the bed outline is turned into a yes/no answer. Object
    /// placement asks it through [`contains_aabb`]; the G-code preview
    /// asks it per extrusion endpoint, so a skirt or brim that wanders off the
    /// plate is flagged by the same outline the viewer draws.
    ///
    /// A small epsilon absorbs float noise, so a point sitting exactly on the
    /// bed edge is inside.
    pub fn contains_xy(&self, x: f64, y: f64) -> bool {
        match self.shape {
            BedShape::Rectangular => {
                x >= self.origin_offset_x - EPS
                    && y >= self.origin_offset_y - EPS
                    && x <= self.origin_offset_x + self.width + EPS
                    && y <= self.origin_offset_y + self.depth + EPS
            }
            BedShape::Circular => {
                let (cx, cy) = self.center_xy();
                let radius = self.width.min(self.depth) / 2.0 + EPS;
                let dx = x - cx;
                let dy = y - cy;
                dx * dx + dy * dy <= radius * radius
            }
        }
    }

    /// Does the whole box fit inside the printable volume?
    ///
    /// Height is checked against `height` and the floor (a model sunk below
    /// z = 0 cannot print). The XY test is [`contains_xy`] over all four
    /// footprint corners, so an object hanging off a circular bed's curved
    /// edge is caught as surely as one off a rectangular bed's straight one.
    ///
    /// A small epsilon absorbs float noise, so an object sitting exactly on
    /// the bed edge or floor is not reported as out of bounds.
    pub fn contains_aabb(&self, aabb: &crate::mesh::types::AABB) -> bool {
        if aabb.min.z < -EPS || aabb.max.z > self.height + EPS {
            return false;
        }

        self.contains_xy(aabb.min.x, aabb.min.y)
            && self.contains_xy(aabb.max.x, aabb.min.y)
            && self.contains_xy(aabb.min.x, aabb.max.y)
            && self.contains_xy(aabb.max.x, aabb.max.y)
    }
}

/// Tolerance on every bed-containment test: 1 µm, below any printable
/// resolution but wide enough to absorb the f32 noise real mesh files carry.
/// STL stores coordinates as f32, so a model authored flat on z = 0 routinely
/// reports a min z a few nanometres either side of it (3DBenchy: −2.7e-6 mm).
/// A tighter epsilon flags such models as out of bounds on every plate.
const EPS: f64 = 1e-3;

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

    fn box_aabb(min: (f64, f64, f64), max: (f64, f64, f64)) -> crate::mesh::types::AABB {
        crate::mesh::types::AABB {
            min: crate::mesh::types::Vertex::new(min.0, min.1, min.2),
            max: crate::mesh::types::Vertex::new(max.0, max.1, max.2),
        }
    }

    #[test]
    fn f32_noise_below_the_floor_is_not_out_of_bounds() {
        // STL coordinates are f32: a model authored flat on z = 0 reports a
        // min z a few nanometres either side of it (3DBenchy: -2.7e-6 mm).
        let bed = BedConfig::default();
        assert!(bed.contains_aabb(&box_aabb((10.0, 10.0, -2.7e-6), (40.0, 40.0, 48.0))));
    }

    #[test]
    fn contains_xy_follows_the_bed_shape() {
        let round = BedConfig {
            width: 200.0,
            depth: 200.0,
            height: 250.0,
            origin_offset_x: 0.0,
            origin_offset_y: 0.0,
            shape: BedShape::Circular,
        };
        // The corner of the bounding box is off the disk even though a
        // rectangular bed of the same extents would accept it.
        assert!(round.contains_xy(100.0, 100.0));
        assert!(!round.contains_xy(190.0, 190.0));
        assert!(BedConfig::default().contains_xy(190.0, 190.0));
        // Exactly on the edge counts as on the bed.
        assert!(BedConfig::default().contains_xy(220.0, 0.0));
        assert!(!BedConfig::default().contains_xy(220.5, 0.0));
    }

    #[test]
    fn genuinely_sunken_or_overhanging_boxes_are_out_of_bounds() {
        let bed = BedConfig::default();
        // Half a millimetre below the bed is a real placement fault.
        assert!(!bed.contains_aabb(&box_aabb((10.0, 10.0, -0.5), (40.0, 40.0, 48.0))));
        // Hanging off the far X edge of a 220 mm bed.
        assert!(!bed.contains_aabb(&box_aabb((200.0, 10.0, 0.0), (240.0, 40.0, 10.0))));
        // Taller than the build volume.
        assert!(!bed.contains_aabb(&box_aabb((10.0, 10.0, 0.0), (40.0, 40.0, 300.0))));
    }
}
