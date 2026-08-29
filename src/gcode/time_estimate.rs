//! Acceleration-aware print-time estimation (issue #117).
//!
//! # Why this module exists
//!
//! A print head never travels a segment at a single constant speed: it
//! accelerates from the previous corner, may cruise at the commanded feedrate,
//! then decelerates into the next corner.  The old estimate — extrusion length
//! divided by the nominal print speed — ignored all of that, plus travel moves,
//! Z lifts and the retract/un-retract ceremony, so it was *systematically
//! optimistic* on small, detailed parts where the head spends most of its time
//! ramping up and down rather than cruising.
//!
//! This module replaces that guess with a **planner-style trapezoidal model**
//! that mirrors what real firmware (Marlin / Klipper) actually does, so the
//! header / footer ETA and the viewer's *Layer Time* colouring match reality.
//!
//! # The one rule
//!
//! **The estimate is derived from the *emitted* G-code, not from the slice
//! geometry.**  We parse the final program text the generator already produced,
//! so every travel move, Z lift, retraction, per-role feedrate and per-role
//! acceleration (`M204` / `SET_VELOCITY_LIMIT`) the generator wrote is accounted
//! for automatically — the estimate can never silently drift away from the moves
//! the printer will run.  This is the same approach PrusaSlicer's
//! `GCodeProcessor` and OctoPrint's analysis queue take.
//!
//! # Motion model
//!
//! ```text
//!   v ▲            cruise (v_nom)
//!     │        ┌───────────────┐
//!     │       ╱                 ╲
//!     │      ╱                   ╲
//! v_e ●─────╱                     ╲─────● v_x
//!     │    accel        L         decel
//!     └───────────────────────────────────► distance
//! ```
//!
//! Each move is a trapezoid (or a triangle when it is too short to reach the
//! commanded feedrate `v_nom`): it enters at `v_e`, leaves at `v_x`, and ramps at
//! the move's acceleration `a`.  The entry/exit speeds at each corner come from a
//! two-pass **look-ahead** (reverse then forward) over the moves of a layer, so a
//! move can only enter/leave a corner as fast as the neighbouring moves and the
//! acceleration budget allow — exactly the constraint a motion planner enforces.
//!
//! Cornering speed uses the **junction-deviation** model (Marlin 2 / Klipper's
//! *square-corner-velocity*): a straight-through join keeps full speed, a right
//! angle is limited to [`DEFAULT_SQUARE_CORNER_VELOCITY_MM_S`], and a hairpin
//! reversal drops to a full stop.  It is derived from the move acceleration and a
//! single firmware-agnostic constant, so no per-axis jerk profile is required
//! (the codebase does not model one).
//!
//! # What a "layer" is here
//!
//! The generator writes one `;LAYER_TIME:` marker per printed layer.  We treat
//! each such marker as the start of a layer bucket: every move that follows a
//! marker (its Z lift, travels and extrusions) is attributed to that layer until
//! the next marker.  Moves *before* the first marker (start script, priming) form
//! a preamble that counts toward the total but toward no single layer.  Because
//! the generator always separates layers with a Z-only move — which the planner
//! treats as a full stop — planning each layer in isolation is exact, not an
//! approximation.
//!
//! # Non-goals
//!
//! * **Not a G-code interpreter.** Only linear moves (`G0`/`G1`), homing
//!   (`G28`), the extruder reset (`G92`) and the two acceleration commands are
//!   understood; arcs, dwell (`G4`) and heating waits are ignored (heating time
//!   is unknowable from the toolpath and is firmware/ambient dependent).
//! * **No per-axis machine limits.** A single acceleration and one cornering
//!   constant stand in for a full `M201`/`M203` machine-limit profile.
//! * **It does not change any moves.** This is a pure measurement pass; the
//!   only thing the caller writes back is the `;LAYER_TIME:` values and the
//!   header/footer total.
//!
//! See also: [`crate::gcode::generator`] (caller + `;LAYER_TIME:` patching),
//! [`crate::gcode::stats::SliceStatistics`] (header/footer consumer), and issue
//! #117.

/// Firmware-agnostic corner speed (mm/s) assumed for a 90° join when the printer
/// does not advertise a jerk / junction-deviation profile.
///
/// Matches Klipper's default `square_corner_velocity`; a right-angle corner is
/// taken at this speed, a straight join keeps full speed, and a reversal stops.
pub(crate) const DEFAULT_SQUARE_CORNER_VELOCITY_MM_S: f64 = 5.0;

/// Fallback acceleration (mm/s²) used only when acceleration control is disabled
/// (`acceleration = 0`, so the generator emits no `M204` / `SET_VELOCITY_LIMIT`)
/// and no value has been seen yet.
///
/// Mirrors `MachineConfig`'s default `max_acceleration` (`crate::config::types`)
/// so the estimate stays consistent with the project's own machine baseline.
pub(crate) const DEFAULT_ACCELERATION_MM_S2: f64 = 1000.0;

/// Distances / speeds below this are treated as zero (floating-point noise).
const EPS: f64 = 1e-9;

/// Tuning inputs for [`estimate_print_time`], resolved once from the slice
/// parameters by the caller.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EstimatorConfig {
    /// Acceleration (mm/s²) assumed before any firmware acceleration command is
    /// seen — and for the whole program when acceleration control is off.
    pub default_accel_mm_s2: f64,
    /// Assumed square-corner velocity (mm/s) for the junction-deviation model.
    pub square_corner_velocity_mm_s: f64,
    /// Machine velocity cap (mm/s) every move is clamped to, or `0` for no cap.
    ///
    /// Mirrors the firmware limit the generator emits (`SET_VELOCITY_LIMIT
    /// VELOCITY=…` / `M203`): the printer runs no faster than this regardless of
    /// the commanded feedrate, so the estimate honours it too.
    pub max_velocity_mm_s: f64,
}

impl Default for EstimatorConfig {
    fn default() -> Self {
        Self {
            default_accel_mm_s2: DEFAULT_ACCELERATION_MM_S2,
            square_corner_velocity_mm_s: DEFAULT_SQUARE_CORNER_VELOCITY_MM_S,
            max_velocity_mm_s: 0.0,
        }
    }
}

impl EstimatorConfig {
    /// Build the estimator inputs from resolved slice parameters.
    ///
    /// The default acceleration follows `params.acceleration` when set (the same
    /// value the generator writes as `M204 P…`); a `0` (acceleration control
    /// disabled) falls back to [`DEFAULT_ACCELERATION_MM_S2`]. The square-corner
    /// velocity follows `params.square_corner_velocity` when set, else
    /// [`DEFAULT_SQUARE_CORNER_VELOCITY_MM_S`]. `params.max_velocity` (`0` =
    /// uncapped) is passed straight through.
    pub(crate) fn from_params(params: &crate::settings::params::SlicingParams) -> Self {
        let default_accel_mm_s2 = if params.acceleration > 0.0 {
            params.acceleration
        } else {
            DEFAULT_ACCELERATION_MM_S2
        };
        let square_corner_velocity_mm_s = if params.square_corner_velocity > 0.0 {
            params.square_corner_velocity
        } else {
            DEFAULT_SQUARE_CORNER_VELOCITY_MM_S
        };
        Self {
            default_accel_mm_s2,
            square_corner_velocity_mm_s,
            max_velocity_mm_s: params.max_velocity.max(0.0),
        }
    }
}

/// Result of a full-program estimate.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TimeEstimate {
    /// Total wall-clock print time in seconds (preamble + every layer).
    pub total_s: f64,
    /// Per-layer print time in seconds, one entry per `;LAYER_TIME:` marker in
    /// program order.  Empty when the program carries no layer markers.
    pub per_layer_s: Vec<f64>,
}

/// A single planned motion, reduced to what the trapezoidal model needs.
#[derive(Debug, Clone, Copy)]
struct PlannedMove {
    /// Travelled distance (mm) along the dominant axis set of the move.
    len_mm: f64,
    /// Commanded feedrate for the move (mm/s).
    nominal_mm_s: f64,
    /// Acceleration in force for the move (mm/s²).
    accel_mm_s2: f64,
    /// Unit XY direction, or `None` for a pure Z or extruder-only move (which the
    /// planner treats as bounded by a full stop at both ends).
    dir: Option<(f64, f64)>,
}

/// Mutable machine state tracked while walking the program.
#[derive(Debug, Clone, Copy)]
struct MotionState {
    x: f64,
    y: f64,
    z: f64,
    e: f64,
    feed_mm_min: f64,
    accel_mm_s2: f64,
}

/// Estimate the print time of a complete G-code program.
///
/// Parses `body` (the generator's emitted program text) and returns the total
/// and per-layer times.  Robust against unknown lines — anything that is not a
/// recognised move or setting is ignored.
pub(crate) fn estimate_print_time(body: &str, cfg: &EstimatorConfig) -> TimeEstimate {
    let default_accel = cfg.default_accel_mm_s2.max(EPS);
    let mut state = MotionState {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        e: 0.0,
        feed_mm_min: 0.0,
        accel_mm_s2: default_accel,
    };

    // The bucket currently being filled. Until the first `;LAYER_TIME:` marker it
    // is the preamble (start script / priming); after each marker it is one
    // printed layer. Buckets are timed and freed as each boundary is crossed, so
    // only one layer's worth of moves is ever held in memory.
    let mut current: Vec<PlannedMove> = Vec::new();
    let mut started_layers = false;
    let mut per_layer_s: Vec<f64> = Vec::new();
    let mut total_s = 0.0_f64;

    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }

        // A layer marker closes the current bucket and opens the next one. It is
        // a comment, so handle it before the comment strip below drops it.
        if line.starts_with(";LAYER_TIME:") {
            let t = plan_and_time(&current, cfg);
            total_s += t;
            if started_layers {
                // The bucket that just closed was a printed layer.
                per_layer_s.push(t);
            }
            // else: it was the preamble — counted toward the total, no layer.
            current.clear();
            started_layers = true;
            continue;
        }

        // Strip trailing comments (`G1 X.. Y.. ; retract`) and skip whole-line
        // comments.
        let code = line.split(';').next().unwrap_or("").trim();
        if code.is_empty() {
            continue;
        }

        let mut tokens = code.split_whitespace();
        let Some(cmd) = tokens.next() else {
            continue;
        };
        let cmd_up = cmd.to_ascii_uppercase();

        match cmd_up.as_str() {
            "G0" | "G1" => {
                if let Some(mv) = plan_linear_move(code, &mut state, cfg.max_velocity_mm_s) {
                    current.push(mv);
                }
            }
            "G28" => {
                // Homing parks the axes at the origin; a bare `G28` homes all.
                let homed_x = axis_value(code, 'X').is_some();
                let homed_y = axis_value(code, 'Y').is_some();
                let homed_z = axis_value(code, 'Z').is_some();
                let home_all = !(homed_x || homed_y || homed_z);
                if home_all || homed_x {
                    state.x = 0.0;
                }
                if home_all || homed_y {
                    state.y = 0.0;
                }
                if home_all || homed_z {
                    state.z = 0.0;
                }
            }
            "G92" => {
                // Redefine the current position without motion (the generator
                // uses `G92 E0` per layer to zero the extruder).
                if let Some(v) = axis_value(code, 'X') {
                    state.x = v;
                }
                if let Some(v) = axis_value(code, 'Y') {
                    state.y = v;
                }
                if let Some(v) = axis_value(code, 'Z') {
                    state.z = v;
                }
                if let Some(v) = axis_value(code, 'E') {
                    state.e = v;
                }
            }
            "M204" => {
                // Printing acceleration: prefer `P`, accept `S` as a fallback.
                if let Some(a) = axis_value(code, 'P').or_else(|| axis_value(code, 'S')) {
                    if a > 0.0 {
                        state.accel_mm_s2 = a;
                    }
                }
            }
            "SET_VELOCITY_LIMIT" => {
                // Klipper's runtime acceleration cap: `ACCEL=<value>`.
                if let Some(a) = keyword_value(code, "ACCEL") {
                    if a > 0.0 {
                        state.accel_mm_s2 = a;
                    }
                }
            }
            _ => {}
        }
    }

    // Close the final bucket: a layer if any markers were seen, else the whole
    // (marker-less) program as a single untracked total.
    let t = plan_and_time(&current, cfg);
    total_s += t;
    if started_layers {
        per_layer_s.push(t);
    }

    TimeEstimate {
        total_s,
        per_layer_s,
    }
}

/// Turn one `G0`/`G1` line into a [`PlannedMove`], advancing `state`.
///
/// `max_velocity_mm_s` (`0` = uncapped) clamps the move's nominal speed to the
/// machine velocity limit the firmware would enforce.
///
/// Returns `None` when the line carries no actual motion (e.g. a bare feedrate
/// change `G1 F1200`).
fn plan_linear_move(
    code: &str,
    state: &mut MotionState,
    max_velocity_mm_s: f64,
) -> Option<PlannedMove> {
    let nx = axis_value(code, 'X').unwrap_or(state.x);
    let ny = axis_value(code, 'Y').unwrap_or(state.y);
    let nz = axis_value(code, 'Z').unwrap_or(state.z);
    let ne = axis_value(code, 'E').unwrap_or(state.e);
    if let Some(f) = axis_value(code, 'F') {
        if f > 0.0 {
            state.feed_mm_min = f;
        }
    }

    let dx = nx - state.x;
    let dy = ny - state.y;
    let dz = nz - state.z;
    let de = ne - state.e;

    // Commit the new position/extruder state regardless of whether the move is
    // timed, so subsequent deltas stay correct.
    state.x = nx;
    state.y = ny;
    state.z = nz;
    state.e = ne;

    let xy = (dx * dx + dy * dy).sqrt();
    let mut nominal_mm_s = state.feed_mm_min / 60.0;
    if max_velocity_mm_s > 0.0 {
        nominal_mm_s = nominal_mm_s.min(max_velocity_mm_s);
    }
    if nominal_mm_s <= EPS {
        return None;
    }

    // Classify by the dominant moving axis set: XY plane first (extrude/travel),
    // then a pure Z lift, then an extruder-only retract/un-retract.
    let (len_mm, dir) = if xy > EPS {
        (xy, Some((dx / xy, dy / xy)))
    } else if dz.abs() > EPS {
        (dz.abs(), None)
    } else if de.abs() > EPS {
        (de.abs(), None)
    } else {
        return None;
    };

    Some(PlannedMove {
        len_mm,
        nominal_mm_s,
        accel_mm_s2: state.accel_mm_s2.max(EPS),
        dir,
    })
}

/// Run the two-pass look-ahead over a layer's moves and sum their trapezoidal
/// times.
fn plan_and_time(moves: &[PlannedMove], cfg: &EstimatorConfig) -> f64 {
    let n = moves.len();
    if n == 0 {
        return 0.0;
    }

    // Junction-limited entry speed for each move (cornering with its
    // predecessor), capped by both neighbours' feedrates. The first move — and
    // any move that is not an XY move or follows a non-XY move — joins at a full
    // stop.
    let mut entry = vec![0.0_f64; n];
    for i in 1..n {
        let junction = match (moves[i - 1].dir, moves[i].dir) {
            (Some(a), Some(b)) => {
                junction_speed(a, b, moves[i].accel_mm_s2, cfg.square_corner_velocity_mm_s)
            }
            _ => 0.0,
        };
        entry[i] = junction
            .min(moves[i].nominal_mm_s)
            .min(moves[i - 1].nominal_mm_s);
    }

    // Reverse pass: cap each entry so the head can still decelerate to the next
    // corner over the move length.
    let mut next_entry = 0.0_f64; // exit speed of the last move is zero
    for i in (0..n).rev() {
        let reachable = (next_entry * next_entry + 2.0 * moves[i].accel_mm_s2 * moves[i].len_mm)
            .max(0.0)
            .sqrt();
        entry[i] = entry[i].min(reachable);
        next_entry = entry[i];
    }

    // Forward pass: cap each entry so it is reachable by accelerating from the
    // previous move's entry over the previous length.
    for i in 1..n {
        let reachable = (entry[i - 1] * entry[i - 1]
            + 2.0 * moves[i - 1].accel_mm_s2 * moves[i - 1].len_mm)
            .max(0.0)
            .sqrt();
        entry[i] = entry[i].min(reachable);
    }
    entry[0] = 0.0;

    // Sum trapezoidal times: exit speed of move i is the entry speed of move i+1
    // (zero for the last move).
    let mut total = 0.0;
    for i in 0..n {
        let v_exit = if i + 1 < n { entry[i + 1] } else { 0.0 };
        total += move_time(
            moves[i].len_mm,
            moves[i].nominal_mm_s,
            moves[i].accel_mm_s2,
            entry[i],
            v_exit,
        );
    }
    total
}

/// Time (s) for a single trapezoidal move of length `len` that enters at `v_e`,
/// leaves at `v_x`, cruises no faster than `v_nom`, and ramps at `a`.
fn move_time(len: f64, v_nom: f64, a: f64, v_e: f64, v_x: f64) -> f64 {
    if len <= EPS {
        return 0.0;
    }
    let v_nom = v_nom.max(EPS);
    if a <= EPS {
        return len / v_nom;
    }
    let v_e = v_e.min(v_nom).max(0.0);
    let v_x = v_x.min(v_nom).max(0.0);

    // Distance needed to ramp entry→cruise and cruise→exit.
    let d_acc = (v_nom * v_nom - v_e * v_e) / (2.0 * a);
    let d_dec = (v_nom * v_nom - v_x * v_x) / (2.0 * a);

    if d_acc + d_dec <= len {
        // Full trapezoid: accelerate, cruise, decelerate.
        let d_cruise = len - d_acc - d_dec;
        (v_nom - v_e) / a + d_cruise / v_nom + (v_nom - v_x) / a
    } else {
        // Triangle: never reach cruise. Solve for the peak speed where the accel
        // and decel ramps meet.
        let v_peak = (((2.0 * a * len) + v_e * v_e + v_x * v_x) / 2.0)
            .max(0.0)
            .sqrt()
            .max(v_e)
            .max(v_x);
        (v_peak - v_e) / a + (v_peak - v_x) / a
    }
}

/// Maximum cornering speed (mm/s) between two unit XY directions using the
/// junction-deviation model.
///
/// A straight-through join returns [`f64::INFINITY`] (no corner limit — the
/// caller caps it to the moves' feedrates); a hairpin reversal returns `0`.
fn junction_speed(u_prev: (f64, f64), u_curr: (f64, f64), accel: f64, scv: f64) -> f64 {
    let a = accel.max(EPS);
    let dot = (u_prev.0 * u_curr.0 + u_prev.1 * u_curr.1).clamp(-1.0, 1.0);
    // Marlin/Klipper convention: `sin(θ/2)` from the *negated* dot product.
    let junction_cos = -dot;
    let sin_half = (0.5 * (1.0 - junction_cos)).max(0.0).sqrt();
    if sin_half >= 1.0 - 1e-9 {
        return f64::INFINITY; // straight-through
    }
    if sin_half <= 1e-9 {
        return 0.0; // reversal
    }
    // Junction deviation calibrated so a 90° corner is taken at `scv`.
    let jd = scv * scv * (std::f64::consts::SQRT_2 - 1.0) / a;
    (a * jd * sin_half / (1.0 - sin_half)).sqrt()
}

/// Read the numeric value of a single-letter axis word (e.g. `X12.3` → `12.3`).
///
/// Matches on a whitespace-delimited token whose first char is `letter`, so it
/// never confuses `E` inside `SET_VELOCITY_LIMIT` for an axis word.
fn axis_value(code: &str, letter: char) -> Option<f64> {
    let up = letter.to_ascii_uppercase();
    for token in code.split_whitespace() {
        let mut chars = token.chars();
        if let Some(first) = chars.next() {
            if first.to_ascii_uppercase() == up {
                return chars.as_str().parse::<f64>().ok();
            }
        }
    }
    None
}

/// Read the value of a `KEYWORD=<number>` argument (Klipper style).
fn keyword_value(code: &str, keyword: &str) -> Option<f64> {
    for token in code.split_whitespace() {
        if let Some((key, val)) = token.split_once('=') {
            if key.eq_ignore_ascii_case(keyword) {
                return val.parse::<f64>().ok();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Time to move `len` mm from rest to rest at feedrate `v` with accel `a`.
    fn rest_to_rest(len: f64, v: f64, a: f64) -> f64 {
        move_time(len, v, a, 0.0, 0.0)
    }

    #[test]
    fn trapezoid_matches_hand_calculation() {
        // 100 mm at 100 mm/s, a = 1000 mm/s².
        // Ramp to 100 mm/s: t = 0.1 s over d = 5 mm. Two ramps = 10 mm.
        // Cruise 90 mm at 100 mm/s = 0.9 s. Ramps = 0.2 s. Total = 1.1 s.
        let t = rest_to_rest(100.0, 100.0, 1000.0);
        assert!((t - 1.1).abs() < 1e-6, "t = {t}");
    }

    #[test]
    fn short_move_is_a_triangle_and_never_reaches_feedrate() {
        // 1 mm at 200 mm/s, a = 1000. Full ramp to 200 needs 20 mm each side, so
        // it is a triangle. Peak = sqrt(a*len) = sqrt(1000) ≈ 31.62 mm/s.
        // Time = 2 * peak / a = 2 * 31.62 / 1000 ≈ 0.06325 s.
        let t = rest_to_rest(1.0, 200.0, 1000.0);
        assert!((t - 0.0632455).abs() < 1e-5, "t = {t}");
    }

    #[test]
    fn slower_acceleration_takes_longer() {
        let fast = rest_to_rest(50.0, 100.0, 4000.0);
        let slow = rest_to_rest(50.0, 100.0, 500.0);
        assert!(slow > fast, "slow {slow} should exceed fast {fast}");
    }

    #[test]
    fn zero_acceleration_falls_back_to_constant_speed() {
        // a ≤ 0 → length / speed. 100 mm at 50 mm/s = 2 s.
        let t = move_time(100.0, 50.0, 0.0, 0.0, 0.0);
        assert!((t - 2.0).abs() < 1e-9, "t = {t}");
    }

    #[test]
    fn junction_straight_is_unlimited_and_reversal_stops() {
        let straight = junction_speed((1.0, 0.0), (1.0, 0.0), 1000.0, 5.0);
        assert!(straight.is_infinite(), "straight join must not be limited");
        let reversal = junction_speed((1.0, 0.0), (-1.0, 0.0), 1000.0, 5.0);
        assert!(reversal < 1e-6, "reversal must stop: {reversal}");
    }

    #[test]
    fn junction_right_angle_equals_square_corner_velocity() {
        // By construction the 90° corner speed equals the configured scv.
        let v = junction_speed((1.0, 0.0), (0.0, 1.0), 1500.0, 5.0);
        assert!((v - 5.0).abs() < 1e-6, "90° corner speed = {v}");
    }

    #[test]
    fn empty_program_is_zero() {
        let est = estimate_print_time("", &EstimatorConfig::default());
        assert_eq!(est.total_s, 0.0);
        assert!(est.per_layer_s.is_empty());
    }

    #[test]
    fn max_velocity_cap_slows_a_fast_move() {
        // A 200 mm move commanded at 300 mm/s (F18000), a = 1000.
        let body = "\
;LAYER_TIME:0.0
M204 P1000
G1 X0 Y0 F18000
G1 X200 Y0 E10 F18000
";
        let uncapped = estimate_print_time(body, &EstimatorConfig::default());
        let capped = estimate_print_time(
            body,
            &EstimatorConfig {
                max_velocity_mm_s: 100.0,
                ..EstimatorConfig::default()
            },
        );
        assert!(
            capped.total_s > uncapped.total_s,
            "capping velocity to 100 must slow the move: capped {} vs uncapped {}",
            capped.total_s,
            uncapped.total_s
        );
    }

    #[test]
    fn square_corner_velocity_config_changes_cornering_time() {
        // A square perimeter: four 90° corners. A higher square-corner velocity
        // lets the head keep more speed through them, so the layer is faster.
        let body = "\
;LAYER_TIME:0.0
M204 P1000
G1 X0 Y0 F6000
G1 X20 Y0 E1 F6000
G1 X20 Y20 E2 F6000
G1 X0 Y20 E3 F6000
G1 X0 Y0 E4 F6000
";
        let slow_corners = estimate_print_time(
            body,
            &EstimatorConfig {
                square_corner_velocity_mm_s: 1.0,
                ..EstimatorConfig::default()
            },
        );
        let fast_corners = estimate_print_time(
            body,
            &EstimatorConfig {
                square_corner_velocity_mm_s: 20.0,
                ..EstimatorConfig::default()
            },
        );
        assert!(
            fast_corners.total_s < slow_corners.total_s,
            "faster cornering must shorten the layer: fast {} vs slow {}",
            fast_corners.total_s,
            slow_corners.total_s
        );
    }

    #[test]
    fn from_params_maps_zero_scv_to_default_and_passes_velocity() {
        let mut params = crate::settings::params::SlicingParams::default();
        params.square_corner_velocity = 0.0; // → estimator default
        params.max_velocity = 250.0;
        let cfg = EstimatorConfig::from_params(&params);
        assert!(
            (cfg.square_corner_velocity_mm_s - DEFAULT_SQUARE_CORNER_VELOCITY_MM_S).abs() < 1e-9
        );
        assert!((cfg.max_velocity_mm_s - 250.0).abs() < 1e-9);

        params.square_corner_velocity = 8.0;
        let cfg = EstimatorConfig::from_params(&params);
        assert!((cfg.square_corner_velocity_mm_s - 8.0).abs() < 1e-9);
    }

    #[test]
    fn single_extrusion_move_from_rest_to_rest() {
        // A layer with one 100 mm extrusion at 100 mm/s (F6000), a = 1000.
        let body = "\
;LAYER_TIME:0.0
M204 P1000
G1 X0 Y0 F6000
G1 X100 Y0 E5 F6000
";
        let est = estimate_print_time(body, &EstimatorConfig::default());
        // First `G1 X0 Y0` is a zero-length move from the origin → no time.
        // The 100 mm extrusion rest-to-rest ≈ 1.1 s.
        assert_eq!(est.per_layer_s.len(), 1);
        assert!(
            (est.per_layer_s[0] - 1.1).abs() < 1e-3,
            "layer time = {}",
            est.per_layer_s[0]
        );
        assert!((est.total_s - est.per_layer_s[0]).abs() < 1e-9);
    }

    #[test]
    fn per_layer_split_and_total_agree() {
        let body = "\
;LAYER_TIME:0.0
M204 P1000
G1 X0 Y0 F6000
G1 X100 Y0 E5 F6000
;LAYER_TIME:0.0
G1 X100 Y0 F6000
G1 X0 Y0 E10 F6000
";
        let est = estimate_print_time(body, &EstimatorConfig::default());
        assert_eq!(est.per_layer_s.len(), 2);
        let sum: f64 = est.per_layer_s.iter().sum();
        assert!(
            (est.total_s - sum).abs() < 1e-9,
            "total {} must equal layer sum {}",
            est.total_s,
            sum
        );
        // Both layers print the same 100 mm segment → equal times.
        assert!((est.per_layer_s[0] - est.per_layer_s[1]).abs() < 1e-6);
    }

    #[test]
    fn retraction_and_travel_add_time_before_first_marker() {
        // Preamble moves (before any layer marker) count toward the total but no
        // layer bucket.
        let body = "\
G1 X50 Y0 F9000
G1 E-1 F3000
G1 E0 F3000
";
        let est = estimate_print_time(body, &EstimatorConfig::default());
        assert!(est.per_layer_s.is_empty());
        assert!(
            est.total_s > 0.0,
            "preamble travel/retraction must take time"
        );
    }

    #[test]
    fn klipper_acceleration_command_is_parsed() {
        // A very low SET_VELOCITY_LIMIT ACCEL should slow the estimate vs default.
        let body_slow = "\
;LAYER_TIME:0.0
SET_VELOCITY_LIMIT ACCEL=200
G1 X0 Y0 F6000
G1 X50 Y0 E5 F6000
";
        let body_fast = "\
;LAYER_TIME:0.0
SET_VELOCITY_LIMIT ACCEL=5000
G1 X0 Y0 F6000
G1 X50 Y0 E5 F6000
";
        let slow = estimate_print_time(body_slow, &EstimatorConfig::default());
        let fast = estimate_print_time(body_fast, &EstimatorConfig::default());
        assert!(
            slow.total_s > fast.total_s,
            "low accel {} should exceed high accel {}",
            slow.total_s,
            fast.total_s
        );
    }

    #[test]
    fn faster_feedrate_never_increases_time() {
        let body =
            |f: i32| format!(";LAYER_TIME:0.0\nM204 P1000\nG1 X0 Y0 F{f}\nG1 X200 Y0 E10 F{f}\n");
        let slow = estimate_print_time(&body(1200), &EstimatorConfig::default());
        let fast = estimate_print_time(&body(9000), &EstimatorConfig::default());
        assert!(
            fast.total_s <= slow.total_s,
            "faster feed {} must not exceed slower feed {}",
            fast.total_s,
            slow.total_s
        );
    }

    #[test]
    fn comments_and_unknown_lines_are_ignored() {
        let body = "\
; a comment
;TYPE:Outer wall
;LAYER_TIME:0.0
M117 hello
G1 X0 Y0 F6000
G1 X10 Y0 E1 F6000 ; extrude
";
        let est = estimate_print_time(body, &EstimatorConfig::default());
        assert_eq!(est.per_layer_s.len(), 1);
        assert!(est.per_layer_s[0] > 0.0);
    }
}
