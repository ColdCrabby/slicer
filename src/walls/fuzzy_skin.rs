//! Fuzzy skin — a cosmetic outer-wall texture.
//!
//! Resamples every layer's closed `OuterWall` bead to a near-uniform vertex
//! spacing and displaces each new vertex perpendicular to the wall by a
//! small pseudo-random amount, replacing a smooth outer surface with a
//! rough, hand-textured one.
//!
//! This is deliberately **not** part of [`super::generate_walls`]: it must
//! run after path ordering and flow compensation (so it perturbs the final
//! geometry, and any per-vertex flow-compensation widths already baked into
//! [`SliceLayer::path_vertex_widths`] are carried along rather than
//! invalidated) and before bed adhesion (whose skirt/brim trace the *clean*
//! `OuterWall` centerlines — see [`crate::adhesion`]). The pipeline in
//! [`crate::core::pipeline`] calls [`apply`] at that point.

use clipper2::{Path, Paths};

use crate::core::{ExtrusionRole, SliceLayer};
use crate::settings::params::SlicingParams;

/// Perturb every layer's closed `OuterWall` beads per
/// [`SlicingParams::fuzzy_skin`].
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

        let mut new_paths: Vec<Path> = Vec::with_capacity(layer.paths.len());
        let mut new_widths: Vec<Option<Vec<f64>>> = Vec::with_capacity(layer.paths.len());

        for (i, path) in layer.paths.iter().enumerate() {
            let widths = layer.path_vertex_widths.get(i).cloned().flatten();
            let is_outer_loop =
                layer.role_for_path(i) == ExtrusionRole::OuterWall && !layer.is_path_open(i);

            if is_outer_loop {
                let seed = path_seed(path, layer.z, i);
                if let Some((fuzzed, fuzzed_widths)) =
                    fuzz_closed_path(path, widths.as_deref(), point_dist, thickness, seed)
                {
                    new_paths.push(fuzzed);
                    new_widths.push(fuzzed_widths);
                    continue;
                }
            }
            new_paths.push(path.clone());
            new_widths.push(widths);
        }

        layer.paths = Paths::new(new_paths);
        layer.path_vertex_widths = new_widths;
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
fn fuzz_closed_path(
    path: &Path,
    source_widths: Option<&[f64]>,
    point_dist_mm: f64,
    thickness_mm: f64,
    seed: u64,
) -> Option<(Path, Option<Vec<f64>>)> {
    let source: Vec<(f64, f64)> = path.iter().map(|p| (p.x(), p.y())).collect();
    let n = source.len();
    if n < 3 {
        return None;
    }
    let source_widths = source_widths.filter(|w| w.len() == n);

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
    }

    Some((points.into(), widths))
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
    fn open_outer_wall_paths_are_left_alone() {
        // Arachne's classify_overhang_perimeters can split a closed OuterWall
        // loop into open arcs; fuzzy skin must not treat those as closed.
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

        assert_eq!(layers[0].paths.get(0), Some(&arc));
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
