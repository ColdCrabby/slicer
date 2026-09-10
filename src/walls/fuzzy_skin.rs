//! Fuzzy skin — a cosmetic outer-wall texture.
//!
//! Resamples every layer's outer-wall bead to a near-uniform vertex spacing
//! and displaces each new vertex perpendicular to the wall by a small
//! pseudo-random amount, replacing a smooth outer surface with a rough,
//! hand-textured one.
//!
//! This is deliberately **not** part of [`super::generate_walls`]: it must
//! run after path ordering and flow compensation (so it perturbs the final
//! geometry, and any per-vertex flow-compensation widths already baked into
//! [`SliceLayer::path_vertex_widths`] are carried along rather than
//! invalidated) and before bed adhesion (whose skirt/brim trace the *clean*
//! `OuterWall` centerlines — see [`crate::adhesion`]). The pipeline in
//! [`crate::core::pipeline`] calls [`apply`] at that point.
//!
//! ## Open arcs (overhang-graded outer walls)
//!
//! `classify_overhang_perimeters` (in [`crate::core::walls`]) can split a
//! closed `OuterWall` loop into several **open** sub-paths where it crosses
//! unsupported air: the in-air run becomes `OverhangPerimeter`, and the
//! supported runs either side keep the `OuterWall` role but are no longer
//! closed. Both roles are fuzzed here, open or closed, so a wall that
//! happens to be overhang-graded doesn't leave bald, un-textured segments
//! next to fuzzed ones. Open arcs are resampled with their **two endpoints
//! left untouched**: adjacent runs from the same split share those exact
//! coordinates (see `classify_overhang_perimeters`'s doc comment), and
//! jittering them would tear a visible gap between two runs of what is,
//! physically, one continuous wall.

use clipper2::{Path, Paths};

use crate::core::{ExtrusionRole, SliceLayer};
use crate::settings::params::SlicingParams;

/// Perturb every layer's outer-wall paths — `OuterWall`, open or closed, and
/// `OverhangPerimeter` (the overhang-graded split of an outer or inner wall)
/// — per [`SlicingParams::fuzzy_skin`].
///
/// No-op when the option is off or either magnitude resolves to nothing, so
/// the default configuration never allocates.
pub fn apply(layers: &mut [SliceLayer], params: &SlicingParams) {
    if !params.fuzzy_skin
        || params.fuzzy_skin_thickness_mm <= 0.0
        || params.fuzzy_skin_point_dist_mm <= 0.0
    {
        return;
    }

    let thickness = params.fuzzy_skin_thickness_mm;
    let point_dist = params.fuzzy_skin_point_dist_mm;

    for layer in layers.iter_mut() {
        if layer.path_vertex_widths.len() < layer.paths.len() {
            layer.path_vertex_widths.resize(layer.paths.len(), None);
        }
        // Resampling rewrites the vertex list, so any per-vertex Z profile has
        // to be resampled with it or it is left describing vertices that no
        // longer exist. Empty stays empty — an ordinary print is flat.
        let has_vertex_z = !layer.path_vertex_z.is_empty();
        if has_vertex_z && layer.path_vertex_z.len() < layer.paths.len() {
            layer.path_vertex_z.resize(layer.paths.len(), None);
        }

        let mut new_paths: Vec<Path> = Vec::with_capacity(layer.paths.len());
        let mut new_widths: Vec<Option<Vec<f64>>> = Vec::with_capacity(layer.paths.len());
        let mut new_z: Vec<Option<Vec<f64>>> = Vec::with_capacity(layer.paths.len());

        for (i, path) in layer.paths.iter().enumerate() {
            let widths = layer.path_vertex_widths.get(i).cloned().flatten();
            let vertex_z = layer.path_vertex_z.get(i).cloned().flatten();
            let role = layer.role_for_path(i);
            let is_fuzzable_wall = matches!(
                role,
                ExtrusionRole::OuterWall | ExtrusionRole::OverhangPerimeter
            );

            if is_fuzzable_wall {
                let seed = path_seed(path, layer.z, i);
                let fuzzed = if layer.is_path_open(i) {
                    fuzz_open_path(
                        path,
                        widths.as_deref(),
                        vertex_z.as_deref(),
                        point_dist,
                        thickness,
                        seed,
                    )
                } else {
                    fuzz_closed_path(
                        path,
                        widths.as_deref(),
                        vertex_z.as_deref(),
                        point_dist,
                        thickness,
                        seed,
                    )
                };
                if let Some((fuzzed_path, fuzzed_widths, fuzzed_z)) = fuzzed {
                    new_paths.push(fuzzed_path);
                    new_widths.push(fuzzed_widths);
                    new_z.push(fuzzed_z);
                    continue;
                }
            }
            new_paths.push(path.clone());
            new_widths.push(widths);
            new_z.push(vertex_z);
        }

        layer.paths = Paths::new(new_paths);
        layer.path_vertex_widths = new_widths;
        if has_vertex_z {
            layer.path_vertex_z = new_z;
        }
    }
}

/// Deterministic per-path seed: same path on the same layer always fuzzes
/// the same way (reproducible builds, stable re-slices), while different
/// layers — and different paths on one layer — diverge.
fn path_seed(path: &Path, z: f64, path_index: usize) -> u64 {
    let (x, y) = path
        .iter()
        .next()
        .map(|p| (p.x(), p.y()))
        .unwrap_or((0.0, 0.0));
    x.to_bits()
        ^ y.to_bits().rotate_left(17)
        ^ z.to_bits().rotate_left(31)
        ^ (path_index as u64).rotate_left(47)
}

/// Resample a closed polygon to ~`point_dist_mm` spacing and displace each
/// new vertex perpendicular to its local segment by a uniform random amount
/// in `[-thickness_mm, thickness_mm]`.
///
/// When `source_widths` is `Some` (per-vertex flow-compensated widths, same
/// length as `path`), the returned width vector is interpolated at the same
/// parametric positions so it stays exactly as long as the returned path —
/// the invariant the G-code generator relies on.
///
/// Returns `None` for a degenerate path (fewer than 3 vertices, or zero
/// perimeter), leaving the caller to keep the original geometry untouched.
#[allow(clippy::type_complexity)]
fn fuzz_closed_path(
    path: &Path,
    source_widths: Option<&[f64]>,
    source_z: Option<&[f64]>,
    point_dist_mm: f64,
    thickness_mm: f64,
    seed: u64,
) -> Option<(Path, Option<Vec<f64>>, Option<Vec<f64>>)> {
    let source: Vec<(f64, f64)> = path.iter().map(|p| (p.x(), p.y())).collect();
    let n = source.len();
    if n < 3 {
        return None;
    }
    let source_widths = source_widths.filter(|w| w.len() == n);
    let source_z = source_z.filter(|v| v.len() == n);

    let mut arc = Vec::with_capacity(n);
    let mut total = 0.0;
    for i in 0..n {
        arc.push(total);
        let (ax, ay) = source[i];
        let (bx, by) = source[(i + 1) % n];
        total += ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt();
    }
    if total <= f64::EPSILON {
        return None;
    }

    let sample_count = ((total / point_dist_mm).round() as i64).max(3) as usize;
    let step = total / sample_count as f64;

    let mut rng = SplitMix64::new(seed);
    let mut points = Vec::with_capacity(sample_count);
    let mut widths = source_widths.map(|_| Vec::with_capacity(sample_count));
    // Per-vertex Z is resampled exactly like width: a resampled vertex sits
    // between two source vertices, so it takes the same fraction of the Z
    // profile that it takes of the width profile. Without this a fuzzed
    // non-planar wall would keep a stale array of the wrong length and lose
    // its shape entirely.
    let mut zs = source_z.map(|_| Vec::with_capacity(sample_count));

    let mut seg = 0usize;
    for k in 0..sample_count {
        let target = step * k as f64;
        while seg + 1 < n && arc[seg + 1] <= target {
            seg += 1;
        }
        let (ax, ay) = source[seg];
        let (bx, by) = source[(seg + 1) % n];
        let (dx, dy) = (bx - ax, by - ay);
        let seg_len = (dx * dx + dy * dy).sqrt();
        let t = if seg_len > f64::EPSILON {
            ((target - arc[seg]) / seg_len).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let normal = if seg_len > f64::EPSILON {
            (-dy / seg_len, dx / seg_len)
        } else {
            (0.0, 0.0)
        };
        let r = (rng.next_f64() * 2.0 - 1.0) * thickness_mm;
        points.push((ax + dx * t + normal.0 * r, ay + dy * t + normal.1 * r));

        if let (Some(sw), Some(out)) = (source_widths, widths.as_mut()) {
            let (wa, wb) = (sw[seg], sw[(seg + 1) % n]);
            out.push(wa + (wb - wa) * t);
        }
        if let (Some(sz), Some(out)) = (source_z, zs.as_mut()) {
            let (za, zb) = (sz[seg], sz[(seg + 1) % n]);
            out.push(za + (zb - za) * t);
        }
    }

    Some((points.into(), widths, zs))
}

/// Resample an **open** wall arc the same way as [`fuzz_closed_path`], except
/// the two endpoints are left exactly where they are.
///
/// An open arc is a partial run of a loop split by
/// `classify_overhang_perimeters`; adjacent runs from the same split share
/// their boundary coordinates exactly, so jittering an endpoint would open a
/// visible gap between an overhang run and the (fuzzed) wall run next to it.
/// Only the interior is perturbed.
///
/// Returns `None` for a degenerate path (fewer than 2 vertices, or zero
/// length).
#[allow(clippy::type_complexity)]
fn fuzz_open_path(
    path: &Path,
    source_widths: Option<&[f64]>,
    source_z: Option<&[f64]>,
    point_dist_mm: f64,
    thickness_mm: f64,
    seed: u64,
) -> Option<(Path, Option<Vec<f64>>, Option<Vec<f64>>)> {
    let source: Vec<(f64, f64)> = path.iter().map(|p| (p.x(), p.y())).collect();
    let n = source.len();
    if n < 2 {
        return None;
    }
    let source_widths = source_widths.filter(|w| w.len() == n);
    let source_z = source_z.filter(|v| v.len() == n);

    let mut arc = Vec::with_capacity(n);
    arc.push(0.0);
    let mut total = 0.0;
    for i in 1..n {
        let (ax, ay) = source[i - 1];
        let (bx, by) = source[i];
        total += ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt();
        arc.push(total);
    }
    if total <= f64::EPSILON {
        return None;
    }

    let sample_count = (((total / point_dist_mm).round() as i64) + 1).max(2) as usize;
    let step = total / (sample_count - 1) as f64;

    let mut rng = SplitMix64::new(seed);
    let mut points = Vec::with_capacity(sample_count);
    let mut widths = source_widths.map(|_| Vec::with_capacity(sample_count));
    let mut zs = source_z.map(|_| Vec::with_capacity(sample_count));

    points.push(source[0]);
    if let (Some(sw), Some(out)) = (source_widths, widths.as_mut()) {
        out.push(sw[0]);
    }
    if let (Some(sz), Some(out)) = (source_z, zs.as_mut()) {
        out.push(sz[0]);
    }

    let mut seg = 0usize;
    for k in 1..sample_count - 1 {
        let target = step * k as f64;
        while seg + 1 < n - 1 && arc[seg + 1] <= target {
            seg += 1;
        }
        let (ax, ay) = source[seg];
        let (bx, by) = source[seg + 1];
        let (dx, dy) = (bx - ax, by - ay);
        let seg_len = (dx * dx + dy * dy).sqrt();
        let t = if seg_len > f64::EPSILON {
            ((target - arc[seg]) / seg_len).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let normal = if seg_len > f64::EPSILON {
            (-dy / seg_len, dx / seg_len)
        } else {
            (0.0, 0.0)
        };
        let r = (rng.next_f64() * 2.0 - 1.0) * thickness_mm;
        points.push((ax + dx * t + normal.0 * r, ay + dy * t + normal.1 * r));

        if let (Some(sw), Some(out)) = (source_widths, widths.as_mut()) {
            let (wa, wb) = (sw[seg], sw[seg + 1]);
            out.push(wa + (wb - wa) * t);
        }
        if let (Some(sz), Some(out)) = (source_z, zs.as_mut()) {
            let (za, zb) = (sz[seg], sz[seg + 1]);
            out.push(za + (zb - za) * t);
        }
    }

    points.push(source[n - 1]);
    if let (Some(sw), Some(out)) = (source_widths, widths.as_mut()) {
        out.push(sw[n - 1]);
    }
    if let (Some(sz), Some(out)) = (source_z, zs.as_mut()) {
        out.push(sz[n - 1]);
    }

    Some((points.into(), widths, zs))
}

/// SplitMix64 — the same cheap, well-mixed generator used for deterministic
/// random seam placement in [`crate::core::pipeline`].
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform value in `[0, 1)`.
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ExtrusionRole;

    fn square_layer(side: f64) -> SliceLayer {
        let mut layer = SliceLayer::new(0.2);
        let half = side / 2.0;
        let sq: Path = vec![(-half, -half), (half, -half), (half, half), (-half, half)].into();
        layer.paths.push(sq);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.path_widths.push(None);
        layer.path_vertex_widths.push(None);
        layer.path_is_open.push(false);
        layer
    }

    fn fuzzy_params() -> SlicingParams {
        SlicingParams {
            fuzzy_skin: true,
            fuzzy_skin_thickness_mm: 0.3,
            fuzzy_skin_point_dist_mm: 0.8,
            ..SlicingParams::default()
        }
    }

    #[test]
    fn disabled_by_default_and_a_no_op() {
        let params = SlicingParams::default();
        assert!(!params.fuzzy_skin);
        let mut layers = vec![square_layer(20.0)];
        let before = layers[0].paths.clone();
        apply(&mut layers, &params);
        assert_eq!(layers[0].paths, before);
    }

    #[test]
    fn perturbs_the_outer_wall_within_the_configured_thickness() {
        let params = fuzzy_params();
        let mut layers = vec![square_layer(20.0)];
        apply(&mut layers, &params);

        let path = layers[0].paths.get(0).expect("one path");
        assert!(
            path.len() > 4,
            "resampling should add vertices along a 20mm square at 0.8mm spacing"
        );

        // Every fuzzed vertex must stay within `thickness` of the original
        // 20x20 square's boundary (measured as max coordinate distance from
        // the nominal 10mm half-extent, which bounds the perpendicular jitter
        // near edges and corners alike).
        for p in path.iter() {
            let dx = (p.x().abs() - 10.0).max(0.0);
            let dy = (p.y().abs() - 10.0).max(0.0);
            assert!(
                dx <= params.fuzzy_skin_thickness_mm + 1e-9,
                "x deviates too far: {}",
                p.x()
            );
            assert!(
                dy <= params.fuzzy_skin_thickness_mm + 1e-9,
                "y deviates too far: {}",
                p.y()
            );
        }
    }

    #[test]
    fn leaves_non_outer_wall_paths_untouched() {
        let params = fuzzy_params();
        let mut layer = square_layer(20.0);
        let inner: Path = vec![(-9.0, -9.0), (9.0, -9.0), (9.0, 9.0), (-9.0, 9.0)].into();
        layer.paths.push(inner.clone());
        layer.path_roles.push(ExtrusionRole::InnerWall);
        layer.path_widths.push(None);
        layer.path_vertex_widths.push(None);
        layer.path_is_open.push(false);

        let mut layers = vec![layer];
        apply(&mut layers, &params);

        assert_eq!(layers[0].paths.get(1), Some(&inner));
    }

    #[test]
    fn open_outer_wall_arcs_are_fuzzed_too() {
        // classify_overhang_perimeters splits a closed OuterWall loop into
        // open runs where it crosses air; the supported runs keep the
        // OuterWall role but are no longer closed. These must not be left
        // smooth next to the fuzzed rest of the wall.
        let params = fuzzy_params();
        let mut layer = SliceLayer::new(0.2);
        let arc: Path = vec![(-10.0, -10.0), (10.0, -10.0), (10.0, 10.0)].into();
        layer.paths.push(arc.clone());
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.path_widths.push(None);
        layer.path_vertex_widths.push(None);
        layer.path_is_open.push(true);

        let mut layers = vec![layer];
        apply(&mut layers, &params);

        let fuzzed = layers[0].paths.get(0).expect("one path");
        assert!(
            fuzzed.len() > arc.len(),
            "the open arc should be resampled and perturbed, not left alone"
        );
    }

    #[test]
    fn overhang_perimeter_arcs_are_fuzzed_too() {
        let params = fuzzy_params();
        let mut layer = SliceLayer::new(0.2);
        let arc: Path = vec![(-10.0, -10.0), (10.0, -10.0), (10.0, 10.0)].into();
        layer.paths.push(arc.clone());
        layer.path_roles.push(ExtrusionRole::OverhangPerimeter);
        layer.path_widths.push(None);
        layer.path_vertex_widths.push(None);
        layer.path_is_open.push(true);

        let mut layers = vec![layer];
        apply(&mut layers, &params);

        let fuzzed = layers[0].paths.get(0).expect("one path");
        assert!(
            fuzzed.len() > arc.len(),
            "an overhang-graded run of the outer wall should be fuzzed too"
        );
    }

    #[test]
    fn open_arc_endpoints_stay_exactly_put() {
        // Two adjacent runs from the same overhang split share an exact
        // boundary coordinate; fuzzing an endpoint would tear a gap between
        // them, so only the interior of an open arc may move.
        let params = fuzzy_params();
        let mut layer = SliceLayer::new(0.2);
        let arc: Path = vec![(-10.0, -10.0), (0.0, -10.0), (10.0, -10.0), (10.0, 10.0)].into();
        layer.paths.push(arc.clone());
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.path_widths.push(None);
        layer.path_vertex_widths.push(None);
        layer.path_is_open.push(true);

        let mut layers = vec![layer];
        apply(&mut layers, &params);

        let fuzzed = layers[0].paths.get(0).expect("one path");
        let first = fuzzed.iter().next().expect("first point");
        let last = fuzzed.iter().next_back().expect("last point");
        let (ox0, oy0) = (
            arc.iter().next().unwrap().x(),
            arc.iter().next().unwrap().y(),
        );
        let (oxn, oyn) = (
            arc.iter().next_back().unwrap().x(),
            arc.iter().next_back().unwrap().y(),
        );
        assert_eq!((first.x(), first.y()), (ox0, oy0));
        assert_eq!((last.x(), last.y()), (oxn, oyn));
    }

    #[test]
    fn is_deterministic_across_runs() {
        let params = fuzzy_params();
        let mut a = vec![square_layer(20.0)];
        let mut b = vec![square_layer(20.0)];
        apply(&mut a, &params);
        apply(&mut b, &params);
        assert_eq!(a[0].paths, b[0].paths);
    }

    #[test]
    fn keeps_vertex_widths_aligned_with_the_resampled_path() {
        let params = fuzzy_params();
        let mut layer = square_layer(20.0);
        layer.path_vertex_widths[0] = Some(vec![0.4, 0.4, 0.4, 0.4]);

        let mut layers = vec![layer];
        apply(&mut layers, &params);

        let path_len = layers[0].paths.get(0).expect("one path").len();
        let widths = layers[0].path_vertex_widths[0]
            .as_ref()
            .expect("widths preserved");
        assert_eq!(
            widths.len(),
            path_len,
            "vertex widths must stay in lockstep with the resampled path"
        );
    }

    #[test]
    fn different_layers_fuzz_differently() {
        let params = fuzzy_params();
        let mut low = SliceLayer::new(0.2);
        let mut high = SliceLayer::new(5.0);
        for layer in [&mut low, &mut high] {
            let sq: Path = vec![(-10.0, -10.0), (10.0, -10.0), (10.0, 10.0), (-10.0, 10.0)].into();
            layer.paths.push(sq);
            layer.path_roles.push(ExtrusionRole::OuterWall);
            layer.path_widths.push(None);
            layer.path_vertex_widths.push(None);
            layer.path_is_open.push(false);
        }
        let mut layers = vec![low, high];
        apply(&mut layers, &params);
        assert_ne!(
            layers[0].paths.get(0),
            layers[1].paths.get(0),
            "different Z should pick a different fuzz pattern"
        );
    }
}
