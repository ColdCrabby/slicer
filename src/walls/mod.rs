//! Wall (perimeter) generation — switchable between generators.
//!
//! This module is the single entry point the slicing pipeline calls to turn
//! raw mesh cross-section contours into extrusion **beads** (variable-width
//! wall paths).  Which algorithm runs is selected per slice by
//! [`SlicingParams::wall_generator`](crate::settings::params::SlicingParams):
//!
//! | [`WallGenerator`] | Module | Status |
//! | --- | --- | --- |
//! | `Classic` | [`classic`] | Fixed-width concentric offsets + thin-wall gap fill (default) |
//! | `Arachne` | [`arachne`] | Variable-width skeletal trapezoidation — **not yet implemented (panics)** |
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
///
/// # Panics
///
/// Panics with an "unimplemented" message when
/// [`WallGenerator::Arachne`] is selected — the Arachne generator is not yet
/// implemented.  Select [`WallGenerator::Classic`] (the default).
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
/// geometry can be captured into `debug` for visual inspection.
///
/// # Panics
///
/// Same as [`generate_walls`]: panics when [`WallGenerator::Arachne`] is
/// selected.
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
