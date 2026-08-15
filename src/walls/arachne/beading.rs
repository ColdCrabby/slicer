//! Beading strategy — how many beads cross a given wall thickness, and how wide
//! they are (Phase 2b of the Arachne generator).
//!
//! Given the *local wall thickness* `t` (= twice the medial-axis
//! [`radius`](super::skeleton::SkeletonNode::radius)), the beading strategy
//! decides:
//!
//! * how many beads span `t` ([`BeadingConfig::optimal_bead_count`]);
//! * each bead's width (mm);
//! * each bead's centerline offset across `t` (0 = one boundary, `t` = the
//!   other), laid out **symmetrically** about the medial axis with any unfilled
//!   remainder as a central gap that becomes infill.
//!
//! This is a self-contained, deterministic function of `t` and the resolved
//! [`WallParams`]; it holds no geometry.  The skeleton walk (Phase 2c) samples
//! it along each medial edge and reconstructs continuous variable-width loops,
//! de-noising count changes shorter than
//! [`WallParams::wall_transition_filter_distance_mm`].
//!
//! ## Strategy (clean-room, principled)
//!
//! * A region thinner than the minimum bead width gets **no** bead (it is left
//!   to infill / is a feature too thin to print).
//! * The bead count grows by one each time `t` exceeds the current count's
//!   optimum (`count × nozzle`) by more than
//!   [`WallParams::wall_transition_threshold`] × nozzle — the hysteresis that
//!   avoids flip-flopping a wall on and off along a slowly tapering feature.
//! * The count is capped by [`WallParams::wall_count`] and by the requirement
//!   that no bead be thinner than the minimum width.
//! * If the capped beads cannot fill `t`, they stay at the optimal (nozzle)
//!   width and the leftover sits in the centre as infill.  Otherwise the beads
//!   share `t` equally (each within `[min, max]` width).
//!
//! Exact CuraEngine parity is **not** a goal; a monotonic, gap-free-where-
//! possible, deterministic layout is.

// Some `Beading` fields (`left_over`, `thickness`, `bead_count`) are part of the
// layout contract but not read in every build configuration (only by the walk's
// callers and the unit tests); keep them without a dead-code warning.
#![allow(dead_code)]

use crate::walls::WallParams;

/// Resolved beading configuration, all widths in absolute mm.
#[derive(Debug, Clone, Copy)]
pub struct BeadingConfig {
    /// Preferred (optimal) bead width — the nozzle diameter.
    pub optimal_width: f64,
    /// Minimum permissible bead width.
    pub min_width: f64,
    /// Maximum permissible bead width.
    pub max_width: f64,
    /// Maximum number of beads across a thickness.
    pub wall_count: usize,
    /// Extra thickness (mm) beyond a count's optimum before a bead is added.
    pub transition_threshold: f64,
}

impl BeadingConfig {
    /// Build from the resolved [`WallParams`].
    pub fn from_wall_params(p: &WallParams) -> Self {
        Self {
            optimal_width: p.nozzle_diameter_mm,
            min_width: p.wall_line_width_min_mm,
            max_width: p.wall_line_width_max_mm,
            wall_count: p.wall_count,
            transition_threshold: p.wall_transition_threshold * p.nozzle_diameter_mm,
        }
    }

    /// Number of beads that should span a wall of thickness `t` (mm).
    ///
    /// Returns 0 for thicknesses below the minimum bead width.  Monotonically
    /// non-decreasing in `t`.
    pub fn optimal_bead_count(&self, t: f64) -> usize {
        if t < self.min_width || self.wall_count == 0 {
            return 0;
        }
        // Grow the count once t exceeds count×optimal by the transition margin.
        let d = self.optimal_width;
        let mut n = (((t - self.transition_threshold) / d).floor() as isize + 1).max(1) as usize;
        n = n.min(self.wall_count);
        // No bead may be thinner than min_width: reduce the count until each
        // bead can be at least min_width wide.
        while n > 1 && t / (n as f64) < self.min_width {
            n -= 1;
        }
        n
    }

    /// Compute the full bead layout for a wall of thickness `t` (mm), choosing
    /// the bead count with [`Self::optimal_bead_count`].
    pub fn compute(&self, t: f64) -> Beading {
        self.layout(t, self.optimal_bead_count(t))
    }

    /// Lay out `n` beads across a wall of thickness `t` (mm).
    ///
    /// Unlike [`Self::compute`], the count `n` is supplied by the caller — the
    /// skeleton walk picks it per node and then de-noises it, so the layout must
    /// honour that choice rather than re-deriving the optimum.  `n == 0` yields
    /// an empty beading whose whole thickness is left over (infill).
    pub fn layout(&self, t: f64, n: usize) -> Beading {
        if n == 0 {
            return Beading {
                bead_count: 0,
                widths: Vec::new(),
                locations: Vec::new(),
                thickness: t,
                left_over: t.max(0.0),
            };
        }

        let per = t / n as f64;
        let (widths, left_over) = if per <= self.max_width {
            // Beads share the thickness equally and fill it exactly.
            (vec![per; n], 0.0)
        } else {
            // Count-limited (capped by wall_count): beads stay at optimal width
            // and the remainder is a central infill gap.
            let w = self.optimal_width;
            (vec![w; n], t - n as f64 * w)
        };

        let locations = symmetric_layout(&widths, t);
        Beading {
            bead_count: n,
            widths,
            locations,
            thickness: t,
            left_over,
        }
    }
}

/// The bead layout for one local wall thickness.
#[derive(Debug, Clone, PartialEq)]
pub struct Beading {
    /// Number of beads across the thickness.
    pub bead_count: usize,
    /// Width (mm) of each bead, ordered from the 0-boundary inward.
    pub widths: Vec<f64>,
    /// Centerline offset (mm) of each bead across the thickness (`0..=thickness`),
    /// laid out symmetrically about `thickness / 2`.
    pub locations: Vec<f64>,
    /// Thickness (mm) this beading was computed for.
    pub thickness: f64,
    /// Unfilled width (mm) left in the centre (becomes infill).
    pub left_over: f64,
}

/// Lay `widths` out across `[0, thickness]` symmetrically about the centre,
/// placing beads inward from both boundaries with any remainder as a central
/// gap.
fn symmetric_layout(widths: &[f64], thickness: f64) -> Vec<f64> {
    let n = widths.len();
    let mut loc = vec![0.0; n];
    let half = n.div_ceil(2);

    // Beads [0, half) march inward from the 0-boundary.
    let mut cursor = 0.0;
    for (i, w) in widths.iter().enumerate().take(half) {
        cursor += w / 2.0;
        loc[i] = cursor;
        cursor += w / 2.0;
    }
    // Remaining beads march inward from the thickness-boundary.
    let mut cursor_r = thickness;
    for j in 0..(n - half) {
        let idx = n - 1 - j;
        cursor_r -= widths[idx] / 2.0;
        loc[idx] = cursor_r;
        cursor_r -= widths[idx] / 2.0;
    }
    loc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::params::SlicingParams;

    fn cfg() -> BeadingConfig {
        // nozzle 0.4, min 0.85×=0.34, max 1.5×=0.6, wall_count 3, threshold 0.6×=0.24
        BeadingConfig::from_wall_params(&WallParams::from_slicing_params(&SlicingParams::default()))
    }

    #[test]
    fn no_bead_below_min_width() {
        let c = cfg();
        assert_eq!(c.optimal_bead_count(0.2), 0, "0.2mm < min 0.34mm → 0 beads");
        assert_eq!(c.compute(0.2).bead_count, 0);
    }

    #[test]
    fn single_bead_around_nozzle_width() {
        let c = cfg();
        assert_eq!(c.optimal_bead_count(0.4), 1);
        let b = c.compute(0.4);
        assert_eq!(b.bead_count, 1);
        assert!((b.widths[0] - 0.4).abs() < 1e-9);
        assert!((b.locations[0] - 0.2).abs() < 1e-9, "single bead centred");
        assert!(b.left_over.abs() < 1e-9);
    }

    #[test]
    fn bead_count_is_monotonic_nondecreasing() {
        let c = cfg();
        let mut prev = 0;
        let mut t = 0.0;
        while t < 5.0 {
            let n = c.optimal_bead_count(t);
            assert!(n >= prev, "count dropped at t={t}: {prev} → {n}");
            assert!(n <= c.wall_count, "count exceeded wall_count at t={t}");
            prev = n;
            t += 0.01;
        }
    }

    #[test]
    fn three_optimal_beads_fill_exactly() {
        let c = cfg();
        // 1.2 mm = 3 × 0.4 → 3 beads, no leftover, evenly spaced.
        let b = c.compute(1.2);
        assert_eq!(b.bead_count, 3);
        for w in &b.widths {
            assert!((w - 0.4).abs() < 1e-9);
        }
        assert!(b.left_over.abs() < 1e-6);
        // Symmetric about 0.6.
        assert!((b.locations[0] - 0.2).abs() < 1e-9);
        assert!((b.locations[1] - 0.6).abs() < 1e-9);
        assert!((b.locations[2] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn thick_wall_is_capped_with_central_infill_gap() {
        let c = cfg();
        // 4 mm wall, cap 3 → 3 optimal-width beads + central leftover.
        let b = c.compute(4.0);
        assert_eq!(b.bead_count, 3);
        for w in &b.widths {
            assert!((w - 0.4).abs() < 1e-9, "capped beads stay at nozzle width");
        }
        assert!(
            (b.left_over - (4.0 - 3.0 * 0.4)).abs() < 1e-9,
            "leftover should be the central infill gap, got {}",
            b.left_over
        );
    }

    #[test]
    fn locations_stay_within_thickness_and_symmetric() {
        let c = cfg();
        for &t in &[0.4, 0.7, 1.0, 1.2, 1.6, 2.0, 3.5] {
            let b = c.compute(t);
            for &l in &b.locations {
                assert!(l >= -1e-9 && l <= t + 1e-9, "loc {l} out of [0,{t}]");
            }
            if b.bead_count > 0 {
                // Outermost/innermost beads mirror about t/2.
                let first = b.locations[0];
                let last = b.locations[b.bead_count - 1];
                assert!(
                    ((first + last) - t).abs() < 1e-6,
                    "outer beads not symmetric for t={t}: {first} + {last} != {t}"
                );
            }
        }
    }

    #[test]
    fn transition_threshold_delays_extra_bead() {
        let c = cfg(); // threshold 0.24 mm
                       // Just past 1 optimal bead (0.4) but below the transition margin stays 1.
        assert_eq!(c.optimal_bead_count(0.55), 1);
        // 2 optimal (0.8) + margin (0.24) = 1.04 mm before 3rd bead appears.
        assert_eq!(c.optimal_bead_count(1.0), 2);
    }
}
