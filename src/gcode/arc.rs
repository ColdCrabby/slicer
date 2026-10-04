//! Circular arcs for `G2`/`G3`: fitting one to a run of points, and breaking
//! one back into the chords a printer follows.
//!
//! The two directions live together on purpose. The arc-fitting experiment
//! replaces a run of short straight extrusions with a single arc ([`fit`]);
//! the time estimator and the G-code viewer read arcs back — the ones this
//! engine writes and the ones other slicers write — as chords ([`chords`]).
//! Both have to agree on what an arc *is*: where it starts, which way it turns
//! and how far it goes. That answer is [`sweep`], computed the way firmware
//! computes it.
//!
//! ## The rule a fitted arc keeps
//!
//! **No point of the straight-line path may lie further than the tolerance
//! from the arc that replaces it** — not its vertices, and not the middle of
//! its segments either. Vertices alone are not enough: every vertex of a
//! hexagon lies on one circle, so a vertex-only fit would print a nut trap
//! round. The middle of a segment is where a straight path cuts inside the
//! curve, and that distance is checked exactly rather than sampled.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// The largest radius a fitted arc may have, in mm.
///
/// Past this a run is so nearly straight that an arc saves nothing over the
/// lines path simplification already left, while the centre offset grows large
/// enough to cost precision in firmware that does its arc arithmetic in single
/// precision.
pub const MAX_RADIUS_MM: f64 = 1000.0;

/// The shortest distance a fitted arc's end may be from its start, in mm.
///
/// Firmware reads an arc whose end equals its start as a **full circle**, so an
/// arc that nearly closes on itself is one rounding step away from printing a
/// whole extra loop. Ten times the 0.001 mm precision positions are written at
/// leaves no rounding that can close it.
pub const MIN_CHORD_MM: f64 = 0.01;

/// The chord sagitta arcs are read back at, in mm — by the time estimator, the
/// G-code viewer, and anything else that needs an arc as straight segments.
///
/// The deviation path simplification allows by default, so a part sliced with
/// arc fitting reads back with the faceting it would have printed with it off:
/// the preview looks the same and the estimate does not jump when the
/// experiment is toggled.
pub const READBACK_SAGITTA_MM: f64 = 0.0125;

/// The most chords [`chords`] will split one arc into, whatever the input.
///
/// A full circle of [`MAX_RADIUS_MM`] at [`READBACK_SAGITTA_MM`] needs about
/// 630; the cap only exists so a malformed arc in someone else's file cannot
/// make the viewer allocate without bound.
const MAX_CHORDS: usize = 1024;

/// An arc as `G2`/`G3` describes it once its start and end are known.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArcFit {
    /// The centre, in absolute coordinates.
    pub center: (f64, f64),
    /// `true` turns clockwise (`G2`), `false` counter-clockwise (`G3`).
    pub clockwise: bool,
}

/// Fit one arc to `points`, start to end, or `None` when no arc keeps every
/// point of that polyline within `tolerance` mm.
///
/// The arc starts exactly at the first point — a `G2`/`G3` begins wherever the
/// nozzle is — and ends exactly at the last, so its centre lies on the
/// perpendicular bisector of the two. That leaves one unknown: how far along
/// the bisector the centre sits. Measured as the *algebraic* distance
/// `|q − c|² − r²`, each interior point's misfit is linear in that unknown, so
/// the least-squares centre is a ratio of two sums — no iteration, and no
/// dependence on which interior point happens to be picked as an anchor. The
/// candidate is then held to the rule in the module docs, exactly, and
/// rejected if it breaks it anywhere.
///
/// Also rejected: a run with nothing to fit (fewer than three points, or no
/// bulge at all), one that turns back on itself, an arc that would close into
/// a full circle, and one larger than [`MAX_RADIUS_MM`].
pub fn fit(points: &[(f64, f64)], tolerance: f64) -> Option<ArcFit> {
    let n = points.len();
    if n < 3 || tolerance.is_nan() || tolerance <= 0.0 {
        return None;
    }
    let start = points[0];
    let end = points[n - 1];
    let (chord_x, chord_y) = (end.0 - start.0, end.1 - start.1);
    let chord = chord_x.hypot(chord_y);
    if chord.is_nan() || chord < MIN_CHORD_MM {
        return None;
    }

    // Centre = mid + offset · normal, radius² = half² + offset².
    let mid = ((start.0 + end.0) * 0.5, (start.1 + end.1) * 0.5);
    let normal = (-chord_y / chord, chord_x / chord);
    let half_sq = 0.25 * chord * chord;
    let (mut num, mut den) = (0.0_f64, 0.0_f64);
    for q in &points[1..n - 1] {
        let (dx, dy) = (q.0 - mid.0, q.1 - mid.1);
        let across = dx * normal.0 + dy * normal.1;
        let misfit = dx * dx + dy * dy - half_sq;
        num += misfit * across;
        den += across * across;
    }
    if den.is_nan() || den <= 0.0 {
        // Every interior point sits on the chord: a straight run.
        return None;
    }
    let offset = num / (2.0 * den);
    let center = (mid.0 + offset * normal.0, mid.1 + offset * normal.1);
    let radius = (half_sq + offset * offset).sqrt();
    if radius.is_nan() || radius > MAX_RADIUS_MM {
        return None;
    }

    // One direction all the way round, and less than a full turn of it.
    let angle_of = |p: (f64, f64)| (p.1 - center.1).atan2(p.0 - center.0);
    let mut previous = angle_of(start);
    let mut direction = 0.0_f64;
    let mut turned = 0.0_f64;
    for &p in &points[1..] {
        let angle = angle_of(p);
        let step = wrap_angle(angle - previous);
        if step == 0.0 || (direction != 0.0 && step.signum() != direction) {
            return None;
        }
        direction = step.signum();
        turned += step;
        previous = angle;
    }
    if turned.abs() >= TAU {
        return None;
    }

    // The rule: vertices within tolerance either side of the circle, and no
    // segment cutting further inside it than the tolerance.
    for pair in points.windows(2) {
        let (p, q) = (pair[0], pair[1]);
        let off_circle = ((q.0 - center.0).hypot(q.1 - center.1) - radius).abs();
        if off_circle > tolerance {
            return None;
        }
        if radius - distance_to_segment(center, p, q) > tolerance {
            return None;
        }
    }

    Some(ArcFit {
        center,
        clockwise: direction < 0.0,
    })
}

/// How far an arc turns, in radians, in its own direction: `[0, 2π]`.
///
/// Computed the way firmware computes it — from the start and end alone,
/// around the centre, in the commanded direction. So an arc whose end is its
/// start is a full circle, and one whose end merely shares its start's angle
/// (a malformed arc) does not turn at all. Anything that reads arcs back must
/// use this rather than its own idea of the arc, or it disagrees with the
/// printer.
pub fn sweep(start: (f64, f64), end: (f64, f64), center: (f64, f64), clockwise: bool) -> f64 {
    let (ax, ay) = (start.0 - center.0, start.1 - center.1);
    let (bx, by) = (end.0 - center.0, end.1 - center.1);
    let counter_clockwise = (ax * by - ay * bx).atan2(ax * bx + ay * by);
    let mut turn = if clockwise {
        -counter_clockwise
    } else {
        counter_clockwise
    };
    if turn < 0.0 {
        turn += TAU;
    }
    if turn == 0.0 {
        let closed = (start.0 - end.0).abs() < 1e-9 && (start.1 - end.1).abs() < 1e-9;
        return if closed { TAU } else { 0.0 };
    }
    turn
}

/// The points a printer steps through to follow an arc: everything after
/// `start`, each chord bowing at most `max_sagitta` mm off the true curve, the
/// last one `end` exactly.
///
/// The radius is taken from the start, as firmware takes it; an end written a
/// rounding step off that circle is simply where the last chord lands.
pub fn chords(
    start: (f64, f64),
    end: (f64, f64),
    center: (f64, f64),
    clockwise: bool,
    max_sagitta: f64,
) -> Vec<(f64, f64)> {
    let radius = (start.0 - center.0).hypot(start.1 - center.1);
    let turn = sweep(start, end, center, clockwise);
    if radius.is_nan() || radius <= 0.0 || turn == 0.0 {
        return vec![end];
    }
    // The angle one chord may span: its sagitta r·(1 − cos(φ/2)) at most
    // `max_sagitta`.
    let per_chord = if max_sagitta.is_nan() || max_sagitta <= 0.0 {
        0.0
    } else if max_sagitta >= radius {
        FRAC_PI_2
    } else {
        2.0 * (1.0 - max_sagitta / radius).acos()
    };
    let count = if per_chord > 0.0 {
        ((turn / per_chord).ceil() as usize).clamp(1, MAX_CHORDS)
    } else {
        MAX_CHORDS
    };

    let start_angle = (start.1 - center.1).atan2(start.0 - center.0);
    let signed_turn = if clockwise { -turn } else { turn };
    let mut out = Vec::with_capacity(count);
    for k in 1..count {
        let angle = start_angle + signed_turn * k as f64 / count as f64;
        out.push((
            center.0 + radius * angle.cos(),
            center.1 + radius * angle.sin(),
        ));
    }
    out.push(end);
    out
}

/// `angle` folded into `(−π, π]`.
fn wrap_angle(angle: f64) -> f64 {
    let mut a = angle % TAU;
    if a <= -PI {
        a += TAU;
    } else if a > PI {
        a -= TAU;
    }
    a
}

/// Distance from `point` to the segment `a`–`b`.
fn distance_to_segment(point: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len_sq = dx * dx + dy * dy;
    let t = if len_sq > 0.0 {
        (((point.0 - a.0) * dx + (point.1 - a.1) * dy) / len_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (point.0 - (a.0 + t * dx)).hypot(point.1 - (a.1 + t * dy))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `count + 1` points on a circle, from `from` radians turning `turn`.
    fn on_circle(
        center: (f64, f64),
        radius: f64,
        from: f64,
        turn: f64,
        count: usize,
    ) -> Vec<(f64, f64)> {
        (0..=count)
            .map(|k| {
                let a = from + turn * k as f64 / count as f64;
                (center.0 + radius * a.cos(), center.1 + radius * a.sin())
            })
            .collect()
    }

    #[test]
    fn a_polygon_on_a_circle_fits_its_own_circle() {
        let points = on_circle((40.0, 25.0), 6.0, 0.3, 1.5, 24);
        let fit = fit(&points, 0.01).expect("a fine polygon is an arc");
        assert!((fit.center.0 - 40.0).abs() < 1e-6 && (fit.center.1 - 25.0).abs() < 1e-6);
        assert!(
            !fit.clockwise,
            "angle increases, so it turns counter-clockwise"
        );
    }

    #[test]
    fn direction_follows_the_path() {
        let mut points = on_circle((0.0, 0.0), 5.0, 0.0, 2.0, 20);
        points.reverse();
        assert!(fit(&points, 0.01).unwrap().clockwise);
    }

    #[test]
    fn a_hexagon_is_not_a_circle() {
        // Every vertex lies on one circle; the edges cut 0.4 mm inside it.
        let hexagon = on_circle((0.0, 0.0), 3.0, 0.0, 5.0 * PI / 3.0, 5);
        assert!(fit(&hexagon, 0.05).is_none());
    }

    #[test]
    fn a_coarse_polygon_needs_a_tolerance_that_covers_its_facets() {
        // 16 edges round a 10 mm radius: each bows 0.19 mm inside the circle.
        let points = on_circle((0.0, 0.0), 10.0, 0.0, PI, 8);
        let sagitta = 10.0 * (1.0 - (PI / 16.0).cos());
        assert!(fit(&points, sagitta * 0.9).is_none());
        assert!(fit(&points, sagitta * 1.1).is_some());
    }

    #[test]
    fn straight_and_s_shaped_runs_do_not_fit() {
        let straight: Vec<_> = (0..6).map(|k| (k as f64, 2.0)).collect();
        assert!(fit(&straight, 0.05).is_none());

        let mut s = on_circle((0.0, 0.0), 5.0, -FRAC_PI_2, 1.0, 6);
        let last = *s.last().unwrap();
        // Continue with the mirror-image bend.
        let center2 = (2.0 * last.0, 2.0 * last.1);
        s.extend(
            on_circle(center2, 5.0, -FRAC_PI_2 + 1.0 + PI, -1.0, 6)
                .into_iter()
                .skip(1),
        );
        assert!(fit(&s, 0.05).is_none());
    }

    #[test]
    fn an_almost_closed_loop_fits_but_a_closed_one_does_not() {
        // Forty facets round a 4 mm radius bow 0.012 mm inside it.
        let almost = on_circle((0.0, 0.0), 4.0, 0.0, TAU - 0.2, 40);
        let fit_almost = fit(&almost, 0.02).expect("one segment short of closed");
        let turned = sweep(almost[0], almost[40], fit_almost.center, false);
        assert!((turned - (TAU - 0.2)).abs() < 1e-9);

        let closed = on_circle((0.0, 0.0), 4.0, 0.0, TAU, 40);
        assert!(fit(&closed, 0.02).is_none(), "start and end coincide");
    }

    #[test]
    fn a_huge_radius_is_left_as_lines() {
        let points = on_circle((0.0, -5000.0), 5000.0, FRAC_PI_2 - 0.004, 0.008, 8);
        assert!(fit(&points, 0.01).is_none());
    }

    #[test]
    fn sweep_reads_direction_and_full_circles_like_firmware() {
        let c = (0.0, 0.0);
        let (a, b) = ((1.0, 0.0), (0.0, 1.0));
        assert!((sweep(a, b, c, false) - FRAC_PI_2).abs() < 1e-12);
        assert!((sweep(a, b, c, true) - 3.0 * FRAC_PI_2).abs() < 1e-12);
        assert!((sweep(a, a, c, false) - TAU).abs() < 1e-12);
        assert_eq!(
            sweep(a, (2.0, 0.0), c, false),
            0.0,
            "same angle, not a circle"
        );
    }

    #[test]
    fn chords_stay_within_their_sagitta_and_land_on_the_end() {
        let (start, end, center) = ((10.0, 0.0), (0.0, 10.0), (0.0, 0.0));
        let points = chords(start, end, center, false, 0.01);
        assert_eq!(*points.last().unwrap(), end);
        let mut previous = start;
        for &p in &points {
            let mid = ((previous.0 + p.0) * 0.5, (previous.1 + p.1) * 0.5);
            let bow = 10.0 - mid.0.hypot(mid.1);
            assert!(bow <= 0.01 + 1e-12, "chord bows {bow}");
            previous = p;
        }
        // A full circle when the end is the start.
        let circle = chords(start, start, center, true, 0.05);
        assert!(circle.len() > 4);
        assert!(
            circle.iter().any(|p| p.1 < -9.0),
            "clockwise from +X passes below"
        );
    }

    #[test]
    fn a_fitted_arc_read_back_as_chords_retraces_the_run() {
        let points = on_circle((12.0, -3.0), 2.5, 1.0, 2.2, 14);
        let fitted = fit(&points, 0.01).unwrap();
        let mut back = vec![points[0]];
        back.extend(chords(
            points[0],
            points[14],
            fitted.center,
            fitted.clockwise,
            0.001,
        ));
        for p in &points {
            let nearest = back
                .iter()
                .map(|q| (q.0 - p.0).hypot(q.1 - p.1))
                .fold(f64::INFINITY, f64::min);
            assert!(nearest < 0.1, "every original vertex is passed again");
        }
    }
}
