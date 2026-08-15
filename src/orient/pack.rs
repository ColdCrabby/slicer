//! Shelf-first-fit layout algorithm for packing objects onto a print bed.
//!
//! ## Algorithm
//!
//! Objects are arranged using a **shelf-first-fit** strategy:
//!
//! 1. Sort objects by footprint area (largest first) so big objects claim
//!    their row before small ones; this gives better utilisation than
//!    arbitrary order.
//! 2. Maintain a list of open *shelves* — horizontal strips of the bed, each
//!    described by its bottom-Y coordinate and an X cursor pointing to where
//!    the next object goes.  Every shelf's height grows as objects are placed.
//! 3. For each object, scan shelves from bottom to top and place it on the
//!    first shelf where it fits (with `spacing_mm` gap after the previous
//!    object on that shelf).  If none fit, open a new shelf directly above
//!    the highest existing one.
//! 4. After all objects are placed, compute the bounding box of the
//!    arrangement and shift every object so the arrangement is centered on
//!    the bed.
//!
//! The algorithm is O(N²) in the number of objects, which is perfectly
//! acceptable for typical scene sizes (1–50 objects).

use crate::scene::bed::BedConfig;

/// One placed item returned by [`pack_footprints`].
#[derive(Debug, Clone)]
pub struct PackedItem {
    /// Original index in the input slice.
    pub index: usize,
    /// X coordinate of the placed object's min-X corner.
    pub x: f64,
    /// Y coordinate of the placed object's min-Y corner.
    pub y: f64,
}

/// Tracks a single horizontal strip of the packing area.
struct Shelf {
    /// Y coordinate of the strip's bottom edge.
    y_bottom: f64,
    /// X coordinate just past the rightmost object placed on this shelf.
    /// The next object starts at `x_cursor + spacing` (or at 0 for the
    /// first object on the shelf).
    x_cursor: f64,
    /// Height of the tallest object placed on this shelf.
    height: f64,
}

/// Arrange `footprints` (width × depth in mm) on `bed` with `spacing_mm`
/// between objects and return one [`PackedItem`] per footprint.
///
/// The result is centered on the bed.  Footprints that are wider than the
/// bed are placed anyway (they extend past the right edge) so that the caller
/// can warn the user without silently discarding objects.
pub fn pack_footprints(
    footprints: &[(f64, f64)], // (width, depth) per object
    bed: &BedConfig,
    spacing_mm: f64,
) -> Vec<PackedItem> {
    if footprints.is_empty() {
        return Vec::new();
    }

    // Sort indices by area descending (largest first → better utilisation).
    let mut order: Vec<usize> = (0..footprints.len()).collect();
    order.sort_unstable_by(|&a, &b| {
        let area_a = footprints[a].0 * footprints[a].1;
        let area_b = footprints[b].0 * footprints[b].1;
        area_b
            .partial_cmp(&area_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut shelves: Vec<Shelf> = Vec::new();
    // Pre-fill result with dummies; indexed by original `idx`.
    let mut result: Vec<PackedItem> = (0..footprints.len())
        .map(|i| PackedItem {
            index: i,
            x: 0.0,
            y: 0.0,
        })
        .collect();

    for &idx in &order {
        let (w, d) = footprints[idx];

        // Find the first shelf that can accommodate this object.
        let mut placed = false;
        for shelf in &mut shelves {
            // Gap before this object: spacing if the shelf already has objects,
            // otherwise no gap (first object starts at x = 0).
            let gap = if shelf.x_cursor > 0.0 {
                spacing_mm
            } else {
                0.0
            };
            let x_start = shelf.x_cursor + gap;
            if x_start + w <= bed.width {
                result[idx].x = x_start;
                result[idx].y = shelf.y_bottom;
                shelf.x_cursor = x_start + w;
                shelf.height = shelf.height.max(d);
                placed = true;
                break;
            }
        }

        if !placed {
            // Open a new shelf above the highest one so far.
            // `y_b + height + spacing` for each shelf gives the Y where the
            // next shelf would start.  `fold(0.0, max)` returns 0.0 for an
            // empty list (first shelf) and max(top_of_shelf + spacing) for a
            // non-empty list — both correct.
            let y_bottom = shelves
                .iter()
                .map(|s| s.y_bottom + s.height + spacing_mm)
                .fold(0.0_f64, f64::max);
            result[idx].x = 0.0;
            result[idx].y = y_bottom;
            shelves.push(Shelf {
                y_bottom,
                x_cursor: w,
                height: d,
            });
        }
    }

    // Center the arrangement on the bed.
    let min_x = result.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
    let min_y = result.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
    let max_x = result
        .iter()
        .zip(footprints.iter())
        .map(|(p, &(w, _))| p.x + w)
        .fold(f64::NEG_INFINITY, f64::max);
    let max_y = result
        .iter()
        .zip(footprints.iter())
        .map(|(p, &(_, d))| p.y + d)
        .fold(f64::NEG_INFINITY, f64::max);

    let (bx, by) = bed.center_xy();
    let cx = (min_x + max_x) / 2.0;
    let cy = (min_y + max_y) / 2.0;
    let dx = bx - cx;
    let dy = by - cy;

    for item in &mut result {
        item.x += dx;
        item.y += dy;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_bed() -> BedConfig {
        BedConfig {
            width: 220.0,
            depth: 220.0,
            height: 250.0,
            origin_offset_x: 0.0,
            origin_offset_y: 0.0,
        }
    }

    #[test]
    fn single_object_centered() {
        let bed = default_bed();
        let fp = [(50.0_f64, 50.0_f64)];
        let items = pack_footprints(&fp, &bed, 2.0);
        assert_eq!(items.len(), 1);
        // Center of item should be at bed center (110, 110).
        let cx = items[0].x + 25.0;
        let cy = items[0].y + 25.0;
        assert!((cx - 110.0).abs() < 1.0, "cx={cx}");
        assert!((cy - 110.0).abs() < 1.0, "cy={cy}");
    }

    #[test]
    fn no_overlap_four_objects() {
        let bed = default_bed();
        // Four 50×50 mm objects with 2 mm spacing — all fit in one row on 220 mm bed.
        // Row occupies: 4×50 + 3×2 = 206 mm < 220 mm.
        let fps: Vec<(f64, f64)> = vec![(50.0, 50.0); 4];
        let items = pack_footprints(&fps, &bed, 2.0);
        assert_eq!(items.len(), 4);
        // Check every pair for non-overlap (AABB test in XY).
        for i in 0..items.len() {
            for j in (i + 1)..items.len() {
                let (wi, di) = fps[items[i].index];
                let (wj, dj) = fps[items[j].index];
                let xi = items[i].x;
                let yi = items[i].y;
                let xj = items[j].x;
                let yj = items[j].y;
                let overlap_x = xi < xj + wj && xi + wi > xj;
                let overlap_y = yi < yj + dj && yi + di > yj;
                assert!(
                    !(overlap_x && overlap_y),
                    "objects {i} and {j} overlap: ({xi:.1},{yi:.1}) and ({xj:.1},{yj:.1})"
                );
            }
        }
    }

    #[test]
    fn wraps_to_second_row() {
        let bed = BedConfig {
            width: 100.0,
            depth: 200.0,
            ..BedConfig::default()
        };
        // Three 50×30 objects with 2 mm spacing — first two fit in row 0,
        // the third must start row 1.
        // Row 0: 50 + 2 + 50 = 102 > 100 → only one object per row at this size.
        // Actually: 50 fits in row 0 (cursor=50), then 50 + 2 = 52 > 100 → new shelf.
        let fps: Vec<(f64, f64)> = vec![(50.0, 30.0); 3];
        let items = pack_footprints(&fps, &bed, 2.0);
        assert_eq!(items.len(), 3);
        // Verify no overlaps.
        for i in 0..items.len() {
            for j in (i + 1)..items.len() {
                let (wi, di) = fps[items[i].index];
                let (wj, dj) = fps[items[j].index];
                let xi = items[i].x;
                let yi = items[i].y;
                let xj = items[j].x;
                let yj = items[j].y;
                let overlap_x = xi < xj + wj && xi + wi > xj;
                let overlap_y = yi < yj + dj && yi + di > yj;
                assert!(
                    !(overlap_x && overlap_y),
                    "objects {i} and {j} overlap in second-row test"
                );
            }
        }
    }

    #[test]
    fn empty_input() {
        let bed = default_bed();
        let items = pack_footprints(&[], &bed, 2.0);
        assert!(items.is_empty());
    }

    #[test]
    fn arrangement_is_centered_on_bed() {
        let bed = default_bed();
        let fps: Vec<(f64, f64)> = vec![(40.0, 40.0); 2];
        let items = pack_footprints(&fps, &bed, 2.0);
        // Two 40×40 objects with 2 mm gap → total width = 82, total depth = 40.
        // Arrangement centre should be at bed centre (110, 110).
        let min_x = items.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
        let max_x = items
            .iter()
            .zip(fps.iter())
            .map(|(p, &(w, _))| p.x + w)
            .fold(f64::NEG_INFINITY, f64::max);
        let min_y = items.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
        let max_y = items
            .iter()
            .zip(fps.iter())
            .map(|(p, &(_, d))| p.y + d)
            .fold(f64::NEG_INFINITY, f64::max);
        let cx = (min_x + max_x) / 2.0;
        let cy = (min_y + max_y) / 2.0;
        assert!((cx - 110.0).abs() < 0.1, "cx={cx:.2}");
        assert!((cy - 110.0).abs() < 0.1, "cy={cy:.2}");
    }
}
