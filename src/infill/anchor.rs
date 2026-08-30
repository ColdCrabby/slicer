//! Anchoring sparse infill to the perimeter it ends against.
//!
//! Every sparse-infill line stops dead where it meets the inner wall. Letting it
//! turn and follow the wall for a short distance does two things: the line is
//! welded to the shell instead of merely touching it, and two lines that meet
//! around a short stretch of wall become one continuous move, removing a
//! retract → travel → un-retract pair.
//!
//! This is a port of libslic3r's `Fill::connect_infill`
//! (`FillBase.cpp:1431-1655`), driven by the same two knobs:
//!
//! | Setting | Meaning |
//! |---|---|
//! | `anchor_length_max` | longest wall stretch that may join **two** lines; `0` disables anchoring entirely |
//! | `anchor_length` | how far a **single** unpaired line end may run along the wall; `0` disables open anchors |
//!
//! The walk always follows the boundary, never cuts across the region, so an
//! anchor can only ever be laid where the fill area already reaches.

use clipper2::*;

/// Distance (mm) within which a line endpoint counts as sitting *on* the
/// boundary.
///
/// Infill lines are clipped to the fill region, so their ends land on its
/// outline to within Clipper2's Centi (0.01 mm) quantisation. Ends further away
/// than this are interior — a line cut short by a filter, say — and are left
/// alone, because anchoring one would strike out across the fill.
const ON_BOUNDARY_TOLERANCE_MM: f64 = 0.05;

/// libslic3r's `FillParams::dont_connect()` threshold: below this the anchoring
/// pass is skipped outright.
const MIN_ANCHOR_MAX_MM: f64 = 0.05;

/// A closed boundary ring with cumulative arc lengths, so a point can be located
/// along it and a stretch between two points walked out.
struct Contour {
    pts: Vec<(f64, f64)>,
    /// Cumulative distance from `pts[0]` to `pts[i]`; `cum[0] == 0`.
    cum: Vec<f64>,
    total: f64,
}

impl Contour {
    fn new(path: &clipper2::Path) -> Option<Self> {
        let mut pts: Vec<(f64, f64)> = path.iter().map(|p| (p.x(), p.y())).collect();
        if pts.len() >= 2 {
            let first = pts[0];
            let last = *pts.last().expect("checked non-empty");
            if (first.0 - last.0).abs() < 1e-9 && (first.1 - last.1).abs() < 1e-9 {
                pts.pop();
            }
        }
        if pts.len() < 3 {
            return None;
        }

        let mut cum = Vec::with_capacity(pts.len());
        let mut acc = 0.0;
        for i in 0..pts.len() {
            cum.push(acc);
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            acc += dist(a, b);
        }
        if acc <= 0.0 {
            return None;
        }
        Some(Self {
            pts,
            cum,
            total: acc,
        })
    }

    /// Distance from `p` to this ring, and the arc position of the closest point.
    fn nearest(&self, p: (f64, f64)) -> (f64, f64) {
        let mut best = (f64::MAX, 0.0);
        for i in 0..self.pts.len() {
            let a = self.pts[i];
            let b = self.pts[(i + 1) % self.pts.len()];
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len_sq = dx * dx + dy * dy;
            let t = if len_sq <= 0.0 {
                0.0
            } else {
                (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len_sq).clamp(0.0, 1.0)
            };
            let proj = (a.0 + t * dx, a.1 + t * dy);
            let d = dist(p, proj);
            if d < best.0 {
                best = (d, self.cum[i] + t * len_sq.sqrt());
            }
        }
        best
    }

    /// The point sitting `s` mm along the ring.
    fn point_at(&self, s: f64) -> (f64, f64) {
        let s = s.rem_euclid(self.total);
        let n = self.pts.len();
        for i in 0..n {
            let seg_start = self.cum[i];
            let seg_end = if i + 1 < n {
                self.cum[i + 1]
            } else {
                self.total
            };
            if s <= seg_end || i + 1 == n {
                let a = self.pts[i];
                let b = self.pts[(i + 1) % n];
                let seg_len = seg_end - seg_start;
                let t = if seg_len <= 0.0 {
                    0.0
                } else {
                    (s - seg_start) / seg_len
                };
                return (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1));
            }
        }
        self.pts[0]
    }

    /// Arc length from `a` to `b` walking in the ring's stored direction.
    fn forward_len(&self, a: f64, b: f64) -> f64 {
        (b - a).rem_euclid(self.total)
    }

    /// The points of the stretch from arc position `a` to `b`, walking forward.
    ///
    /// Includes every ring vertex in between plus the exact end point, but not
    /// the start point — the caller already has that as the line's own endpoint.
    fn walk(&self, a: f64, b: f64) -> Vec<(f64, f64)> {
        let span = self.forward_len(a, b);
        let n = self.pts.len();
        let mut out = Vec::new();
        // First ring vertex strictly past `a`; wraps to 0 when `a` lies in the
        // closing segment.
        let start = (0..n).find(|&i| self.cum[i] > a + 1e-12).unwrap_or(0);
        for k in 0..n {
            let idx = (start + k) % n;
            let travelled = self.forward_len(a, self.cum[idx]);
            if travelled >= span {
                break;
            }
            out.push(self.pts[idx]);
        }
        out.push(self.point_at(b));
        out
    }
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()
}

/// Which end of which polyline sits where on the boundary.
struct EndRef {
    contour: usize,
    arc: f64,
    /// Index into the working polyline list; updated as polylines merge.
    poly: usize,
    /// `true` when this is the polyline's first point.
    at_start: bool,
    consumed: bool,
}

/// Join and anchor infill lines along `boundary`.
///
/// Returns the polylines to print. Lines whose ends are not on the boundary, or
/// that find no partner, come back unchanged.
pub(crate) fn connect_infill(
    lines: Paths,
    boundary: &Paths,
    anchor_length_mm: f64,
    anchor_length_max_mm: f64,
) -> Paths {
    if anchor_length_max_mm < MIN_ANCHOR_MAX_MM || lines.is_empty() || boundary.is_empty() {
        return lines;
    }

    let contours: Vec<Contour> = boundary.iter().filter_map(Contour::new).collect();
    if contours.is_empty() {
        return lines;
    }

    // Working set of polylines. `None` marks one that has been absorbed by a merge.
    let mut polys: Vec<Option<Vec<(f64, f64)>>> = lines
        .iter()
        .map(|p| Some(p.iter().map(|v| (v.x(), v.y())).collect::<Vec<_>>()))
        .collect();

    let mut ends: Vec<EndRef> = Vec::new();
    for (i, poly) in polys.iter().enumerate() {
        let Some(pts) = poly else { continue };
        if pts.len() < 2 {
            continue;
        }
        for at_start in [true, false] {
            let p = if at_start {
                pts[0]
            } else {
                *pts.last().expect("len >= 2")
            };
            if let Some((contour, arc)) = locate_on_boundary(&contours, p) {
                ends.push(EndRef {
                    contour,
                    arc,
                    poly: i,
                    at_start,
                    consumed: false,
                });
            }
        }
    }
    if ends.is_empty() {
        return lines;
    }

    // ── Pair up line ends around short stretches of wall ─────────────────────
    //
    // Only ends that are *neighbours* along a contour are candidates: with no
    // other end between them, the wall stretch that joins them is unclaimed.
    // Working shortest-first means the cheapest joins are made before a longer
    // one can consume an end it needed.
    let mut candidates: Vec<(f64, usize, usize)> = Vec::new();
    for (c, contour) in contours.iter().enumerate() {
        let mut on_contour: Vec<usize> =
            (0..ends.len()).filter(|&e| ends[e].contour == c).collect();
        if on_contour.len() < 2 {
            continue;
        }
        on_contour.sort_by(|&a, &b| ends[a].arc.total_cmp(&ends[b].arc));
        for k in 0..on_contour.len() {
            let from = on_contour[k];
            let to = on_contour[(k + 1) % on_contour.len()];
            let len = contour.forward_len(ends[from].arc, ends[to].arc);
            if len <= anchor_length_max_mm {
                candidates.push((len, from, to));
            }
        }
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));

    for (_, from, to) in candidates {
        if ends[from].consumed || ends[to].consumed {
            continue;
        }
        // Both ends of one polyline: joining them would close it into a loop.
        if ends[from].poly == ends[to].poly {
            continue;
        }
        merge_across_boundary(&mut polys, &mut ends, &contours, from, to);
    }

    // ── Anchor whatever is still loose ───────────────────────────────────────
    //
    // A lone line end runs along the wall for up to `anchor_length`, capped at
    // half the free stretch so two neighbouring anchors cannot overlap.
    if anchor_length_mm > 0.0 {
        let free = free_span_after(&contours, &ends);
        for e in 0..ends.len() {
            if ends[e].consumed {
                continue;
            }
            let reach = anchor_length_mm.min(free[e] * 0.5);
            if reach <= ON_BOUNDARY_TOLERANCE_MM {
                continue;
            }
            let contour = &contours[ends[e].contour];
            let target = ends[e].arc + reach;
            let tail = contour.walk(ends[e].arc, target);
            let (poly_idx, at_start) = (ends[e].poly, ends[e].at_start);
            if let Some(pts) = polys[poly_idx].as_mut() {
                if at_start {
                    for p in tail {
                        pts.insert(0, p);
                    }
                } else {
                    pts.extend(tail);
                }
            }
        }
    }

    let mut out = Paths::default();
    for poly in polys.into_iter().flatten() {
        if poly.len() >= 2 {
            let path: clipper2::Path = poly.into();
            out.push(path);
        }
    }
    out
}

/// Locate a point on the nearest boundary contour, if it sits on one.
fn locate_on_boundary(contours: &[Contour], p: (f64, f64)) -> Option<(usize, f64)> {
    let mut best: Option<(f64, usize, f64)> = None;
    for (i, contour) in contours.iter().enumerate() {
        let (d, arc) = contour.nearest(p);
        if best.is_none_or(|(bd, _, _)| d < bd) {
            best = Some((d, i, arc));
        }
    }
    best.filter(|&(d, _, _)| d <= ON_BOUNDARY_TOLERANCE_MM)
        .map(|(_, i, arc)| (i, arc))
}

/// For every end, how much wall is free before the next end along the contour.
fn free_span_after(contours: &[Contour], ends: &[EndRef]) -> Vec<f64> {
    let mut span = vec![f64::MAX; ends.len()];
    for (c, contour) in contours.iter().enumerate() {
        let mut on_contour: Vec<usize> =
            (0..ends.len()).filter(|&e| ends[e].contour == c).collect();
        if on_contour.is_empty() {
            continue;
        }
        if on_contour.len() == 1 {
            span[on_contour[0]] = contour.total;
            continue;
        }
        on_contour.sort_by(|&a, &b| ends[a].arc.total_cmp(&ends[b].arc));
        for k in 0..on_contour.len() {
            let from = on_contour[k];
            let to = on_contour[(k + 1) % on_contour.len()];
            span[from] = contour.forward_len(ends[from].arc, ends[to].arc);
        }
    }
    span
}

/// Splice the wall stretch between two line ends and merge their polylines.
fn merge_across_boundary(
    polys: &mut [Option<Vec<(f64, f64)>>],
    ends: &mut [EndRef],
    contours: &[Contour],
    from: usize,
    to: usize,
) {
    let (a_idx, b_idx) = (ends[from].poly, ends[to].poly);
    if polys[a_idx].is_none() || polys[b_idx].is_none() {
        return;
    }
    let mut a = polys[a_idx].take().expect("checked above");
    let mut b = polys[b_idx].take().expect("checked above");

    // Orient so the join happens at `a`'s tail and `b`'s head.
    if ends[from].at_start {
        a.reverse();
    }
    if !ends[to].at_start {
        b.reverse();
    }

    let bridge = contours[ends[from].contour].walk(ends[from].arc, ends[to].arc);
    a.extend(bridge);
    a.extend(b);

    let merged = a_idx;
    polys[merged] = Some(a);
    polys[b_idx] = None;

    ends[from].consumed = true;
    ends[to].consumed = true;

    // The two *outer* ends now bound the merged polyline.
    for e in ends.iter_mut() {
        if e.consumed {
            continue;
        }
        if e.poly == a_idx {
            // `a` may have been reversed; its surviving end is now the head.
            e.at_start = true;
        } else if e.poly == b_idx {
            e.poly = merged;
            e.at_start = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(w: f64, h: f64) -> Paths {
        let path: clipper2::Path = vec![(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)].into();
        Paths::new(vec![path])
    }

    /// Three horizontal lines spanning a 10×6 box, as the scanline would emit.
    fn horizontal_lines() -> Paths {
        let mut lines = Paths::default();
        for y in [1.0, 3.0, 5.0] {
            let p: clipper2::Path = vec![(0.0, y), (10.0, y)].into();
            lines.push(p);
        }
        lines
    }

    fn total_length(paths: &Paths) -> f64 {
        paths
            .iter()
            .map(|p| {
                let pts: Vec<(f64, f64)> = p.iter().map(|v| (v.x(), v.y())).collect();
                pts.windows(2).map(|w| dist(w[0], w[1])).sum::<f64>()
            })
            .sum()
    }

    #[test]
    fn zero_max_disables_anchoring() {
        let lines = horizontal_lines();
        let before = lines.len();
        let out = connect_infill(lines, &rect(10.0, 6.0), 5.0, 0.0);
        assert_eq!(out.len(), before, "anchoring must be off at anchor_max = 0");
    }

    #[test]
    fn short_wall_stretch_joins_two_lines() {
        // Line ends 2 mm apart up the left/right wall are joined into one path.
        let out = connect_infill(horizontal_lines(), &rect(10.0, 6.0), 0.0, 5.0);
        assert_eq!(
            out.len(),
            1,
            "three lines around 2 mm wall stretches should merge into one"
        );
    }

    #[test]
    fn long_wall_stretch_is_left_alone() {
        // With a cap below the 2 mm wall stretch, nothing may be joined.
        let out = connect_infill(horizontal_lines(), &rect(10.0, 6.0), 0.0, 1.0);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn open_anchor_extends_a_lone_end() {
        // One line, so there is no partner to join: each end may still run along
        // the wall by `anchor_length`, capped at half the free stretch.
        let mut lines = Paths::default();
        let p: clipper2::Path = vec![(0.0, 3.0), (10.0, 3.0)].into();
        lines.push(p);
        let plain = total_length(&lines);

        let anchored = connect_infill(lines, &rect(10.0, 6.0), 1.0, 1.0);
        assert_eq!(anchored.len(), 1);
        assert!(
            total_length(&anchored) > plain + 1.0,
            "expected both ends to gain an anchor, got {:.2} vs {plain:.2}",
            total_length(&anchored)
        );
    }

    #[test]
    fn zero_anchor_length_leaves_lone_ends_bare() {
        let mut lines = Paths::default();
        let p: clipper2::Path = vec![(0.0, 3.0), (10.0, 3.0)].into();
        lines.push(p);
        let plain = total_length(&lines);

        let out = connect_infill(lines, &rect(10.0, 6.0), 0.0, 1.0);
        assert!((total_length(&out) - plain).abs() < 1e-6);
    }

    #[test]
    fn interior_ends_are_never_anchored() {
        // A line floating in the middle of the region touches no wall, so there
        // is nothing to anchor it to.
        let mut lines = Paths::default();
        let p: clipper2::Path = vec![(3.0, 3.0), (6.0, 3.0)].into();
        lines.push(p);
        let plain = total_length(&lines);

        let out = connect_infill(lines, &rect(10.0, 6.0), 5.0, 5.0);
        assert!((total_length(&out) - plain).abs() < 1e-6);
    }
}
