//! Bed-adhesion helpers — skirt, brim, and raft generation.
//!
//! This module is the single place that turns the
//! [`AdhesionType`](crate::settings::params::AdhesionType) selection (plus its
//! sub-parameters) into extra first-layer / sub-object geometry.  It runs at the
//! very end of [`process_mesh`](crate::core::process_mesh), after walls,
//! surfaces, infill, path ordering, and flow compensation, so it never perturbs
//! the object's own toolpaths.
//!
//! # What each helper produces
//!
//! | Type  | Geometry | Role | Where |
//! | ----- | -------- | ---- | ----- |
//! | Skirt | `skirt_loops` concentric loops offset out from the object outline, over the first `skirt_height` layers | [`Skirt`](crate::core::ExtrusionRole::Skirt) | prepended to those layers |
//! | Brim  | apron loops hugging the object (outer, hole, or corner-only) on the first layer | [`Skirt`](crate::core::ExtrusionRole::Skirt) | prepended to layer 0 |
//! | Raft  | sacrificial base + interface layers under the object; object shifted up by raft height + air gap | [`Support`](crate::core::ExtrusionRole::Support) | prepended as whole layers |
//!
//! # Coordinate conventions
//!
//! All geometry is in millimetres (Clipper2 `Centi` precision).  The wall
//! generator emits `OuterWall` **centerlines** inset `d/2` from the true model
//! surface, so the object footprint is reconstructed by inflating the unioned
//! `OuterWall` paths outward by `d/2` (`d` = nozzle diameter).  Winding is
//! preserved throughout (CCW solids, CW holes) so Clipper2 treats holes as
//! voids — see `AGENTS.md` § "Clipper2 Fill Rules".

use clipper2::*;

use crate::core::{ExtrusionRole, OverhangClass, SliceLayer};
use crate::settings::params::{AdhesionType, BrimType, SlicingParams};

/// Round-join polygon inflate — the offset primitive used throughout this
/// module.  A positive `delta` grows the solid region outward (and shrinks
/// CW holes inward); a negative `delta` does the reverse.
fn offset(paths: &Paths, delta: f64) -> Paths {
    offset_join(paths, delta, JoinType::Round)
}

/// [`offset`] with an explicit join type.  `Miter` keeps sharp corners sharp
/// (needed for "ears" corner detection); `Round` bevels them.
fn offset_join(paths: &Paths, delta: f64, join: JoinType) -> Paths {
    if paths.is_empty() {
        return Paths::default();
    }
    inflate(paths.clone(), delta, join, EndType::Polygon, 2.0)
}

/// Merge a set of contours into a clean region, preserving winding so holes
/// stay voids.  `NonZero` is correct here because the input already carries
/// Clipper2-consistent winding (CCW solids, CW holes) from the wall generator.
fn clean_union(paths: Paths) -> Paths {
    if paths.is_empty() {
        return Paths::default();
    }
    union(paths, Paths::default(), FillRule::NonZero).unwrap_or_default()
}

/// Keep only the outer (positive-area, CCW) contours of a region, dropping hole
/// sub-paths.  Used for skirt loops, which enclose the whole object and must
/// never appear inside a hole.
fn outer_only(paths: Paths) -> Paths {
    Paths::new(
        paths
            .iter()
            .filter(|p| p.signed_area() > 0.0)
            .cloned()
            .collect(),
    )
}

/// Extract the object footprint on `layer` as a filled region (with holes).
///
/// Unions every `OuterWall` centerline, then inflates by `d/2` to recover the
/// true model outline.  Returns an empty region when the layer has no walls.
fn object_footprint(layer: &SliceLayer, d: f64) -> Paths {
    let outer = Paths::new(
        layer
            .paths
            .iter()
            .enumerate()
            .filter(|(i, _)| layer.role_for_path(*i) == ExtrusionRole::OuterWall)
            .map(|(_, p)| p.clone())
            .collect(),
    );
    if outer.is_empty() {
        return Paths::default();
    }
    let merged = clean_union(outer);
    offset(&merged, d / 2.0)
}

/// Like [`object_footprint`] but with a **miter** join, so sharp model corners
/// stay sharp instead of being bevelled by the round offset.  Used only for
/// "ears" brim corner detection, where a rounded corner would erase the very
/// feature we are looking for.
fn object_footprint_sharp(layer: &SliceLayer, d: f64) -> Paths {
    let outer = Paths::new(
        layer
            .paths
            .iter()
            .enumerate()
            .filter(|(i, _)| layer.role_for_path(*i) == ExtrusionRole::OuterWall)
            .map(|(_, p)| p.clone())
            .collect(),
    );
    if outer.is_empty() {
        return Paths::default();
    }
    let merged = clean_union(outer);
    offset_join(&merged, d / 2.0, JoinType::Miter)
}

/// Append a closed loop path to a layer's parallel path arrays.
fn push_loop(layer: &mut SliceLayer, path: Path, role: ExtrusionRole, width: f64) {
    if path.len() < 3 {
        return;
    }
    layer.paths.push(path);
    layer.path_roles.push(role);
    layer.path_widths.push(Some(width));
    layer.path_vertex_widths.push(None);
    layer.path_is_open.push(false);
    if !layer.path_overhang.is_empty() {
        layer.path_overhang.push(OverhangClass::None);
    }
}

/// Pad every parallel per-path array on `layer` up to `layer.paths.len()` so a
/// subsequent prepend keeps all arrays aligned.
fn normalise(layer: &mut SliceLayer) {
    let n = layer.paths.len();
    while layer.path_roles.len() < n {
        layer.path_roles.push(ExtrusionRole::OuterWall);
    }
    while layer.path_widths.len() < n {
        layer.path_widths.push(None);
    }
    while layer.path_vertex_widths.len() < n {
        layer.path_vertex_widths.push(None);
    }
    while layer.path_is_open.len() < n {
        layer.path_is_open.push(false);
    }
    // Only pad `path_overhang` when the layer was graded (non-empty); an empty
    // vector is the "not graded" sentinel and must stay empty.
    if !layer.path_overhang.is_empty() {
        while layer.path_overhang.len() < n {
            layer.path_overhang.push(OverhangClass::None);
        }
    }
}

/// Prepend `additions` (a fully-populated adhesion layer) in front of `layer`'s
/// existing paths so the adhesion loops print **first**.
fn prepend(layer: &mut SliceLayer, additions: SliceLayer) {
    if additions.paths.is_empty() {
        return;
    }
    normalise(layer);
    let additions_count = additions.paths.len();
    let mut paths = additions.paths;
    let mut roles = additions.path_roles;
    let mut widths = additions.path_widths;
    let mut vwidths = additions.path_vertex_widths;
    let mut is_open = additions.path_is_open;

    for p in layer.paths.iter() {
        paths.push(p.clone());
    }
    roles.extend(layer.path_roles.iter().copied());
    widths.extend(layer.path_widths.iter().copied());
    vwidths.extend(layer.path_vertex_widths.iter().cloned());
    is_open.extend(layer.path_is_open.iter().copied());

    // Prepend `None` for the (unclassified) adhesion loops so the object's own
    // overhang classes stay aligned to their walls.  Empty stays empty.
    let overhang = if layer.path_overhang.is_empty() {
        Vec::new()
    } else {
        let mut o = vec![OverhangClass::None; additions_count];
        o.extend(layer.path_overhang.iter().copied());
        o
    };

    layer.paths = paths;
    layer.path_roles = roles;
    layer.path_widths = widths;
    layer.path_vertex_widths = vwidths;
    layer.path_is_open = is_open;
    layer.path_overhang = overhang;
}

/// Concentric offset loops stepping **outward** from `footprint`, keeping only
/// outer contours.  Loop `k` sits at `start_gap + d/2 + k·d` from the object.
fn outward_loops(footprint: &Paths, start_gap: f64, count: usize, d: f64) -> Vec<Path> {
    let mut out = Vec::new();
    for k in 0..count {
        let delta = start_gap + d / 2.0 + k as f64 * d;
        let ring = outer_only(offset(footprint, delta));
        for c in ring.iter() {
            if c.len() >= 3 {
                out.push(c.clone());
            }
        }
    }
    out
}

/// Concentric loops stepping **inward** into each hole of `footprint` (inner
/// brim).  Each hole is treated as its own CCW polygon and deflated.
fn hole_loops(footprint: &Paths, start_gap: f64, count: usize, d: f64) -> Vec<Path> {
    let mut out = Vec::new();
    for hole in footprint.iter().filter(|p| p.signed_area() < 0.0) {
        // Flip the CW hole to a CCW polygon so a negative offset steps inward.
        let mut pts: Vec<(f64, f64)> = hole.iter().map(|p| (p.x(), p.y())).collect();
        pts.reverse();
        let poly: Path = pts.into();
        let poly_paths = Paths::new(vec![poly]);
        for k in 0..count {
            let delta = -(start_gap + d / 2.0 + k as f64 * d);
            let ring = offset(&poly_paths, delta);
            for c in ring.iter() {
                if c.len() >= 3 {
                    out.push(c.clone());
                }
            }
        }
    }
    out
}

/// Build a regular polygon approximating a circle of `radius` at `center`.
fn circle(center: (f64, f64), radius: f64, segments: usize) -> Path {
    let mut pts = Vec::with_capacity(segments);
    for i in 0..segments {
        let a = 2.0 * std::f64::consts::PI * i as f64 / segments as f64;
        pts.push((center.0 + radius * a.cos(), center.1 + radius * a.sin()));
    }
    pts.into()
}

/// Fill an arbitrary region with concentric loops spaced `d`, from the region
/// boundary inward.  Used for "ears" brim discs.
fn concentric_loops(region: &Paths, d: f64, max_loops: usize) -> Vec<Path> {
    let mut out = Vec::new();
    let mut inset = d / 2.0;
    for _ in 0..max_loops {
        let ring = offset(region, -inset);
        if ring.is_empty() {
            break;
        }
        for c in ring.iter() {
            if c.len() >= 3 {
                out.push(c.clone());
            }
        }
        inset += d;
    }
    out
}

/// Detect sharp convex corners on the outer contours of `footprint`.
///
/// A corner is "sharp convex" when the left-turn exterior angle at a vertex of
/// a CCW contour exceeds `min_turn` radians.  Returns the corner points.
fn convex_corners(footprint: &Paths, min_turn: f64) -> Vec<(f64, f64)> {
    let mut corners = Vec::new();
    for contour in footprint.iter().filter(|p| p.signed_area() > 0.0) {
        let pts: Vec<(f64, f64)> = contour.iter().map(|p| (p.x(), p.y())).collect();
        let n = pts.len();
        if n < 3 {
            continue;
        }
        for i in 0..n {
            let prev = pts[(i + n - 1) % n];
            let cur = pts[i];
            let next = pts[(i + 1) % n];
            let e1 = (cur.0 - prev.0, cur.1 - prev.1);
            let e2 = (next.0 - cur.0, next.1 - cur.1);
            let l1 = (e1.0 * e1.0 + e1.1 * e1.1).sqrt();
            let l2 = (e2.0 * e2.0 + e2.1 * e2.1).sqrt();
            if l1 < 1e-6 || l2 < 1e-6 {
                continue;
            }
            let cross = e1.0 * e2.1 - e1.1 * e2.0;
            let dot = e1.0 * e2.0 + e1.1 * e2.1;
            // Signed exterior turn angle; positive = left turn = convex on CCW.
            let signed_turn = cross.atan2(dot);
            if signed_turn > min_turn {
                corners.push(cur);
            }
        }
    }
    corners
}

/// Generate skirt loops around the object outline on the first `skirt_height`
/// layers.
fn generate_skirt(layers: &mut [SliceLayer], params: &SlicingParams, d: f64) {
    if params.skirt_loops == 0 || layers.is_empty() {
        return;
    }
    let footprint = object_footprint(&layers[0], d);
    if footprint.is_empty() {
        return;
    }
    let loops = outward_loops(&footprint, params.skirt_distance, params.skirt_loops, d);
    if loops.is_empty() {
        return;
    }
    let height = params.skirt_height.max(1).min(layers.len());
    for layer in layers.iter_mut().take(height) {
        let mut adds = SliceLayer::new(layer.z);
        // Outermost loop first so the skirt closes toward the object.
        for path in loops.iter().rev() {
            push_loop(&mut adds, path.clone(), ExtrusionRole::Skirt, d);
        }
        prepend(layer, adds);
    }
}

/// Generate brim loops on the first layer per the configured [`BrimType`].
fn generate_brim(layers: &mut [SliceLayer], params: &SlicingParams, d: f64) {
    if layers.is_empty() || params.brim_width <= 0.0 {
        return;
    }
    let footprint = object_footprint(&layers[0], d);
    if footprint.is_empty() {
        return;
    }
    let count = (params.brim_width / d).ceil() as usize;
    if count == 0 {
        return;
    }
    let sep = params.brim_separation;

    let mut loops: Vec<Path> = Vec::new();
    match params.brim_type {
        BrimType::OuterOnly => {
            loops.extend(outward_loops(&footprint, sep, count, d));
        }
        BrimType::InnerOnly => {
            loops.extend(hole_loops(&footprint, sep, count, d));
        }
        BrimType::OuterAndInner => {
            loops.extend(outward_loops(&footprint, sep, count, d));
            loops.extend(hole_loops(&footprint, sep, count, d));
        }
        BrimType::Ears => {
            // Detect corners on a sharp (miter) footprint so the round offset
            // doesn't bevel away the very corners we're looking for.
            let sharp = object_footprint_sharp(&layers[0], d);
            // Exterior turn > 60° (π/3) ⇒ interior angle < 120°: a sharp corner.
            let corners = convex_corners(&sharp, std::f64::consts::FRAC_PI_3);
            if !corners.is_empty() {
                let radius = sep + params.brim_width;
                let discs = Paths::new(corners.iter().map(|&c| circle(c, radius, 48)).collect());
                let disc_region = clean_union(discs);
                // Keep only the material outside the object (+ separation).
                let keepout = offset(&footprint, sep);
                let ear_region =
                    difference(disc_region, keepout, FillRule::NonZero).unwrap_or_default();
                loops.extend(concentric_loops(&ear_region, d, count));
            }
        }
    }

    if loops.is_empty() {
        return;
    }
    let mut adds = SliceLayer::new(layers[0].z);
    // Outermost first so the brim finishes adjacent to the wall.
    for path in loops.iter().rev() {
        push_loop(&mut adds, path.clone(), ExtrusionRole::Skirt, d);
    }
    prepend(&mut layers[0], adds);
}

/// Build the raft: sacrificial base + interface layers under the object, and
/// return them; the caller shifts the object layers up.
fn build_raft(object: &[SliceLayer], params: &SlicingParams, d: f64) -> Vec<SliceLayer> {
    if object.is_empty() {
        return Vec::new();
    }
    let n = if params.raft_layers > 0 {
        params.raft_layers
    } else {
        2
    };
    let footprint = object_footprint(&object[0], d);
    if footprint.is_empty() {
        return Vec::new();
    }
    // Expand the raft outline generously beyond the object for grip.
    let expansion = (params.brim_width.max(3.0)).max(2.0 * d);
    let outline = outer_only(offset(&footprint, expansion));
    if outline.is_empty() {
        return Vec::new();
    }

    let lh = params.layer_height;
    // Density → line spacing maps through the infill module's fixed 0.4 mm ref.
    let base_spacing = (2.5 * d).max(d);
    let base_density = (0.4 / base_spacing).clamp(0.05, 1.0);
    let iface_density = (0.4 / d).clamp(0.05, 1.0);

    let mut raft = Vec::with_capacity(n);
    for i in 0..n {
        let z = lh * 0.5 + i as f64 * lh;
        let mut layer = SliceLayer::new(z);
        // First layer = coarse base; remaining = finer interface.
        let (density, angle, width) = if i == 0 {
            (base_density, 0.0_f64, base_spacing.min(2.0 * d))
        } else {
            let a = if i % 2 == 1 {
                std::f64::consts::FRAC_PI_2
            } else {
                0.0
            };
            (iface_density, a, d)
        };
        let lines = crate::infill::generate_infill(
            &outline,
            crate::infill::InfillPattern::Rectilinear,
            density,
            angle,
            z,
        );
        for line in lines.iter() {
            if line.len() < 2 {
                continue;
            }
            layer.paths.push(line.clone());
            layer.path_roles.push(ExtrusionRole::Support);
            layer.path_widths.push(Some(width));
            layer.path_vertex_widths.push(None);
            layer.path_is_open.push(true);
        }
        if !layer.paths.is_empty() {
            raft.push(layer);
        }
    }
    raft
}

/// Apply the configured bed-adhesion helper to `layers` in place.
///
/// Dispatches on [`SlicingParams::adhesion_type`].  Skirt/brim mutate the first
/// layer(s); raft prepends new layers and shifts every object layer up by the
/// raft height plus [`SlicingParams::raft_air_gap`].
pub fn apply_adhesion(layers: &mut Vec<SliceLayer>, params: &SlicingParams) {
    let d = if params.nozzle_diameter_mm > 0.0 {
        params.nozzle_diameter_mm
    } else {
        0.4
    };

    match params.adhesion_type {
        AdhesionType::None => {}
        AdhesionType::Skirt => generate_skirt(layers, params, d),
        AdhesionType::Brim => generate_brim(layers, params, d),
        AdhesionType::Raft => {
            let raft = build_raft(layers, params, d);
            if raft.is_empty() {
                return;
            }
            let raft_height = raft.len() as f64 * params.layer_height;
            let z_shift = raft_height + params.raft_air_gap.max(0.0);
            for layer in layers.iter_mut() {
                layer.z += z_shift;
            }
            let mut combined = raft;
            combined.append(layers);
            *layers = combined;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A layer with a single 20×20 mm CCW square `OuterWall` centered at origin.
    fn square_layer(z: f64) -> SliceLayer {
        let mut layer = SliceLayer::new(z);
        let sq: Path = vec![(-10.0, -10.0), (10.0, -10.0), (10.0, 10.0), (-10.0, 10.0)].into();
        layer.paths.push(sq);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer
    }

    fn base_params() -> SlicingParams {
        SlicingParams {
            nozzle_diameter_mm: 0.4,
            layer_height: 0.2,
            adhesion_type: AdhesionType::None,
            ..Default::default()
        }
    }

    #[test]
    fn skirt_adds_loops_to_first_layer() {
        let mut layers = vec![square_layer(0.1), square_layer(0.3)];
        let mut p = base_params();
        p.adhesion_type = AdhesionType::Skirt;
        p.skirt_loops = 2;
        p.skirt_distance = 2.0;
        p.skirt_height = 1;
        apply_adhesion(&mut layers, &p);
        let skirts = layers[0]
            .path_roles
            .iter()
            .filter(|r| **r == ExtrusionRole::Skirt)
            .count();
        assert_eq!(skirts, 2, "expected two skirt loops on layer 0");
        // skirt_height = 1 ⇒ layer 1 untouched.
        assert!(!layers[1].path_roles.contains(&ExtrusionRole::Skirt));
        // Skirt loops print first.
        assert_eq!(layers[0].role_for_path(0), ExtrusionRole::Skirt);
    }

    #[test]
    fn skirt_height_spans_multiple_layers() {
        let mut layers = vec![square_layer(0.1), square_layer(0.3), square_layer(0.5)];
        let mut p = base_params();
        p.adhesion_type = AdhesionType::Skirt;
        p.skirt_loops = 1;
        p.skirt_height = 2;
        apply_adhesion(&mut layers, &p);
        for l in layers.iter().take(2) {
            assert!(l.path_roles.contains(&ExtrusionRole::Skirt));
        }
        assert!(!layers[2].path_roles.contains(&ExtrusionRole::Skirt));
    }

    #[test]
    fn brim_outer_adds_expected_loop_count() {
        let mut layers = vec![square_layer(0.1)];
        let mut p = base_params();
        p.adhesion_type = AdhesionType::Brim;
        p.brim_type = BrimType::OuterOnly;
        p.brim_width = 2.0; // 2.0 / 0.4 = 5 loops
        p.brim_separation = 0.0;
        apply_adhesion(&mut layers, &p);
        let brim = layers[0]
            .path_roles
            .iter()
            .filter(|r| **r == ExtrusionRole::Skirt)
            .count();
        assert_eq!(brim, 5, "expected ceil(2.0/0.4)=5 outer brim loops");
    }

    #[test]
    fn raft_prepends_layers_and_shifts_object() {
        let mut layers = vec![square_layer(0.1), square_layer(0.3)];
        let mut p = base_params();
        p.adhesion_type = AdhesionType::Raft;
        p.raft_layers = 3;
        p.raft_air_gap = 0.1;
        let orig_first_z = layers[0].z;
        apply_adhesion(&mut layers, &p);
        assert!(layers.len() > 2, "raft layers must be prepended");
        // Raft base sits on the bed.
        assert!(layers[0].z < orig_first_z + 1e-9);
        // Raft uses Support role.
        assert!(layers[0]
            .path_roles
            .iter()
            .all(|r| *r == ExtrusionRole::Support));
        // Object layer shifted up by raft height + air gap.
        let raft_count = layers.len() - 2;
        let shift = raft_count as f64 * p.layer_height + p.raft_air_gap;
        let obj_first = &layers[raft_count];
        assert!((obj_first.z - (orig_first_z + shift)).abs() < 1e-6);
    }

    #[test]
    fn brim_ears_stamps_corner_discs() {
        let mut layers = vec![square_layer(0.1)];
        let mut p = base_params();
        p.adhesion_type = AdhesionType::Brim;
        p.brim_type = BrimType::Ears;
        p.brim_width = 3.0;
        apply_adhesion(&mut layers, &p);
        let ears = layers[0]
            .path_roles
            .iter()
            .filter(|r| **r == ExtrusionRole::Skirt)
            .count();
        assert!(
            ears > 0,
            "ears brim must stamp loops at the square's corners"
        );
    }

    #[test]
    fn none_is_a_noop() {
        let mut layers = vec![square_layer(0.1)];
        let before = layers[0].paths.len();
        let p = base_params();
        apply_adhesion(&mut layers, &p);
        assert_eq!(layers[0].paths.len(), before);
    }
}
