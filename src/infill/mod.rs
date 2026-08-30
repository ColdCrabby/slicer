//! Infill pattern generation for 3D printing.
//!
//! This module provides functions to generate various infill patterns within closed
//! perimeter regions. Infill provides internal structure and strength while minimizing
//! material usage.
//!
//! # Pattern Types
//!
//! - **Rectilinear**: Parallel lines alternating direction per layer (fastest)
//! - **Grid**: Perpendicular lines forming a grid pattern (stronger)
//! - **Honeycomb**: Hexagonal cells (good strength-to-weight ratio)
//! - **Gyroid**: 3D mathematical pattern (best strength, isotropic)
//! - **TpmsD**: Triply Periodic Minimal Surface - Diamond (organic, isotropic structure)
//!
//! # Usage
//!
//! ```rust,no_run
//! use slicer_engine::infill::{generate_infill, FillParams, InfillPattern};
//! use clipper2::Paths;
//!
//! let perimeter_paths = Paths::default(); // from slice_mesh
//! let infill_paths = generate_infill(
//!     &perimeter_paths,
//!     &FillParams {
//!         pattern: InfillPattern::TpmsD,
//!         density: 0.2,           // 20% density
//!         spacing_mm: 0.357,      // flow spacing of one bead
//!         angle_offset: 0.0,      // layer rotation
//!         z_height: 0.2,          // Z height in mm
//!     },
//! );
//! ```

use clipper2::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

mod anchor;
mod concentric;
mod grid;
mod gyroid;
mod honeycomb;
mod rectilinear;
mod tpms_d;
mod utils;

pub(crate) use anchor::connect_infill;
pub(crate) use concentric::generate_concentric;
use grid::generate_grid;
use gyroid::generate_gyroid;
use honeycomb::generate_honeycomb;
use rectilinear::{generate_multiline, generate_rectilinear, Sweep};
use tpms_d::generate_tpms_d;
use utils::clip_lines_to_region;

/// Supported infill patterns.
///
/// Serialised with the variant names as-is (`"Rectilinear"`, `"TpmsD"`, …) —
/// that is the shape already stored in saved profiles and written by the UI, so
/// it must not be renamed. `parse` is what accepts the human/Orca spellings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub enum InfillPattern {
    /// Parallel lines alternating direction per layer (default, fastest).
    #[default]
    Rectilinear,
    /// Parallel lines that keep the same angle on every layer.
    AlignedRectilinear,
    /// Perpendicular lines forming a grid pattern (stronger).
    Grid,
    /// Three line sets 60° apart (stronger than grid, same material).
    Triangles,
    /// Triangles with every third set offset half a pitch, forming stars.
    TriHexagon,
    /// Three line sets whose phase shifts with Z, forming stacked cubes.
    Cubic,
    /// Hexagonal cells (good strength-to-weight ratio).
    Honeycomb,
    /// Loops following the region outline.
    Concentric,
    /// 3D mathematical pattern (experimental, best strength).
    Gyroid,
    /// Triply Periodic Minimal Surface - Diamond (organic, isotropic structure).
    TpmsD,
}

impl InfillPattern {
    /// Parse pattern name from string (case-insensitive).
    ///
    /// Accepts OrcaSlicer's spellings alongside our own so an imported profile
    /// maps without a translation table.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().replace('_', "-").as_str() {
            "rectilinear" | "linear" | "line" => Some(Self::Rectilinear),
            "aligned-rectilinear" | "alignedrectilinear" => Some(Self::AlignedRectilinear),
            "grid" => Some(Self::Grid),
            "triangles" => Some(Self::Triangles),
            "tri-hexagon" | "trihexagon" | "stars" => Some(Self::TriHexagon),
            "cubic" => Some(Self::Cubic),
            "honeycomb" | "hexagonal" => Some(Self::Honeycomb),
            "concentric" => Some(Self::Concentric),
            "gyroid" => Some(Self::Gyroid),
            "tpms-d" | "tpmsd" => Some(Self::TpmsD),
            _ => None,
        }
    }

    /// Get the canonical name of this pattern.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Rectilinear => "rectilinear",
            Self::AlignedRectilinear => "aligned-rectilinear",
            Self::Grid => "grid",
            Self::Triangles => "triangles",
            Self::TriHexagon => "tri-hexagon",
            Self::Cubic => "cubic",
            Self::Honeycomb => "honeycomb",
            Self::Concentric => "concentric",
            Self::Gyroid => "gyroid",
            Self::TpmsD => "tpms-d",
        }
    }

    /// Whether the fill angle alternates by 90° between layers.
    ///
    /// **Only single-sweep rectilinear does.** Alternation exists so that
    /// consecutive layers of parallel lines cross instead of stacking into
    /// unsupported walls — a question that only arises when the layer draws
    /// lines in *one* direction.
    ///
    /// Every other pattern is harmed by it, which is why libslic3r's
    /// multi-sweep and cellular fills all override `_layer_angle` to `0`:
    ///
    /// - `Honeycomb` is a **cellular** pattern. Its walls have to stack layer
    ///   over layer to form tubes; rotating the lattice 90° drops each layer's
    ///   walls onto the previous layer's voids, so nothing stacks and the walls
    ///   print over air.
    /// - `Triangles` / `TriHexagon` / `Cubic` already sweep three directions, so
    ///   rotating only misregisters the lattice against the layer below.
    /// - `Grid` sweeps 0° and 90°, so a 90° rotation maps it onto itself.
    /// - `AlignedRectilinear` opts out by definition.
    /// - `Concentric`, `Gyroid` and `TpmsD` ignore the angle entirely.
    pub fn alternates_per_layer(&self) -> bool {
        matches!(self, Self::Rectilinear)
    }

    /// Whether the generated paths still need clipping to the region.
    ///
    /// Line-based patterns are drawn across the whole bounding box and clipped
    /// afterwards; concentric loops are produced *by* offsetting the region
    /// inward, so they are inside it by construction and clipping would only
    /// fragment them at the boundary tolerance.
    fn needs_clipping(&self) -> bool {
        !matches!(self, Self::Concentric)
    }
}

/// Floor on the bead spacing handed to a pattern, so a degenerate configuration
/// can never divide by zero or spin the scanline forever.
const MIN_SPACING_MM: f64 = 0.01;

/// Fill pattern for a **solid** region — top surface, bottom surface, or the
/// internal solid layers `solid_infill_every_layers` inserts.
///
/// Solid fill is always 100 % dense, so unlike [`InfillPattern`] these variants
/// differ only in the *order and connectivity* of the lines, which is what
/// decides how a visible surface looks. Mirrors PrusaSlicer's `top_fill_pattern`
/// / `bottom_fill_pattern` and OrcaSlicer's `top_surface_pattern` /
/// `bottom_surface_pattern`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SurfacePattern {
    /// Back-and-forth serpentine: every second line is drawn in reverse.
    Rectilinear,
    /// Serpentine that keeps a constant angle on every layer instead of
    /// cross-hatching (PrusaSlicer/Orca `alignedrectilinear`).
    AlignedRectilinear,
    /// One-way sweep with the line ends joined along the region boundary.
    #[default]
    Monotonic,
    /// One-way sweep with the lines left separate (Orca `monotonicline`,
    /// PrusaSlicer `monotoniclines`).
    MonotonicLine,
    /// Loops following the region outline, stepping inward one bead at a time.
    Concentric,
}

impl SurfacePattern {
    /// Parse a pattern name, accepting both the PrusaSlicer and OrcaSlicer
    /// spellings so an imported profile maps cleanly either way.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().replace(['_', ' '], "-").as_str() {
            "rectilinear" | "line" | "linear" => Some(Self::Rectilinear),
            "aligned-rectilinear" | "alignedrectilinear" => Some(Self::AlignedRectilinear),
            "monotonic" => Some(Self::Monotonic),
            // Orca spells it "monotonicline", PrusaSlicer "monotoniclines".
            "monotonic-line" | "monotonicline" | "monotonic-lines" | "monotoniclines" => {
                Some(Self::MonotonicLine)
            }
            "concentric" => Some(Self::Concentric),
            _ => None,
        }
    }

    /// Canonical name of this pattern.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Rectilinear => "rectilinear",
            Self::AlignedRectilinear => "aligned-rectilinear",
            Self::Monotonic => "monotonic",
            Self::MonotonicLine => "monotonic-line",
            Self::Concentric => "concentric",
        }
    }

    /// Whether the fill lines are all drawn in the same direction.
    ///
    /// This is what removes the direction-dependent sheen a serpentine leaves on
    /// a visible top surface — the nozzle never travels back across a line it
    /// just laid.
    pub fn is_monotonic(&self) -> bool {
        matches!(self, Self::Monotonic | Self::MonotonicLine)
    }

    /// Whether consecutive fill lines may be joined by a connector.
    ///
    /// `MonotonicLine` is exactly `Monotonic` with connection disabled
    /// (libslic3r sets `anchor_length_max = 0`, `FillRectilinear.cpp:3006-3014`).
    pub fn connects_lines(&self) -> bool {
        !matches!(self, Self::MonotonicLine)
    }

    /// Whether the fill angle alternates by 90° between layers.
    ///
    /// Aligned rectilinear deliberately does not, so the fill direction is
    /// identical on every layer.
    pub fn alternates_per_layer(&self) -> bool {
        !matches!(self, Self::AlignedRectilinear)
    }
}

/// Everything a pattern generator needs besides the boundary itself.
///
/// `spacing_mm` is the **flow spacing** of one infill bead —
/// `width − layer_height × (1 − π/4)`, libslic3r's `Flow::spacing()` — not the
/// nozzle diameter and not the nominal bead width. Resolve it from
/// `core::sparse_infill_nominal_width_mm` + `core::extrusion_flow_spacing_mm`
/// so the pitch a pattern lays its lines at and the flow the G-code generator
/// charges for them come from the same number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FillParams {
    /// Pattern geometry to generate.
    pub pattern: InfillPattern,
    /// Fraction of the cross-section to fill, `0.0`–`1.0`.
    pub density: f64,
    /// Flow spacing of a single bead in mm (see the struct docs).
    pub spacing_mm: f64,
    /// Layer rotation in radians, for the 2D patterns that alternate.
    pub angle_offset: f64,
    /// Z coordinate of the layer in mm, for the 3D patterns.
    pub z_height: f64,
}

/// Center-to-center pitch (mm) of one sweep of parallel fill lines.
///
/// The universal libslic3r relation (`FillRectilinear.cpp:2778`):
///
/// ```text
/// line_spacing = spacing / density
/// ```
///
/// so the deposited material per unit area is `spacing / line_spacing = density`.
/// Patterns that lay **several** sweeps across the same region divide the density
/// by the sweep count first (`fill_surface_by_multilines`,
/// `FillRectilinear.cpp:2956-2970`) — see [`sweep_pitch_mm`].
pub(crate) fn line_pitch_mm(spacing_mm: f64, density: f64) -> f64 {
    (spacing_mm / density.clamp(1e-6, 1.0)).max(MIN_SPACING_MM)
}

/// Pitch of one sweep when `sweeps` sets of parallel lines share the region.
///
/// Each sweep only carries `density / sweeps` of the fill, so its lines sit
/// `sweeps × spacing / density` apart. Without this a grid deposits twice the
/// requested density, and triangles three times.
pub(crate) fn sweep_pitch_mm(spacing_mm: f64, density: f64, sweeps: usize) -> f64 {
    line_pitch_mm(spacing_mm, density / sweeps.max(1) as f64)
}

/// Generate infill paths within the given perimeter regions.
///
/// # Arguments
/// * `perimeters` - Closed contour paths defining the boundaries
/// * `fill` - Pattern, density, bead spacing, layer rotation and Z height
///
/// # Returns
/// A `Paths` collection containing the infill line segments clipped to the perimeter regions.
/// Returns empty paths if density is zero or perimeters are empty.
pub fn generate_infill(perimeters: &Paths, fill: &FillParams) -> Paths {
    // Early exit for no infill or invalid density
    if fill.density <= 0.0 || perimeters.is_empty() {
        return Paths::default();
    }

    let density = fill.density.clamp(0.0, 1.0);
    let spacing = fill.spacing_mm.max(MIN_SPACING_MM);
    let angle_offset = fill.angle_offset;

    // `perimeters` is already the correctly-bounded interior region produced by
    // `calculate_interior_region` in `add_infill_to_layers`.  Do NOT apply an
    // additional inward offset here: the caller has already placed the boundary
    // at the inner edge of the innermost wall (accounting for all wall beads
    // and the configured infill-overlap percentage).  A second inward deflation
    // was causing a double-inset that collapsed the infill region entirely on
    // features narrower than ~2× the extra offset, producing the "missing
    // infill on many layers" artifact visible on complex geometry (e.g. the
    // 3DBenchy cabin and chimney transition layers).
    let raw_lines = match fill.pattern {
        InfillPattern::Rectilinear | InfillPattern::AlignedRectilinear => {
            generate_rectilinear(perimeters, spacing, density, angle_offset)
        }
        InfillPattern::Grid => generate_grid(perimeters, spacing, density, angle_offset),
        InfillPattern::Triangles => generate_multiline(
            perimeters,
            spacing,
            density,
            &triangle_sweeps(angle_offset, 0.0),
        ),
        InfillPattern::TriHexagon => {
            // Every third sweep is offset half its own pitch, which is what turns
            // three overlaid triangle grids into the star pattern
            // (`FillStars`, `FillRectilinear.cpp:3039-3047`).
            let shift = 1.5 * spacing / density.clamp(1e-6, 1.0);
            generate_multiline(
                perimeters,
                spacing,
                density,
                &triangle_sweeps(angle_offset, shift),
            )
        }
        InfillPattern::Cubic => {
            // The per-sweep phase walks with Z, so the three grids interlock into
            // stacked cubes instead of stacking as flat triangles
            // (`FillCubic`, `FillRectilinear.cpp:3050-3059`).
            let dx = 0.5_f64.sqrt() * fill.z_height;
            generate_multiline(
                perimeters,
                spacing,
                density,
                &[
                    Sweep {
                        angle: angle_offset,
                        shift: dx,
                    },
                    Sweep {
                        angle: angle_offset + std::f64::consts::FRAC_PI_3,
                        shift: -dx,
                    },
                    Sweep {
                        angle: angle_offset + 2.0 * std::f64::consts::FRAC_PI_3,
                        shift: dx,
                    },
                ],
            )
        }
        InfillPattern::Honeycomb => generate_honeycomb(perimeters, spacing, density, angle_offset),
        InfillPattern::Concentric => generate_concentric(perimeters, spacing, density, 0.0),
        InfillPattern::Gyroid => generate_gyroid(perimeters, spacing, density, fill.z_height),
        InfillPattern::TpmsD => generate_tpms_d(perimeters, spacing, density, fill.z_height),
    };

    if !fill.pattern.needs_clipping() {
        return raw_lines;
    }
    // Clip the generated lines to the infill region boundaries
    clip_lines_to_region(&raw_lines, perimeters)
}

/// Three line sets 60° apart, optionally phase-shifting the third.
fn triangle_sweeps(angle: f64, third_shift: f64) -> [Sweep; 3] {
    [
        Sweep::at(angle),
        Sweep::at(angle + std::f64::consts::FRAC_PI_3),
        Sweep {
            angle: angle + 2.0 * std::f64::consts::FRAC_PI_3,
            shift: third_shift,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infill_pattern_default() {
        assert_eq!(InfillPattern::default(), InfillPattern::Rectilinear);
    }

    #[test]
    fn test_infill_pattern_from_str() {
        assert_eq!(
            InfillPattern::parse("rectilinear"),
            Some(InfillPattern::Rectilinear)
        );
        assert_eq!(
            InfillPattern::parse("linear"),
            Some(InfillPattern::Rectilinear)
        );
        assert_eq!(InfillPattern::parse("grid"), Some(InfillPattern::Grid));
        assert_eq!(InfillPattern::parse("GRID"), Some(InfillPattern::Grid));
        assert_eq!(
            InfillPattern::parse("honeycomb"),
            Some(InfillPattern::Honeycomb)
        );
        assert_eq!(InfillPattern::parse("gyroid"), Some(InfillPattern::Gyroid));
        assert_eq!(InfillPattern::parse("tpms-d"), Some(InfillPattern::TpmsD));
        assert_eq!(InfillPattern::parse("tpmsd"), Some(InfillPattern::TpmsD));
        assert_eq!(InfillPattern::parse("invalid"), None);
    }

    /// Total extruded length of a fill, in mm.
    fn total_length(paths: &Paths) -> f64 {
        paths
            .iter()
            .map(|p| {
                let pts: Vec<(f64, f64)> = p.iter().map(|v| (v.x(), v.y())).collect();
                pts.windows(2)
                    .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
                    .sum::<f64>()
            })
            .sum()
    }

    #[test]
    fn every_line_pattern_deposits_the_density_it_is_asked_for() {
        // The invariant the whole spacing rework exists to guarantee: line
        // length per unit area is `density / spacing`, whatever the pattern.
        // A pattern that lays several sweeps must split the density between
        // them — an earlier grid ran two full-density passes and deposited
        // double.
        let size = 80.0;
        let square: Path = vec![(0.0, 0.0), (size, 0.0), (size, size), (0.0, size)].into();
        let region = Paths::new(vec![square]);
        let spacing = 0.4;
        let density = 0.2;
        let expected = density / spacing * size * size;

        for pattern in [
            InfillPattern::Rectilinear,
            InfillPattern::AlignedRectilinear,
            InfillPattern::Grid,
            InfillPattern::Triangles,
            InfillPattern::TriHexagon,
            InfillPattern::Cubic,
            InfillPattern::Honeycomb,
        ] {
            let fill = generate_infill(
                &region,
                &FillParams {
                    pattern,
                    density,
                    spacing_mm: spacing,
                    angle_offset: 0.0,
                    z_height: 4.0,
                },
            );
            let observed = total_length(&fill);
            let ratio = observed / expected;
            assert!(
                (0.9..1.1).contains(&ratio),
                "{pattern:?} deposited {observed:.0} mm where {expected:.0} mm was asked for ({ratio:.2}×)"
            );
        }
    }

    #[test]
    fn concentric_loops_survive_unclipped() {
        // Concentric loops are produced *by* offsetting the region inward, so
        // they are inside it by construction; running them through the line
        // clipper would only fragment them at the boundary tolerance.
        let square: Path = vec![(0.0, 0.0), (40.0, 0.0), (40.0, 40.0), (0.0, 40.0)].into();
        let region = Paths::new(vec![square]);
        let fill = generate_infill(
            &region,
            &FillParams {
                pattern: InfillPattern::Concentric,
                density: 0.2,
                spacing_mm: 0.4,
                angle_offset: 0.0,
                z_height: 0.2,
            },
        );
        assert!(!fill.is_empty());
        assert!(
            fill.iter().all(|p| p.len() > 4),
            "loops must stay whole, not be chopped into segments"
        );
    }

    #[test]
    fn only_single_sweep_rectilinear_alternates_per_layer() {
        // Alternation exists so consecutive layers of *parallel* lines cross.
        // Applying it to a cellular or multi-sweep pattern is actively harmful:
        // a 90° flip drops honeycomb's walls onto the previous layer's voids,
        // so nothing stacks into a tube.
        assert!(InfillPattern::Rectilinear.alternates_per_layer());
        for pattern in [
            InfillPattern::AlignedRectilinear,
            InfillPattern::Grid,
            InfillPattern::Triangles,
            InfillPattern::TriHexagon,
            InfillPattern::Cubic,
            InfillPattern::Honeycomb,
            InfillPattern::Concentric,
            InfillPattern::Gyroid,
            InfillPattern::TpmsD,
        ] {
            assert!(
                !pattern.alternates_per_layer(),
                "{pattern:?} must keep one orientation across layers"
            );
        }
    }

    #[test]
    fn cellular_patterns_repeat_identically_across_layers() {
        // The same region at two heights must give honeycomb the same lattice —
        // that identity is what turns its walls into vertical tubes.
        let square: Path = vec![(0.0, 0.0), (40.0, 0.0), (40.0, 40.0), (0.0, 40.0)].into();
        let region = Paths::new(vec![square]);
        let at = |z: f64| -> Vec<(i64, i64)> {
            generate_infill(
                &region,
                &FillParams {
                    pattern: InfillPattern::Honeycomb,
                    density: 0.2,
                    spacing_mm: 0.4,
                    angle_offset: 0.0,
                    z_height: z,
                },
            )
            .iter()
            .flat_map(|p| {
                p.iter()
                    .map(|v| {
                        (
                            (v.x() * 100.0).round() as i64,
                            (v.y() * 100.0).round() as i64,
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
        };
        assert_eq!(at(0.2), at(0.4), "honeycomb cells must stack");
    }

    #[test]
    fn infill_pattern_parses_orca_spellings() {
        assert_eq!(
            InfillPattern::parse("alignedrectilinear"),
            Some(InfillPattern::AlignedRectilinear)
        );
        assert_eq!(
            InfillPattern::parse("tri-hexagon"),
            Some(InfillPattern::TriHexagon)
        );
        assert_eq!(InfillPattern::parse("cubic"), Some(InfillPattern::Cubic));
        assert_eq!(
            InfillPattern::parse("concentric"),
            Some(InfillPattern::Concentric)
        );
        // Only aligned rectilinear opts out of the per-layer cross-hatch.
        assert!(!InfillPattern::AlignedRectilinear.alternates_per_layer());
        assert!(InfillPattern::Rectilinear.alternates_per_layer());
    }

    #[test]
    fn infill_pattern_wire_format_is_unchanged() {
        // Saved profiles and the UI write the variant names verbatim; renaming
        // them would silently fail to deserialize every stored profile.
        assert_eq!(
            serde_json::to_string(&InfillPattern::Gyroid).unwrap(),
            "\"Gyroid\""
        );
        assert_eq!(
            serde_json::from_str::<InfillPattern>("\"Rectilinear\"").unwrap(),
            InfillPattern::Rectilinear
        );
    }

    #[test]
    fn surface_pattern_parses_both_slicer_spellings() {
        // PrusaSlicer writes "monotoniclines", OrcaSlicer "monotonicline"; an
        // imported profile must map cleanly either way.
        for spelling in ["monotonicline", "monotoniclines", "monotonic-line"] {
            assert_eq!(
                SurfacePattern::parse(spelling),
                Some(SurfacePattern::MonotonicLine),
                "{spelling}"
            );
        }
        assert_eq!(
            SurfacePattern::parse("monotonic"),
            Some(SurfacePattern::Monotonic)
        );
        assert_eq!(
            SurfacePattern::parse("alignedrectilinear"),
            Some(SurfacePattern::AlignedRectilinear)
        );
        assert_eq!(
            SurfacePattern::parse("Concentric"),
            Some(SurfacePattern::Concentric)
        );
        assert_eq!(SurfacePattern::parse("gyroid"), None);
    }

    #[test]
    fn surface_pattern_traits_match_libslic3r() {
        // Monotonic-line is monotonic with connection switched off — the single
        // difference libslic3r encodes as `anchor_length_max = 0`.
        assert!(SurfacePattern::Monotonic.is_monotonic());
        assert!(SurfacePattern::MonotonicLine.is_monotonic());
        assert!(SurfacePattern::Monotonic.connects_lines());
        assert!(!SurfacePattern::MonotonicLine.connects_lines());
        assert!(!SurfacePattern::Rectilinear.is_monotonic());
        // Aligned rectilinear is the one pattern that keeps its angle per layer.
        assert!(!SurfacePattern::AlignedRectilinear.alternates_per_layer());
        assert!(SurfacePattern::Rectilinear.alternates_per_layer());
    }

    #[test]
    fn test_infill_pattern_name() {
        assert_eq!(InfillPattern::Rectilinear.name(), "rectilinear");
        assert_eq!(InfillPattern::Grid.name(), "grid");
        assert_eq!(InfillPattern::Honeycomb.name(), "honeycomb");
        assert_eq!(InfillPattern::Gyroid.name(), "gyroid");
        assert_eq!(InfillPattern::TpmsD.name(), "tpms-d");
    }

    #[test]
    fn test_generate_infill_empty_perimeters() {
        let perimeters = Paths::default();
        let infill = generate_infill(
            &perimeters,
            &FillParams {
                pattern: InfillPattern::Rectilinear,
                density: 0.2,
                spacing_mm: 0.357,
                angle_offset: 0.0,
                z_height: 0.2,
            },
        );
        assert!(infill.is_empty());
    }

    #[test]
    fn test_generate_infill_zero_density() {
        let mut perimeters = Paths::default();
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        perimeters.push(square);

        let infill = generate_infill(
            &perimeters,
            &FillParams {
                pattern: InfillPattern::Rectilinear,
                density: 0.0,
                spacing_mm: 0.357,
                angle_offset: 0.0,
                z_height: 0.2,
            },
        );
        assert!(infill.is_empty());
    }

    #[test]
    fn test_generate_infill_rectilinear_basic() {
        let mut perimeters = Paths::default();
        // Use a larger square to ensure there's space for infill after offset
        let square: Path = vec![(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)].into();
        perimeters.push(square);

        let infill = generate_infill(
            &perimeters,
            &FillParams {
                pattern: InfillPattern::Rectilinear,
                density: 0.2,
                spacing_mm: 0.357,
                angle_offset: 0.0,
                z_height: 0.2,
            },
        );

        // Should generate some infill lines (non-empty)
        assert!(!infill.is_empty(), "Expected infill lines to be generated");
    }

    #[test]
    fn test_generate_infill_honeycomb_basic() {
        let mut perimeters = Paths::default();
        let square: Path = vec![(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)].into();
        perimeters.push(square);

        let infill = generate_infill(
            &perimeters,
            &FillParams {
                pattern: InfillPattern::Honeycomb,
                density: 0.2,
                spacing_mm: 0.357,
                angle_offset: 0.0,
                z_height: 0.2,
            },
        );

        // Should generate honeycomb pattern
        assert!(
            !infill.is_empty(),
            "Expected honeycomb infill to be generated"
        );
    }

    #[test]
    fn test_generate_infill_gyroid_basic() {
        let mut perimeters = Paths::default();
        let square: Path = vec![(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)].into();
        perimeters.push(square);

        let infill = generate_infill(
            &perimeters,
            &FillParams {
                pattern: InfillPattern::Gyroid,
                density: 0.2,
                spacing_mm: 0.357,
                angle_offset: 0.0,
                z_height: 0.2,
            },
        );

        // Should generate gyroid pattern
        assert!(!infill.is_empty(), "Expected gyroid infill to be generated");
    }

    #[test]
    fn test_generate_infill_tpms_d_basic() {
        let mut perimeters = Paths::default();
        let square: Path = vec![(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)].into();
        perimeters.push(square);

        let infill = generate_infill(
            &perimeters,
            &FillParams {
                pattern: InfillPattern::TpmsD,
                density: 0.2,
                spacing_mm: 0.357,
                angle_offset: 0.0,
                z_height: 0.2,
            },
        );

        // Should generate tpms-d pattern
        assert!(!infill.is_empty(), "Expected tpms-d infill to be generated");
    }
}
