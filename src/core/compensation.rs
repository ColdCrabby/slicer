//! Dimensional compensation — XY size correction and elephant-foot removal.
//!
//! Both corrections reshape the **raw cross-section contours** produced by
//! [`slice_mesh`](crate::core::slice_mesh), before any wall bead exists. That
//! placement is the whole point: a wall generator turns a contour into beads, so
//! correcting the contour corrects every bead, the interior, the surfaces and
//! the infill in one move. Correcting the beads afterwards would leave the
//! interior — and therefore the solid surfaces — sized to the uncorrected model.
//!
//! # XY size compensation
//!
//! A signed uniform offset applied to every layer. It exists to cancel a
//! *systematic* dimensional error of a machine/material pairing (a nozzle that
//! consistently prints 0.1 mm oversize). It is a plain offset with mitred
//! joins, because dimensional fidelity is exactly what it is for.
//!
//! # Elephant foot
//!
//! The first layer is squashed into the bed, so it bulges outward and the base
//! of the print measures oversize. The fix is to shrink the layers nearest the
//! bed — but a uniform shrink is a bad fix, and this module exists to be a good
//! one. Four rules shape it:
//!
//! | Rule | What it does |
//! | --- | --- |
//! | **Medial-limited** | The shrink at a point is capped by the *local feature width* there, so a feature never gets narrower than [`min_contour_width_mm`](CompensationConfig::min_contour_width_mm). |
//! | **Layer-0-only or tapered** | Full correction at the bed, ramping linearly to zero over [`elephant_foot_layers`](CompensationConfig::elephant_foot_layers). |
//! | **Raft-gated** | Skipped entirely on a raft: the object's first layer lands on sacrificial material across an air gap and is never squashed into the bed. |
//! | **Cliff-guarded** | Withheld where the *model itself* flares steeply outward, so a narrow base under a wide body is never undercut. |
//!
//! The medial limit is the one that matters most in practice. A uniform inward
//! offset of 0.2 mm deletes every first-layer feature narrower than 0.4 mm —
//! embossed text, logo strokes, thin ribs — which is precisely the detail a
//! first layer is judged on. So the shrink is computed *per contour vertex*
//! from the largest circle that fits inside the material there, and the result
//! is a **variable offset** rather than a uniform one.
//!
//! ```text
//! feature width w    │ shrink applied per side
//! ───────────────────┼──────────────────────────
//! w ≤ w_min          │ 0            (untouched)
//! w_min … w_min+2δ   │ (w − w_min)/2 (partial)
//! w ≥ w_min + 2δ     │ δ            (full)
//! ```
//!
//! In every case the feature comes out `max(w_min, w − 2δ)` wide, so nothing
//! thin is ever erased.
//!
//! # Non-goals
//!
//! Hole-specific compensation (growing holes by a separate delta) is **not**
//! implemented here; it needs per-contour hole classification and its own
//! setting.

use clipper2::{EndType, FillRule, JoinType, Path, Paths};

use crate::core::types::{ExtrusionRole, SliceLayer};
use crate::settings::params::{AdhesionType, SlicingParams};

/// Automatic [`min_contour_width_mm`](CompensationConfig::min_contour_width_mm),
/// as a multiple of the outer-wall extrusion width.
///
/// A feature narrower than its own perimeter bead cannot be printed as anything
/// but a single line, so 1.5 beads is the width below which shrinking starts
/// destroying rather than correcting — the same reference PrusaSlicer's
/// elephant-foot pass uses.
const MIN_CONTOUR_WIDTH_WALL_MULT: f64 = 1.5;

/// Outward flare per layer, as a multiple of the outer-wall extrusion width,
/// that is still ordinary model geometry rather than a cliff.
///
/// Chamfered and filleted bases are everywhere — both the Voron cube and the
/// filament caddy in this repo's own corpus flare 0.1–0.2 mm on their second
/// layer — and a user who asks for 0.2 mm of correction expects 0.2 mm of
/// correction, not a silent halving because their model has a lead-in. Below
/// this the guard stays out of the way entirely.
const CLIFF_FREE_WALL_MULT: f64 = 1.0;

/// Outward flare per layer, as a multiple of the outer-wall extrusion width, at
/// which compensation is withheld completely.
///
/// Three beads in a single layer is not a chamfer — it is a pedestal under a
/// wider body, or a base the model deliberately flares away from. Undercutting
/// that is never a correction: it deepens an overhang the model already has and
/// costs bed grip exactly where the print can least afford it. Between the two
/// multipliers the correction ramps down linearly, so the guard is a guard and
/// not a governor.
const CLIFF_LIMIT_WALL_MULT: f64 = 3.0;

/// Contour resampling interval, as a fraction of the capped medial radius.
///
/// The variable offset moves vertices along their own normals, so it is only as
/// smooth as the contour is dense. A quarter of the radius that the limit acts
/// over resolves the feature-width field without exploding the vertex count.
const RESAMPLE_FRACTION: f64 = 0.25;

/// Bounds on the resample interval (mm). The floor stays clear of Clipper2's
/// 0.01 mm quantisation; the ceiling keeps a huge model's contours affordable.
const RESAMPLE_MIN_MM: f64 = 0.05;
const RESAMPLE_MAX_MM: f64 = 0.5;

/// Arc-length window for the running **maximum** of the medial radius, as a
/// multiple of the capped radius.
///
/// The largest circle that fits inside the material touching a point collapses
/// toward zero all along a *convex corner* — true, but useless as a limit,
/// because it would leave an uncompensated nub on every corner of every model.
/// Taking the largest radius within a short walk along the contour restores the
/// corner, since its neighbours sit on real material.
///
/// On its own this maximum would also lift a thin rib **attached** to a thick
/// body, pinching the rib off at its root. The `opposed` reading from
/// [`SegmentGrid::tangent_radii`] is what caps it back down.
const REGULARISE_ARC_MULT: f64 = 2.0;

/// Arc-length window for smoothing the final per-vertex shrink, as a multiple
/// of the capped medial radius. Smoothing may only ever *reduce* the shrink
/// (see [`smooth_downward`]), so it can soften a transition but never re-erode
/// a feature the medial limit just protected.
const SMOOTH_ARC_MULT: f64 = 1.0;

/// Cap on the mitre displacement at a sharp corner, as a multiple of the shrink
/// applied there — the same limit this codebase passes to Clipper2's offsetter.
///
/// A corner vertex has to travel further than the edges either side of it to
/// keep both of them the requested distance in, and at a needle-sharp corner
/// "further" tends to infinity. Clamping trades an exact point for a small
/// bevel, which is what every polygon offsetter does.
const MITRE_LIMIT: f64 = 2.0;

/// Below this the geometry is rounding noise: 1 nm, far under Clipper2's 10 µm
/// quantisation.
const EPS_MM: f64 = 1e-6;

/// Simplification tolerance for the offset result, at half of Clipper2's 10 µm
/// quantisation. Resampling multiplies a contour's vertex count several-fold;
/// this drops the ones that ended up carrying no shape the grid can represent,
/// so the wall generator is not handed a needlessly dense polygon.
const SIMPLIFY_EPS_MM: f64 = 0.005;

/// Resolved dimensional-compensation settings for one slice.
///
/// Produced by [`CompensationConfig::resolve`], which returns `None` when there
/// is nothing to do — the common case, since both corrections default to off.
#[derive(Debug, Clone, PartialEq)]
pub struct CompensationConfig {
    /// Signed uniform XY offset in mm, applied to **every** layer. Negative
    /// shrinks the model, positive grows it. Zero disables.
    pub xy_size_mm: f64,
    /// Inward shrink in mm applied at the bed. Zero disables.
    pub elephant_foot_mm: f64,
    /// Number of layers the shrink ramps to zero over. `1` corrects the first
    /// layer only.
    pub elephant_foot_layers: usize,
    /// Width in mm no feature may be shrunk below.
    pub min_contour_width_mm: f64,
    /// Outward model flare in mm that is still ordinary geometry: below this
    /// the cliff guard does nothing at all.
    pub cliff_free_mm: f64,
    /// Outward model flare in mm at which compensation is fully withheld.
    pub cliff_limit_mm: f64,
    /// The print's layer height, used to recognise which layers actually sit on
    /// the bed.
    pub layer_height_mm: f64,
}

impl CompensationConfig {
    /// Resolve the compensation settings, or `None` when this slice needs none.
    ///
    /// Returns `None` for the default configuration, which is what keeps the
    /// pass a true no-op: an unconfigured slice never touches its contours, so
    /// its output is byte-identical to a build without this module.
    pub fn resolve(params: &SlicingParams) -> Option<Self> {
        let xy_size_mm = if params.xy_size_compensation_mm.abs() > EPS_MM {
            params.xy_size_compensation_mm
        } else {
            0.0
        };

        // Raft gate: the object's first layer is printed onto sacrificial
        // material across `raft_air_gap`, never squashed into the bed, so there
        // is no elephant foot to remove. Shrinking it there would only lose
        // grip on the raft.
        let on_raft = params.adhesion_type == AdhesionType::Raft && params.raft_layers > 0;
        let elephant_foot_mm = if params.elephant_foot_compensation_mm > EPS_MM && !on_raft {
            params.elephant_foot_compensation_mm
        } else {
            0.0
        };

        if xy_size_mm == 0.0 && elephant_foot_mm == 0.0 {
            return None;
        }

        let wall_width = crate::core::outer_wall_nominal_width_mm(params);
        let min_contour_width_mm = if params.elephant_foot_min_contour_width_mm > 0.0 {
            params.elephant_foot_min_contour_width_mm
        } else {
            MIN_CONTOUR_WIDTH_WALL_MULT * wall_width
        };

        Some(Self {
            xy_size_mm,
            elephant_foot_mm,
            elephant_foot_layers: params.elephant_foot_layers.max(1),
            min_contour_width_mm,
            cliff_free_mm: CLIFF_FREE_WALL_MULT * wall_width,
            cliff_limit_mm: CLIFF_LIMIT_WALL_MULT * wall_width,
            layer_height_mm: params.layer_height,
        })
    }

    /// The shrink applied to layer `index`, tapering linearly to zero.
    ///
    /// Layer 0 always receives the full correction; with the default single
    /// layer every layer above it receives none.
    fn shrink_for_layer(&self, index: usize) -> f64 {
        let n = self.elephant_foot_layers;
        if index >= n {
            return 0.0;
        }
        self.elephant_foot_mm * (n - index) as f64 / n as f64
    }

    /// Widest medial radius the limit can act over: beyond this the full shrink
    /// applies unconditionally, so nothing further away needs measuring.
    fn radius_cap(&self, shrink: f64) -> f64 {
        self.min_contour_width_mm * 0.5 + shrink
    }
}

/// Apply XY size and elephant-foot compensation to a sliced model, in place.
///
/// A convenience over [`CompensationConfig::resolve`] + [`apply_with_config`]
/// for tests. The pipeline resolves the config itself, because it logs the
/// numbers it resolved.
///
/// # Preconditions
///
/// Runs on the output of [`slice_mesh`](crate::core::slice_mesh), where every
/// path is a closed [`OuterWall`](ExtrusionRole::OuterWall) contour and no
/// per-path metadata exists yet. The pass rewrites a layer's **whole** path
/// list, so a layer carrying anything else would have its paths silently
/// relabelled and its parallel arrays desynchronised.
///
/// That precondition is why nothing here is exported past `core`, matching the
/// other passes `process_mesh` sequences (`apply_single_wall_restrictions`,
/// `classify_overhang_perimeters`): this is a *stage*, not an operation
/// callable on an arbitrary `SliceLayer`.
#[cfg(test)]
pub(crate) fn apply_dimensional_compensation(layers: &mut [SliceLayer], params: &SlicingParams) {
    let Some(config) = CompensationConfig::resolve(params) else {
        return;
    };
    apply_with_config(layers, &config);
}

/// [`apply_dimensional_compensation`] against an already-resolved config.
pub(crate) fn apply_with_config(layers: &mut [SliceLayer], config: &CompensationConfig) {
    if layers.is_empty() {
        return;
    }
    debug_assert!(
        layers.iter().all(is_raw_slice_output),
        "dimensional compensation rewrites whole path lists, so it must run on \
         raw slice_mesh output — before walls, surfaces or infill exist"
    );

    // ── XY size compensation: one uniform offset, every layer ────────────────
    if config.xy_size_mm != 0.0 {
        for layer in layers.iter_mut() {
            let Some(normalised) = union_even_odd(layer.paths.clone()) else {
                continue;
            };
            let offset = clipper2::inflate(
                normalised,
                config.xy_size_mm,
                JoinType::Miter,
                EndType::Polygon,
                2.0,
            );
            set_contours(layer, offset);
        }
    }

    if config.elephant_foot_mm <= 0.0 {
        return;
    }

    // ── Elephant foot: variable, medial-limited offset near the bed ──────────
    //
    // Walking bottom-up matters: layer `i`'s cliff guard reads layer `i + 1`,
    // which must still be the model's own geometry. Since a layer is rewritten
    // only after the layer below has already consulted it, that holds without
    // snapshotting anything.
    for index in 0..config.elephant_foot_layers.min(layers.len()) {
        let shrink = config.shrink_for_layer(index);
        if shrink <= EPS_MM {
            continue;
        }
        // Only layers that genuinely rest on the bed have an elephant foot. An
        // object lifted clear of the plate (or printed object-by-object from a
        // raised start) slices its own layer 0 in mid-air, where there is
        // nothing to squash against.
        if !sits_on_bed(layers[index].z, index, config.layer_height_mm) {
            continue;
        }

        let Some(normalised) = union_even_odd(layers[index].paths.clone()) else {
            continue;
        };
        let next = layers
            .get(index + 1)
            .and_then(|layer| union_even_odd(layer.paths.clone()));

        let compensated = medial_limited_shrink(&normalised, shrink, next.as_ref(), config);
        set_contours(&mut layers[index], compensated);
    }
}

/// Whether the layer at `index` with height `z` is resting on the build plate.
///
/// [`slice_mesh`](crate::core::slice_mesh) samples layer `i` at
/// `z_min + (i + 0.5) · h`, so a model on the plate puts layer `i` at exactly
/// `(i + 0.5) · h`. The tolerance is the scene engine's own bed epsilon: STL
/// coordinates are `f32`, so a model resting on the bed lands a few millionths
/// of a millimetre either side of zero.
fn sits_on_bed(z: f64, index: usize, layer_height_mm: f64) -> bool {
    const BED_TOLERANCE_MM: f64 = 1e-3;
    layer_height_mm > 0.0 && z <= (index as f64 + 0.5) * layer_height_mm + BED_TOLERANCE_MM
}

/// Shrink `contours` inward by at most `shrink` mm, limited per vertex by the
/// local feature width and by the model's own flare on `next`.
fn medial_limited_shrink(
    contours: &Paths,
    shrink: f64,
    next: Option<&Paths>,
    config: &CompensationConfig,
) -> Paths {
    let radius_cap = config.radius_cap(shrink);
    let step = (radius_cap * RESAMPLE_FRACTION).clamp(RESAMPLE_MIN_MM, RESAMPLE_MAX_MM);

    let mut resampled: Vec<Contour> = contours
        .iter()
        .filter_map(|path| Contour::resample(path, step))
        .collect();
    if resampled.is_empty() {
        return contours.clone();
    }

    // One index over the whole layer, so a feature squeezed between an island
    // and a neighbouring hole is measured as thin even though the two belong to
    // different sub-paths.
    let own = SegmentGrid::build(&resampled, radius_cap);
    let arc_skip = radius_cap * REGULARISE_ARC_MULT;
    let ahead = next.map(|paths| {
        let contours: Vec<Contour> = paths
            .iter()
            .filter_map(|path| Contour::resample(path, step))
            .collect();
        let grid = SegmentGrid::build(&contours, config.cliff_limit_mm);
        (grid, PolygonSet::from_paths(paths))
    });

    for contour in resampled.iter_mut() {
        // Two readings of the local half-width, with disjoint failure modes.
        let mut near = Vec::with_capacity(contour.points.len());
        let mut opposed = Vec::with_capacity(contour.points.len());
        for i in 0..contour.points.len() {
            let (n, o) = own.tangent_radii(contour.points[i], contour.normals[i], radius_cap);
            near.push(n);
            opposed.push(o);
        }

        // A convex corner drags `near` toward zero along its whole approach; a
        // short running maximum restores it. That maximum can leak a thick
        // body's radius into an attached rib, so `opposed` — which measures the
        // rib against its own facing side — caps it back down.
        let lifted = running_max(&near, &contour.arc, contour.total, arc_skip);
        let radii: Vec<f64> = lifted
            .iter()
            .zip(&opposed)
            .map(|(l, o)| l.min(*o))
            .collect();

        let limited: Vec<f64> = radii
            .iter()
            .enumerate()
            .map(|(i, radius)| {
                let medial = (radius - config.min_contour_width_mm * 0.5).clamp(0.0, shrink);
                let cliff = match &ahead {
                    Some((grid, polygons)) => {
                        // The contour normal points into the material, so its
                        // negation is the direction any flare reaches out along.
                        let normal = contour.normals[i];
                        shrink
                            * cliff_factor(
                                contour.points[i],
                                [-normal[0], -normal[1]],
                                grid,
                                polygons,
                                config.cliff_free_mm,
                                config.cliff_limit_mm,
                            )
                    }
                    None => shrink,
                };
                medial.min(cliff)
            })
            .collect();

        contour.offsets = smooth_downward(
            &limited,
            &contour.arc,
            contour.total,
            radius_cap * SMOOTH_ARC_MULT,
        );
    }

    let displaced: Vec<Path> = resampled.iter().map(Contour::displaced).collect();

    // `Positive` discards the reversed folds a variable offset creates where it
    // overruns a concavity, while a clockwise hole still subtracts correctly.
    let cleaned = clipper2::union(
        Paths::new(displaced),
        Paths::new(vec![]),
        FillRule::Positive,
    )
    .unwrap_or_else(|_| contours.clone());

    // The medial limit means no feature can shrink past `min_contour_width_mm`,
    // so an empty result is impossible by construction — but this is the layer
    // the print stands on, and losing it would leave the model with no base at
    // all. Keep the original rather than trust that reasoning at runtime.
    if cleaned.is_empty() {
        return contours.clone();
    }

    // Resampling multiplied the vertex count; drop the collinear runs it added
    // before handing the contours to the wall generator.
    cleaned.simplify(SIMPLIFY_EPS_MM, false)
}

/// How much of the requested shrink the model's own profile allows at `point`,
/// as a factor in `0.0..=1.0`.
///
/// `1.0` where the layer above sits over `point`, is inset from it, or flares
/// no further than `cliff_free_mm` — a vertical wall, an inward taper, or the
/// chamfered base that most printable models have, all of which are exactly
/// where an elephant foot forms. It ramps to `0.0` as the flare reaches
/// `cliff_limit_mm`, because there the base is already the narrowest part of
/// the model and cutting into it would deepen an overhang instead of
/// correcting a bulge.
///
/// The flare is measured **along `outward`**, by walking out of the layer above
/// until its material ends. The nearest boundary in *any* direction is not the
/// same question and gets it wrong: a deep ledge can overhang while some
/// unrelated edge — a rim running tangentially past, the near side of a hole —
/// sits closer, reads under `free_mm`, and waves the full shrink through.
fn cliff_factor(
    point: [f64; 2],
    outward: [f64; 2],
    grid: &SegmentGrid,
    polygons: &PolygonSet,
    free_mm: f64,
    limit_mm: f64,
) -> f64 {
    if limit_mm <= free_mm + EPS_MM {
        return 1.0;
    }
    if !polygons.contains(point) {
        // The layer above is inset here: the shrink cuts into material that
        // overhangs nothing.
        return 1.0;
    }
    let flare = grid.ray_exit_distance(point, outward, limit_mm);
    ((limit_mm - flare) / (limit_mm - free_mm)).clamp(0.0, 1.0)
}

// ─────────────────────────────────────────────────────────────────────────────
// Contours
// ─────────────────────────────────────────────────────────────────────────────

/// A closed contour resampled to a near-uniform vertex spacing, carrying the
/// inward unit normal and cumulative arc length at every vertex.
struct Contour {
    points: Vec<[f64; 2]>,
    /// Unit normal at each vertex pointing **into the material**. Used to
    /// *measure* the material, so it must stay unit length.
    normals: Vec<[f64; 2]>,
    /// Displacement per 1 mm of shrink at each vertex — the unit normal at a
    /// straight run, the mitre vector at a corner. Used to *move* the vertex.
    mitres: Vec<[f64; 2]>,
    /// Arc length from `points[0]` to `points[i]`.
    arc: Vec<f64>,
    /// Total closed-loop length.
    total: f64,
    /// Per-vertex inward displacement, filled in by the shrink pass.
    offsets: Vec<f64>,
}

impl Contour {
    /// Resample `path` so no edge is longer than `step`, keeping every original
    /// vertex so corners survive.
    ///
    /// Returns `None` for a degenerate path that cannot describe an area.
    fn resample(path: &Path, step: f64) -> Option<Self> {
        let source: Vec<[f64; 2]> = path.iter().map(|p| [p.x(), p.y()]).collect();
        if source.len() < 3 || step <= 0.0 {
            return None;
        }

        let mut points: Vec<[f64; 2]> = Vec::with_capacity(source.len() * 2);
        for i in 0..source.len() {
            let a = source[i];
            let b = source[(i + 1) % source.len()];
            if points.last().is_none_or(|last| distance(*last, a) > EPS_MM) {
                points.push(a);
            }
            let span = distance(a, b);
            if span <= step {
                continue;
            }
            let divisions = (span / step).ceil() as usize;
            for k in 1..divisions {
                let t = k as f64 / divisions as f64;
                points.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
            }
        }
        // The loop closes back onto the first vertex; drop a duplicate tail.
        while points.len() >= 2 && distance(points[0], *points.last().unwrap()) <= EPS_MM {
            points.pop();
        }
        if points.len() < 3 {
            return None;
        }

        // The material lies to the **left** of the direction of travel for both
        // windings Clipper2 produces — counter-clockwise islands and clockwise
        // holes alike — so one rule gives an inward normal for every contour.
        let count = points.len();
        let mut normals = Vec::with_capacity(count);
        let mut mitres = Vec::with_capacity(count);
        for i in 0..count {
            let prev = points[(i + count - 1) % count];
            let here = points[i];
            let next = points[(i + 1) % count];
            let incoming = left_normal(here[0] - prev[0], here[1] - prev[1]);
            let outgoing = left_normal(next[0] - here[0], next[1] - here[1]);
            normals.push(normalise([
                incoming[0] + outgoing[0],
                incoming[1] + outgoing[1],
            ]));
            mitres.push(mitre_vector(incoming, outgoing));
        }

        let mut arc = Vec::with_capacity(count);
        let mut travelled = 0.0;
        for i in 0..count {
            arc.push(travelled);
            travelled += distance(points[i], points[(i + 1) % count]);
        }

        Some(Self {
            points,
            normals,
            mitres,
            arc,
            total: travelled,
            offsets: Vec::new(),
        })
    }

    fn segments(&self) -> impl Iterator<Item = Seg> + '_ {
        let count = self.points.len();
        (0..count).map(move |i| {
            let a = self.points[i];
            let b = self.points[(i + 1) % count];
            Seg {
                a,
                b,
                normal: left_normal(b[0] - a[0], b[1] - a[1]),
            }
        })
    }

    /// The contour with every vertex pushed inward by its own offset.
    fn displaced(&self) -> Path {
        let points: Vec<(f64, f64)> = self
            .points
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let d = self.offsets.get(i).copied().unwrap_or(0.0);
                let m = self.mitres[i];
                (p[0] + m[0] * d, p[1] + m[1] * d)
            })
            .collect();
        points.into()
    }
}

/// Displacement, per unit of inward offset, that keeps both edges meeting at a
/// vertex exactly that far in.
///
/// The two offset edges intersect at `p + δ · (n₁ + n₂) / (1 + n₁·n₂)`, which
/// reduces to the plain normal along a straight run (`n₁ = n₂`) and stretches to
/// the mitre point at a corner — a right angle needs `√2 · δ` along its
/// bisector to move both of its edges in by `δ`. Sharp corners are clamped to
/// [`MITRE_LIMIT`].
fn mitre_vector(incoming: [f64; 2], outgoing: [f64; 2]) -> [f64; 2] {
    let sum = [incoming[0] + outgoing[0], incoming[1] + outgoing[1]];
    let denominator = 1.0 + incoming[0] * outgoing[0] + incoming[1] * outgoing[1];
    if denominator <= EPS_MM {
        // A near-180° turn: the contour doubles back on itself and there is no
        // meaningful mitre point. Fall back to the averaged normal.
        return normalise(sum);
    }
    let vector = [sum[0] / denominator, sum[1] / denominator];
    let length = (vector[0] * vector[0] + vector[1] * vector[1]).sqrt();
    if length > MITRE_LIMIT {
        [
            vector[0] * MITRE_LIMIT / length,
            vector[1] * MITRE_LIMIT / length,
        ]
    } else {
        vector
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Arc-length filters
// ─────────────────────────────────────────────────────────────────────────────

/// Largest value within `window` of arc length either side of each vertex.
fn running_max(values: &[f64], arc: &[f64], total: f64, window: f64) -> Vec<f64> {
    window_filter(
        values,
        arc,
        total,
        window,
        |acc, v| acc.max(v),
        f64::NEG_INFINITY,
    )
}

/// Mean of the values within `window` arc length, but never above the original.
///
/// Averaging alone could raise the shrink at a thin spot back toward its
/// thicker neighbours — re-eroding the very feature the medial limit protected.
/// Clamping the result back down to the input makes smoothing strictly
/// one-sided: it can soften a step, never deepen a cut.
fn smooth_downward(values: &[f64], arc: &[f64], total: f64, window: f64) -> Vec<f64> {
    let mut sums = window_filter(values, arc, total, window, |acc, v| acc + v, 0.0);
    let counts = window_filter(
        &vec![1.0; values.len()],
        arc,
        total,
        window,
        |acc, v| acc + v,
        0.0,
    );
    for (i, sum) in sums.iter_mut().enumerate() {
        let mean = if counts[i] > 0.0 {
            *sum / counts[i]
        } else {
            values[i]
        };
        *sum = mean.min(values[i]);
    }
    sums
}

/// Fold `values` over a circular arc-length window around every vertex.
fn window_filter(
    values: &[f64],
    arc: &[f64],
    total: f64,
    window: f64,
    fold: impl Fn(f64, f64) -> f64,
    identity: f64,
) -> Vec<f64> {
    let count = values.len();
    let mut out = Vec::with_capacity(count);
    if count == 0 {
        return out;
    }
    if window <= 0.0 || total <= 0.0 {
        return values.to_vec();
    }

    for i in 0..count {
        let mut acc = fold(identity, values[i]);
        for direction in [1isize, -1isize] {
            let mut j = i;
            loop {
                j = ((j as isize + direction).rem_euclid(count as isize)) as usize;
                if j == i {
                    break;
                }
                // Circular arc distance between vertex `i` and vertex `j`.
                let raw = (arc[j] - arc[i]).abs();
                let separation = raw.min(total - raw);
                if separation > window {
                    break;
                }
                acc = fold(acc, values[j]);
            }
        }
        out.push(acc);
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Spatial index
// ─────────────────────────────────────────────────────────────────────────────

/// One resampled contour segment, with the inward normal of its own surface.
///
/// That normal is what lets [`SegmentGrid::tangent_radii`] tell an *opposite
/// wall* from the *adjoining edge of a corner*: a wall across a thin feature
/// faces you, a corner's other edge runs away at right angles.
#[derive(Clone, Copy)]
struct Seg {
    a: [f64; 2],
    b: [f64; 2],
    /// Unit normal of this segment, pointing into the material.
    normal: [f64; 2],
}

/// Uniform grid over contour segments, for the bounded proximity queries this
/// module needs.
///
/// Both queries have a hard radius — the medial limit stops mattering past
/// `radius_cap`, the cliff guard past `cliff_limit_mm` — so a flat grid sized to
/// that radius answers each in constant expected time regardless of model size.
struct SegmentGrid {
    origin: [f64; 2],
    cell: f64,
    nx: usize,
    ny: usize,
    buckets: Vec<Vec<u32>>,
    segments: Vec<Seg>,
}

/// How squarely a surface must face a point to count as its *opposite wall*.
///
/// Normals more than 120° apart. A thin feature's two sides face each other
/// head-on (−1); a corner's adjoining edge sits at right angles (0) and is
/// rightly ignored, because the material does not stop there — it turns.
const FACING_COS: f64 = -0.5;

/// Ceiling on grid buckets, so a large plate cannot allocate an enormous index.
const MAX_BUCKETS: usize = 1 << 22;

impl SegmentGrid {
    fn build(contours: &[Contour], query_radius: f64) -> Self {
        let segments: Vec<Seg> = contours.iter().flat_map(Contour::segments).collect();

        let mut min = [f64::INFINITY, f64::INFINITY];
        let mut max = [f64::NEG_INFINITY, f64::NEG_INFINITY];
        for segment in &segments {
            for point in [segment.a, segment.b] {
                min[0] = min[0].min(point[0]);
                min[1] = min[1].min(point[1]);
                max[0] = max[0].max(point[0]);
                max[1] = max[1].max(point[1]);
            }
        }
        if segments.is_empty() {
            min = [0.0, 0.0];
            max = [0.0, 0.0];
        }

        let mut cell = query_radius.max(0.25);
        let (mut nx, mut ny) = (0, 0);
        for _ in 0..24 {
            nx = (((max[0] - min[0]) / cell).ceil() as usize + 1).max(1);
            ny = (((max[1] - min[1]) / cell).ceil() as usize + 1).max(1);
            if nx.saturating_mul(ny) <= MAX_BUCKETS {
                break;
            }
            cell *= 2.0;
        }

        let mut grid = Self {
            origin: min,
            cell,
            nx,
            ny,
            buckets: vec![Vec::new(); nx * ny],
            segments,
        };
        for index in 0..grid.segments.len() {
            let segment = grid.segments[index];
            let (x0, y0) = grid.cell_of(segment.a);
            let (x1, y1) = grid.cell_of(segment.b);
            for y in y0.min(y1)..=y0.max(y1) {
                for x in x0.min(x1)..=x0.max(x1) {
                    grid.buckets[y * grid.nx + x].push(index as u32);
                }
            }
        }
        grid
    }

    fn cell_of(&self, point: [f64; 2]) -> (usize, usize) {
        let x = ((point[0] - self.origin[0]) / self.cell).floor();
        let y = ((point[1] - self.origin[1]) / self.cell).floor();
        (
            (x.max(0.0) as usize).min(self.nx.saturating_sub(1)),
            (y.max(0.0) as usize).min(self.ny.saturating_sub(1)),
        )
    }

    /// Visit every segment whose cell lies within `radius` of `point`.
    ///
    /// A segment straddling a cell boundary is visited once per cell it touches.
    /// Both callers fold with `min`, so a repeat costs a little time and changes
    /// nothing — cheaper than the bookkeeping to suppress it.
    fn for_each_near(&self, point: [f64; 2], radius: f64, mut visit: impl FnMut(&Seg)) {
        if self.segments.is_empty() {
            return;
        }
        let span = (radius / self.cell).ceil() as isize + 1;
        let (cx, cy) = self.cell_of(point);
        for dy in -span..=span {
            let y = cy as isize + dy;
            if y < 0 || y >= self.ny as isize {
                continue;
            }
            for dx in -span..=span {
                let x = cx as isize + dx;
                if x < 0 || x >= self.nx as isize {
                    continue;
                }
                for &index in &self.buckets[y as usize * self.nx + x as usize] {
                    visit(&self.segments[index as usize]);
                }
            }
        }
    }

    /// Two readings of the local **half-width** at `point`, where the material
    /// lies along `normal`: `(near, opposed)`.
    ///
    /// Both are the radius of the largest circle that fits inside the material
    /// while touching `point` — half the thickness of a rib, the fillet radius
    /// inside a concave corner, the distance to the far side of a wide body
    /// (clamped to `cap`). A circle of radius `r` centred at `point + r · normal`
    /// stays clear of a boundary point `q` exactly while
    /// `r ≤ |q − point|² / (2 (q − point) · normal)`, so the tightest such bound
    /// over the neighbourhood is the answer — and since that quotient is never
    /// below `|q − point| / 2`, no `q` further than `2 · cap` away can tighten it
    /// below `cap`. That is what bounds the search.
    ///
    /// The bound is minimised over each segment **continuously**, interior
    /// included (see [`segment_tangent_radius`]), not just at its endpoints.
    /// Sampling endpoints alone minimises over a strict subset and can only
    /// *overstate* the safe radius: between two parallel walls `w` apart, a
    /// vertex phase error `s` reports `w/2 + s²/(2w)`, which at the 0.5 mm
    /// resample ceiling exceeds 0.05 mm — enough to break the minimum-width
    /// guarantee this whole module exists to provide.
    ///
    /// The two readings differ in which surfaces may tighten the bound, and
    /// **neither is usable alone**:
    ///
    /// - `near` accepts every surface, so it collapses toward zero all along a
    ///   convex corner — true (no circle fits through a corner) but useless as a
    ///   limit, since it would leave every corner of every model uncompensated.
    /// - `opposed` accepts only surfaces that **face** `point` (see
    ///   [`FACING_COS`]), which is what an *opposite wall* does and what a
    ///   corner's adjoining edge does not. It reads a thin feature's true
    ///   thickness anywhere along it, tip included, and ignores corners
    ///   entirely — so on its own it would also ignore a genuinely sharp spike.
    ///
    /// Their failure modes are disjoint, so the caller takes the smaller of
    /// `opposed` and a short running maximum of `near` (see [`running_max`]).
    /// That combination is what stops a thick body's radius leaking down an
    /// attached rib and pinching it off at the root.
    fn tangent_radii(&self, point: [f64; 2], normal: [f64; 2], cap: f64) -> (f64, f64) {
        let (mut near, mut opposed) = (cap, cap);
        self.for_each_near(point, cap * 2.0, |segment| {
            let radius = segment_tangent_radius(point, normal, segment.a, segment.b, cap);
            near = near.min(radius);
            if segment.normal[0] * normal[0] + segment.normal[1] * normal[1] <= FACING_COS {
                opposed = opposed.min(radius);
            }
        });
        (near, opposed)
    }

    /// Distance from `point` to the first boundary crossing along `direction`,
    /// saturating at `cap`.
    ///
    /// Used to measure how far the next layer's material actually reaches
    /// *outward* from a point, which the nearest-boundary distance does not
    /// answer: a ledge can overhang deeply while some unrelated edge — a
    /// tangential rim, the near side of a hole — sits closer and hides it.
    ///
    /// A crossing at the origin counts, and must: on a plain vertical wall the
    /// layer above traces this very outline, so the point starts *on* the
    /// boundary and the honest answer is zero flare.
    fn ray_exit_distance(&self, point: [f64; 2], direction: [f64; 2], cap: f64) -> f64 {
        let mut best = cap;
        // Any crossing within `cap` of `point` belongs to a segment that also
        // passes within `cap` of it, so a disc query of that radius is complete.
        self.for_each_near(point, cap, |segment| {
            let edge = [segment.b[0] - segment.a[0], segment.b[1] - segment.a[1]];
            let denominator = direction[0] * edge[1] - direction[1] * edge[0];
            if denominator.abs() <= EPS_MM {
                // Parallel: a grazing edge never marks the exit.
                return;
            }
            let offset = [point[0] - segment.a[0], point[1] - segment.a[1]];
            // Position along the edge, then along the ray. Both come from
            // Cramer's rule on `s·D − t·E = A − P`; note `t` carries the
            // opposite sign to `s`, because it is `−(F × D) / (D × E)`.
            let t = (offset[1] * direction[0] - offset[0] * direction[1]) / denominator;
            if !(0.0..=1.0).contains(&t) {
                return;
            }
            let s = (edge[0] * offset[1] - edge[1] * offset[0]) / denominator;
            if s >= 0.0 {
                best = best.min(s);
            }
        });
        best
    }
}

/// The tightest tangent-circle bound any point of segment `a`–`b` imposes.
///
/// Minimises `r(t) = |d(t)|² / (2 d(t) · n)` — with `d(t) = a + t(b − a) − p` —
/// over `t ∈ [0, 1]` where the denominator is positive. Differentiating gives a
/// quadratic in `t`, so the minimum is exact: it sits either at a stationary
/// point inside the interval or at one of its ends. Where the denominator
/// vanishes `r → +∞`, so that end of the valid range never binds and can be
/// ignored rather than clipped.
fn segment_tangent_radius(
    point: [f64; 2],
    normal: [f64; 2],
    a: [f64; 2],
    b: [f64; 2],
    cap: f64,
) -> f64 {
    let u = [a[0] - point[0], a[1] - point[1]];
    let v = [b[0] - a[0], b[1] - a[1]];

    let uu = u[0] * u[0] + u[1] * u[1];
    let uv = u[0] * v[0] + u[1] * v[1];
    let vv = v[0] * v[0] + v[1] * v[1];
    let un = u[0] * normal[0] + u[1] * normal[1];
    let vn = v[0] * normal[0] + v[1] * normal[1];

    // `r` at parameter `t`, or `None` where the point is not on the material
    // side (or is `point` itself).
    let radius_at = |t: f64| -> Option<f64> {
        let along = un + t * vn;
        if along <= EPS_MM {
            return None;
        }
        let squared = uu + 2.0 * t * uv + t * t * vv;
        if squared <= EPS_MM * EPS_MM {
            return None;
        }
        Some(squared / (2.0 * along))
    };

    let mut best = cap;
    for t in [0.0, 1.0] {
        if let Some(r) = radius_at(t) {
            best = best.min(r);
        }
    }

    // d/dt of the quotient, cleared of its positive denominator:
    //   |v|²(v·n) t² + 2|v|²(u·n) t + 2(u·v)(u·n) − (v·n)|u|² = 0
    let qa = vv * vn;
    let qb = 2.0 * vv * un;
    let qc = 2.0 * uv * un - vn * uu;

    let mut consider = |t: f64| {
        if (0.0..=1.0).contains(&t) {
            if let Some(r) = radius_at(t) {
                best = best.min(r);
            }
        }
    };

    if qa.abs() <= EPS_MM * EPS_MM {
        // Segment parallel to the surface at `point`: the quadratic degenerates
        // to a line, whose single root is the closest approach.
        if qb.abs() > EPS_MM * EPS_MM {
            consider(-qc / qb);
        }
    } else {
        let discriminant = qb * qb - 4.0 * qa * qc;
        if discriminant >= 0.0 {
            let root = discriminant.sqrt();
            consider((-qb + root) / (2.0 * qa));
            consider((-qb - root) / (2.0 * qa));
        }
    }

    best
}

/// Point-in-polygon test over a whole layer's contours.
///
/// Even-odd crossing over every sub-path, which is what makes it winding-safe:
/// a point inside a hole crosses the island once and the hole once, and reads
/// as outside without the caller having to classify contours first.
///
/// Edges are bucketed by the horizontal bands they span, so a query only tests
/// the edges its own scanline can actually cross. Testing every edge instead is
/// quadratic in the layer's complexity — fine for a cube, not for a full plate
/// of detailed parts, and this runs once per resampled vertex.
struct PolygonSet {
    /// Y of the first band's lower edge.
    origin: f64,
    /// Height of one band.
    band: f64,
    /// Edges (as `[x0, y0, x1, y1]`) overlapping each band.
    bands: Vec<Vec<[f64; 4]>>,
}

/// Target average edges per band; the band height is derived from it.
const POLYGON_BAND_TARGET: usize = 8;
/// Ceiling on band count, so a pathological layer cannot allocate unboundedly.
const POLYGON_MAX_BANDS: usize = 1 << 16;

impl PolygonSet {
    fn from_paths(paths: &Paths) -> Self {
        let mut edges: Vec<[f64; 4]> = Vec::new();
        let (mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY);
        for path in paths.iter() {
            let points: Vec<[f64; 2]> = path.iter().map(|p| [p.x(), p.y()]).collect();
            if points.len() < 3 {
                continue;
            }
            for i in 0..points.len() {
                let a = points[i];
                let b = points[(i + 1) % points.len()];
                edges.push([a[0], a[1], b[0], b[1]]);
                min_y = min_y.min(a[1]);
                max_y = max_y.max(a[1]);
            }
        }
        if edges.is_empty() {
            return Self {
                origin: 0.0,
                band: 1.0,
                bands: vec![Vec::new()],
            };
        }

        let height = (max_y - min_y).max(EPS_MM);
        let count = (edges.len() / POLYGON_BAND_TARGET).clamp(1, POLYGON_MAX_BANDS);
        let band = height / count as f64;

        let mut bands = vec![Vec::new(); count];
        for edge in edges {
            let lo = edge[1].min(edge[3]);
            let hi = edge[1].max(edge[3]);
            let first = Self::band_of(lo, min_y, band, count);
            let last = Self::band_of(hi, min_y, band, count);
            for slot in bands.iter_mut().take(last + 1).skip(first) {
                slot.push(edge);
            }
        }

        Self {
            origin: min_y,
            band,
            bands,
        }
    }

    fn band_of(y: f64, origin: f64, band: f64, count: usize) -> usize {
        let index = ((y - origin) / band).floor();
        (index.max(0.0) as usize).min(count - 1)
    }

    fn contains(&self, point: [f64; 2]) -> bool {
        let index = Self::band_of(point[1], self.origin, self.band, self.bands.len());
        let mut inside = false;
        for edge in &self.bands[index] {
            let (ax, ay, bx, by) = (edge[0], edge[1], edge[2], edge[3]);
            if (ay > point[1]) != (by > point[1]) {
                let t = (point[1] - ay) / (by - ay);
                if point[0] < ax + t * (bx - ax) {
                    inside = !inside;
                }
            }
        }
        inside
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Layer plumbing & small vector helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Whether `layer` is untouched [`slice_mesh`](crate::core::slice_mesh) output:
/// closed outer contours and nothing else.
///
/// The compensation pass rewrites a layer's whole path list, which is only
/// sound while no per-path metadata exists to keep in step with it. Asserting
/// the precondition is cheaper — and clearer — than carrying six parallel
/// arrays through an offset that will never see them.
fn is_raw_slice_output(layer: &SliceLayer) -> bool {
    layer.path_widths.is_empty()
        && layer.path_vertex_widths.is_empty()
        && layer.path_is_open.is_empty()
        && layer.path_overhang.is_empty()
        && layer.path_heights.is_empty()
        && layer.path_objects.is_empty()
        && layer
            .path_roles
            .iter()
            .all(|role| *role == ExtrusionRole::OuterWall)
}

/// Replace the layer's contours, keeping `path_roles` the same length.
fn set_contours(layer: &mut SliceLayer, contours: Paths) {
    layer.path_roles = vec![ExtrusionRole::OuterWall; contours.len()];
    layer.paths = contours;
}

/// Normalise winding and self-overlap, as both wall generators do.
///
/// Returns `None` when Clipper cannot produce a normalised result: every later
/// step reads winding to tell material from void, so continuing with raw paths
/// would risk offsetting a hole the wrong way. Skipping the layer leaves it
/// uncorrected, which is always the safer failure.
fn union_even_odd(paths: Paths) -> Option<Paths> {
    if paths.is_empty() {
        return None;
    }
    clipper2::union(paths, Paths::new(vec![]), FillRule::EvenOdd)
        .ok()
        .filter(|result| !result.is_empty())
}

fn left_normal(dx: f64, dy: f64) -> [f64; 2] {
    normalise([-dy, dx])
}

fn normalise(v: [f64; 2]) -> [f64; 2] {
    let length = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if length <= EPS_MM {
        [0.0, 0.0]
    } else {
        [v[0] / length, v[1] / length]
    }
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Default nozzle is 0.4 mm, so the automatic minimum contour width is
    /// 1.5 × 0.4 = 0.6 mm throughout these tests.
    const AUTO_MIN_WIDTH: f64 = 0.6;

    fn params() -> SlicingParams {
        SlicingParams {
            layer_height: 0.2,
            nozzle_diameter_mm: 0.4,
            ..SlicingParams::default()
        }
    }

    /// A closed rectangle, counter-clockwise (a solid island).
    fn rect(x: f64, y: f64, w: f64, h: f64) -> Path {
        vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)].into()
    }

    /// A closed rectangle, clockwise (a hole).
    fn hole(x: f64, y: f64, w: f64, h: f64) -> Path {
        vec![(x, y), (x, y + h), (x + w, y + h), (x + w, y)].into()
    }

    /// Stack `contours` into layers at the Z heights `slice_mesh` would use.
    fn layers_of(contours: Vec<Vec<Path>>, layer_height: f64) -> Vec<SliceLayer> {
        contours
            .into_iter()
            .enumerate()
            .map(|(i, paths)| {
                let mut layer = SliceLayer::new((i as f64 + 0.5) * layer_height);
                layer.path_roles = vec![ExtrusionRole::OuterWall; paths.len()];
                layer.paths = Paths::new(paths);
                layer
            })
            .collect()
    }

    fn bounds_of(layer: &SliceLayer) -> (f64, f64, f64, f64) {
        let bounds = layer.paths.bounds();
        (
            bounds.min.x(),
            bounds.min.y(),
            bounds.max.x(),
            bounds.max.y(),
        )
    }

    fn area_of(layer: &SliceLayer) -> f64 {
        layer.paths.iter().map(|p| p.signed_area()).sum::<f64>()
    }

    /// Width of the layer's material along the horizontal line `y`, summed over
    /// every span — the measurement the medial limit is defined in terms of.
    fn width_at(layer: &SliceLayer, y: f64) -> f64 {
        crossings_at(layer, |a, b| (a[1], b[1]), |a, b| (a[0], b[0]), y)
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| c[1] - c[0])
            .sum()
    }

    /// The material intervals the vertical line `x` passes through, bottom-up.
    ///
    /// Ribs in these tests run horizontally, so their *thickness* is a vertical
    /// measurement; returning the intervals lets a test name the one it means
    /// rather than summing a body and a stroke together.
    fn vertical_spans(layer: &SliceLayer, x: f64) -> Vec<(f64, f64)> {
        crossings_at(layer, |a, b| (a[0], b[0]), |a, b| (a[1], b[1]), x)
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| (c[0], c[1]))
            .collect()
    }

    /// Sorted crossings of a scan line, generic over which axis is scanned.
    ///
    /// `axis` picks the coordinate compared against `at`; `other` picks the one
    /// interpolated and returned.
    fn crossings_at(
        layer: &SliceLayer,
        axis: fn([f64; 2], [f64; 2]) -> (f64, f64),
        other: fn([f64; 2], [f64; 2]) -> (f64, f64),
        at: f64,
    ) -> Vec<f64> {
        let mut crossings: Vec<f64> = Vec::new();
        for path in layer.paths.iter() {
            let points: Vec<[f64; 2]> = path.iter().map(|p| [p.x(), p.y()]).collect();
            for i in 0..points.len() {
                let a = points[i];
                let b = points[(i + 1) % points.len()];
                let (a_axis, b_axis) = axis(a, b);
                if (a_axis > at) != (b_axis > at) {
                    let t = (at - a_axis) / (b_axis - a_axis);
                    let (a_other, b_other) = other(a, b);
                    crossings.push(a_other + t * (b_other - a_other));
                }
            }
        }
        crossings.sort_by(|a, b| a.partial_cmp(b).unwrap());
        crossings
    }

    /// Thickness of the single material span crossing `x` inside `y_lo..y_hi`.
    fn rib_thickness(layer: &SliceLayer, x: f64, y_lo: f64, y_hi: f64) -> f64 {
        vertical_spans(layer, x)
            .into_iter()
            .filter(|(lo, hi)| *lo >= y_lo && *hi <= y_hi)
            .map(|(lo, hi)| hi - lo)
            .sum()
    }

    // ── Defaults & gates ────────────────────────────────────────────────────

    #[test]
    fn default_settings_resolve_to_no_compensation_at_all() {
        assert_eq!(CompensationConfig::resolve(&params()), None);
    }

    #[test]
    fn a_disabled_pass_leaves_every_contour_byte_identical() {
        let mut layers = layers_of(vec![vec![rect(0.0, 0.0, 20.0, 20.0)]; 3], 0.2);
        let before = layers[0].paths.clone();
        apply_dimensional_compensation(&mut layers, &params());
        assert_eq!(layers[0].paths, before);
    }

    #[test]
    fn a_raft_disables_elephant_foot_because_the_object_never_touches_the_bed() {
        let mut p = params();
        p.elephant_foot_compensation_mm = 0.2;
        p.adhesion_type = AdhesionType::Raft;
        p.raft_layers = 3;

        assert_eq!(
            CompensationConfig::resolve(&p),
            None,
            "a raft leaves nothing for this pass to do"
        );

        // …but XY size compensation is a machine correction and still applies.
        p.xy_size_compensation_mm = -0.05;
        let config = CompensationConfig::resolve(&p).expect("xy compensation still resolves");
        assert_eq!(config.elephant_foot_mm, 0.0);
        assert_eq!(config.xy_size_mm, -0.05);
    }

    #[test]
    fn a_brim_or_skirt_does_not_disable_elephant_foot() {
        for adhesion in [AdhesionType::None, AdhesionType::Skirt, AdhesionType::Brim] {
            let mut p = params();
            p.elephant_foot_compensation_mm = 0.2;
            p.adhesion_type = adhesion;
            let config = CompensationConfig::resolve(&p).expect("resolves");
            assert_eq!(
                config.elephant_foot_mm, 0.2,
                "{adhesion:?} still presses the first layer into the bed"
            );
        }
    }

    #[test]
    fn the_taper_ramps_linearly_and_stops() {
        let mut p = params();
        p.elephant_foot_compensation_mm = 0.3;
        p.elephant_foot_layers = 3;
        let config = CompensationConfig::resolve(&p).expect("resolves");

        assert!((config.shrink_for_layer(0) - 0.3).abs() < 1e-9);
        assert!((config.shrink_for_layer(1) - 0.2).abs() < 1e-9);
        assert!((config.shrink_for_layer(2) - 0.1).abs() < 1e-9);
        assert_eq!(config.shrink_for_layer(3), 0.0);
    }

    #[test]
    fn the_automatic_minimum_width_tracks_the_outer_wall_width() {
        let mut p = params();
        p.elephant_foot_compensation_mm = 0.2;
        assert!(
            (CompensationConfig::resolve(&p)
                .unwrap()
                .min_contour_width_mm
                - 0.6)
                .abs()
                < 1e-9
        );

        p.outer_wall_line_width = 0.6;
        assert!(
            (CompensationConfig::resolve(&p)
                .unwrap()
                .min_contour_width_mm
                - 0.9)
                .abs()
                < 1e-9
        );

        p.elephant_foot_min_contour_width_mm = 1.2;
        assert!(
            (CompensationConfig::resolve(&p)
                .unwrap()
                .min_contour_width_mm
                - 1.2)
                .abs()
                < 1e-9
        );
    }

    // ── The offset itself ───────────────────────────────────────────────────

    #[test]
    fn the_first_layer_shrinks_by_the_full_amount_and_the_rest_do_not() {
        let mut p = params();
        p.elephant_foot_compensation_mm = 0.2;

        let mut layers = layers_of(vec![vec![rect(0.0, 0.0, 20.0, 20.0)]; 3], 0.2);
        apply_dimensional_compensation(&mut layers, &p);

        let (min_x, min_y, max_x, max_y) = bounds_of(&layers[0]);
        for (label, value, expected) in [
            ("min x", min_x, 0.2),
            ("min y", min_y, 0.2),
            ("max x", max_x, 19.8),
            ("max y", max_y, 19.8),
        ] {
            assert!(
                (value - expected).abs() < 0.02,
                "{label}: expected {expected}, got {value}"
            );
        }

        assert_eq!(
            bounds_of(&layers[1]),
            (0.0, 0.0, 20.0, 20.0),
            "only the layers at the bed are corrected"
        );
    }

    #[test]
    fn a_hole_grows_by_the_same_amount_the_outside_shrinks() {
        let mut p = params();
        p.elephant_foot_compensation_mm = 0.2;

        let contours = vec![rect(0.0, 0.0, 20.0, 20.0), hole(8.0, 8.0, 4.0, 4.0)];
        let mut layers = layers_of(vec![contours; 3], 0.2);
        apply_dimensional_compensation(&mut layers, &p);

        // Squish narrows a hole just as it widens the outside, so the
        // correction has to open it back up.
        let solid = width_at(&layers[0], 10.0);
        assert!(
            (solid - (19.6 - 4.4)).abs() < 0.1,
            "expected a 19.6 mm island minus a 4.4 mm hole, measured {solid}"
        );
    }

    #[test]
    fn xy_size_compensation_applies_to_every_layer() {
        let mut p = params();
        p.xy_size_compensation_mm = -0.1;

        let mut layers = layers_of(vec![vec![rect(0.0, 0.0, 20.0, 20.0)]; 3], 0.2);
        apply_dimensional_compensation(&mut layers, &p);

        for (i, layer) in layers.iter().enumerate() {
            let (min_x, _, max_x, _) = bounds_of(layer);
            assert!(
                (min_x - 0.1).abs() < 1e-6 && (max_x - 19.9).abs() < 1e-6,
                "layer {i} should be uniformly shrunk, got {min_x}..{max_x}"
            );
        }
    }

    #[test]
    fn xy_and_elephant_foot_compose_on_the_first_layer() {
        let mut p = params();
        p.xy_size_compensation_mm = -0.1;
        p.elephant_foot_compensation_mm = 0.2;

        let mut layers = layers_of(vec![vec![rect(0.0, 0.0, 20.0, 20.0)]; 3], 0.2);
        apply_dimensional_compensation(&mut layers, &p);

        let (min_x, _, max_x, _) = bounds_of(&layers[0]);
        assert!(
            (min_x - 0.3).abs() < 0.02 && (max_x - 19.7).abs() < 0.02,
            "first layer takes both corrections, got {min_x}..{max_x}"
        );
        let (next_min, _, next_max, _) = bounds_of(&layers[1]);
        assert!((next_min - 0.1).abs() < 1e-6 && (next_max - 19.9).abs() < 1e-6);
    }

    // ── Medial limit — the reason this module exists ────────────────────────

    #[test]
    fn a_thin_first_layer_glyph_survives_a_shrink_that_would_erase_it() {
        let mut p = params();
        // 0.3 mm per side takes 0.6 mm off a 0.5 mm stroke: a uniform inward
        // offset deletes it outright, which is what this pass exists to avoid.
        p.elephant_foot_compensation_mm = 0.3;

        // A body on the plate with a separate embossed stroke beside it.
        let plate = vec![rect(0.0, 0.0, 20.0, 3.0), rect(4.0, 6.0, 12.0, 0.5)];
        let mut layers = layers_of(vec![plate.clone(), plate], 0.2);
        apply_dimensional_compensation(&mut layers, &p);

        let stroke = rib_thickness(&layers[0], 10.0, 5.0, 8.0);
        assert!(
            stroke > 0.0,
            "the glyph stroke must not be erased, measured {stroke} mm"
        );
        assert!(
            (stroke - 0.5).abs() < 1e-6,
            "a stroke already thinner than the {AUTO_MIN_WIDTH} mm floor must be left \
             entirely alone, measured {stroke} mm"
        );

        // The body it sits beside is 3 mm thick and takes the full correction.
        let body = rib_thickness(&layers[0], 10.0, -1.0, 4.0);
        assert!(
            (body - 2.4).abs() < 0.05,
            "the body takes the full correction, measured {body} mm"
        );
    }

    #[test]
    fn a_feature_is_never_shrunk_below_the_minimum_width() {
        let mut p = params();
        p.elephant_foot_compensation_mm = 0.3;

        // 0.9 mm thick: thick enough to shrink, but not by the full 0.6 mm the
        // uncapped correction would take off.
        let bar = vec![rect(0.0, 0.0, 12.0, 0.9)];
        let mut layers = layers_of(vec![bar.clone(), bar], 0.2);
        apply_dimensional_compensation(&mut layers, &p);

        let thickness = rib_thickness(&layers[0], 6.0, -1.0, 2.0);
        assert!(
            (thickness - AUTO_MIN_WIDTH).abs() < 0.06,
            "a 0.9 mm rib should stop at the {AUTO_MIN_WIDTH} mm floor, measured {thickness} mm"
        );
    }

    #[test]
    fn a_wide_body_still_gets_the_full_correction_beside_a_protected_rib() {
        let mut p = params();
        p.elephant_foot_compensation_mm = 0.2;

        // A 20 mm block and a 0.5 mm rib on the same layer: the limit is local,
        // so protecting the rib must not spare the block.
        let plate = vec![rect(0.0, 0.0, 20.0, 20.0), rect(0.0, 25.0, 12.0, 0.5)];
        let mut layers = layers_of(vec![plate.clone(), plate], 0.2);
        apply_dimensional_compensation(&mut layers, &p);

        let block = width_at(&layers[0], 10.0);
        assert!(
            (block - 19.6).abs() < 0.05,
            "the block takes the full correction, measured {block} mm"
        );
        let rib = rib_thickness(&layers[0], 6.0, 24.0, 26.0);
        assert!(
            (rib - 0.5).abs() < 1e-6,
            "the rib beside it is untouched, measured {rib} mm"
        );
    }

    // ── Cliff guard ─────────────────────────────────────────────────────────

    #[test]
    fn an_ordinary_chamfered_base_still_gets_the_full_correction() {
        let mut p = params();
        p.elephant_foot_compensation_mm = 0.2;

        // Most printable models flare a little on their second layer — a
        // chamfer or fillet as a lead-in. The guard has to stay out of the way
        // there, or a user who asks for 0.2 mm silently gets less.
        let mut layers = layers_of(
            vec![
                vec![rect(0.2, 0.2, 19.6, 19.6)],
                vec![rect(0.0, 0.0, 20.0, 20.0)],
                vec![rect(0.0, 0.0, 20.0, 20.0)],
            ],
            0.2,
        );
        apply_dimensional_compensation(&mut layers, &p);

        let width = width_at(&layers[0], 10.0);
        assert!(
            (width - (19.6 - 0.4)).abs() < 0.05,
            "a 0.2 mm chamfer is not a cliff; expected 19.2 mm, measured {width} mm"
        );
    }

    #[test]
    fn a_thin_rib_attached_to_a_thick_body_does_not_pinch_off_at_its_root() {
        let mut p = params();
        p.elephant_foot_compensation_mm = 0.3;

        // The dangerous shape: a 0.5 mm fin joined to a 20 mm block, as a
        // card-slot fin or an embossed glyph stroke touching its border. The
        // corner-restoring running maximum can see the block's radius from a
        // point on the fin, so the fin's own opposite side has to override it.
        let joined = Paths::new(vec![
            rect(0.0, 0.0, 20.0, 20.0),
            rect(20.0, 9.75, 10.0, 0.5),
        ]);
        let merged: Vec<Path> =
            clipper2::union(joined, Paths::new(vec![]), clipper2::FillRule::NonZero)
                .expect("union")
                .iter()
                .cloned()
                .collect();

        let mut layers = layers_of(vec![merged.clone(), merged], 0.2);
        apply_dimensional_compensation(&mut layers, &p);

        // Measure the fin clear of the block it grows out of.
        let fin = rib_thickness(&layers[0], 26.0, 8.0, 12.0);
        assert!(
            fin > 0.0,
            "the fin must survive at its root, measured {fin} mm"
        );
        assert!(
            (fin - 0.5).abs() < 1e-6,
            "a 0.5 mm fin is already under the {AUTO_MIN_WIDTH} mm floor and must be \
             untouched even where it joins the block, measured {fin} mm"
        );

        // The block it hangs off still takes the full correction.
        let block = width_at(&layers[0], 3.0);
        assert!(
            (block - 19.4).abs() < 0.05,
            "the block takes the full correction, measured {block} mm"
        );
    }

    #[test]
    fn a_tapering_wedge_keeps_its_width_along_its_thin_stretch() {
        let mut p = params();
        p.elephant_foot_compensation_mm = 0.3;

        // A long wedge narrowing to a point: everything past the halfway mark
        // is under the floor and must not be cut into.
        let wedge: Path = vec![(0.0, 0.0), (20.0, 0.9), (20.0, -0.9)].into();
        let mut layers = layers_of(vec![vec![wedge.clone()], vec![wedge]], 0.2);
        apply_dimensional_compensation(&mut layers, &p);

        for x in [4.0, 8.0, 12.0] {
            let here = rib_thickness(&layers[0], x, -2.0, 2.0);
            let nominal = 2.0 * 0.9 * x / 20.0;
            assert!(
                here >= nominal.min(AUTO_MIN_WIDTH) - 0.06,
                "at x={x} the wedge is {nominal:.2} mm wide and must not be cut \
                 below the {AUTO_MIN_WIDTH} mm floor, measured {here:.3} mm"
            );
        }
    }

    #[test]
    fn compensation_is_withheld_under_a_body_that_flares_out_above_it() {
        let mut p = params();
        p.elephant_foot_compensation_mm = 0.2;

        // A 5 mm pedestal carrying a 20 mm body: shrinking the pedestal would
        // deepen an overhang the model already has, and cost bed grip where the
        // print can least afford it.
        let mut layers = layers_of(
            vec![
                vec![rect(7.5, 7.5, 5.0, 5.0)],
                vec![rect(0.0, 0.0, 20.0, 20.0)],
                vec![rect(0.0, 0.0, 20.0, 20.0)],
            ],
            0.2,
        );
        let before = area_of(&layers[0]);
        apply_dimensional_compensation(&mut layers, &p);
        let after = area_of(&layers[0]);

        assert!(
            (after - before).abs() < 0.05,
            "the pedestal must be left alone: {before} mm² → {after} mm²"
        );
    }

    #[test]
    fn a_vertical_wall_is_fully_compensated_despite_the_cliff_guard() {
        let mut p = params();
        p.elephant_foot_compensation_mm = 0.2;

        // The guard reads the layer above; on a plain vertical wall that layer
        // sits exactly over this one and must not hold the correction back.
        let mut layers = layers_of(vec![vec![rect(0.0, 0.0, 20.0, 20.0)]; 3], 0.2);
        apply_dimensional_compensation(&mut layers, &p);

        let width = width_at(&layers[0], 10.0);
        assert!(
            (width - 19.6).abs() < 0.05,
            "a vertical wall takes the full correction, measured {width} mm"
        );
    }

    #[test]
    fn an_inward_taper_is_fully_compensated() {
        let mut p = params();
        p.elephant_foot_compensation_mm = 0.2;

        // The model narrows going up, so the correction removes material that
        // overhangs nothing at all.
        let mut layers = layers_of(
            vec![
                vec![rect(0.0, 0.0, 20.0, 20.0)],
                vec![rect(1.0, 1.0, 18.0, 18.0)],
            ],
            0.2,
        );
        apply_dimensional_compensation(&mut layers, &p);

        let width = width_at(&layers[0], 10.0);
        assert!(
            (width - 19.6).abs() < 0.05,
            "an inward taper takes the full correction, measured {width} mm"
        );
    }

    // ── Bed contact ─────────────────────────────────────────────────────────

    #[test]
    fn an_object_lifted_off_the_plate_gets_no_elephant_foot() {
        let mut p = params();
        p.elephant_foot_compensation_mm = 0.2;

        let mut layers = layers_of(vec![vec![rect(0.0, 0.0, 20.0, 20.0)]; 3], 0.2);
        // Same model, started 5 mm in the air: nothing squashes it.
        for (i, layer) in layers.iter_mut().enumerate() {
            layer.z = 5.0 + (i as f64 + 0.5) * 0.2;
        }
        let before = layers[0].paths.clone();
        apply_dimensional_compensation(&mut layers, &p);

        assert_eq!(layers[0].paths, before);
    }

    #[test]
    fn sits_on_bed_accepts_the_slicers_own_layer_heights() {
        // `slice_mesh` samples layer `i` at `(i + 0.5) · h` for a bed-resting
        // model, and higher for anything lifted.
        assert!(sits_on_bed(0.1, 0, 0.2));
        assert!(sits_on_bed(0.3, 1, 0.2));
        assert!(!sits_on_bed(5.1, 0, 0.2));
    }

    // ── Geometry helpers ────────────────────────────────────────────────────

    #[test]
    fn the_mitre_vector_moves_both_edges_of_a_corner_in_by_the_same_amount() {
        // A right-angle corner: each edge must end up δ further in, which needs
        // √2 · δ of travel along the bisector.
        let mitre = mitre_vector([1.0, 0.0], [0.0, 1.0]);
        assert!((mitre[0] - 1.0).abs() < 1e-9 && (mitre[1] - 1.0).abs() < 1e-9);

        // A straight run collapses to the plain normal.
        let straight = mitre_vector([0.0, 1.0], [0.0, 1.0]);
        assert!((straight[0]).abs() < 1e-9 && (straight[1] - 1.0).abs() < 1e-9);

        // A needle-sharp corner is clamped rather than shot to infinity.
        let sharp = mitre_vector([1.0, 0.0], [-0.999, 0.0447]);
        let length = (sharp[0] * sharp[0] + sharp[1] * sharp[1]).sqrt();
        assert!(
            length <= MITRE_LIMIT + 1e-9,
            "mitre length {length} exceeds the limit"
        );
    }

    #[test]
    fn the_tangent_radius_reads_half_a_ribs_width() {
        // A 1 mm wide horizontal strip: the largest circle touching its lower
        // edge and staying inside has radius 0.5 mm. Both readings agree here —
        // the far side of the strip is a long walk around either end.
        let strip = Contour::resample(&rect(0.0, 0.0, 20.0, 1.0), 0.1).expect("resamples");
        let grid = SegmentGrid::build(std::slice::from_ref(&strip), 2.0);

        let (near, opposed) = grid.tangent_radii([10.0, 0.0], [0.0, 1.0], 2.0);
        for (label, radius) in [("near", near), ("opposed", opposed)] {
            assert!(
                (radius - 0.5).abs() < 0.02,
                "{label}: expected 0.5 mm, measured {radius} mm"
            );
        }
    }

    #[test]
    fn near_collapses_along_a_corner_while_opposed_ignores_it() {
        // The whole reason two readings exist. `near` collapses not just AT a
        // convex corner but all along its approach, because the adjoining edge
        // caps every circle nearby. `opposed` ignores that edge — it runs away
        // at right angles rather than facing the point — and reports the block.
        let block = Contour::resample(&rect(0.0, 0.0, 20.0, 20.0), 0.1).expect("resamples");
        let grid = SegmentGrid::build(std::slice::from_ref(&block), 0.6);

        // A point on the left edge, a third of a millimetre up from the corner.
        let probe = block
            .points
            .iter()
            .position(|p| p[0].abs() < 1e-9 && (p[1] - 0.3).abs() < 0.06)
            .expect("a sample near the corner");
        let (near, opposed) = grid.tangent_radii(block.points[probe], block.normals[probe], 0.6);

        assert!(
            near < 0.4,
            "near is capped by the adjoining bottom edge, got {near}"
        );
        assert!(
            (opposed - 0.6).abs() < 1e-9,
            "opposed sees only the far wall of a 20 mm block, got {opposed}"
        );
    }

    #[test]
    fn opposed_reads_a_thin_ribs_thickness_right_up_to_its_tip() {
        // The complement: on a thin rib the two sides DO face each other, so
        // `opposed` reports the real half-width — including near the tip, where
        // an arc-distance rule would have lost it.
        let rib = Contour::resample(&rect(0.0, 0.0, 10.0, 0.5), 0.05).expect("resamples");
        let grid = SegmentGrid::build(std::slice::from_ref(&rib), 0.6);

        for target_x in [5.0, 9.8] {
            let probe = rib
                .points
                .iter()
                .position(|p| p[1].abs() < 1e-9 && (p[0] - target_x).abs() < 0.03)
                .expect("a sample on the rib's lower edge");
            let (_, opposed) = grid.tangent_radii(rib.points[probe], rib.normals[probe], 0.6);
            assert!(
                (opposed - 0.25).abs() < 0.02,
                "at x={target_x} the rib is 0.5 mm thick, opposed read {opposed}"
            );
        }
    }

    #[test]
    fn the_tangent_radius_is_exact_between_the_vertices_not_just_at_them() {
        // Sampling endpoints alone minimises over a strict subset, so it can
        // only *overstate* the safe radius — and overstating it is what breaks
        // the minimum-width guarantee. Between parallel walls `w` apart a
        // vertex phase error `s` reports `w/2 + s²/(2w)`; at a coarse step that
        // is tens of microns of slack.
        //
        // A 1 mm strip sampled at the 0.5 mm resample ceiling, probed from
        // exactly between two opposite vertices, is the worst case.
        let strip = Contour::resample(&rect(0.0, 0.0, 20.0, 1.0), 0.5).expect("resamples");
        let grid = SegmentGrid::build(std::slice::from_ref(&strip), 2.0);

        let (near, opposed) = grid.tangent_radii([10.25, 0.0], [0.0, 1.0], 2.0);
        for (label, radius) in [("near", near), ("opposed", opposed)] {
            assert!(
                (radius - 0.5).abs() < 1e-9,
                "{label}: the strip is exactly 1 mm thick, measured {radius} mm"
            );
        }
    }

    #[test]
    fn segment_tangent_radius_matches_a_dense_numeric_sweep() {
        // Cross-check the closed-form minimum against brute force on a set of
        // awkward configurations: skewed, oblique, and facing away.
        let cases = [
            ([0.0, 0.0], [0.0, 1.0], [-1.0, 0.7], [1.3, 0.4]),
            ([0.0, 0.0], [0.0, 1.0], [0.4, 2.0], [2.5, 0.9]),
            ([0.0, 0.0], [0.6, 0.8], [-2.0, 1.5], [2.0, 1.1]),
            ([0.0, 0.0], [0.0, 1.0], [-1.0, -0.5], [1.0, -0.4]),
            ([0.0, 0.0], [0.0, 1.0], [0.2, 0.05], [0.9, 3.0]),
        ];

        for (point, normal, a, b) in cases {
            let cap = 5.0;
            let exact = segment_tangent_radius(point, normal, a, b, cap);

            let mut brute = cap;
            for k in 0..=200_000 {
                let t = k as f64 / 200_000.0;
                let d = [
                    a[0] + t * (b[0] - a[0]) - point[0],
                    a[1] + t * (b[1] - a[1]) - point[1],
                ];
                let along = d[0] * normal[0] + d[1] * normal[1];
                if along <= EPS_MM {
                    continue;
                }
                let squared = d[0] * d[0] + d[1] * d[1];
                if squared <= EPS_MM * EPS_MM {
                    continue;
                }
                brute = brute.min(squared / (2.0 * along));
            }

            assert!(
                (exact - brute).abs() < 1e-6,
                "closed form {exact} disagrees with the sweep {brute} for \
                 a={a:?} b={b:?} n={normal:?}"
            );
            assert!(
                exact <= brute + 1e-12,
                "the closed form must never overstate the safe radius"
            );
        }
    }

    #[test]
    fn the_ray_exit_distance_finds_the_far_side_it_is_aimed_at() {
        // Deliberately off-grid: a crossing that lands exactly on a resampled
        // vertex can be found even by a parameterisation with the wrong sign,
        // so the geometry here is chosen not to.
        let block = Contour::resample(&rect(0.0, 0.0, 7.31, 5.17), 0.05).expect("resamples");
        let grid = SegmentGrid::build(std::slice::from_ref(&block), 12.0);

        let probe = [1.234, 2.713];
        for (label, direction, expected) in [
            ("+X", [1.0, 0.0], 7.31 - 1.234),
            ("-X", [-1.0, 0.0], 1.234),
            ("+Y", [0.0, 1.0], 5.17 - 2.713),
            ("-Y", [0.0, -1.0], 2.713),
        ] {
            let measured = grid.ray_exit_distance(probe, direction, 12.0);
            assert!(
                (measured - expected).abs() < 1e-9,
                "{label}: expected {expected} mm to the far side, measured {measured} mm"
            );
        }

        // A diagonal, to pin the parameterisation off the axes too.
        let diagonal = [0.6, 0.8];
        let measured = grid.ray_exit_distance(probe, diagonal, 12.0);
        let to_right: f64 = (7.31 - 1.234) / 0.6;
        let to_top: f64 = (5.17 - 2.713) / 0.8;
        assert!(
            (measured - to_right.min(to_top)).abs() < 1e-9,
            "diagonal: expected {} mm, measured {measured} mm",
            to_right.min(to_top)
        );
    }

    #[test]
    fn the_ray_exit_distance_saturates_and_treats_a_boundary_start_as_zero_flare() {
        let block = Contour::resample(&rect(0.0, 0.0, 40.0, 40.0), 0.1).expect("resamples");
        let grid = SegmentGrid::build(std::slice::from_ref(&block), 1.2);

        // Far side well beyond the cap.
        assert!((grid.ray_exit_distance([20.0, 20.0], [1.0, 0.0], 1.2) - 1.2).abs() < 1e-9);

        // Starting exactly *on* the boundary reads zero, and must: that is the
        // plain vertical wall, where the layer above traces the same outline as
        // this one. Zero flare is inside the free zone, so the correction goes
        // ahead in full — which is the whole point, since a vertical wall is
        // where an elephant foot actually forms.
        for direction in [[-1.0, 0.0], [1.0, 0.0]] {
            let measured = grid.ray_exit_distance([0.0, 20.0], direction, 1.2);
            assert!(
                measured.abs() < 1e-9,
                "a boundary start is zero flare, measured {measured} mm going {direction:?}"
            );
        }

        // Material strictly behind the ray never counts.
        assert!((grid.ray_exit_distance([39.5, 20.0], [1.0, 0.0], 1.2) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn the_cliff_guard_measures_the_flare_outward_not_the_nearest_edge() {
        // A deep ledge overhanging the point, with an unrelated edge running
        // close by tangentially. The nearest boundary in *any* direction is
        // that tangential edge, which would wave the full shrink through; the
        // outward ray sees the ledge and holds it back. Off-grid coordinates,
        // so no crossing can land on a resampled vertex by luck.
        let ledge = Paths::new(vec![
            rect(-10.13, 0.0, 20.26, 5.0),
            // A neighbouring island 0.29 mm away tangentially.
            rect(0.29, 6.0, 4.0, 5.0),
        ]);
        let contours: Vec<Contour> = ledge
            .iter()
            .filter_map(|p| Contour::resample(p, 0.05))
            .collect();
        let grid = SegmentGrid::build(&contours, 1.2);
        let polygons = PolygonSet::from_paths(&ledge);

        // Sitting inside the wide ledge, which reaches 10.13 mm outward (−X):
        // far past the 1.2 mm limit, so the shrink is withheld entirely.
        let probe = [0.0, 2.713];
        let deep = cliff_factor(probe, [-1.0, 0.0], &grid, &polygons, 0.4, 1.2);
        assert!(
            deep <= 1e-9,
            "a 10 mm overhang must withhold the shrink entirely, got {deep}"
        );

        // A shallow lip: material ending 0.31 mm out is inside the free zone,
        // so the correction proceeds in full.
        let shelf = Paths::new(vec![rect(-10.13, 0.0, 10.44, 5.0)]);
        let shelf_contours: Vec<Contour> = shelf
            .iter()
            .filter_map(|p| Contour::resample(p, 0.05))
            .collect();
        let shelf_grid = SegmentGrid::build(&shelf_contours, 1.2);
        let shelf_polygons = PolygonSet::from_paths(&shelf);
        let shallow = cliff_factor(probe, [1.0, 0.0], &shelf_grid, &shelf_polygons, 0.4, 1.2);
        assert!(
            (shallow - 1.0).abs() < 1e-9,
            "a 0.31 mm lip is not a cliff, got {shallow}"
        );

        // And partway between the two the guard ramps rather than switching.
        let mid = cliff_factor(probe, [-1.0, 0.0], &shelf_grid, &shelf_polygons, 0.4, 20.0);
        assert!(
            mid > 0.0 && mid < 1.0,
            "a 10.13 mm flare against a 20 mm limit should ramp, got {mid}"
        );
    }

    #[test]
    fn the_running_max_lifts_a_corners_zero_reading_without_lifting_a_ribs() {
        let arc: Vec<f64> = (0..10).map(|i| i as f64 * 0.1).collect();
        // A lone dip, as a convex corner produces.
        let dip = vec![0.5, 0.5, 0.5, 0.5, 0.0, 0.5, 0.5, 0.5, 0.5, 0.5];
        let lifted = running_max(&dip, &arc, 1.0, 0.25);
        assert!((lifted[4] - 0.5).abs() < 1e-9, "the dip is filled in");

        // A sustained low run, as a thin rib produces, stays low.
        let rib = vec![0.3; 10];
        let kept = running_max(&rib, &arc, 1.0, 0.25);
        assert!(kept.iter().all(|v| (v - 0.3).abs() < 1e-9));
    }

    #[test]
    fn smoothing_can_only_ever_reduce_the_shrink() {
        let arc: Vec<f64> = (0..8).map(|i| i as f64 * 0.1).collect();
        let values = vec![0.2, 0.2, 0.2, 0.0, 0.0, 0.2, 0.2, 0.2];
        let smoothed = smooth_downward(&values, &arc, 0.8, 0.25);

        for (i, v) in smoothed.iter().enumerate() {
            assert!(
                *v <= values[i] + 1e-12,
                "index {i}: smoothing raised {} to {v}",
                values[i]
            );
        }
        assert!(smoothed[3] <= 1e-12, "a protected vertex stays protected");
    }

    #[test]
    fn the_polygon_set_reads_a_hole_as_outside() {
        let paths = Paths::new(vec![rect(0.0, 0.0, 20.0, 20.0), hole(8.0, 8.0, 4.0, 4.0)]);
        let polygons = PolygonSet::from_paths(&paths);

        assert!(polygons.contains([2.0, 2.0]), "inside the island");
        assert!(!polygons.contains([10.0, 10.0]), "inside the hole");
        assert!(!polygons.contains([-1.0, -1.0]), "outside everything");
    }
}
