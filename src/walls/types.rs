//! Core data types shared by the wall generators.

use crate::settings::params::SlicingParams;
use clipper2::Path;

/// Resolved wall parameters with all values in absolute mm.
///
/// Constructed from [`SlicingParams`] via [`WallParams::from_slicing_params`].
/// Shared by every wall generator ([`super::classic`], [`super::arachne`]).
pub struct WallParams {
    /// Nozzle diameter in mm.
    pub nozzle_diameter_mm: f64,
    /// Maximum number of perimeter beads per shell.
    pub wall_count: usize,
    /// Minimum bead width in mm (= `wall_line_width_min × nozzle_diameter_mm`).
    pub wall_line_width_min_mm: f64,
    /// Maximum bead width in mm (= `wall_line_width_max × nozzle_diameter_mm`).
    pub wall_line_width_max_mm: f64,
    /// Number of innermost beads that may absorb residual width variation.
    pub wall_distribution_count: usize,
    /// Minimum medial gap-fill run length in mm; shorter runs are dropped as
    /// faceting noise.  `0` keeps every run that spans at least one segment.
    pub gap_fill_min_length_mm: f64,
    /// Print the outer wall first (`true`) or the inner walls first (`false`,
    /// the default).  Controls per-island bead emission order; see
    /// [`SlicingParams::external_perimeters_first`].
    pub external_perimeters_first: bool,
    /// Fill narrow residual cores with extra concentric perimeter loops instead
    /// of leaving them for sparse infill.  See
    /// [`SlicingParams::extra_perimeters`].
    pub extra_perimeters: bool,
    /// Widest residual core (in mm) that `extra_perimeters` will fill with
    /// loops; wider cores are left for infill.  `= extra_perimeters_max_gap ×
    /// nozzle_diameter_mm`.
    pub extra_perimeters_max_gap_mm: f64,
    /// Emit beads for **thin features** — model material too narrow for even one
    /// full perimeter.  **Classic generator only**: Arachne fills thin features
    /// from the medial axis by construction and ignores this flag.
    /// See [`SlicingParams::thin_walls`].
    pub thin_walls: bool,
}

impl WallParams {
    /// Build [`WallParams`] from the slicing-parameter bag.
    pub fn from_slicing_params(params: &SlicingParams) -> Self {
        let d = params.nozzle_diameter_mm;
        Self {
            nozzle_diameter_mm: d,
            wall_count: params.wall_count,
            wall_line_width_min_mm: params.wall_line_width_min * d,
            wall_line_width_max_mm: params.wall_line_width_max * d,
            wall_distribution_count: params.wall_distribution_count,
            gap_fill_min_length_mm: params.gap_fill_min_length_mm,
            external_perimeters_first: params.external_perimeters_first,
            extra_perimeters: params.extra_perimeters,
            extra_perimeters_max_gap_mm: params.extra_perimeters_max_gap * d,
            thin_walls: params.thin_walls,
        }
    }
}

/// A single computed extrusion bead produced by a wall generator.
pub struct Bead {
    /// Centerline path (a closed polygon offset inward from the shell boundary).
    pub path: Path,
    /// Extrusion width in mm for this bead.
    pub width_mm: f64,
    /// True if this is the outermost wall bead, false for inner walls.
    pub is_outer: bool,
}

/// Sub-phase timing breakdown for [`crate::walls::generate_walls`].
///
/// All times are the **sum of CPU time across all rayon worker threads**; they
/// will be larger than the wall-clock duration of the phase on multi-core machines.
/// The ratio of the two counters reveals where the per-island cost is concentrated.
pub struct WallTimings {
    /// Total CPU time (all threads) spent inside collapse depth calculation.
    pub collapse_depth_ms: u64,
    /// Total CPU time (all threads) spent in bead-centerline [`shrink`](super::beads::shrink) calls.
    pub bead_shrink_ms: u64,
}
