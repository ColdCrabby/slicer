use crate::mesh::types::Mesh;
use glam::Vec3;
use std::collections::HashMap;
use std::f64::consts::PI;

use super::types::AutoOrientOptions;

/// Maximum number of flat-face candidates kept from the normal histogram.
/// Higher values give marginally better coverage at the cost of more scoring
/// work.  64 is ample for all practical FDM models.
const MAX_FLAT_CANDIDATES: usize = 64;

/// Collect the "floor direction" candidates to evaluate.
///
/// Uses a fast O(F) normal-direction histogram:
/// - Each face normal is snapped to a coarse ~6° grid.
/// - Areas are accumulated per bucket.
/// - The top [`MAX_FLAT_CANDIDATES`] buckets (by total area) become candidates.
///
/// This merges near-duplicate normals on curved surfaces into a single
/// representative direction — avoiding the thousands of near-identical
/// candidates that `compute_coplanar_groups` would produce for an organic
/// mesh like a Benchy — while still capturing every significant flat region.
///
/// If [`AutoOrientOptions::allow_rotations`] is `true`, also adds ~128 points
/// uniformly distributed on the sphere (Fibonacci spiral).
///
/// The mesh's own `−Z` is always **first** in the result so the caller can give
/// "stay as the author placed it" the tie-break, and so a model that is already
/// well-oriented is never spun for a floating-point-sized improvement.
pub(super) fn build_candidates(
    _mesh: &Mesh,
    options: &AutoOrientOptions,
    normals: &[Vec3],
    areas: &[f64],
) -> Vec<Vec3> {
    // -------------------------------------------------------------------------
    // 1. Area-weighted normal histogram with ~6° angular bucketing.
    //
    // Each component is rounded to the nearest 0.1 step in [-1, 1], giving
    // ≈6° angular resolution.  All near-duplicate normals (e.g., the slightly
    // curved triangles on a Benchy hull) collapse into the same bucket, so we
    // get one clean representative direction instead of hundreds of near-copies.
    // -------------------------------------------------------------------------
    let mut dir_accum: HashMap<[i32; 3], (Vec3, f64)> = HashMap::new();

    for (i, n) in normals.iter().enumerate() {
        let area = areas[i];
        let key = [
            (n.x * 10.0).round() as i32,
            (n.y * 10.0).round() as i32,
            (n.z * 10.0).round() as i32,
        ];
        let entry = dir_accum.entry(key).or_insert((Vec3::ZERO, 0.0));
        entry.0 += *n * area as f32;
        entry.1 += area;
    }

    // Sort buckets by accumulated area (descending), take top MAX_FLAT_CANDIDATES.
    let mut buckets: Vec<(Vec3, f64)> = dir_accum
        .into_values()
        .filter(|(_, area)| *area > 1e-6)
        .collect();
    buckets.sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    buckets.truncate(MAX_FLAT_CANDIDATES);

    // Lead with the mesh's own −Z so "leave it as the author exported it" wins
    // every tie — see `STAY_PUT_BONUS`, which keys off index 0.
    let mut candidates: Vec<Vec3> = vec![Vec3::NEG_Z];
    candidates.extend(buckets.into_iter().filter_map(|(n_sum, area)| {
        let n = (n_sum / area as f32).normalize_or_zero();
        // Reject near-degenerate normals whose magnitude is less than 0.5
        // (squared: 0.25). This guards against floating-point cancellation
        // when many tiny faces produce an almost-zero sum vector.
        if n.length_squared() > 0.25 {
            Some(n)
        } else {
            None
        }
    }));

    // -------------------------------------------------------------------------
    // 2. Fibonacci sphere (allow_rotations only)
    //
    // Adds ~128 uniformly-distributed directions to cover organic shapes with
    // no prominent flat regions (figurines, characters, etc.).
    // -------------------------------------------------------------------------
    if options.allow_rotations {
        const N: usize = 128;
        let golden = (1.0 + 5.0_f64.sqrt()) / 2.0;
        for i in 0..N {
            let cos_theta = 1.0 - 2.0 * (i as f64 + 0.5) / N as f64;
            let theta = cos_theta.acos() as f32;
            let phi = (2.0 * PI * i as f64 / golden) as f32;
            candidates.push(Vec3::new(
                theta.sin() * phi.cos(),
                theta.sin() * phi.sin(),
                theta.cos(),
            ));
        }
    }

    candidates
}
