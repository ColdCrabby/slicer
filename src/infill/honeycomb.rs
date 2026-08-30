//! Honeycomb (hexagonal) infill pattern implementation.
//!
//! Generates a true hexagonal tiling — every cell wall drawn exactly once —
//! following libslic3r's `FillHoneycomb` cell relations.

use super::line_pitch_mm;
use super::utils::calculate_bounds;
use clipper2::*;

/// Generate honeycomb (hexagonal) infill pattern.
///
/// The lattice is built from libslic3r's `FillHoneycomb` relations:
///
/// ```text
/// distance = spacing / density          // column pitch
/// hex_side = distance / (√3 / 2)        // cell side length
/// hex_width = 2 × distance = √3 × side  // vertex-to-vertex across the flats
/// ```
///
/// which is exactly the geometry that makes the deposited material equal
/// `density`: a hexagon of side `a` owns three walls of length `a` (each shared
/// with a neighbour) over an area of `(3√3/2)·a²`, so line length per unit area
/// is `2/(√3·a)` — and with `a = 2·spacing/(√3·density)` that comes to
/// `density / spacing`.
///
/// The tiling is emitted as **continuous zig-zag polylines** plus the vertical
/// walls between them, so every wall is extruded once. The previous
/// implementation stamped whole 6-edge hexagons on an inconsistent lattice,
/// which drew shared walls twice and used an ad-hoc cell size unrelated to the
/// bead width.
///
/// # Arguments
/// * `region` - The infill region boundaries
/// * `spacing` - Flow spacing of one bead in mm (`width − h·(1 − π/4)`)
/// * `density` - Infill density as a fraction (0.0-1.0)
/// * `angle_offset` - Rotation angle in radians for this layer
///
/// # Returns
/// Paths containing the hexagonal cell walls
pub fn generate_honeycomb(region: &Paths, spacing: f64, density: f64, angle_offset: f64) -> Paths {
    if density <= 0.0 || region.is_empty() {
        return Paths::default();
    }

    let Some((min_x, min_y, max_x, max_y)) = calculate_bounds(region) else {
        return Paths::default();
    };

    // libslic3r cell relations.
    let distance = line_pitch_mm(spacing, density);
    let hex_side = distance / (3.0_f64.sqrt() / 2.0);
    let hex_width = 2.0 * distance; // == √3 × hex_side
    let row_pitch = 1.5 * hex_side;

    let cos_a = angle_offset.cos();
    let sin_a = angle_offset.sin();

    // The lattice lives in a pattern space that is the world rotated about the
    // **origin** — not about the region's centre.  Anchoring to world
    // coordinates is what makes the cells of every layer land on top of one
    // another; keying the phase to the region's bounding box instead let the
    // lattice slide by a fraction of a cell whenever the interior changed shape,
    // so the walls never stacked into tubes.
    let place = |x: f64, y: f64| -> (f64, f64) { (x * cos_a - y * sin_a, x * sin_a + y * cos_a) };
    let to_pattern =
        |x: f64, y: f64| -> (f64, f64) { (x * cos_a + y * sin_a, -x * sin_a + y * cos_a) };

    // Pointy-top hexagons: centres at (i·hex_width + row_offset, j·row_pitch),
    // odd rows shifted half a cell. Vertices sit at (0, ±side) and
    // (±hex_width/2, ±side/2).
    let half_w = hex_width / 2.0;
    let half_s = hex_side / 2.0;

    // Cover the region's bounding box in pattern space: rotate its four corners
    // and take their extent, padded by one cell so partial cells at the edge are
    // still drawn.
    let corners = [
        to_pattern(min_x, min_y),
        to_pattern(max_x, min_y),
        to_pattern(max_x, max_y),
        to_pattern(min_x, max_y),
    ];
    let px_min = corners.iter().map(|c| c.0).fold(f64::INFINITY, f64::min);
    let px_max = corners
        .iter()
        .map(|c| c.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let py_min = corners.iter().map(|c| c.1).fold(f64::INFINITY, f64::min);
    let py_max = corners
        .iter()
        .map(|c| c.1)
        .fold(f64::NEG_INFINITY, f64::max);

    let j_min = ((py_min - hex_side) / row_pitch).floor() as i64;
    let j_max = ((py_max + hex_side) / row_pitch).ceil() as i64;
    let i_min = ((px_min - hex_width) / hex_width).floor() as i64;
    let i_max = ((px_max + hex_width) / hex_width).ceil() as i64;

    let mut lines = Paths::default();

    for j in j_min..=j_max {
        let y = j as f64 * row_pitch;
        let row_shift = if j.rem_euclid(2) == 1 { half_w } else { 0.0 };

        // The row's *upper* pair of walls on every cell chain into one
        // continuous zig-zag; the row below emits its own, which doubles as this
        // row's lower walls. Extending the row range by one below covers the
        // bottom edge of the pattern.
        let mut zigzag: Vec<(f64, f64)> = Vec::with_capacity(((i_max - i_min) * 2 + 2) as usize);
        for i in i_min..=i_max {
            let x = row_shift + i as f64 * hex_width;
            if i == i_min {
                zigzag.push(place(x - half_w, y + half_s));
            }
            zigzag.push(place(x, y + hex_side));
            zigzag.push(place(x + half_w, y + half_s));
        }
        if zigzag.len() >= 2 {
            let path: Path = zigzag.into();
            lines.push(path);
        }

        // Vertical walls joining this row's lower zig-zag to its upper one.
        for i in i_min..=i_max {
            let x = row_shift + i as f64 * hex_width - half_w;
            let seg: Path = vec![place(x, y - half_s), place(x, y + half_s)].into();
            lines.push(seg);
        }
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

    /// Wall length inside a central window, counting each segment whose
    /// midpoint falls in it.
    ///
    /// Measuring by midpoint keeps the estimate unbiased: clipping segments at
    /// the window edge would systematically drop the ones that straddle it and
    /// under-report by roughly `edge_length / window_width`.
    fn length_in_window(paths: &Paths, lo: f64, hi: f64) -> f64 {
        let mut length = 0.0;
        for path in paths.iter() {
            let pts: Vec<(f64, f64)> = path.iter().map(|v| (v.x(), v.y())).collect();
            for w in pts.windows(2) {
                let mx = 0.5 * (w[0].0 + w[1].0);
                let my = 0.5 * (w[0].1 + w[1].1);
                if mx >= lo && mx <= hi && my >= lo && my <= hi {
                    length += ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt();
                }
            }
        }
        length
    }

    #[test]
    fn test_honeycomb_empty_region() {
        let region = Paths::default();
        let result = generate_honeycomb(&region, SPACING, 0.2, 0.0);
        assert!(result.is_empty());
    }

    #[test]
    fn test_honeycomb_zero_density() {
        let result = generate_honeycomb(&square(20.0), SPACING, 0.0, 0.0);
        assert!(result.is_empty());
    }

    #[test]
    fn test_honeycomb_generates_cells() {
        let result = generate_honeycomb(&square(20.0), SPACING, 0.2, 0.0);
        assert!(!result.is_empty(), "Should generate honeycomb pattern");
    }

    #[test]
    fn lattice_is_anchored_to_world_coordinates() {
        // Honeycomb is a *cellular* pattern: its walls only become tubes if
        // every layer's cells land on the layer below. Two differently-sized
        // regions must therefore produce the same lattice, not one keyed to
        // each region's own centre.
        let mut offset_region = Paths::default();
        let inner: Path = vec![(7.0, 3.0), (53.0, 3.0), (53.0, 49.0), (7.0, 49.0)].into();
        offset_region.push(inner);

        let wide = generate_honeycomb(&square(60.0), 0.4, 0.2, 0.0);
        let narrow = generate_honeycomb(&offset_region, 0.4, 0.2, 0.0);

        // Compare the vertical cell walls that fall inside the shared area.
        let walls = |paths: &Paths| -> Vec<(i64, i64)> {
            let mut v: Vec<(i64, i64)> = paths
                .iter()
                .filter_map(|p| {
                    let pts: Vec<(f64, f64)> = p.iter().map(|v| (v.x(), v.y())).collect();
                    (pts.len() == 2 && (pts[0].0 - pts[1].0).abs() < 1e-9).then(|| {
                        (
                            (pts[0].0 * 100.0).round() as i64,
                            (pts[0].1 * 100.0).round() as i64,
                        )
                    })
                })
                .filter(|(x, y)| (1000..=5000).contains(x) && (1000..=4000).contains(y))
                .collect();
            v.sort_unstable();
            v
        };

        let a = walls(&wide);
        let b = walls(&narrow);
        assert!(!a.is_empty(), "expected cell walls in the sampled window");
        assert_eq!(
            a, b,
            "the lattice moved when the region changed — cells will not stack"
        );
    }

    #[test]
    fn honeycomb_line_length_matches_requested_density() {
        // Wall length per unit area must be `density / spacing`, i.e. the same
        // material a rectilinear fill at that density would lay.
        let spacing = 0.4;
        let density = 0.2;
        let size = 60.0;
        let lines = generate_honeycomb(&square(size), spacing, density, 0.0);

        let (lo, hi) = (size * 0.25, size * 0.75);
        let observed = length_in_window(&lines, lo, hi) / ((hi - lo) * (hi - lo)) * spacing;
        assert!(
            (observed - density).abs() < 0.01,
            "honeycomb deposited {observed:.3} where {density:.3} was requested"
        );
    }

    /// Distance between neighbouring vertical cell walls.
    ///
    /// Rows are offset by half a cell, so consecutive walls across the whole
    /// lattice sit `distance` apart — libslic3r's `m.distance = spacing / density`,
    /// half the `hex_width` between two walls of the *same* row.
    fn wall_pitch(paths: &Paths) -> f64 {
        let mut xs: Vec<f64> = paths
            .iter()
            .filter_map(|p| {
                let pts: Vec<(f64, f64)> = p.iter().map(|v| (v.x(), v.y())).collect();
                (pts.len() == 2 && (pts[0].0 - pts[1].0).abs() < 1e-9).then_some(pts[0].0)
            })
            .collect();
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        xs.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
        assert!(xs.len() > 3, "expected several columns, got {}", xs.len());
        xs[2] - xs[1]
    }

    #[test]
    fn honeycomb_cells_scale_with_bead_width() {
        // Cell size follows libslic3r's `distance = spacing / density`, so a wider
        // bead at the same density means proportionally larger cells.
        for (spacing, density) in [(0.35, 0.2), (0.70, 0.2), (0.4, 0.35)] {
            let pitch = wall_pitch(&generate_honeycomb(&square(60.0), spacing, density, 0.0));
            let expected = spacing / density;
            // Clipper2 stores coordinates at Centi (0.01 mm) precision, so two
            // quantised x values can differ from the exact pitch by up to 0.02.
            assert!(
                (pitch - expected).abs() < 0.02,
                "spacing {spacing} at density {density}: expected {expected} mm wall pitch, got {pitch}"
            );
        }
    }
}
