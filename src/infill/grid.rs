//! Grid infill pattern implementation.
//!
//! Generates perpendicular lines in both directions, creating a grid pattern
//! for improved strength in all directions.

use super::rectilinear::{generate_multiline, Sweep};
use clipper2::*;

/// Generate grid infill pattern (perpendicular lines).
///
/// Creates two sets of parallel lines at 90° to each other, forming a grid.
/// This pattern provides better strength than rectilinear at the same density.
///
/// The two sweeps each carry **half** the requested density
/// (libslic3r `fill_surface_by_multilines`, `FillRectilinear.cpp:2956-2970`), so
/// a grid at 20 % deposits 20 % — not the 40 % an earlier implementation laid by
/// running two full-density sweeps.
///
/// # Arguments
/// * `region` - The infill region boundaries
/// * `spacing` - Flow spacing of one bead in mm (`width − h·(1 − π/4)`)
/// * `density` - Infill density as a fraction (0.0-1.0)
/// * `angle_offset` - Rotation angle in radians for the first set of lines
///
/// # Returns
/// Paths containing perpendicular line segments forming a grid
pub fn generate_grid(region: &Paths, spacing: f64, density: f64, angle_offset: f64) -> Paths {
    generate_multiline(
        region,
        spacing,
        density,
        &[
            Sweep::at(angle_offset),
            Sweep::at(angle_offset + std::f64::consts::FRAC_PI_2),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::super::rectilinear::generate_rectilinear;
    use super::*;

    const SPACING: f64 = 0.357;

    fn square(size: f64) -> Paths {
        let mut region = Paths::default();
        let path: Path = vec![(0.0, 0.0), (size, 0.0), (size, size), (0.0, size)].into();
        region.push(path);
        region
    }

    #[test]
    fn test_grid_empty_region() {
        let region = Paths::default();
        let result = generate_grid(&region, SPACING, 0.2, 0.0);
        assert!(result.is_empty());
    }

    #[test]
    fn grid_deposits_the_requested_density_not_double() {
        // Total grid line length must match a single rectilinear sweep at the
        // same density: two perpendicular half-density sweeps, not two full ones.
        let region = square(40.0);
        let rect = generate_rectilinear(&region, SPACING, 0.2, 0.0);
        let grid = generate_grid(&region, SPACING, 0.2, 0.0);
        let spread = grid.len() as i64 - rect.len() as i64;
        assert!(
            spread.abs() <= 2,
            "grid ({}) should lay about as many lines as rectilinear ({})",
            grid.len(),
            rect.len()
        );
    }

    #[test]
    fn grid_lines_run_in_two_directions() {
        let grid = generate_grid(&square(40.0), SPACING, 0.2, 0.0);
        let mut horizontal = 0;
        let mut vertical = 0;
        for path in grid.iter() {
            let pts: Vec<_> = path.iter().collect();
            if pts.len() < 2 {
                continue;
            }
            if (pts[0].y() - pts[1].y()).abs() < 1e-6 {
                horizontal += 1;
            } else if (pts[0].x() - pts[1].x()).abs() < 1e-6 {
                vertical += 1;
            }
        }
        assert!(horizontal > 0 && vertical > 0, "expected both directions");
    }
}
