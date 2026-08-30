//! Gyroid infill pattern implementation.
//!
//! Generates a 3D mathematical surface pattern based on the gyroid minimal surface.
//! This provides excellent strength properties and isotropic behavior.
//!
//! Based on CuraEngine's gyroid implementation:
//! https://github.com/Ultimaker/CuraEngine/blob/main/src/infill/GyroidInfill.cpp

use super::utils::calculate_bounds;
use clipper2::*;
use std::f64::consts::PI;

/// Generate gyroid infill pattern.
///
/// Creates a mathematically-defined 3D surface pattern based on the gyroid
/// minimal surface. The gyroid equation is: sin(x)cos(y) + sin(y)cos(z) + sin(z)cos(x) = 0
///
/// This implementation follows Cura's algorithm which solves the gyroid equation
/// analytically to generate smooth wavey lines that follow the surface.
///
/// # Arguments
/// * `region` - The infill region boundaries
/// * `spacing` - Flow spacing of one bead in mm (`width − h·(1 − π/4)`)
/// * `density` - Infill density as a fraction (0.0-1.0)
/// * `z_height` - The Z coordinate of the current layer in mm
///
/// # Returns
/// Paths representing the gyroid surface at the current Z-height
pub fn generate_gyroid(region: &Paths, spacing: f64, density: f64, z_height: f64) -> Paths {
    if density <= 0.0 || region.is_empty() {
        return Paths::default();
    }

    let bounds = calculate_bounds(region);
    if bounds.is_none() {
        return Paths::default();
    }
    let (min_x, min_y, max_x, max_y) = bounds.unwrap();

    // libslic3r's `FillGyroid::DensityAdjust`: the sinusoidal surface packs more
    // line length into a period than a straight sweep does, so the naive
    // `spacing / density` pitch would badly over-fill. The corrected wave period
    // is `2π × spacing / (density × 2.44)` — within 7 % of the `line_distance ×
    // 2.41` Cura relation this generator was originally built on, but now driven
    // by the real bead spacing instead of a hardcoded 0.4 mm.
    const DENSITY_ADJUST: f64 = 2.44;
    let density_adjusted = (density * DENSITY_ADJUST).max(1e-6);
    let pitch_f = (2.0 * PI * spacing / density_adjusted).max(1e-3);

    // Cura works in microns and uses `while step > 500` to subdivide. We work
    // in millimeters (step ≈ 1mm), so that condition would never trigger and
    // we'd be left with only 4 sample points per pitch — producing extremely
    // jagged waves. Always force at least 16 samples per pitch, matching the
    // smoothness Cura achieves in its native units.
    let num_steps: i32 = 16;
    let step_f = pitch_f / num_steps as f64;

    // Convert Z height to radians based on pitch
    let z_rads = 2.0 * PI * z_height / pitch_f;
    let cos_z = z_rads.cos();
    let sin_z = z_rads.sin();

    let mut result = Paths::default();

    // Choose between vertical or horizontal lines based on which gives better results
    if sin_z.abs() <= cos_z.abs() {
        // Generate "vertical" lines (lines that vary more in X than Y)
        generate_vertical_lines(
            (min_x, min_y, max_x, max_y),
            pitch_f,
            step_f,
            num_steps,
            cos_z,
            sin_z,
            &mut result,
        );
    } else {
        // Generate "horizontal" lines (lines that vary more in Y than X)
        generate_horizontal_lines(
            (min_x, min_y, max_x, max_y),
            pitch_f,
            step_f,
            num_steps,
            cos_z,
            sin_z,
            &mut result,
        );
    }

    result
}

/// Generate vertical gyroid lines (varying more in X direction)
fn generate_vertical_lines(
    bounds: (f64, f64, f64, f64),
    pitch: f64,
    step: f64,
    num_steps: i32,
    cos_z: f64,
    sin_z: f64,
    result: &mut Paths,
) {
    let (min_x, min_y, max_x, max_y) = bounds;

    let phase_offset = if cos_z < 0.0 { PI } else { 0.0 } + PI;

    // Calculate X coordinates for odd and even columns
    let mut odd_line_coords = Vec::new();
    let mut even_line_coords = Vec::new();

    for i in 0..num_steps {
        let y = i as f64 * step;
        let y_rads = 2.0 * PI * y / pitch;

        let a = cos_z;
        let b = (y_rads + phase_offset).sin();
        let odd_c = sin_z * (y_rads + phase_offset).cos();
        let even_c = sin_z * (y_rads + phase_offset + PI).cos();
        let h = (a * a + b * b).sqrt();

        // Clamp asin arguments to [-1, 1] to prevent NaN from float precision
        // errors. Without this, slight overshoots produce NaN coordinates which
        // appear as weird straight lines in the output.
        let odd_x_rads = if h > f64::EPSILON {
            (odd_c / h).clamp(-1.0, 1.0).asin() + (b / h).clamp(-1.0, 1.0).asin()
        } else {
            0.0
        } - PI / 2.0;

        let even_x_rads = if h > f64::EPSILON {
            (even_c / h).clamp(-1.0, 1.0).asin() + (b / h).clamp(-1.0, 1.0).asin()
        } else {
            0.0
        } - PI / 2.0;

        odd_line_coords.push(odd_x_rads / PI * pitch);
        even_line_coords.push(even_x_rads / PI * pitch);
    }

    // Generate columns
    let mut num_columns = 0;
    let mut x = ((min_x / pitch).floor() - 2.25) * pitch;

    while x <= max_x + pitch / 2.0 {
        let mut line_points = Vec::new();
        let mut y = ((min_y / pitch).floor() - 1.0) * pitch;

        while y <= max_y + pitch {
            for i in 0..num_steps as usize {
                let x_offset = if num_columns & 1 == 1 {
                    odd_line_coords[i]
                } else {
                    even_line_coords[i]
                } / 2.0
                    + pitch;

                line_points.push((x + x_offset, y + (i as f64 * step)));
            }
            y += pitch;
        }

        if line_points.len() >= 2 {
            // Filter out any NaN/infinite points that may have slipped through;
            // these would create the "weird straight lines at strange angles".
            let valid_points: Vec<(f64, f64)> = line_points
                .into_iter()
                .filter(|(px, py)| px.is_finite() && py.is_finite())
                .collect();
            if valid_points.len() >= 2 {
                let path: Path = valid_points.into();
                result.push(path);
            }
        }

        num_columns += 1;
        x += pitch / 2.0;
    }
}

/// Generate horizontal gyroid lines (varying more in Y direction)
fn generate_horizontal_lines(
    bounds: (f64, f64, f64, f64),
    pitch: f64,
    step: f64,
    num_steps: i32,
    cos_z: f64,
    sin_z: f64,
    result: &mut Paths,
) {
    let (min_x, min_y, max_x, max_y) = bounds;

    let phase_offset = if sin_z < 0.0 { PI } else { 0.0 };

    // Calculate Y coordinates for odd and even rows
    let mut odd_line_coords = Vec::new();
    let mut even_line_coords = Vec::new();

    for i in 0..num_steps {
        let x = i as f64 * step;
        let x_rads = 2.0 * PI * x / pitch;

        let a = sin_z;
        let b = (x_rads + phase_offset).cos();
        let odd_c = cos_z * (x_rads + phase_offset + PI).sin();
        let even_c = cos_z * (x_rads + phase_offset).sin();
        let h = (a * a + b * b).sqrt();

        // Clamp asin arguments to [-1, 1] to prevent NaN from float precision
        // errors. Without this, slight overshoots produce NaN coordinates which
        // appear as weird straight lines in the output.
        let odd_y_rads = if h > f64::EPSILON {
            (odd_c / h).clamp(-1.0, 1.0).asin() + (b / h).clamp(-1.0, 1.0).asin()
        } else {
            0.0
        } + PI / 2.0;

        let even_y_rads = if h > f64::EPSILON {
            (even_c / h).clamp(-1.0, 1.0).asin() + (b / h).clamp(-1.0, 1.0).asin()
        } else {
            0.0
        } + PI / 2.0;

        odd_line_coords.push(odd_y_rads / PI * pitch);
        even_line_coords.push(even_y_rads / PI * pitch);
    }

    // Generate rows
    let mut num_rows = 0;
    let mut y = ((min_y / pitch).floor() - 1.0) * pitch;

    while y <= max_y + pitch / 2.0 {
        let mut line_points = Vec::new();
        let mut x = ((min_x / pitch).floor() - 1.0) * pitch;

        while x <= max_x + pitch {
            for i in 0..num_steps as usize {
                let y_offset = if num_rows & 1 == 1 {
                    odd_line_coords[i]
                } else {
                    even_line_coords[i]
                } / 2.0;

                line_points.push((x + (i as f64 * step), y + y_offset));
            }
            x += pitch;
        }

        if line_points.len() >= 2 {
            // Filter out any NaN/infinite points that may have slipped through;
            // these would create the "weird straight lines at strange angles".
            let valid_points: Vec<(f64, f64)> = line_points
                .into_iter()
                .filter(|(px, py)| px.is_finite() && py.is_finite())
                .collect();
            if valid_points.len() >= 2 {
                let path: Path = valid_points.into();
                result.push(path);
            }
        }

        num_rows += 1;
        y += pitch / 2.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gyroid_empty_region() {
        let region = Paths::default();
        let result = generate_gyroid(&region, 0.357, 0.2, 0.2);
        assert!(result.is_empty());
    }

    #[test]
    fn test_gyroid_zero_density() {
        let mut region = Paths::default();
        let square: Path = vec![(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)].into();
        region.push(square);

        let result = generate_gyroid(&region, 0.357, 0.0, 0.2);
        assert!(result.is_empty());
    }

    #[test]
    fn test_gyroid_generates_lines() {
        let mut region = Paths::default();
        let square: Path = vec![(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)].into();
        region.push(square);

        let result = generate_gyroid(&region, 0.357, 0.2, 0.2);
        assert!(!result.is_empty(), "Should generate gyroid pattern");

        // Verify lines have multiple points (wavy lines, not straight)
        for path in result.iter() {
            assert!(path.len() >= 2, "Each path should have at least 2 points");
        }
    }

    #[test]
    fn test_gyroid_varies_by_layer() {
        let mut region = Paths::default();
        let square: Path = vec![(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)].into();
        region.push(square);

        let result1 = generate_gyroid(&region, 0.357, 0.2, 0.2);
        let result2 = generate_gyroid(&region, 0.357, 0.2, 0.4);

        // Different Z heights should produce different patterns
        assert!(!result1.is_empty());
        assert!(!result2.is_empty());

        // The patterns should be different (different number of paths or different geometry)
        // This is a simple check - in practice the patterns will differ significantly
        let has_difference = result1.len() != result2.len()
            || result1.iter().zip(result2.iter()).any(|(p1, p2)| {
                p1.len() != p2.len()
                    || p1.iter().zip(p2.iter()).any(|(pt1, pt2)| {
                        (pt1.x() - pt2.x()).abs() > 0.01 || (pt1.y() - pt2.y()).abs() > 0.01
                    })
            });

        assert!(
            has_difference,
            "Patterns at different Z heights should differ"
        );
    }
}
