//! TPMS-D (Triply Periodic Minimal Surface - Diamond) infill pattern.
//!
//! TPMS-D is a mathematically-defined isosurface that creates a continuous,
//! organic structure with excellent strength-to-weight ratio and uniform
//! stress distribution. For 2D layer slicing, we evaluate the surface at
//! the layer's Z height and generate line segments along the contours.
//!
//! The TPMS-D surface function is:
//! `cos(x)*cos(y)*cos(z) - sin(x)*sin(y)*sin(z) = 0`
//!
//! For a given Z height, we sample this in 2D and generate lines where
//! the function crosses zero.

use super::utils::{calculate_bounds, chain_segments_into_polylines};

/// Density correction for the TPMS-D level set.
///
/// Calibrated so the pattern deposits the density it is asked for: measured on a
/// Voron cube at the default 20 %, TPMS-D lays 34.7 m of line where a
/// rectilinear fill lays 34.9 m. It is far larger than the gyroid's `2.44`
/// because one TPMS-D period contributes much less curve length to a horizontal
/// slice than one gyroid wave does.
const TPMS_D_DENSITY_ADJUST: f64 = 13.2;

/// Marching-squares samples taken across one pattern period.
const SAMPLES_PER_PERIOD: f64 = 8.0;
use clipper2::*;

/// Generate TPMS-D infill pattern.
///
/// Creates lines following the TPMS-D mathematical surface. The pattern varies
/// continuously with Z height (between layers), creating smooth 3D transitions
/// and organic, isotropic strength distribution.
///
/// # Arguments
/// * `region` - The infill region boundaries
/// * `spacing` - Flow spacing of one bead in mm (`width − h·(1 − π/4)`)
/// * `density` - Infill density as a fraction (0.0-1.0)
/// * `z_height` - Z coordinate of the layer (primary source of pattern variation)
///
/// # Returns
/// Paths containing line segments following the TPMS-D surface
pub fn generate_tpms_d(region: &Paths, spacing: f64, density: f64, z_height: f64) -> Paths {
    if density <= 0.0 || region.is_empty() {
        return Paths::default();
    }

    let bounds = calculate_bounds(region);
    if bounds.is_none() {
        return Paths::default();
    }

    let (min_x, min_y, max_x, max_y) = bounds.unwrap();

    // Pattern period, in the same `2π × spacing / (density × adjust)` form the
    // gyroid uses — TPMS-D is the same family of minimal surface, it just packs
    // a different amount of curve into a period, hence its own constant.
    //
    // See [`TPMS_D_DENSITY_ADJUST`]: the previous hand-tuned 2 mm period (at a
    // 0.4 mm bead) deposited barely a seventh of the requested density.
    const DENSITY_ADJUST: f64 = TPMS_D_DENSITY_ADJUST;
    let pattern_scale =
        (2.0 * std::f64::consts::PI * spacing / (density.max(0.1) * DENSITY_ADJUST)).max(1e-3);

    // Marching-squares resolution. Sampling several points per period is what
    // keeps the contours smooth; the old 1.5× multiplier left barely a handful
    // of samples across the whole model and produced visibly faceted waves.
    let mut resolution =
        ((max_x - min_x).max(max_y - min_y) / pattern_scale * SAMPLES_PER_PERIOD) as usize;
    resolution = (resolution as u32).clamp(10, 800) as usize;

    let mut lines = Paths::default();

    // Evaluate the TPMS-D surface on a 2D grid and generate contour lines
    let step_x = (max_x - min_x) / (resolution as f64);
    let step_y = (max_y - min_y) / (resolution as f64);

    // Store grid values for contour line generation
    let mut grid = vec![vec![0.0; resolution + 1]; resolution + 1];

    // Evaluate the TPMS-D function at each grid point
    // z_height varies the pattern naturally in 3D space
    let scale_z = z_height / pattern_scale;

    for (i, row) in grid.iter_mut().enumerate().take(resolution + 1) {
        for (j, cell) in row.iter_mut().enumerate().take(resolution + 1) {
            let x = min_x + i as f64 * step_x;
            let y = min_y + j as f64 * step_y;

            // Scale coordinates by pattern scale (no phase offset)
            let scale_x = x / pattern_scale;
            let scale_y = y / pattern_scale;

            // TPMS-D surface function - varies continuously by z_height
            *cell = tpms_d_function(scale_x, scale_y, scale_z);
        }
    }

    // Generate contour lines where the function crosses zero
    for i in 0..resolution {
        for j in 0..resolution {
            // Check the four corners of each cell
            let v00 = grid[i][j];
            let v10 = grid[i + 1][j];
            let v01 = grid[i][j + 1];
            let v11 = grid[i + 1][j + 1];

            // Find zero crossings in this cell
            let edges = [
                (v00, v10, 0), // bottom edge
                (v10, v11, 1), // right edge
                (v01, v11, 2), // top edge
                (v00, v01, 3), // left edge
            ];

            let mut crossings = Vec::new();

            for (v_a, v_b, edge_index) in edges.iter() {
                if (v_a * v_b) <= 0.0 && (v_a - v_b).abs() > 1e-10 {
                    let t = v_a / (v_a - v_b);
                    let (x, y) = match edge_index {
                        0 => (min_x + (i as f64 + t) * step_x, min_y + j as f64 * step_y),
                        1 => (
                            min_x + (i + 1) as f64 * step_x,
                            min_y + (j as f64 + t) * step_y,
                        ),
                        2 => (
                            min_x + (i as f64 + t) * step_x,
                            min_y + (j + 1) as f64 * step_y,
                        ),
                        3 => (min_x + i as f64 * step_x, min_y + (j as f64 + t) * step_y),
                        _ => (0.0, 0.0),
                    };
                    crossings.push((x, y));
                }
            }

            // Connect crossings into line segments
            if crossings.len() == 2 {
                let path: Path = crossings.into();
                lines.push(path);
            } else if crossings.len() == 4 {
                // 4 crossings: connect pairs
                let path1: Path = vec![crossings[0], crossings[1]].into();
                let path2: Path = vec![crossings[2], crossings[3]].into();
                lines.push(path1);
                lines.push(path2);
            }
        }
    }

    // The marching squares above emit one short segment per grid cell. Printed
    // as separate paths those are isolated sub-cell dabs — most of them shorter
    // than `min_infill_extrusion_mm`, so the splat filter would delete them and
    // the pattern would all but vanish. Chain them back into the curves the
    // tracer actually found.
    let tolerance = step_x.min(step_y) * 0.25;
    chain_segments_into_polylines(&lines, tolerance)
}

/// Evaluate the TPMS-D surface at a 3D point.
/// TPMS-D formula: cos(x)*cos(y)*cos(z) - sin(x)*sin(y)*sin(z)
fn tpms_d_function(x: f64, y: f64, z: f64) -> f64 {
    x.cos() * y.cos() * z.cos() - x.sin() * y.sin() * z.sin()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tpms_d_empty_region() {
        let region = Paths::default();
        let result = generate_tpms_d(&region, 0.357, 0.2, 0.0);
        assert!(result.is_empty());
    }

    #[test]
    fn test_tpms_d_zero_density() {
        let mut region = Paths::default();
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        region.push(square);
        let result = generate_tpms_d(&region, 0.357, 0.0, 0.0);
        assert!(result.is_empty());
    }

    #[test]
    fn test_tpms_d_generates_lines() {
        let mut region = Paths::default();
        let square: Path = vec![(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)].into();
        region.push(square);

        let result = generate_tpms_d(&region, 0.357, 0.3, 0.0);
        // Should generate some line segments
        assert!(!result.is_empty());
    }

    #[test]
    fn test_tpms_d_function() {
        // Test function evaluates correctly at origin
        let v = tpms_d_function(0.0, 0.0, 0.0);
        // cos(0) * cos(0) * cos(0) - sin(0) * sin(0) * sin(0) = 1 * 1 * 1 - 0 = 1
        assert!((v - 1.0).abs() < 1e-10);

        // Function should vary smoothly
        let v1 = tpms_d_function(0.1, 0.1, 0.1);
        let v2 = tpms_d_function(0.2, 0.1, 0.1);
        assert_ne!(v1, v2);
    }
}
