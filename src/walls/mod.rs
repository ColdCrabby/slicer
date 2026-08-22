//! Wall (perimeter) generation — switchable between generators.
//!
//! This module is the single entry point the slicing pipeline calls to turn
//! raw mesh cross-section contours into extrusion **beads** (variable-width
//! wall paths).  Which algorithm runs is selected per slice by
//! [`SlicingParams::wall_generator`](crate::settings::params::SlicingParams):
//!
//! | [`WallGenerator`] | Module | Status |
//! | --- | --- | --- |
//! | `Classic` | [`classic`] | Fixed-width concentric offsets + thin-wall gap fill |
//! | `Arachne` | [`arachne`] | Medial-axis offset loops + variable-width gap fill (default) |
//!
//! Both generators emit the same output shape — the layer's `OuterWall` /
//! `InnerWall` contours are replaced with bead paths and per-path widths, and
//! every non-perimeter path (surfaces, infill) is preserved in its original
//! order after the walls — so everything downstream of this module is
//! generator-agnostic.

mod arachne;
mod beads;
mod classic;
mod types;

pub use types::{Bead, WallParams, WallTimings};

use crate::core::SliceLayer;
use crate::settings::params::{SlicingParams, WallGenerator};

/// Generate wall paths for every layer using the configured generator.
///
/// Replaces the raw `OuterWall` / `InnerWall` contours produced by
/// [`crate::core::slice_mesh`] with generated beads.  Dispatches on
/// [`SlicingParams::wall_generator`].
pub fn generate_walls(layers: &mut [SliceLayer], params: &SlicingParams) -> WallTimings {
    let wall_params = WallParams::from_slicing_params(params);
    match params.wall_generator {
        WallGenerator::Classic => classic::generate_classic_walls(layers, &wall_params),
        WallGenerator::Arachne => arachne::generate_arachne_walls(layers, &wall_params),
    }
}

/// Debug-mode counterpart to [`generate_walls`].
///
/// Runs the selected generator sequentially (no `rayon`) so intermediate
/// geometry can be captured into `debug` for visual inspection.  Only the
/// classic generator captures debug snapshots; the Arachne generator runs
/// normally.
#[cfg(not(target_arch = "wasm32"))]
pub fn generate_walls_debug(
    layers: &mut [SliceLayer],
    params: &SlicingParams,
    debug: &mut crate::debug::DebugGeometry,
) -> WallTimings {
    let wall_params = WallParams::from_slicing_params(params);
    match params.wall_generator {
        WallGenerator::Classic => {
            classic::generate_classic_walls_debug(layers, &wall_params, debug)
        }
        WallGenerator::Arachne => arachne::generate_arachne_walls(layers, &wall_params),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ExtrusionRole;
    use crate::settings::params::WallGenerator;
    use clipper2::Path;

    fn square_layer() -> SliceLayer {
        let mut layer = SliceLayer::new(0.2);
        let sq: Path = vec![(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)].into();
        layer.paths.push(sq);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.path_widths.push(None);
        layer.path_vertex_widths.push(None);
        layer.path_is_open.push(false);
        layer
    }

    #[test]
    fn arachne_is_the_default_generator() {
        assert_eq!(
            SlicingParams::default().wall_generator,
            WallGenerator::Arachne
        );
    }

    #[test]
    fn dispatch_routes_to_each_generator_without_panicking() {
        for generator in [WallGenerator::Classic, WallGenerator::Arachne] {
            let params = SlicingParams {
                wall_generator: generator,
                ..SlicingParams::default()
            };
            let mut layers = vec![square_layer()];
            let _ = generate_walls(&mut layers, &params);
            assert!(
                !layers[0].paths.is_empty(),
                "{generator:?} must produce wall paths"
            );
            assert_eq!(
                layers[0].role_for_path(0),
                ExtrusionRole::OuterWall,
                "{generator:?} first path must be the outer wall"
            );
        }
    }
}
