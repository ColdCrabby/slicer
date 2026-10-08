//! Arc fitting: curved walls printed as `G2`/`G3` arcs instead of runs of
//! short straight moves.
//!
//! A curved wall reaches the printer as dozens of tiny `G1` moves per circle.
//! Each one is a command to send, parse and plan, and on a printer fed over a
//! serial line, or with a small planner buffer, enough of them in a row can
//! outrun it — the head stutters on exactly the curves that should be smoothest.
//! One arc says the same thing in one line.
//!
//! It is an experiment, off by default, and it only works on firmware that
//! accepts arcs: Marlin built with `ARC_SUPPORT`, Klipper with a `[gcode_arcs]`
//! section, RepRapFirmware as shipped. Checking that is the user's job — the
//! slicer cannot see how the printer was built.
//!
//! ## What it touches
//!
//! **Walls only** — outer, inner and overhang. Walls are where the curves are:
//! infill and solid fill are straight lines, gap fill changes width along its
//! length, and support and skirt gain nothing worth the risk. A wall the user
//! asked to be rough (fuzzy skin) stays rough.
//!
//! Within a wall, an arc only ever replaces moves one `G2`/`G3` can say
//! exactly: consecutive, flat, uncommented extrusions with the same role,
//! width, feedrate and filament per millimetre — an arc carries one of each.
//! Anything between them (a travel, a retract, a marker, a fan change) ends the
//! run, so nothing is reordered and nothing is dropped.
//!
//! ## What it promises
//!
//! The nozzle path moves by at most the tolerance; [`crate::gcode::arc`] says
//! exactly what that covers. The filament does not change: an arc deposits the
//! sum of what the moves it replaces would have, so totals, statistics and every
//! later absolute `E` stay as they were. (An arc is a hair longer than the
//! chords it replaces — a fraction of a percent on any curve worth fitting — so
//! its bead is laid a hair thinner, far inside what flow calibration resolves.)

use serde_json::json;

use crate::core::ExtrusionRole;
use crate::gcode::arc::{self, ArcFit};
use crate::gcode::{Move, MoveFilter, MoveProgram};
use crate::plugin::{Plugin, PluginManifest};
use crate::settings::params::SlicingParams;
use crate::walls::fuzzy_skin;

/// The plugin's id — and its settings namespace, so it must never change
/// casually.
const ID: &str = "arc-fitting";

/// How far, in mm, the nozzle path may move when straight moves become an arc.
///
/// Twice the default path-simplification tolerance: the segments that reach
/// this filter already bow up to that far inside the curve they were simplified
/// from, so a tighter budget would turn away the very curves it exists for.
const DEFAULT_TOLERANCE_MM: f64 = 0.025;

/// The fewest straight moves worth replacing with one arc.
const MIN_SEGMENTS: usize = 3;

/// The most straight moves one arc may replace.
///
/// Every extension refits the whole candidate, so this bounds the work per arc;
/// a longer run simply continues as the next arc.
const MAX_SEGMENTS: usize = 512;

/// Prints curved walls as arcs. Off by default.
pub struct ArcFitting;

impl Plugin for ArcFitting {
    fn manifest(&self) -> PluginManifest {
        PluginManifest::experiment(
            ID,
            "Arc fitting",
            "Prints curved walls as arc moves (G2/G3) instead of many short straight \
             ones — smaller files and fewer commands for the printer to plan. Your \
             firmware must support arcs (Marlin: ARC_SUPPORT, Klipper: [gcode_arcs]); \
             without it, curved walls print wrong.",
        )
    }

    fn settings_schema(&self) -> Option<serde_json::Value> {
        Some(json!({
            "properties": {
                "tolerance_mm": {
                    "type": "number",
                    "title": "Arc tolerance",
                    "description": "How far the nozzle path may move when straight moves \
                        are replaced by an arc. Larger values turn more of each curve into \
                        arcs. Keep it at least twice Path tolerance, or curves the \
                        simplifier left coarse will not fit.\n\
                        **Default:** 0.025 mm. **Typical:** 0.02–0.05 mm.",
                    "default": DEFAULT_TOLERANCE_MM,
                    "minimum": 0.005,
                    "maximum": 0.1,
                    "x-unit": "mm",
                    "x-step": 0.005,
                    "x-tier": "expert",
                },
            }
        }))
    }

    fn move_filter(&self) -> Option<Box<dyn MoveFilter>> {
        Some(Box::new(FitArcs))
    }
}

/// The filter: finds runs of wall extrusions and replaces what fits with arcs.
struct FitArcs;

impl MoveFilter for FitArcs {
    fn name(&self) -> &str {
        ID
    }

    fn filter(&self, program: &mut MoveProgram, params: &SlicingParams) {
        if !params.plugin_enabled(ID) {
            return;
        }
        let tolerance = params.plugin_f64(ID, "tolerance_mm", DEFAULT_TOLERANCE_MM);
        if tolerance.is_nan() || tolerance <= 0.0 {
            return;
        }
        let rules = Rules {
            tolerance,
            relative_e: params.use_relative_e_distances,
            rough_walls: fuzzy_skin::is_active(params),
        };
        let moves = std::mem::take(program.moves_mut());
        *program.moves_mut() = fit_arcs(moves, &rules);
    }
}

/// What the filter was told, resolved once per program.
struct Rules {
    tolerance: f64,
    relative_e: bool,
    /// Fuzzy skin is on, so the walls it roughens are not to be smoothed.
    rough_walls: bool,
}

impl Rules {
    /// Whether an extrusion of `role` may become part of an arc.
    fn fits_role(&self, role: ExtrusionRole) -> bool {
        matches!(
            role,
            ExtrusionRole::OuterWall | ExtrusionRole::InnerWall | ExtrusionRole::OverhangPerimeter
        ) && !(self.rough_walls && fuzzy_skin::roughens(role))
    }
}

/// The settings a `G2`/`G3` can only state once, so every move an arc replaces
/// must share them.
#[derive(Debug, Clone, Copy)]
struct Bead {
    role: ExtrusionRole,
    width_mm: f64,
    feed_mm_min: f64,
    /// Filament per millimetre of path.
    flow: f64,
}

impl Bead {
    /// Whether a move laying `other` can share one arc with this one.
    fn matches(&self, other: &Bead) -> bool {
        self.role == other.role
            && (self.width_mm - other.width_mm).abs() <= 1e-9
            && (self.feed_mm_min - other.feed_mm_min).abs() <= 1e-9
            // Same width and layer give the same flow to rounding; anything
            // further apart is a deliberate change an arc would flatten.
            && (self.flow - other.flow).abs() <= 1e-3 * self.flow
    }
}

/// One straight extrusion waiting to be fitted.
struct Segment {
    /// The move itself, emitted unchanged when no arc takes it.
    original: Move,
    /// Where it ends, as written.
    end: (f64, f64),
    /// The E it prints and the filament it stands for.
    e: f64,
    de: f64,
}

/// A run of segments that may become arcs: consecutive, and all the same bead.
#[derive(Default)]
struct Run {
    /// Where the first segment starts, as written.
    start: (f64, f64),
    bead: Option<Bead>,
    segments: Vec<Segment>,
}

impl Run {
    fn accepts(&self, bead: &Bead) -> bool {
        self.bead.is_none_or(|b| b.matches(bead))
    }

    fn push(&mut self, start: (f64, f64), bead: Bead, segment: Segment) {
        if self.segments.is_empty() {
            self.start = start;
            self.bead = Some(bead);
        }
        self.segments.push(segment);
    }

    /// Fit what can be fitted and emit everything into `out`, in order.
    ///
    /// Greedy from the front: grow each arc while it still fits, and when the
    /// shortest one does not, pass the first segment through and try again from
    /// the next.
    fn flush(&mut self, out: &mut Vec<Move>, rules: &Rules) {
        let segments = std::mem::take(&mut self.segments);
        let bead = self.bead.take();
        let Some(bead) = bead.filter(|_| segments.len() >= MIN_SEGMENTS) else {
            out.extend(segments.into_iter().map(|s| s.original));
            return;
        };

        let points: Vec<(f64, f64)> = std::iter::once(self.start)
            .chain(segments.iter().map(|s| s.end))
            .collect();
        let n = segments.len();
        let mut pending = segments.into_iter();
        let mut i = 0;
        while i < n {
            let mut best: Option<(usize, ArcFit)> = None;
            let mut j = i + MIN_SEGMENTS;
            while j <= n && j - i <= MAX_SEGMENTS {
                match arc::fit(&points[i..=j], rules.tolerance) {
                    Some(fit) => {
                        best = Some((j, fit));
                        j += 1;
                    }
                    None => break,
                }
            }
            match best {
                Some((j, fit)) => {
                    let taken: Vec<Segment> = pending.by_ref().take(j - i).collect();
                    out.push(arc_move(&taken, points[i], points[j], fit, bead, rules));
                    i = j;
                }
                None => {
                    if let Some(segment) = pending.next() {
                        out.push(segment.original);
                    }
                    i += 1;
                }
            }
        }
    }
}

/// The one move that replaces `taken`, from `start` to `end`.
fn arc_move(
    taken: &[Segment],
    start: (f64, f64),
    end: (f64, f64),
    fit: ArcFit,
    bead: Bead,
    rules: &Rules,
) -> Move {
    let de: f64 = taken.iter().map(|s| s.de).sum();
    // Absolute E is a running total, so the arc ends where its last move did;
    // relative E is the arc's own filament.
    let e = if rules.relative_e {
        de
    } else {
        taken.last().map_or(de, |s| s.e)
    };
    Move::Arc {
        x: end.0,
        y: end.1,
        i: fit.center.0 - start.0,
        j: fit.center.1 - start.1,
        clockwise: fit.clockwise,
        e,
        de,
        feed_mm_min: bead.feed_mm_min,
        role: bead.role,
        width_mm: bead.width_mm,
        comment: None,
    }
}

/// Rewrite `moves`, replacing the wall runs that fit with arcs.
fn fit_arcs(moves: Vec<Move>, rules: &Rules) -> Vec<Move> {
    let mut out = Vec::with_capacity(moves.len());
    // Where the nozzle is, at the planner's full precision — `None` once text
    // has passed that may have moved it.
    let mut at: Option<(f64, f64)> = None;
    let mut run = Run::default();

    for m in moves {
        if let Some(c) = candidate(&m, at, rules) {
            if !run.accepts(&c.bead) {
                run.flush(&mut out, rules);
            }
            at = m.end_xy();
            run.push(
                c.start,
                c.bead,
                Segment {
                    original: m,
                    end: c.end,
                    e: c.e,
                    de: c.de,
                },
            );
            continue;
        }
        run.flush(&mut out, rules);
        at = position_after(&m, at);
        out.push(m);
    }
    run.flush(&mut out, rules);
    out
}

/// A straight extrusion an arc could take in, as the fit sees it.
struct Candidate {
    /// Where it starts and ends, as written — what the fit works on.
    start: (f64, f64),
    end: (f64, f64),
    bead: Bead,
    e: f64,
    de: f64,
}

/// `m` as a [`Candidate`], or `None` when it cannot take part in an arc.
fn candidate(m: &Move, at: Option<(f64, f64)>, rules: &Rules) -> Option<Candidate> {
    let Move::Extrude {
        x,
        y,
        z: None,
        e,
        de,
        feed_mm_min,
        role,
        width_mm,
        comment: None,
    } = m
    else {
        return None;
    };
    let from = at?;
    if !rules.fits_role(*role) || de.is_nan() || *de <= 0.0 {
        return None;
    }
    // Filament per millimetre is measured at full precision, as the emitter
    // charged it: written positions are rounded, and on a short segment that
    // alone would make two moves of one bead look different.
    let length = (x - from.0).hypot(y - from.1);
    if length.is_nan() || length <= 0.0 {
        return None;
    }
    Some(Candidate {
        start: (written(from.0), written(from.1)),
        end: (written(*x), written(*y)),
        bead: Bead {
            role: *role,
            width_mm: *width_mm,
            feed_mm_min: *feed_mm_min,
            flow: de / length,
        },
        e: *e,
        de: *de,
    })
}

/// Where the nozzle is after `m`, starting from `at`.
fn position_after(m: &Move, at: Option<(f64, f64)>) -> Option<(f64, f64)> {
    match m {
        Move::Raw(text) => (!may_move_head(text)).then_some(at).flatten(),
        Move::ZMove { .. } | Move::Extruder { .. } => at,
        _ => m.end_xy(),
    }
}

/// A position as every dialect writes it: three decimals.
///
/// The fit runs on these, not on the planner's full precision, so the arc it
/// checks is the arc the firmware is handed — its start included, which a
/// `G2`/`G3` takes from wherever the previous line left the nozzle.
fn written(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Whether raw text might move the head, leaving its position unknown.
///
/// An arc's centre is written relative to where it starts, so starting one
/// from a guessed position draws the wrong circle. Comments and M-codes leave
/// the head where it is — the ones that park it (a filament change, a pause)
/// put it back — and so do the handful of G-codes and Klipper commands the
/// emitter writes between moves. Anything else — a custom script, a macro, a
/// tool change — might not, and costs only the first move of the next run.
fn may_move_head(text: &str) -> bool {
    text.lines().any(|line| {
        let code = line.split(';').next().unwrap_or("").trim();
        let mut words = code.split_whitespace();
        let Some(command) = words.next() else {
            return false;
        };
        let command = command.to_ascii_uppercase();
        if command.starts_with('M') {
            return false;
        }
        match command.as_str() {
            "G4"
            | "G10"
            | "G11"
            | "G17"
            | "G21"
            | "G90"
            | "SET_PRESSURE_ADVANCE"
            | "SET_VELOCITY_LIMIT"
            | "SET_FAN_SPEED"
            | "SET_HEATER_TEMPERATURE"
            | "EXCLUDE_OBJECT_START"
            | "EXCLUDE_OBJECT_END"
            | "EXCLUDE_OBJECT_DEFINE" => false,
            // Re-zeroing the extruder keeps XY; redefining X or Y does not.
            "G92" => words.any(|w| matches!(w.as_bytes()[0].to_ascii_uppercase(), b'X' | b'Y')),
            _ => true,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::ENABLED_KEY;
    use std::f64::consts::{PI, TAU};

    fn enabled() -> SlicingParams {
        let mut p = SlicingParams::default();
        p.set_plugin_value(ID, ENABLED_KEY, json!(true));
        p
    }

    /// `count + 1` points round a circle, from `from` radians turning `turn`.
    fn round(
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

    /// What the emitter writes for a wall along `points`: a travel to the
    /// first, a marker, then one straight extrusion to each of the rest, at a
    /// constant flow and with an absolute E.
    fn wall(points: &[(f64, f64)], role: ExtrusionRole) -> MoveProgram {
        let mut p = MoveProgram::new();
        p.push(Move::Travel {
            x: points[0].0,
            y: points[0].1,
            feed_mm_min: 9000.0,
            comment: None,
        });
        p.raw(";TYPE:Outer wall\n");
        let mut e = 2.0;
        for pair in points.windows(2) {
            let de = 0.033 * (pair[1].0 - pair[0].0).hypot(pair[1].1 - pair[0].1);
            e += de;
            p.push(Move::Extrude {
                x: pair[1].0,
                y: pair[1].1,
                z: None,
                e,
                de,
                feed_mm_min: 1800.0,
                role,
                width_mm: 0.45,
                comment: None,
            });
        }
        p
    }

    fn fitted(mut program: MoveProgram, params: &SlicingParams) -> MoveProgram {
        FitArcs.filter(&mut program, params);
        program
    }

    fn arcs(program: &MoveProgram) -> usize {
        program
            .moves()
            .iter()
            .filter(|m| matches!(m, Move::Arc { .. }))
            .count()
    }

    fn filament(program: &MoveProgram) -> f64 {
        program.moves().iter().map(Move::delta_e).sum()
    }

    #[test]
    fn it_does_nothing_until_switched_on() {
        let program = wall(
            &round((20.0, 20.0), 5.0, 0.0, 5.0, 40),
            ExtrusionRole::OuterWall,
        );
        let before = program.clone();
        let after = fitted(program, &SlicingParams::default());
        assert_eq!(before.moves(), after.moves());
    }

    #[test]
    fn a_round_wall_becomes_one_arc_and_keeps_its_filament() {
        let points = round((20.0, 20.0), 5.0, 0.0, 5.0, 40);
        let program = wall(&points, ExtrusionRole::OuterWall);
        let total = filament(&program);
        let last_e = match program.moves().last() {
            Some(Move::Extrude { e, .. }) => *e,
            _ => unreachable!(),
        };

        let after = fitted(program, &enabled());
        assert_eq!(arcs(&after), 1, "{:?}", after.moves());
        assert_eq!(after.len(), 3, "travel, marker, one arc");
        assert!((filament(&after) - total).abs() < 1e-12);

        let Some(Move::Arc {
            x,
            y,
            i,
            j,
            e,
            clockwise,
            ..
        }) = after.moves().last()
        else {
            panic!("the wall ends in an arc");
        };
        assert_eq!(*e, last_e, "absolute E ends where the last move did");
        assert!(!clockwise);
        assert!((x - points[40].0).abs() < 1e-3 && (y - points[40].1).abs() < 1e-3);
        // The centre is written relative to the start of the arc.
        let centre = (points[0].0 + i, points[0].1 + j);
        assert!((centre.0 - 20.0).abs() < 1e-3 && (centre.1 - 20.0).abs() < 1e-3);
    }

    #[test]
    fn relative_e_gives_an_arc_its_own_filament() {
        let mut program = wall(
            &round((0.0, 0.0), 8.0, 1.0, 2.0, 30),
            ExtrusionRole::InnerWall,
        );
        for m in program.moves_mut() {
            if let Move::Extrude { e, de, .. } = m {
                *e = *de;
            }
        }
        let total = filament(&program);
        let mut params = enabled();
        params.use_relative_e_distances = true;
        let after = fitted(program, &params);
        let Some(Move::Arc { e, de, .. }) = after.moves().last() else {
            panic!("the wall ends in an arc");
        };
        assert!((e - total).abs() < 1e-12 && (de - total).abs() < 1e-12);
    }

    #[test]
    fn only_walls_are_fitted() {
        let points = round((0.0, 0.0), 8.0, 0.0, 3.0, 30);
        for role in [
            ExtrusionRole::Infill,
            ExtrusionRole::GapFill,
            ExtrusionRole::Skirt,
        ] {
            let program = wall(&points, role);
            assert_eq!(arcs(&fitted(program, &enabled())), 0, "{role:?}");
        }
        let overhang = wall(&points, ExtrusionRole::OverhangPerimeter);
        assert_eq!(arcs(&fitted(overhang, &enabled())), 1);
    }

    #[test]
    fn a_hexagon_stays_a_hexagon() {
        // A nut trap: every vertex on one circle, and nothing round about it.
        let program = wall(
            &round((5.0, 5.0), 3.0, 0.0, 5.0 * PI / 3.0, 5),
            ExtrusionRole::InnerWall,
        );
        assert_eq!(arcs(&fitted(program, &enabled())), 0);
    }

    #[test]
    fn anything_between_moves_ends_the_run_and_keeps_its_place() {
        let points = round((0.0, 0.0), 6.0, 0.0, 4.0, 40);
        let mut program = wall(&points, ExtrusionRole::OuterWall);
        program
            .moves_mut()
            .insert(22, Move::Raw("M106 S255\n".into()));
        let after = fitted(program, &enabled());
        assert_eq!(arcs(&after), 2, "one arc either side of the fan change");
        let fan = after
            .moves()
            .iter()
            .position(|m| matches!(m, Move::Raw(t) if t.starts_with("M106")))
            .unwrap();
        assert!(matches!(after.moves()[fan - 1], Move::Arc { .. }));
        assert!(matches!(after.moves()[fan + 1], Move::Arc { .. }));
    }

    #[test]
    fn a_commented_move_is_left_as_it_was() {
        let points = round((0.0, 0.0), 6.0, 0.0, 4.0, 40);
        let mut program = wall(&points, ExtrusionRole::OuterWall);
        let closing = program.moves_mut().last_mut().unwrap().clone();
        *program.moves_mut().last_mut().unwrap() = closing.with_comment("close contour");
        let after = fitted(program, &enabled());
        assert!(matches!(
            after.moves().last(),
            Some(Move::Extrude { comment: Some(c), .. }) if c == "close contour"
        ));
        assert_eq!(arcs(&after), 1);
    }

    #[test]
    fn a_change_of_width_ends_the_run() {
        let points = round((0.0, 0.0), 6.0, 0.0, 4.0, 40);
        let mut program = wall(&points, ExtrusionRole::InnerWall);
        for m in program.moves_mut().iter_mut().skip(22) {
            if let Move::Extrude { width_mm, de, .. } = m {
                *width_mm = 0.4;
                *de *= 0.4 / 0.45;
            }
        }
        assert_eq!(arcs(&fitted(program, &enabled())), 2);
    }

    #[test]
    fn fuzzy_skin_keeps_the_walls_it_roughens() {
        let points = round((0.0, 0.0), 6.0, 0.0, 4.0, 40);
        let mut params = enabled();
        params.fuzzy_skin = true;
        params.fuzzy_skin_thickness_mm = 0.3;
        params.fuzzy_skin_point_dist_mm = 0.8;
        assert_eq!(
            arcs(&fitted(wall(&points, ExtrusionRole::OuterWall), &params)),
            0
        );
        assert_eq!(
            arcs(&fitted(wall(&points, ExtrusionRole::InnerWall), &params)),
            1
        );
    }

    #[test]
    fn a_command_that_may_move_the_head_costs_the_next_move_only() {
        let points = round((0.0, 0.0), 6.0, 0.0, 4.0, 40);

        // The emitter's own markers and settings keep the position known…
        let mut known = wall(&points, ExtrusionRole::OuterWall);
        known.moves_mut().insert(
            1,
            Move::Raw("M204 S500\nSET_PRESSURE_ADVANCE ADVANCE=0.04\n".into()),
        );
        let known = fitted(known, &enabled());
        assert!(matches!(known.moves().last(), Some(Move::Arc { .. })));
        assert_eq!(known.len(), 4, "travel, raw, marker, one arc");

        // …a tool change or a macro might not, so the first move stays a line
        // and the arc starts from where that line ends.
        let mut unknown = wall(&points, ExtrusionRole::OuterWall);
        unknown.moves_mut().insert(1, Move::Raw("T1\n".into()));
        let unknown = fitted(unknown, &enabled());
        assert!(matches!(unknown.moves()[3], Move::Extrude { .. }));
        assert!(matches!(unknown.moves()[4], Move::Arc { .. }));
    }

    #[test]
    fn an_arc_renders_as_g2_or_g3_with_its_centre() {
        let program = wall(
            &round((10.0, 10.0), 4.0, 0.0, -TAU / 3.0, 24),
            ExtrusionRole::OuterWall,
        );
        let after = fitted(program, &enabled());
        let text = after.render(&crate::gcode::dialects::MarlinDialect);
        let line = text.lines().last().unwrap();
        assert!(line.starts_with("G2 X"), "clockwise: {line}");
        assert!(
            line.contains(" I-4.0000 J0.0000 ") && line.ends_with(" F1800"),
            "{line}"
        );
    }

    #[test]
    fn written_positions_match_the_dialect() {
        // The fit rounds the way every dialect writes; pin the two together.
        let text = Move::Travel {
            x: 1.23456,
            y: -0.0004,
            feed_mm_min: 600.0,
            comment: None,
        }
        .render(&crate::gcode::dialects::MarlinDialect);
        assert!(text.starts_with(&format!(
            "G1 X{:.3} Y{:.3}",
            written(1.23456),
            written(-0.0004)
        )));
    }

    #[test]
    fn its_settings_leave_the_engines_own_gate_alone() {
        let schema = ArcFitting.settings_schema().unwrap();
        let props = schema["properties"].as_object().unwrap();
        assert!(!props.contains_key(ENABLED_KEY));
        assert_eq!(
            props["tolerance_mm"]["default"],
            json!(DEFAULT_TOLERANCE_MM)
        );
    }
}
