//! Concentric fill: loops following the region outline, stepping inward.
//!
//! Shared by sparse infill and by the solid-surface `Concentric` pattern — the
//! geometry is identical, only the density differs.

use clipper2::*;

/// Maximum number of loops generated for one region, as a guard against a
/// pathological offset sequence that never collapses.
const MAX_LOOPS: usize = 4096;

/// Fill a region with concentric loops stepping inward `spacing / density` at a
/// time.
///
/// libslic3r's `FillConcentric`: each level is an `offset2` — erode by
/// `distance + spacing/2`, dilate back by `spacing/2` — so the net inward step
/// is exactly `distance` while any sliver narrower than half a bead is wiped out
/// instead of collapsing into a degenerate micro-loop.
///
/// Loops are emitted with their first point repeated at the end: the G-code
/// generator only auto-closes inherently closed roles (walls, skirt), so an
/// explicit closing vertex is what keeps a fill loop from ending one segment
/// short of its start.
///
/// Loops shorter than `min_length_mm` are dropped — a sub-threshold ring is the
/// same isolated dab the sparse-infill splat filter exists to remove.
pub fn generate_concentric(
    region: &Paths,
    spacing: f64,
    density: f64,
    min_length_mm: f64,
) -> Paths {
    if region.is_empty() || spacing <= 0.0 || density <= 0.0 {
        return Paths::new(vec![]);
    }

    let distance = (spacing / density.clamp(1e-6, 1.0)).max(0.01);
    let min_len = min_length_mm.max(0.0);
    let mut result = Paths::new(vec![]);
    let mut current = region.clone();

    for _ in 0..MAX_LOOPS {
        if current.is_empty() {
            break;
        }

        for loop_path in current.iter() {
            let mut pts: Vec<(f64, f64)> = loop_path.iter().map(|p| (p.x(), p.y())).collect();
            if pts.len() < 3 {
                continue;
            }
            let perimeter: f64 = pts
                .iter()
                .zip(pts.iter().cycle().skip(1))
                .take(pts.len())
                .map(|(a, b)| ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt())
                .sum();
            if perimeter < min_len {
                continue;
            }
            pts.push(pts[0]);
            let path: Path = pts.into();
            result.push(path);
        }

        let eroded = inflate(
            current.clone(),
            -(distance + spacing * 0.5),
            JoinType::Round,
            EndType::Polygon,
            2.0,
        );
        if eroded.is_empty() {
            break;
        }
        current = inflate(
            eroded,
            spacing * 0.5,
            JoinType::Round,
            EndType::Polygon,
            2.0,
        );
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(size: f64) -> Paths {
        let path: Path = vec![(0.0, 0.0), (size, 0.0), (size, size), (0.0, size)].into();
        Paths::new(vec![path])
    }

    #[test]
    fn empty_region_yields_nothing() {
        assert!(generate_concentric(&Paths::default(), 0.4, 1.0, 0.0).is_empty());
    }

    #[test]
    fn loops_close_explicitly() {
        let loops = generate_concentric(&square(20.0), 0.4, 1.0, 0.0);
        assert!(!loops.is_empty());
        for l in loops.iter() {
            let pts: Vec<(f64, f64)> = l.iter().map(|p| (p.x(), p.y())).collect();
            let first = pts[0];
            let last = *pts.last().expect("non-empty");
            assert!((first.0 - last.0).abs() < 1e-6 && (first.1 - last.1).abs() < 1e-6);
        }
    }

    #[test]
    fn lower_density_means_fewer_loops() {
        let dense = generate_concentric(&square(20.0), 0.4, 1.0, 0.0);
        let sparse = generate_concentric(&square(20.0), 0.4, 0.2, 0.0);
        assert!(
            sparse.len() < dense.len(),
            "sparse ({}) should need fewer loops than solid ({})",
            sparse.len(),
            dense.len()
        );
    }
}
