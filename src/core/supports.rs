//! Support-structure generation.
//!
//! Detects overhangs steeper than [`SlicingParams::support_threshold_angle`],
//! projects the unsupported area downward with horizontal (XY) and vertical (Z)
//! clearance from the model, classifies dense **interface** contact layers, and
//! fills the resulting columns with a grid/zig-zag pattern tagged
//! [`ExtrusionRole::Support`].  Both support styles are produced:
//!
//! | Style                    | Column carried downward                     | Merging |
//! |--------------------------|---------------------------------------------|---------|
//! | [`SupportType::Normal`]  | full overhang footprint (straight-down grid) | none    |
//! | [`SupportType::Tree`]    | eroded trunk core (tapered organic columns)  | morphological close per layer |
//!
//! `Tree` is a pragmatic approximation of full branching tree support: each
//! overhang seeds a trunk that is narrower than its contact pad, and nearby
//! trunks are merged as they descend so the columns flow together like
//! branches.  It is **not** a physically optimal collision-avoiding tree
//! (that is a much larger algorithm); the honest limitation is surfaced by
//! [`SlicingParams::unsupported_feature_warnings`].
//!
//! # Pipeline placement
//!
//! Runs after infill and before path ordering (TSP) so support paths are
//! ordered alongside the rest of the layer.  It only reads `OuterWall` paths
//! (via [`perimeter_paths_of`]) to derive the model footprint, so it never
//! disturbs wall/surface/infill geometry.

use clipper2::*;

use crate::settings::params::{SlicingParams, SupportType};

use super::surfaces::{generate_rectilinear_infill, perimeter_paths_of};
use super::types::{ExtrusionRole, SliceLayer};

/// Overhang islands smaller than this (mm²) are ignored — they are slicing
/// noise along near-vertical faceted walls, not genuine unsupported area.
const SUPPORT_MIN_OVERHANG_AREA_MM2: f64 = 1.0;

/// Support-region islands smaller than this (mm²) are dropped after projection —
/// too small to print a stable column.
const SUPPORT_MIN_REGION_AREA_MM2: f64 = 1.0;

/// Extra horizontal tolerance (mm) added to the per-layer overhang step so that
/// the tiny facet-to-facet jitter of a near-vertical wall does not register as
/// an overhang.
const OVERHANG_FACET_TOLERANCE_MM: f64 = 0.05;

/// ── Clipper2 helpers with empty-input guards ──────────────────────────────
///
/// Clipper2 boolean ops on an empty operand can throw; these wrappers keep the
/// accumulation loop total and readable.
fn poly_union(a: &Paths, b: &Paths) -> Paths {
    if a.is_empty() {
        return b.clone();
    }
    if b.is_empty() {
        return a.clone();
    }
    union(a.clone(), b.clone(), FillRule::NonZero).unwrap_or_else(|_| a.clone())
}

fn poly_difference(a: &Paths, b: &Paths) -> Paths {
    if a.is_empty() || b.is_empty() {
        return a.clone();
    }
    difference(a.clone(), b.clone(), FillRule::NonZero).unwrap_or_else(|_| a.clone())
}

fn poly_intersect(a: &Paths, b: &Paths) -> Paths {
    if a.is_empty() || b.is_empty() {
        return Paths::new(vec![]);
    }
    intersect(a.clone(), b.clone(), FillRule::NonZero).unwrap_or_default()
}

fn poly_inflate(a: &Paths, delta: f64) -> Paths {
    if a.is_empty() || delta.abs() < 1e-9 {
        return a.clone();
    }
    inflate(a.clone(), delta, JoinType::Round, EndType::Polygon, 2.0)
}

/// Morphological close: dilate by `radius` then erode back.  Bridges gaps up to
/// `2·radius` between nearby columns (merging tree trunks) without materially
/// growing the total area.
fn poly_close(a: &Paths, radius: f64) -> Paths {
    if a.is_empty() || radius <= 1e-6 {
        return a.clone();
    }
    let grown = poly_inflate(a, radius);
    poly_inflate(&grown, -radius)
}

fn filter_small(paths: &Paths, min_area_mm2: f64) -> Paths {
    if min_area_mm2 <= 0.0 || paths.is_empty() {
        return paths.clone();
    }
    Paths::new(
        paths
            .iter()
            .filter(|p| p.signed_area().abs() >= min_area_mm2)
            .cloned()
            .collect(),
    )
}

/// Generate support structures for all layers, appending
/// [`ExtrusionRole::Support`] paths in place.
///
/// A no-op when `support_enabled` is false or the model has fewer than two
/// layers (nothing can overhang the bed on the first layer).
pub fn generate_supports(layers: &mut [SliceLayer], params: &SlicingParams) {
    if !params.support_enabled {
        return;
    }
    let n = layers.len();
    if n < 2 {
        return;
    }

    let lh = params.layer_height.max(1e-4);
    let ext_w = params.nozzle_diameter_mm.max(0.1);

    // Maximum horizontal shift, per layer, that a wall can advance without
    // needing support.  `support_threshold_angle` is measured from vertical:
    // 45° → shift = layer_height (the classic 45° rule); a smaller angle
    // triggers support on gentler overhangs.
    let angle = params.support_threshold_angle.clamp(0.0, 89.0);
    let max_step = lh * angle.to_radians().tan() + OVERHANG_FACET_TOLERANCE_MM;

    // ── 1. Model footprint per layer (union of OuterWall contours) ──────────
    let footprints: Vec<Paths> = layers
        .iter()
        .map(|layer| {
            let outer = perimeter_paths_of(layer);
            if outer.is_empty() {
                Paths::new(vec![])
            } else {
                union(outer, Paths::new(vec![]), FillRule::NonZero).unwrap_or_default()
            }
        })
        .collect();

    // ── 2. Overhang region per layer ───────────────────────────────────────
    // The part of layer i not covered by layer i-1 grown outward by `max_step`.
    let mut overhang: Vec<Paths> = vec![Paths::new(vec![]); n];
    for i in 1..n {
        if footprints[i].is_empty() {
            continue;
        }
        let grown_prev = poly_inflate(&footprints[i - 1], max_step);
        let raw = poly_difference(&footprints[i], &grown_prev);
        overhang[i] = filter_small(&raw, SUPPORT_MIN_OVERHANG_AREA_MM2);
    }

    // ── 3. Register each overhang at its top-contact (activation) layer ─────
    // An overhang at layer i is contacted `z_gap` layers below it, leaving an
    // air gap for clean removal.
    let z_gap = params.support_z_gap_layers;
    let mut add_at: Vec<Paths> = vec![Paths::new(vec![]); n];
    #[allow(clippy::needless_range_loop)]
    for i in 1..n {
        if overhang[i].is_empty() {
            continue;
        }
        let activate = (i as isize - 1 - z_gap as isize).max(0) as usize;
        add_at[activate] = poly_union(&add_at[activate], &overhang[i]);
    }

    // ── 4. Descend, accumulating support columns ───────────────────────────
    let is_tree = params.support_type == SupportType::Tree;
    let xy = params.support_xy_distance_mm.max(0.0);
    let iface = params.support_interface_layers;
    let merge_radius = ext_w * 2.0;
    let trunk_taper = ext_w * 2.0;

    // `acc` is the running column carried downward.  For tree support the
    // carried column is the eroded trunk; for normal it is the full overhang.
    let mut acc = Paths::new(vec![]);
    // Per-layer printed regions, split into interface (dense) and body.
    let mut body_regions: Vec<Paths> = vec![Paths::new(vec![]); n];
    let mut iface_regions: Vec<Paths> = vec![Paths::new(vec![]); n];

    for i in (0..n).rev() {
        // Seed newly-activated contacts into the carried column.
        if !add_at[i].is_empty() {
            let seed = if is_tree {
                // Erode the contact pad to a narrower trunk core; if the pad is
                // too small to erode without vanishing, carry it whole.
                let eroded = poly_inflate(&add_at[i], -trunk_taper);
                if eroded.is_empty() {
                    add_at[i].clone()
                } else {
                    eroded
                }
            } else {
                add_at[i].clone()
            };
            acc = poly_union(&acc, &seed);
        }

        if acc.is_empty() {
            continue;
        }

        // Tree: merge nearby trunks so branches flow together as they descend.
        if is_tree {
            acc = poly_close(&acc, merge_radius);
        }

        // Horizontal clearance: remove the model (+ XY distance) at this layer.
        let clip = poly_inflate(&footprints[i], xy);
        let column = poly_difference(&acc, &clip);
        let column = filter_small(&column, SUPPORT_MIN_REGION_AREA_MM2);
        if column.is_empty() {
            continue;
        }

        // Top interface: the full overhang pad(s) whose contact band covers this
        // layer — [i, i+iface-1] — giving a dense, flat surface under the
        // overhang regardless of trunk erosion.
        let mut top_full = Paths::new(vec![]);
        if iface > 0 {
            for pad in add_at.iter().take((i + iface).min(n)).skip(i) {
                if !pad.is_empty() {
                    top_full = poly_union(&top_full, pad);
                }
            }
        }
        let top_if = poly_difference(&top_full, &clip);

        // The printed footprint is the union of the load-bearing column and the
        // (possibly wider) top contact pad.
        let total = poly_union(&column, &top_if);

        // Bottom interface: where the column rests on model within `iface`
        // layers below (contact that must detach cleanly).
        let mut below = Paths::new(vec![]);
        if iface > 0 {
            let lo = i.saturating_sub(iface);
            for f in footprints.iter().take(i).skip(lo) {
                below = poly_union(&below, f);
            }
        }
        let bot_if = poly_intersect(&total, &below);

        let iface_region = poly_union(&top_if, &bot_if);
        let body_region = poly_difference(&total, &iface_region);

        iface_regions[i] = iface_region;
        body_regions[i] = body_region;
    }

    // ── 5. Fill each layer's support regions and append Support paths ───────
    let body_dens = params.support_density.clamp(0.02, 1.0);
    let iface_dens = params.support_interface_density.clamp(0.05, 1.0);
    let body_spacing = ext_w / body_dens;
    let iface_spacing = ext_w / iface_dens;
    let min_len = params.min_infill_extrusion_mm;

    for i in 0..n {
        let body = &body_regions[i];
        let iface_r = &iface_regions[i];
        if body.is_empty() && iface_r.is_empty() {
            continue;
        }

        // Alternate direction each layer for inter-layer bonding.
        let even = i % 2 == 0;
        let body_angle = if even { 0.0 } else { 90.0 };
        let iface_angle = if even { 45.0 } else { 135.0 };

        let mut support_paths: Vec<Path> = Vec::new();

        if !body.is_empty() {
            let lines = generate_rectilinear_infill(body, body_spacing, body_angle, min_len);
            support_paths.extend(lines.iter().cloned());
        }
        if !iface_r.is_empty() {
            let lines = generate_rectilinear_infill(iface_r, iface_spacing, iface_angle, min_len);
            support_paths.extend(lines.iter().cloned());
        }

        for path in support_paths {
            push_support_path(&mut layers[i], path, ext_w);
        }
    }
}

/// Append a single support polyline to `layer`, keeping the parallel per-path
/// vectors aligned.  Support strands are **open** polylines (never closed
/// loops) and carry an explicit extrusion width equal to the nozzle diameter.
fn push_support_path(layer: &mut SliceLayer, path: Path, width: f64) {
    let target = layer.paths.len();
    // Pad any parallel vectors that lagged behind (earlier stages such as
    // infill only push `paths` + `path_roles`); the accessors already treat a
    // missing entry as the default we pad with here, so this is lossless.
    while layer.path_roles.len() < target {
        layer.path_roles.push(ExtrusionRole::OuterWall);
    }
    while layer.path_widths.len() < target {
        layer.path_widths.push(None);
    }
    while layer.path_vertex_widths.len() < target {
        layer.path_vertex_widths.push(None);
    }
    while layer.path_is_open.len() < target {
        layer.path_is_open.push(false);
    }

    layer.paths.push(path);
    layer.path_roles.push(ExtrusionRole::Support);
    layer.path_widths.push(Some(width));
    layer.path_vertex_widths.push(None);
    layer.path_is_open.push(true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::ExtrusionRole;
    use clipper2::Path;

    /// Build a square OuterWall contour centered at (cx, cy) with the given
    /// half-size, as a single-island layer at height `z`.
    fn square_layer(z: f64, cx: f64, cy: f64, half: f64) -> SliceLayer {
        let mut layer = SliceLayer::new(z);
        let mut p = Path::new(vec![]);
        p.push(Point::new(cx - half, cy - half));
        p.push(Point::new(cx + half, cy - half));
        p.push(Point::new(cx + half, cy + half));
        p.push(Point::new(cx - half, cy + half));
        layer.paths.push(p);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer
    }

    fn support_path_count(layer: &SliceLayer) -> usize {
        (0..layer.paths.len())
            .filter(|&i| layer.role_for_path(i) == ExtrusionRole::Support)
            .count()
    }

    fn params_with_supports() -> SlicingParams {
        SlicingParams {
            support_enabled: true,
            layer_height: 0.2,
            nozzle_diameter_mm: 0.4,
            support_threshold_angle: 45.0,
            ..SlicingParams::default()
        }
    }

    #[test]
    fn disabled_supports_are_a_noop() {
        let mut layers = vec![
            square_layer(0.2, 0.0, 0.0, 5.0),
            square_layer(0.4, 20.0, 0.0, 5.0),
        ];
        let params = SlicingParams {
            support_enabled: false,
            ..params_with_supports()
        };
        generate_supports(&mut layers, &params);
        assert_eq!(support_path_count(&layers[0]), 0);
        assert_eq!(support_path_count(&layers[1]), 0);
    }

    #[test]
    fn straight_wall_needs_no_support() {
        // Two identical stacked squares — a vertical wall, no overhang.
        let mut layers = vec![
            square_layer(0.2, 0.0, 0.0, 5.0),
            square_layer(0.4, 0.0, 0.0, 5.0),
        ];
        let params = params_with_supports();
        generate_supports(&mut layers, &params);
        assert_eq!(support_path_count(&layers[1]), 0);
        assert_eq!(support_path_count(&layers[0]), 0);
    }

    #[test]
    fn floating_overhang_generates_support_below() {
        // A block that appears in mid-air, laterally offset from the base so it
        // overhangs nothing beneath it — must be supported down to the bed.
        let mut layers = Vec::new();
        // Layers 0..3: base column at origin.
        for k in 0..3 {
            layers.push(square_layer(0.2 * (k as f64 + 1.0), 0.0, 0.0, 4.0));
        }
        // Layers 3..6: a shelf far to the side (its underside is unsupported).
        for k in 3..6 {
            layers.push(square_layer(0.2 * (k as f64 + 1.0), 30.0, 0.0, 6.0));
        }
        let params = params_with_supports();
        generate_supports(&mut layers, &params);

        // The overhang shelf sits at layers 3..6; support must be produced on at
        // least one layer below it (with the default 1-layer Z gap).
        let total_support: usize = layers.iter().map(support_path_count).sum();
        assert!(
            total_support > 0,
            "expected support paths under the floating overhang"
        );
        // Support strands must be open polylines tagged Support.
        for layer in &layers {
            for i in 0..layer.paths.len() {
                if layer.role_for_path(i) == ExtrusionRole::Support {
                    assert!(layer.is_path_open(i), "support paths must be open");
                    assert_eq!(layer.width_for_path(i), Some(0.4));
                }
            }
        }
    }

    #[test]
    fn tree_and_normal_both_produce_support() {
        let build = |ty: SupportType| {
            let mut layers = Vec::new();
            for k in 0..3 {
                layers.push(square_layer(0.2 * (k as f64 + 1.0), 0.0, 0.0, 3.0));
            }
            for k in 3..7 {
                layers.push(square_layer(0.2 * (k as f64 + 1.0), 25.0, 0.0, 8.0));
            }
            let params = SlicingParams {
                support_type: ty,
                ..params_with_supports()
            };
            generate_supports(&mut layers, &params);
            layers.iter().map(support_path_count).sum::<usize>()
        };
        assert!(build(SupportType::Normal) > 0, "normal support expected");
        assert!(build(SupportType::Tree) > 0, "tree support expected");
    }
}
