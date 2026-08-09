//! Arachne variable-width perimeter generator — **not yet implemented**.
//!
//! This is the placeholder for the real [Arachne][arachne-paper] generator
//! (Kuipers et al. 2020): a skeletal-trapezoidation of each shell polygon
//! (built on a segment Voronoi diagram) that emits beads whose extrusion
//! width varies continuously along their length, with graded bead-count
//! transitions.  It is the generator CuraEngine / PrusaSlicer / OrcaSlicer
//! ship as their "Arachne" wall mode.
//!
//! Selecting [`WallGenerator::Arachne`](crate::settings::params::WallGenerator)
//! currently panics; use [`WallGenerator::Classic`] until this is built.
//!
//! [arachne-paper]: https://dl.acm.org/doi/10.1145/3386569.3392408

use super::types::{WallParams, WallTimings};
use crate::core::SliceLayer;

/// Placeholder entry point for the Arachne generator.
///
/// # Panics
///
/// Always — the Arachne generator is not implemented yet.
pub fn generate_arachne_walls(_layers: &mut [SliceLayer], _params: &WallParams) -> WallTimings {
    unimplemented!(
        "the Arachne wall generator is not implemented yet; \
         set `wall_generator = \"classic\"` (the default)"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::params::SlicingParams;

    #[test]
    #[should_panic(expected = "Arachne wall generator is not implemented")]
    fn arachne_generator_panics_until_implemented() {
        let params = WallParams::from_slicing_params(&SlicingParams::default());
        let mut layers: Vec<SliceLayer> = Vec::new();
        let _ = generate_arachne_walls(&mut layers, &params);
    }
}
