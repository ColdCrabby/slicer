//! Rectilinear (linear) infill pattern implementation.
//!
//! Generates parallel lines that alternate direction by 90° each layer for
//! optimal mechanical strength and minimal material usage.

use super::utils::calculate_bounds;
use super::{line_pitch_mm, sweep_pitch_mm};
use clipper2::*;

/// One set of parallel lines: the angle it runs at and how far the whole set is
/// shifted perpendicular to that angle.
///
/// Mirrors libslic3r's `SweepParams { angle, pattern_shift }`
/// (`FillRectilinear.cpp:2956`). The shift is what turns three identical sweeps
/// into stars or cubic.
#[derive(Debug, Clone, Copy)]
pub struct Sweep {
    /// Direction of the lines, in radians.
    pub angle: f64,
    /// Offset of the whole set perpendicular to `angle`, in mm.
    pub shift: f64,
}

impl Sweep {
    /// A sweep at `angle` with no phase shift.
    pub fn at(angle: f64) -> Self {
        Self { angle, shift: 0.0 }
    }
}

/// Generate rectilinear (parallel line) infill pattern.
///
/// Lines alternate direction by 90° each layer using the angle_offset.
/// This pattern is the fastest to generate and print.
///
/// # Arguments
/// * `region` - The infill region boundaries
/// * `spacing` - Flow spacing of one bead in mm (`width − h·(1 − π/4)`)
/// * `density` - Infill density as a fraction (0.0-1.0)
/// * `angle_offset` - Rotation angle in radians for this layer
///
/// # Returns
/// Paths containing parallel line segments
pub fn generate_rectilinear(
    region: &Paths,
    spacing: f64,
    density: f64,
    angle_offset: f64,
) -> Paths {
    if density <= 0.0 {
        return Paths::default();
    }
    generate_lines(
        region,
        line_pitch_mm(spacing, density),
        Sweep::at(angle_offset),
    )
}

/// Generate several sweeps of parallel lines across the same region.
///
/// libslic3r's `fill_surface_by_multilines`: the requested density is split
/// evenly across the sweeps *before* the pitch is computed
/// (`FillRectilinear.cpp:2956-2970`), so N sweeps together deposit exactly
/// `density`, not `N × density`.
pub fn generate_multiline(region: &Paths, spacing: f64, density: f64, sweeps: &[Sweep]) -> Paths {
    if density <= 0.0 || sweeps.is_empty() {
        return Paths::default();
    }
    let pitch = sweep_pitch_mm(spacing, density, sweeps.len());
    let mut lines = Paths::default();
    for sweep in sweeps {
        for path in generate_lines(region, pitch, *sweep).iter() {
            lines.push(path.clone());
        }
    }
    lines
}

/// Lay one sweep of parallel lines at `pitch` across the region's bounding box.
///
/// The lines are unclipped by design — [`super::generate_infill`] runs the
/// boolean intersection once for every pattern.
fn generate_lines(region: &Paths, pitch: f64, sweep: Sweep) -> Paths {
    let Some((min_x, min_y, max_x, max_y)) = calculate_bounds(region) else {
        return Paths::default();
    };

    let cos_a = sweep.angle.cos();
    let sin_a = sweep.angle.sin();

    let mut lines = Paths::default();
    let diagonal = ((max_x - min_x).powi(2) + (max_y - min_y).powi(2)).sqrt();
    let center_x = (min_x + max_x) / 2.0;
    let center_y = (min_y + max_y) / 2.0;

    // Anchor the phase to world coordinates (offset by the requested shift)
    // rather than to the region's own centre, so successive layers — whose
    // interior regions differ slightly — keep their infill lines stacked
    // instead of drifting a fraction of a pitch per layer.
    let half = diagonal / 2.0;
    let center_offset = center_x * cos_a + center_y * sin_a;
    let first = ((center_offset - half - sweep.shift) / pitch).ceil() * pitch + sweep.shift;

    let mut offset = first;
    while offset <= center_offset + half {
        let along = offset - center_offset;
        let line_start = (
            center_x - diagonal * sin_a + along * cos_a,
            center_y + diagonal * cos_a + along * sin_a,
        );
        let line_end = (
            center_x + diagonal * sin_a + along * cos_a,
            center_y - diagonal * cos_a + along * sin_a,
        );

        let path: Path = vec![line_start, line_end].into();
        lines.push(path);

        offset += pitch;
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPACING: f64 = 0.357;

    fn square(size: f64) -> Paths {
        let mut region = Paths::default();
        let path: Path = vec![(0.0, 0.0), (size, 0.0), (size, size), (0.0, size)].into();
        region.push(path);
        region
    }

    #[test]
    fn test_rectilinear_empty_region() {
        let region = Paths::default();
        let result = generate_rectilinear(&region, SPACING, 0.2, 0.0);
        assert!(result.is_empty());
    }

    #[test]
    fn test_rectilinear_zero_density() {
        let result = generate_rectilinear(&square(10.0), SPACING, 0.0, 0.0);
        assert!(result.is_empty());
    }

    #[test]
    fn test_rectilinear_generates_lines() {
        let result = generate_rectilinear(&square(20.0), SPACING, 0.2, 0.0);
        assert!(!result.is_empty(), "Should generate infill lines");
    }

    /// Offsets of the generated lines along the sweep axis. At `angle_offset = 0`
    /// the lines run along Y and are spaced along X.
    fn line_offsets(paths: &Paths) -> Vec<f64> {
        let mut v: Vec<f64> = paths
            .iter()
            .filter_map(|p| p.iter().next().map(|pt| pt.x()))
            .collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v
    }

    #[test]
    fn line_pitch_follows_spacing_over_density() {
        // Adjacent lines must sit `spacing / density` apart — the libslic3r
        // relation that makes deposited material match the requested density.
        let xs = line_offsets(&generate_rectilinear(&square(50.0), 0.4, 0.25, 0.0));
        assert!(xs.len() > 3, "expected several lines, got {}", xs.len());
        let gap = xs[2] - xs[1];
        assert!(
            (gap - 1.6).abs() < 1e-6,
            "expected 0.4/0.25 = 1.6 mm pitch, got {gap}"
        );
    }

    #[test]
    fn multiline_splits_density_across_sweeps() {
        // Two sweeps at the same total density must each be half as dense as a
        // single sweep — otherwise a grid deposits 2× the requested density.
        let single = generate_rectilinear(&square(50.0), 0.4, 0.25, 0.0);
        let double = generate_multiline(
            &square(50.0),
            0.4,
            0.25,
            &[Sweep::at(0.0), Sweep::at(std::f64::consts::FRAC_PI_2)],
        );
        assert!(
            double.len() <= single.len() + 2,
            "two half-density sweeps ({}) should total about one full sweep ({})",
            double.len(),
            single.len()
        );
    }

    #[test]
    fn line_phase_is_anchored_to_world_coordinates() {
        // Two regions with different centres must produce collinear lines, so
        // successive layers stack instead of drifting a fraction of a pitch.
        let mut wide = Paths::default();
        let a: Path = vec![(0.0, 0.0), (50.0, 0.0), (50.0, 50.0), (0.0, 50.0)].into();
        wide.push(a);
        let mut narrow = Paths::default();
        let b: Path = vec![(3.0, 3.0), (47.0, 3.0), (47.0, 47.0), (3.0, 47.0)].into();
        narrow.push(b);

        let inner = |paths: &Paths| -> Vec<f64> {
            line_offsets(paths)
                .into_iter()
                .filter(|x| (10.0..40.0).contains(x))
                .collect()
        };

        let from_wide = inner(&generate_rectilinear(&wide, 0.4, 0.25, 0.0));
        let from_narrow = inner(&generate_rectilinear(&narrow, 0.4, 0.25, 0.0));
        assert_eq!(from_wide.len(), from_narrow.len());
        for (a, b) in from_wide.iter().zip(from_narrow.iter()) {
            assert!((a - b).abs() < 1e-6, "lines drifted: {a} vs {b}");
        }
    }
}
