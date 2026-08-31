//! Dimensional compensation — correcting a machine's systematic XY error.
//!
//! A printer does not lay a bead exactly where it is told. Squish, die swell and
//! belt tension conspire to make finished parts consistently a little over- or
//! under-sized, and holes consistently tight. Neither is a slicing error, so
//! neither can be fixed by slicing more carefully — the correction is a
//! deliberate, measured offset applied to the geometry before it is turned into
//! toolpaths.
//!
//! # Why here, and nowhere else
//!
//! This pass runs in exactly one place: **between [`slice_mesh`] and wall
//! generation**, on the raw contours, and it is the only window that works.
//!
//! Everything downstream is expressed *relative to the contour the wall
//! generator consumed* — `calculate_interior_region`'s `−0.5·d` correction, the
//! wall-bead-footprint clip that surfaces and infill subtract, and adhesion's
//! recovery of the object outline by inflating layer-0 `OuterWall` beads by
//! `d/2`. Compensate the contour and every one of those relations stays exactly
//! true, because the compensated contour simply *becomes* the model surface as
//! far as the rest of the engine is concerned.
//!
//! **Never offset the bead centrelines instead.** Moving `OuterWall` by δ after
//! generation leaves `InnerWall` where it was, so wall spacing becomes `d ± δ`
//! and `compute_wall_bead_footprint` reports a footprint that does not match
//! what is printed. The footprint clip corrects bead-*count* error; it cannot
//! correct centreline displacement.
//!
//! # The two deltas
//!
//! They compose, and they are deliberately separate because the two dialects
//! this engine imports from disagree about what one number should mean:
//!
//! | Setting | Effect | Dialect |
//! | --- | --- | --- |
//! | `xy_size_compensation` | Uniform inflate of the whole region. Positive grows the part and therefore *tightens* holes. | PrusaSlicer/Slic3r `xy_size_compensation` |
//! | `xy_hole_compensation` | Adjusts enclosed voids only, applied after the above. Positive *enlarges* holes. | Orca/Bambu `xy_hole_compensation` |
//!
//! Splitting them is what lets a press-fit hole be opened up without moving the
//! part's outside dimensions, and it is why a single signed "contour delta"
//! field would get imported Orca profiles wrong by `2δ` on every hole.
//!
//! # Winding is load-bearing
//!
//! The mesh slicer does not guarantee consistent winding (see AGENTS.md §
//! "Clipper2 Fill Rules"), so a raw `inflate` over its output would grow some
//! holes and shrink others. The pass therefore normalises with an `EvenOdd`
//! union first — after which solids are CCW and holes CW — and every subsequent
//! set operation uses `NonZero`, which honours those CW sub-paths as voids.
//! `Positive` would discard them and turn every hole solid.
//!
//! [`slice_mesh`]: super::slicer::slice_mesh

use clipper2::*;

use super::types::{ExtrusionRole, SliceLayer};

/// What a compensation run changed, for the caller to log.
///
/// Compensation can legitimately erase geometry — a negative delta larger than
/// half a thin rib's width consumes the rib — so the pass counts what it
/// removed rather than silently fabricating it back. A part that loses features
/// is telling the user their compensation is too aggressive; a part that
/// silently keeps them would hide it until the print came out wrong.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CompensationReport {
    /// Layers that held contours before compensation and hold none after.
    pub emptied_layers: usize,
    /// Net change in contour count across every layer. Negative means features
    /// were consumed; positive means an offset split one island into several.
    pub contour_delta: i64,
}

impl CompensationReport {
    /// True when the run removed geometry the user probably wanted to keep.
    pub fn has_losses(&self) -> bool {
        self.emptied_layers > 0 || self.contour_delta < 0
    }
}

/// Offset every contour to correct a machine's systematic XY error.
///
/// Call **immediately after [`slice_mesh`]** and before wall generation; see the
/// module docs for why no other position is correct. A zero delta pair is a
/// no-op and returns without touching the layers, so the default configuration
/// slices byte-identically.
///
/// [`slice_mesh`]: super::slicer::slice_mesh
pub fn apply_dimensional_compensation(
    layers: &mut [SliceLayer],
    size_delta_mm: f64,
    hole_delta_mm: f64,
) -> CompensationReport {
    if !is_active(size_delta_mm, hole_delta_mm) {
        return CompensationReport::default();
    }

    #[cfg(not(target_arch = "wasm32"))]
    let compensated: Vec<Paths> = {
        use rayon::prelude::*;
        layers
            .par_iter()
            .map(|layer| compensate_layer(&layer.paths, size_delta_mm, hole_delta_mm))
            .collect()
    };
    #[cfg(target_arch = "wasm32")]
    let compensated: Vec<Paths> = layers
        .iter()
        .map(|layer| compensate_layer(&layer.paths, size_delta_mm, hole_delta_mm))
        .collect();

    let mut report = CompensationReport::default();

    for (layer, region) in layers.iter_mut().zip(compensated) {
        let before = layer.paths.len();
        let after = region.len();
        if before > 0 && after == 0 {
            report.emptied_layers += 1;
        }
        report.contour_delta += after as i64 - before as i64;

        // Compensation runs before any other pass has annotated the layer, so
        // the only parallel array carrying data is `path_roles` — every contour
        // out of the slicer is an `OuterWall`. Rebuilding both together keeps
        // them the same length; the remaining arrays stay empty and continue to
        // resolve through the "shorter array means default" convention.
        layer.paths = region;
        layer.path_roles = vec![ExtrusionRole::OuterWall; after];
    }

    report
}

/// True when either delta is large enough to change the geometry.
///
/// The threshold is 1 nm — far below any meaningful compensation, but above the
/// float noise a round-tripped profile value can carry.
fn is_active(size_delta_mm: f64, hole_delta_mm: f64) -> bool {
    const EPS: f64 = 1e-6;
    (size_delta_mm.is_finite() && size_delta_mm.abs() > EPS)
        || (hole_delta_mm.is_finite() && hole_delta_mm.abs() > EPS)
}

/// Compensate one layer's contours.
fn compensate_layer(paths: &Paths, size_delta_mm: f64, hole_delta_mm: f64) -> Paths {
    if paths.is_empty() {
        return Paths::default();
    }

    // Normalise winding first: the slicer's contours carry no consistent
    // orientation, and every step below depends on solids being CCW and holes
    // CW. `EvenOdd` is the winding-independent rule that establishes it.
    let mut region = match union(paths.clone(), Paths::default(), FillRule::EvenOdd) {
        Ok(r) if !r.is_empty() => r,
        // A degenerate layer that will not normalise is left exactly as it was
        // rather than dropped: compensation is a correction, not a filter.
        _ => return paths.clone(),
    };

    if size_delta_mm.is_finite() && size_delta_mm.abs() > 1e-6 {
        region = inflate(
            region,
            size_delta_mm,
            JoinType::Round,
            EndType::Polygon,
            2.0,
        );
        if region.is_empty() {
            return region;
        }
    }

    if hole_delta_mm.is_finite() && hole_delta_mm.abs() > 1e-6 {
        region = compensate_holes(region, hole_delta_mm);
    }

    region
}

/// Resize enclosed voids by `delta` without moving the outer contour.
///
/// Works for both signs by *filling every hole solid and re-punching it at the
/// new size*, rather than adding or removing a ring — a subtraction alone would
/// silently do nothing for a negative delta, because the material it would
/// remove is already absent.
fn compensate_holes(region: Paths, delta_mm: f64) -> Paths {
    // A hole is a CW sub-path. Reversed, it becomes an ordinary positive
    // polygon describing the void itself, which is far easier to reason about
    // than an inside-out one.
    let voids = Paths::new(
        region
            .iter()
            .filter(|p| p.signed_area() < 0.0)
            .map(reverse_path)
            .collect::<Vec<_>>(),
    );
    if voids.is_empty() {
        return region;
    }

    let Ok(filled) = union(region.clone(), voids.clone(), FillRule::NonZero) else {
        return region;
    };

    let resized = inflate(voids, delta_mm, JoinType::Round, EndType::Polygon, 2.0);
    if resized.is_empty() {
        // Every void was consumed by a large negative delta — the holes are
        // filled in, which is exactly what was asked for.
        return filled;
    }

    difference(filled, resized, FillRule::NonZero).unwrap_or(region)
}

/// Reverse a path's vertex order, flipping its orientation and the sign of its
/// area.
fn reverse_path(path: &Path) -> Path {
    let mut points: Vec<(f64, f64)> = path.iter().map(|p| (p.x(), p.y())).collect();
    points.reverse();
    Path::from(points)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A CCW square of side `size` centred on the origin.
    fn square(size: f64) -> Path {
        let h = size / 2.0;
        Path::from(vec![(-h, -h), (h, -h), (h, h), (-h, h)])
    }

    /// A CW square of side `size` centred on the origin — a hole.
    fn square_hole(size: f64) -> Path {
        let h = size / 2.0;
        Path::from(vec![(-h, -h), (-h, h), (h, h), (h, -h)])
    }

    fn layer_with(paths: Vec<Path>) -> SliceLayer {
        let mut layer = SliceLayer::new(0.2);
        for p in paths {
            layer.paths.push(p);
            layer.path_roles.push(ExtrusionRole::OuterWall);
        }
        layer
    }

    /// Longest side of the axis-aligned bounding box of the positive contours.
    fn outer_extent(paths: &Paths) -> f64 {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in paths.iter().filter(|p| p.signed_area() > 0.0) {
            for pt in p.iter() {
                lo = lo.min(pt.x());
                hi = hi.max(pt.x());
            }
        }
        hi - lo
    }

    fn void_area(paths: &Paths) -> f64 {
        paths
            .iter()
            .filter(|p| p.signed_area() < 0.0)
            .map(|p| p.signed_area().abs())
            .sum()
    }

    #[test]
    fn zero_deltas_are_a_no_op() {
        let mut layers = vec![layer_with(vec![square(20.0)])];
        let before = layers[0].paths.clone();

        let report = apply_dimensional_compensation(&mut layers, 0.0, 0.0);

        assert_eq!(report, CompensationReport::default());
        assert_eq!(
            layers[0].paths.len(),
            before.len(),
            "an off-by-default pass must not touch the geometry at all"
        );
    }

    #[test]
    fn positive_size_compensation_grows_the_part() {
        let mut layers = vec![layer_with(vec![square(20.0)])];

        apply_dimensional_compensation(&mut layers, 0.25, 0.0);

        let extent = outer_extent(&layers[0].paths);
        assert!(
            (extent - 20.5).abs() < 0.05,
            "20mm square + 0.25mm per side should measure ~20.5mm, got {extent:.3}"
        );
    }

    #[test]
    fn negative_size_compensation_shrinks_the_part() {
        let mut layers = vec![layer_with(vec![square(20.0)])];

        apply_dimensional_compensation(&mut layers, -0.25, 0.0);

        let extent = outer_extent(&layers[0].paths);
        assert!(
            (extent - 19.5).abs() < 0.05,
            "20mm square - 0.25mm per side should measure ~19.5mm, got {extent:.3}"
        );
    }

    /// The PrusaSlicer semantic: one number grows the part, and the material
    /// necessarily expands *into* the hole.
    #[test]
    fn size_compensation_tightens_holes_as_a_side_effect() {
        let mut layers = vec![layer_with(vec![square(20.0), square_hole(6.0)])];
        let before = void_area(&layers[0].paths.clone());

        apply_dimensional_compensation(&mut layers, 0.25, 0.0);

        let after = void_area(&layers[0].paths);
        assert!(
            after < before,
            "growing the part must shrink its holes ({before:.2} -> {after:.2} mm²)"
        );
    }

    /// The Orca semantic, and the reason the two deltas are separate fields:
    /// holes open up while the outside stays put.
    #[test]
    fn hole_compensation_enlarges_holes_without_moving_the_contour() {
        let mut layers = vec![layer_with(vec![square(20.0), square_hole(6.0)])];
        let extent_before = outer_extent(&layers[0].paths.clone());
        let void_before = void_area(&layers[0].paths.clone());

        apply_dimensional_compensation(&mut layers, 0.0, 0.2);

        let extent_after = outer_extent(&layers[0].paths);
        let void_after = void_area(&layers[0].paths);
        assert!(
            void_after > void_before,
            "positive hole compensation must enlarge the void ({void_before:.2} -> {void_after:.2} mm²)"
        );
        assert!(
            (extent_after - extent_before).abs() < 0.05,
            "the outer contour must not move ({extent_before:.3} -> {extent_after:.3} mm)"
        );
    }

    /// A subtraction alone would be a silent no-op here — the material it would
    /// remove is already absent. This is the case the fill-and-re-punch
    /// implementation exists for.
    #[test]
    fn negative_hole_compensation_tightens_holes() {
        let mut layers = vec![layer_with(vec![square(20.0), square_hole(6.0)])];
        let void_before = void_area(&layers[0].paths.clone());

        apply_dimensional_compensation(&mut layers, 0.0, -0.2);

        let void_after = void_area(&layers[0].paths);
        assert!(
            void_after < void_before,
            "negative hole compensation must tighten the void ({void_before:.2} -> {void_after:.2} mm²)"
        );
    }

    /// Holes survive as holes. Using `FillRule::Positive` anywhere in this pass
    /// would drop the CW sub-paths and fill every hole with solid material.
    #[test]
    fn holes_are_never_filled_in_by_the_fill_rule() {
        let mut layers = vec![layer_with(vec![square(20.0), square_hole(6.0)])];

        apply_dimensional_compensation(&mut layers, 0.1, 0.0);

        assert!(
            layers[0].paths.iter().any(|p| p.signed_area() < 0.0),
            "the hole must still be a CW void after compensation"
        );
    }

    /// Winding out of the mesh slicer is not guaranteed, so a CW outer contour
    /// must still grow rather than shrink.
    #[test]
    fn inconsistent_input_winding_is_normalised_before_offsetting() {
        // Same 20mm square, wound backwards.
        let reversed = reverse_path(&square(20.0));
        let mut layers = vec![layer_with(vec![reversed])];

        apply_dimensional_compensation(&mut layers, 0.25, 0.0);

        let extent = outer_extent(&layers[0].paths);
        assert!(
            (extent - 20.5).abs() < 0.05,
            "a CW input contour must still grow to ~20.5mm, got {extent:.3}"
        );
    }

    /// An over-aggressive negative delta consumes a small feature. That is the
    /// honest outcome, and the report is how the user finds out.
    #[test]
    fn over_shrinking_reports_the_loss_instead_of_hiding_it() {
        let mut layers = vec![layer_with(vec![square(0.4)])];

        let report = apply_dimensional_compensation(&mut layers, -1.0, 0.0);

        assert!(
            layers[0].paths.is_empty(),
            "a 0.4mm feature cannot survive 1mm of inward compensation"
        );
        assert_eq!(report.emptied_layers, 1);
        assert!(report.has_losses());
    }

    #[test]
    fn roles_stay_in_step_with_paths() {
        let mut layers = vec![layer_with(vec![square(20.0), square_hole(6.0)])];

        apply_dimensional_compensation(&mut layers, 0.15, 0.1);

        assert_eq!(
            layers[0].paths.len(),
            layers[0].path_roles.len(),
            "the parallel arrays must not drift apart"
        );
        assert!(layers[0]
            .path_roles
            .iter()
            .all(|r| *r == ExtrusionRole::OuterWall));
    }
}
