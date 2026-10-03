//! Organic tree support.
//!
//! Turns the contact pads under each overhang into branches that grow down to
//! the bed — or, when they cannot get there, onto a top surface of the model —
//! leaning to merge with their neighbours and to steer around the part, and
//! thickening as they descend.  [`super::supports`] owns everything around it:
//! overhang detection, paint, the dense interface pads under the overhang, and
//! filling the regions returned here.
//!
//! # The model, as a branch sees it
//!
//! A branch is a chain of nodes, one per layer, each a disc of known radius.
//! Two kinds of region decide where a node may sit:
//!
//! - **Collision** — the model grown by the XY clearance plus the node's
//!   radius, over a window of `z_gap` layers above and below the node's own.
//!   The window is what keeps a branch the same air gap away from a top
//!   surface it passes over, or an overhang it passes under, as the tips keep
//!   from the overhang they hold.
//! - **Avoidance** — every position from which a branch can *no longer* get
//!   down without colliding, leaning at most the branch angle per layer.  It is
//!   built bottom-up, `avoid[i] = collision[i] ∪ erode(avoid[i−1], step)`: a
//!   point is lost if it collides, or if every point within one step of it on
//!   the layer below is lost.  A node outside it is guaranteed a way down;
//!   inside it, it is already too late.  One variant counts only the bed as a
//!   way down, the other also a top surface of the model.
//!
//! Avoidance is what lets a branch start sidestepping an obstacle several
//! layers before it reaches it, instead of running into it and stopping.
//! Collision is stored once for all radii (it is a distance test); avoidance is
//! built per radius on a coarse ladder, for the radii a branch can reach.
//!
//! # From tips to roots
//!
//! 1. **Tips** — each contact pad is sampled at the support pitch on a lattice
//!    fixed to the bed, so tips on successive layers line up, with a tip along
//!    every stretch of a pad's edge.  A spot a branch already passes under is
//!    held by that branch and gets no tip of its own.
//! 2. **Descent** — layer by layer from the top.  A branch drifts toward its
//!    nearest neighbour at the preferred angle, merges with it when they meet,
//!    and sidesteps the avoidance at up to the branch angle.  Its radius grows
//!    from the tip to the branch diameter, then by the diameter angle; a branch
//!    still thin from its tip plans as if it were full size, so it steps away
//!    from a wall while there is still time to grow.  Where there is no room
//!    it stops growing, then thins, rather than giving up.  A branch that can
//!    no longer reach the bed rests on the model where that is allowed; one
//!    with nowhere to go is pruned together with everything it was holding up,
//!    so nothing is left hanging in the air.
//! 3. **Smoothing** — positions relax toward their neighbours along each
//!    branch, never into the model and never past the branch angle, which turns
//!    the staircase of per-layer decisions into curves.
//! 4. **Drawing** — every node becomes a capsule reaching halfway to its
//!    neighbours above and below, so a leaning branch keeps its full width
//!    between layers.

use std::collections::HashMap;

use clipper2::*;

use super::supports::{poly_difference, poly_inflate, poly_union};

/// Radius ladder ratio for the avoidance maps.  Each rung costs one pass over
/// every layer, so the ladder is coarse; a node plans with the rung at or below
/// its radius and is then checked exactly, so the coarseness never lets a
/// branch into the model.
const RADIUS_LADDER_RATIO: f64 = 1.25;

/// Largest branch radius, as a multiple of the branch radius.  The diameter
/// angle keeps thickening a trunk all the way down; past this a tall print
/// would grow trunks wider than anything they hold.
const MAX_RADIUS_BRANCH_MULT: f64 = 2.5;

/// Simplification tolerance (mm) for the avoidance maps.  They are planning
/// aids, re-checked exactly, so a few hundredths of a millimetre is free — and
/// erosion otherwise multiplies their vertex counts layer after layer.
const AVOIDANCE_SIMPLIFY_MM: f64 = 0.03;

/// Farthest apart (mm) two branches may be and still lean toward each other.
/// Past this, merging them would leave both leaning for most of the drop.
const MAX_ATTRACTION_MM: f64 = 15.0;

/// Simplification tolerance (mm) for the drawn branch outlines.
const DRAW_SIMPLIFY_MM: f64 = 0.01;

/// How far (mm) a thin branch looks for the room it needs to grow.
const STEER_SEARCH_MM: f64 = 3.0;

/// Smoothing passes over the finished branches.
const SMOOTHING_ITERATIONS: usize = 24;

/// Chord length (mm) a node's outline is drawn with.  Every later step offsets
/// these outlines several times, and its cost follows their vertex count; a
/// 0.4 mm chord strays from a 1.5 mm circle by about a hundredth of a
/// millimetre.
const CIRCLE_CHORD_MM: f64 = 0.4;

/// Segments per full circle when drawing a node: at least…
const MIN_CIRCLE_SEGMENTS: usize = 8;
/// …and at most.
const MAX_CIRCLE_SEGMENTS: usize = 32;

/// How the tree is shaped.  Lengths in mm, angles in radians.
#[derive(Debug, Clone)]
pub(super) struct TreeSettings {
    /// Steepest a branch may lean from vertical.
    pub max_angle: f64,
    /// How far a branch leans when nothing forces it further.
    pub preferred_angle: f64,
    /// Radius where a branch meets the contact pad.
    pub tip_radius: f64,
    /// Radius a branch grows to below its tip.
    pub branch_radius: f64,
    /// Radius gained per millimetre of descent once a branch is full size.
    pub growth_per_mm: f64,
    /// Clearance from the model, measured from the wall centrelines the
    /// footprints are made of.
    pub xy: f64,
    /// Air layers kept between a branch and the model above and below it.
    pub z_gap: usize,
    /// Distance between tips on a contact pad.
    pub tip_spacing: f64,
    /// Whether a branch that cannot reach the bed may stand on the model.
    pub rest_on_model: bool,
}

/// What the tree grows from.  Every slice is indexed by layer.
pub(super) struct TreeInput<'a> {
    /// Height of each layer's slicing plane.
    pub z: &'a [f64],
    /// Model footprints, as outer-wall centrelines.
    pub footprints: &'a [Paths],
    /// Model cross-sections out to the surface — what a branch may stand on.
    pub solid: &'a [Paths],
    /// Contact pads: where support first appears at each layer.  Tips start
    /// here.
    pub contacts: &'a [Paths],
    /// The painted-enforcer share of the contacts, which may stand on the
    /// model even where the settings say otherwise.
    pub enforced: &'a [Paths],
    /// Extra no-go regions (painted blockers), kept clear like the model.
    pub obstacles: &'a [Paths],
}

/// The grown tree, by layer.
pub(super) struct TreeOutput {
    /// Branch cross-sections, clear of the model and its XY clearance.
    pub branches: Vec<Paths>,
    /// The contact pads a surviving branch holds up.  A pad no branch reached
    /// would be printed in mid-air, so the caller prints only these.
    pub supported_contacts: Vec<Paths>,
}

type Pt = (f64, f64);

#[inline]
fn sub(a: Pt, b: Pt) -> Pt {
    (a.0 - b.0, a.1 - b.1)
}

#[inline]
fn add(a: Pt, b: Pt) -> Pt {
    (a.0 + b.0, a.1 + b.1)
}

#[inline]
fn scale(a: Pt, s: f64) -> Pt {
    (a.0 * s, a.1 * s)
}

#[inline]
fn norm(a: Pt) -> f64 {
    a.0.hypot(a.1)
}

#[inline]
fn dist(a: Pt, b: Pt) -> f64 {
    norm(sub(a, b))
}

#[inline]
fn lerp(a: Pt, b: Pt, t: f64) -> Pt {
    add(a, scale(sub(b, a), t))
}

/// Closest point to `p` on the segment `a`–`b`.
fn closest_on_segment(p: Pt, a: Pt, b: Pt) -> Pt {
    let ab = sub(b, a);
    let len2 = ab.0 * ab.0 + ab.1 * ab.1;
    if len2 <= 1e-18 {
        return a;
    }
    let t = (((p.0 - a.0) * ab.0 + (p.1 - a.1) * ab.1) / len2).clamp(0.0, 1.0);
    add(a, scale(ab, t))
}

// ── Point queries against a polygon set ─────────────────────────────────────

/// A polygon set indexed for point queries: containment and the nearest
/// boundary point.
///
/// Edges are bucketed into horizontal bands, so a containment test reads one
/// band and a nearest-point search reads the few bands its radius spans.  It
/// costs one integer per edge per band touched — a full grid per layer, per
/// radius, per avoidance variant would not fit a browser tab.
///
/// The sets queried here come out of Clipper2 boolean operations, so their
/// contours never overlap and even-odd containment is exact.
#[derive(Default)]
struct Region {
    edges: Vec<[f64; 4]>,
    bands: Vec<Vec<u32>>,
    min_y: f64,
    band_h: f64,
    bbox: [f64; 4],
}

impl Region {
    fn new(paths: &Paths) -> Self {
        let mut edges = Vec::new();
        let mut bbox = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for path in paths.iter() {
            let pts: Vec<Pt> = path.iter().map(|p| (p.x(), p.y())).collect();
            if pts.len() < 3 {
                continue;
            }
            for (k, &a) in pts.iter().enumerate() {
                let b = pts[(k + 1) % pts.len()];
                if a == b {
                    continue;
                }
                bbox[0] = bbox[0].min(a.0);
                bbox[1] = bbox[1].min(a.1);
                bbox[2] = bbox[2].max(a.0);
                bbox[3] = bbox[3].max(a.1);
                edges.push([a.0, a.1, b.0, b.1]);
            }
        }
        if edges.is_empty() {
            return Self::default();
        }
        let height = (bbox[3] - bbox[1]).max(1e-6);
        let count = (edges.len() / 6).clamp(1, 8192);
        let band_h = (height / count as f64).max(1e-3);
        let count = (height / band_h).ceil() as usize + 1;
        let mut bands = vec![Vec::new(); count];
        for (id, e) in edges.iter().enumerate() {
            let lo = ((e[1].min(e[3]) - bbox[1]) / band_h).floor().max(0.0) as usize;
            let hi = ((e[1].max(e[3]) - bbox[1]) / band_h).floor().max(0.0) as usize;
            for band in bands.iter_mut().take(hi.min(count - 1) + 1).skip(lo) {
                band.push(id as u32);
            }
        }
        Self {
            edges,
            bands,
            min_y: bbox[1],
            band_h,
            bbox,
        }
    }

    fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    fn band_of(&self, y: f64) -> usize {
        (((y - self.min_y) / self.band_h).floor().max(0.0) as usize).min(self.bands.len() - 1)
    }

    /// Whether `p` lies inside the set (even-odd).
    fn contains(&self, p: Pt) -> bool {
        if self.is_empty()
            || p.0 < self.bbox[0]
            || p.0 > self.bbox[2]
            || p.1 < self.bbox[1]
            || p.1 > self.bbox[3]
        {
            return false;
        }
        let mut inside = false;
        for &id in &self.bands[self.band_of(p.1)] {
            let [x0, y0, x1, y1] = self.edges[id as usize];
            if (y0 <= p.1) != (y1 <= p.1) {
                let x = x0 + (p.1 - y0) * (x1 - x0) / (y1 - y0);
                if x > p.0 {
                    inside = !inside;
                }
            }
        }
        inside
    }

    /// The boundary point nearest `p`, if one lies within `max_d`.
    fn nearest_boundary(&self, p: Pt, max_d: f64) -> Option<(Pt, f64)> {
        if self.is_empty()
            || p.0 < self.bbox[0] - max_d
            || p.0 > self.bbox[2] + max_d
            || p.1 < self.bbox[1] - max_d
            || p.1 > self.bbox[3] + max_d
        {
            return None;
        }
        let mut best = max_d;
        let mut found = None;
        let lo = self.band_of(p.1 - max_d);
        let hi = self.band_of(p.1 + max_d);
        for band in &self.bands[lo..=hi] {
            for &id in band {
                let [x0, y0, x1, y1] = self.edges[id as usize];
                if x0.min(x1) > p.0 + best
                    || x0.max(x1) < p.0 - best
                    || y0.min(y1) > p.1 + best
                    || y0.max(y1) < p.1 - best
                {
                    continue;
                }
                let q = closest_on_segment(p, (x0, y0), (x1, y1));
                let d = dist(p, q);
                if d < best {
                    best = d;
                    found = Some(q);
                }
            }
        }
        found.map(|q| (q, best))
    }

    /// Whether `p` is inside the set or within `d` of it.
    fn within(&self, p: Pt, d: f64) -> bool {
        self.contains(p) || self.nearest_boundary(p, d).is_some()
    }

    /// The point nearest `p` that lies outside the set, if one is within
    /// `max_d` — `p` itself when it is already outside.
    fn nearest_outside(&self, p: Pt, max_d: f64) -> Option<Pt> {
        if !self.contains(p) {
            return Some(p);
        }
        let (q, d) = self.nearest_boundary(p, max_d)?;
        let out = if d > 1e-9 {
            scale(sub(q, p), 1.0 / d)
        } else {
            return None;
        };
        // Step just past the boundary; a sliver of the set can sit right
        // behind it, so try a couple of distances.
        for eps in [0.01, 0.03, 0.08] {
            let c = add(q, scale(out, eps));
            if dist(c, p) <= max_d && !self.contains(c) {
                return Some(c);
            }
        }
        None
    }
}

// ── The model, as a branch sees it ──────────────────────────────────────────

/// Which ways down a branch may count on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Only the bed ends this branch.
    ToBed,
    /// A top surface of the model will do as well.
    ToModel,
}

/// Avoidance for one rung of the radius ladder.
struct Rung {
    to_bed: Vec<Region>,
    to_model: Vec<Region>,
}

/// Collision, resting surfaces and the per-radius avoidance maps.
struct Volumes {
    n: usize,
    xy: f64,
    gap: usize,
    /// Most a branch may move sideways between layer `i` and `i − 1`.
    steps: Vec<f64>,
    /// The model (and any obstacles) within the Z window of each layer, as
    /// centreline regions — collision is a distance test against these.
    blocked_paths: Vec<Paths>,
    blocked: Vec<Region>,
    /// Where a branch on layer `i` may stand: the model `gap + 1` layers down.
    rest_paths: Vec<Paths>,
    rest: Vec<Region>,
    radii: Vec<f64>,
    rungs: Vec<Option<Rung>>,
    allow_model: bool,
}

impl Volumes {
    fn new(input: &TreeInput, settings: &TreeSettings, steps: Vec<f64>, allow_model: bool) -> Self {
        let n = input.footprints.len();
        let gap = settings.z_gap;
        let blocked_paths: Vec<Paths> = (0..n)
            .map(|i| {
                let lo = i.saturating_sub(gap);
                let hi = (i + gap).min(n - 1);
                let mut acc = Paths::new(vec![]);
                for j in lo..=hi {
                    acc = poly_union(&acc, &input.footprints[j]);
                    if let Some(o) = input.obstacles.get(j) {
                        acc = poly_union(&acc, o);
                    }
                }
                simplify(acc, AVOIDANCE_SIMPLIFY_MM, false)
            })
            .collect();
        let blocked = blocked_paths.iter().map(Region::new).collect();
        let rest_paths: Vec<Paths> = (0..n)
            .map(|i| match i.checked_sub(gap + 1) {
                Some(k) => input.solid[k].clone(),
                None => Paths::new(vec![]),
            })
            .collect();
        let rest = rest_paths.iter().map(Region::new).collect();

        let mut radii = vec![settings.tip_radius];
        let max_r = settings.branch_radius * MAX_RADIUS_BRANCH_MULT;
        let mut r = settings.branch_radius.max(settings.tip_radius);
        while r <= max_r + 1e-9 {
            if r > radii[radii.len() - 1] + 1e-6 {
                radii.push(r);
            }
            r *= RADIUS_LADDER_RATIO;
        }
        let rungs = radii.iter().map(|_| None).collect();
        Self {
            n,
            xy: settings.xy,
            gap,
            steps,
            blocked_paths,
            blocked,
            rest_paths,
            rest,
            radii,
            rungs,
            allow_model,
        }
    }

    /// Whether a disc of radius `r` at `p` on `layer` meets the model.
    fn collides(&self, layer: usize, p: Pt, r: f64) -> bool {
        self.blocked[layer].within(p, self.xy + r)
    }

    /// Whether a branch of radius `r` at `p` may stand on the model here.
    fn can_rest(&self, layer: usize, p: Pt, r: f64) -> bool {
        layer > self.gap && self.rest[layer].contains(p) && !self.collides(layer, p, r)
    }

    /// The ladder rung a node of radius `r` plans with: the largest at or
    /// below it.
    fn rung_of(&self, r: f64) -> usize {
        self.radii.iter().rposition(|&x| x <= r + 1e-9).unwrap_or(0)
    }

    /// Build the avoidance maps for one rung of the radius ladder.
    fn build_rung(&self, r: f64) -> Rung {
        let mut to_bed = Vec::with_capacity(self.n);
        let mut to_model = Vec::with_capacity(self.n);
        let mut bed_prev = Paths::new(vec![]);
        let mut model_prev = Paths::new(vec![]);
        for i in 0..self.n {
            let collision = simplify(
                poly_inflate(&self.blocked_paths[i], self.xy + r),
                AVOIDANCE_SIMPLIFY_MM,
                false,
            );
            let step = self.steps[i];
            let bed = if i == 0 {
                collision.clone()
            } else {
                poly_union(&collision, &poly_inflate(&bed_prev, -step))
            };
            let bed = simplify(bed, AVOIDANCE_SIMPLIFY_MM, false);
            to_bed.push(Region::new(&bed));
            bed_prev = bed;
            if self.allow_model {
                let model = if i == 0 {
                    collision
                } else {
                    // A point over a top surface is a way down in itself.
                    let carried =
                        poly_difference(&poly_inflate(&model_prev, -step), &self.rest_paths[i]);
                    poly_union(&collision, &carried)
                };
                let model = simplify(model, AVOIDANCE_SIMPLIFY_MM, false);
                to_model.push(Region::new(&model));
                model_prev = model;
            }
        }
        Rung { to_bed, to_model }
    }

    fn ensure(&mut self, rung: usize) {
        if self.rungs[rung].is_none() {
            self.rungs[rung] = Some(self.build_rung(self.radii[rung]));
        }
    }

    /// Build every rung a branch could ever plan with, up to radius `max_r`.
    ///
    /// The rungs are independent of each other, so where threads are
    /// available they are built at once; elsewhere this is a no-op and each
    /// rung is built the first time a branch asks for it.
    fn prebuild(&mut self, max_r: f64) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use rayon::prelude::*;
            let todo: Vec<usize> = (0..self.radii.len())
                .filter(|&k| self.rungs[k].is_none() && self.radii[k] <= max_r + 1e-9)
                .collect();
            let built: Vec<(usize, Rung)> = todo
                .par_iter()
                .map(|&k| (k, self.build_rung(self.radii[k])))
                .collect();
            for (k, rung) in built {
                self.rungs[k] = Some(rung);
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = max_r;
    }

    fn avoidance(&mut self, mode: Mode, rung: usize, layer: usize) -> &Region {
        self.ensure(rung);
        let maps = self.rungs[rung].as_ref().expect("rung was just built");
        match mode {
            Mode::ToModel if self.allow_model => &maps.to_model[layer],
            _ => &maps.to_bed[layer],
        }
    }

    /// Whether a node planning with `rung` can still get down from `p`.
    fn free(&mut self, mode: Mode, rung: usize, layer: usize, p: Pt) -> bool {
        !self.avoidance(mode, rung, layer).contains(p)
    }

    /// Where a node at `from` (one layer up) should go on `layer`, heading for
    /// `want`: the nearest point to `want` it can reach, that still has a way
    /// down and that a disc of radius `r` fits at.
    fn place(
        &mut self,
        layer: usize,
        from: Pt,
        want: Pt,
        r: f64,
        rung: usize,
        mode: Mode,
    ) -> Option<Pt> {
        let step = self.steps[layer + 1];
        let mut candidates = Vec::with_capacity(3);
        if let Some(q) = self
            .avoidance(mode, rung, layer)
            .nearest_outside(want, step * 2.0)
        {
            candidates.push(q);
        }
        if let Some(q) = self
            .avoidance(mode, rung, layer)
            .nearest_outside(from, step)
        {
            candidates.push(q);
        }
        candidates
            .into_iter()
            .filter(|&q| dist(q, from) <= step + 1e-6)
            .find(|&q| !self.collides(layer, q, r))
    }

    /// One step from `from` toward the nearest point that is free for
    /// `goal_rung`, landing somewhere that is already free for `safe_rung` —
    /// how a thin branch hugging a wall works its way out to where it has room
    /// to grow, when that room is more than a step away.
    fn steer(
        &mut self,
        layer: usize,
        from: Pt,
        r: f64,
        goal_rung: usize,
        safe_rung: usize,
        mode: Mode,
    ) -> Option<Pt> {
        let step = self.steps[layer + 1];
        let goal = self.avoidance(mode, goal_rung, layer);
        if !goal.contains(from) {
            return None;
        }
        let (q, d) = goal.nearest_boundary(from, STEER_SEARCH_MM)?;
        if d <= 1e-9 {
            return None;
        }
        let c = add(from, scale(sub(q, from), step.min(d) / d));
        (self.free(mode, safe_rung, layer, c) && !self.collides(layer, c, r)).then_some(c)
    }
}

// ── Branches ────────────────────────────────────────────────────────────────

/// What ends a branch at its lowest node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Root {
    Bed,
    Model,
}

#[derive(Debug, Clone)]
struct Node {
    layer: usize,
    pos: Pt,
    radius: f64,
    /// Height of the highest tip feeding this node — growth is measured from it.
    tip_z: f64,
    /// Tips feeding this node; heavier branches pull merges toward themselves.
    weight: u32,
    mode: Mode,
    may_rest: bool,
    /// Nodes on the layer above that continue into this one.
    parents: Vec<usize>,
    /// The node on the layer below this one continues into.
    child: Option<usize>,
    tip: bool,
    root: Option<Root>,
    /// Could not continue: removed together with everything it holds up.
    lost: bool,
}

struct Grower<'a> {
    input: &'a TreeInput<'a>,
    settings: &'a TreeSettings,
    volumes: Volumes,
    /// Preferred sideways drift between layer `i` and `i − 1`.
    pref_steps: Vec<f64>,
    nodes: Vec<Node>,
    by_layer: Vec<Vec<usize>>,
}

impl<'a> Grower<'a> {
    fn radius_at(&self, tip_z: f64, z: f64) -> f64 {
        let s = self.settings;
        let below = (tip_z - z).max(0.0);
        let tip_len = (s.branch_radius - s.tip_radius).max(0.0) / s.max_angle.tan().max(0.1);
        let r = if below < tip_len {
            s.tip_radius + (s.branch_radius - s.tip_radius) * (below / tip_len.max(1e-9))
        } else {
            s.branch_radius + (below - tip_len) * s.growth_per_mm
        };
        r.min(s.branch_radius * MAX_RADIUS_BRANCH_MULT)
    }

    fn push(&mut self, node: Node) -> usize {
        let id = self.nodes.len();
        self.by_layer[node.layer].push(id);
        self.nodes.push(node);
        id
    }

    /// Seed the tips for `layer`'s contact pad.
    fn seed_tips(&mut self, layer: usize) {
        let input = self.input;
        let pad = &input.contacts[layer];
        if pad.is_empty() {
            return;
        }
        let s = self.settings;
        let spacing = s.tip_spacing;
        let enforced = Region::new(&input.enforced[layer]);
        let z = input.z[layer];

        // Positions already held: branches passing this layer, and tips taken.
        // A passing branch holds a full pitch around it, as a tip would — on a
        // slope, where every layer adds a fresh sliver of contact, that is what
        // spaces the tips a pitch apart along the surface rather than along
        // each layer's sliver.
        let mut taken: Vec<(Pt, f64)> = self.by_layer[layer]
            .iter()
            .map(|&id| (self.nodes[id].pos, self.nodes[id].radius.max(spacing)))
            .collect();
        let mut grid = PointGrid::new(spacing.max(0.5));
        for (i, &(p, _)) in taken.iter().enumerate() {
            grid.insert(p, i);
        }

        let tip_rung = 0;
        for p in sample_pad(pad, spacing, s.tip_radius) {
            let held = grid
                .near(p, spacing * 2.0 + s.branch_radius * MAX_RADIUS_BRANCH_MULT)
                .any(|i| dist(taken[i].0, p) < taken[i].1);
            if held {
                continue;
            }
            let may_rest = s.rest_on_model || enforced.within(p, s.tip_radius);
            // A tip that collides may shuffle a little, never far: it must
            // still sit under the pad it holds.
            let p = if self.volumes.collides(layer, p, s.tip_radius) {
                match self.volumes.blocked[layer]
                    .nearest_boundary(p, self.volumes.xy + s.tip_radius * 2.0)
                {
                    Some((q, d)) if d > 1e-9 => {
                        let away = scale(sub(p, q), 1.0 / d);
                        let c = add(q, scale(away, self.volumes.xy + s.tip_radius + 0.02));
                        if dist(c, p) > s.tip_radius
                            || self.volumes.collides(layer, c, s.tip_radius)
                        {
                            continue;
                        }
                        c
                    }
                    _ => continue,
                }
            } else {
                p
            };
            let mode = if self.volumes.free(Mode::ToBed, tip_rung, layer, p) {
                Mode::ToBed
            } else if may_rest
                && self.volumes.allow_model
                && self.volumes.free(Mode::ToModel, tip_rung, layer, p)
            {
                Mode::ToModel
            } else {
                continue;
            };
            let idx = taken.len();
            taken.push((p, spacing * 0.7));
            grid.insert(p, idx);
            self.push(Node {
                layer,
                pos: p,
                radius: s.tip_radius,
                tip_z: z,
                weight: 1,
                mode,
                may_rest,
                parents: Vec::new(),
                child: None,
                tip: true,
                root: None,
                lost: false,
            });
        }
    }

    /// Carry every active branch on `layer` down to `layer − 1`.
    fn descend(&mut self, layer: usize) {
        let below = layer - 1;
        let z_below = self.input.z[below];
        let step = self.volumes.steps[layer];
        let pref = self.pref_steps[layer];
        let active: Vec<usize> = self.by_layer[layer]
            .iter()
            .copied()
            .filter(|&id| self.nodes[id].root.is_none())
            .collect();
        if active.is_empty() {
            return;
        }

        // 1. Where each branch would like to go: toward its nearest neighbour.
        let mut grid = PointGrid::new(4.0);
        for (k, &id) in active.iter().enumerate() {
            grid.insert(self.nodes[id].pos, k);
        }
        // Branches only lean toward a partner they can meet with a fifth of the
        // remaining drop to spare — closing the gap from both sides at the
        // preferred angle — so merges finish well above the bed.
        let reach = (2.0 * 0.8 * layer as f64 * pref).min(MAX_ATTRACTION_MM);
        let mut want: Vec<Pt> = Vec::with_capacity(active.len());
        for &id in &active {
            let a = &self.nodes[id];
            let mut best: Option<(f64, Pt)> = None;
            for k in grid.near(a.pos, reach) {
                let b = &self.nodes[active[k]];
                if active[k] == id {
                    continue;
                }
                let d = dist(a.pos, b.pos);
                if d > reach {
                    continue;
                }
                // Heavier partners count as nearer, so twigs feed trunks.
                let score = d / (1.0 + 0.05 * b.weight as f64);
                if best.is_none_or(|(s, _)| score < s) {
                    best = Some((score, b.pos));
                }
            }
            want.push(match best {
                Some((_, target)) => {
                    let d = dist(a.pos, target);
                    let close = (d * 0.5).min(pref);
                    if d > 1e-9 {
                        add(a.pos, scale(sub(target, a.pos), close / d))
                    } else {
                        a.pos
                    }
                }
                None => a.pos,
            });
        }

        // 2. Validate each move against avoidance and collision, with the
        //    fallbacks a branch has when its first choice is out of reach:
        //    first stop growing, then thin out step by step, and only then —
        //    where allowed — give up on the bed and head for the model.  A
        //    branch squeezed thin regrows at most one step per layer, so it
        //    never jumps straight back into what squeezed it.
        let mut next: Vec<Option<(Pt, f64, Mode)>> = Vec::with_capacity(active.len());
        for (k, &id) in active.iter().enumerate() {
            let a = self.nodes[id].clone();
            let grown = self.radius_at(a.tip_z, z_below).min(a.radius + step);
            let tip = self.settings.tip_radius;
            let radii = [
                grown,
                a.radius,
                (a.radius * 0.7).max(tip),
                (a.radius * 0.45).max(tip),
                tip,
            ];
            let modes: &[Mode] = if a.mode == Mode::ToBed && a.may_rest && self.volumes.allow_model
            {
                &[Mode::ToBed, Mode::ToModel]
            } else {
                std::slice::from_ref(&a.mode)
            };
            // A thin branch plans as if it were already full size whenever it
            // can: steering clear of a wall while still a tip is what leaves
            // it room to grow, instead of meeting the wall at full width.
            let full = self.volumes.rung_of(self.settings.branch_radius);
            let mut placed = None;
            'tries: for &mode in modes {
                let mut last = f64::NAN;
                for &radius in &radii {
                    if radius == last {
                        continue;
                    }
                    last = radius;
                    let own = self.volumes.rung_of(radius);
                    for rung in [own.max(full), own] {
                        if let Some(q) = self
                            .volumes
                            .place(below, a.pos, want[k], radius, rung, mode)
                        {
                            placed = Some((q, radius, mode));
                            break 'tries;
                        }
                        if rung == own {
                            break;
                        }
                        if let Some(q) = self.volumes.steer(below, a.pos, radius, rung, own, mode) {
                            placed = Some((q, radius, mode));
                            break 'tries;
                        }
                    }
                }
            }
            if placed.is_none() {
                self.nodes[id].lost = true;
            }
            next.push(placed);
        }

        // 3. Merge branches that can meet on the layer below.  Heaviest
        //    first, so a trunk gathers its twigs rather than the other way
        //    round.  Two branches meet where each can still reach in one step:
        //    the weighted middle of where they were headed, or failing that,
        //    of where they are.
        let mut order: Vec<usize> = (0..active.len()).filter(|&k| next[k].is_some()).collect();
        order.sort_by(|&x, &y| {
            self.nodes[active[y]]
                .weight
                .cmp(&self.nodes[active[x]].weight)
                .then(x.cmp(&y))
        });
        let mut grid = PointGrid::new(2.0);
        for &k in &order {
            grid.insert(self.nodes[active[k]].pos, k);
        }
        let mut done = vec![false; active.len()];
        for &k in &order {
            if done[k] {
                continue;
            }
            done[k] = true;
            let (pos_k, r_k, mode_k) = next[k].expect("filtered");
            let here_k = self.nodes[active[k]].pos;
            let mut group = vec![k];
            let mut pos = pos_k;
            let mut radius = r_k;
            let mut mode = mode_k;
            let mut weight = self.nodes[active[k]].weight;
            let mut tip_z = self.nodes[active[k]].tip_z;
            // A merged branch may stand on the model only if every branch in
            // it may: build-plate-only branches never join one that rests.
            let mut may_rest = self.nodes[active[k]].may_rest;
            let mut candidates: Vec<usize> = grid
                .near(here_k, 2.0 * step)
                .filter(|&j| !done[j] && j != k)
                .collect();
            candidates.sort_by(|&x, &y| {
                let dx = dist(self.nodes[active[x]].pos, here_k);
                let dy = dist(self.nodes[active[y]].pos, here_k);
                dx.total_cmp(&dy).then(x.cmp(&y))
            });
            for j in candidates {
                let (_, _, mode_j) = next[j].expect("filtered");
                let w_j = self.nodes[active[j]].weight;
                let members: Vec<usize> = group.iter().copied().chain(std::iter::once(j)).collect();
                let total: f64 = members
                    .iter()
                    .map(|&m| self.nodes[active[m]].weight as f64)
                    .sum();
                let middle = |at: &dyn Fn(usize) -> Pt| -> Pt {
                    members.iter().fold((0.0, 0.0), |acc, &m| {
                        add(
                            acc,
                            scale(at(m), self.nodes[active[m]].weight as f64 / total),
                        )
                    })
                };
                let headed = middle(&|m| next[m].expect("filtered").0);
                let current = middle(&|m| self.nodes[active[m]].pos);
                let merged_tip_z = tip_z.max(self.nodes[active[j]].tip_z);
                let thickest = members
                    .iter()
                    .map(|&m| self.nodes[active[m]].radius)
                    .fold(0.0, f64::max);
                let merged_r = self.radius_at(merged_tip_z, z_below).min(thickest + step);
                let merged_mode = if mode == Mode::ToBed && mode_j == Mode::ToBed {
                    Mode::ToBed
                } else {
                    Mode::ToModel
                };
                let merged_may_rest = may_rest && self.nodes[active[j]].may_rest;
                if merged_mode == Mode::ToModel && !merged_may_rest {
                    continue;
                }
                let rung = self.volumes.rung_of(merged_r);
                let mut chosen = None;
                for m in [headed, current] {
                    let reachable = members
                        .iter()
                        .all(|&x| dist(self.nodes[active[x]].pos, m) <= step + 1e-6);
                    if reachable
                        && self.volumes.free(merged_mode, rung, below, m)
                        && !self.volumes.collides(below, m, merged_r)
                    {
                        chosen = Some(m);
                        break;
                    }
                }
                let Some(merged) = chosen else {
                    continue;
                };
                done[j] = true;
                group.push(j);
                pos = merged;
                radius = merged_r;
                mode = merged_mode;
                weight += w_j;
                tip_z = merged_tip_z;
                may_rest = merged_may_rest;
            }
            let id = self.push(Node {
                layer: below,
                pos,
                radius,
                tip_z,
                weight,
                mode,
                may_rest,
                parents: group.iter().map(|&m| active[m]).collect(),
                child: None,
                tip: false,
                root: None,
                lost: false,
            });
            for &m in &group {
                self.nodes[active[m]].child = Some(id);
            }
            // A branch that has come down onto a top surface stops here.
            if below == 0 {
                self.nodes[id].root = Some(Root::Bed);
            } else if mode == Mode::ToModel && self.volumes.can_rest(below, pos, radius) {
                self.nodes[id].root = Some(Root::Model);
            }
        }
    }

    /// Mark every node whose branch failed below it, so nothing is printed
    /// hanging from a branch that never reached anything.
    fn prune(&mut self) -> Vec<bool> {
        // Children are always created after their parents, so walking the ids
        // backwards settles each child before any node above it.
        let mut dead = vec![false; self.nodes.len()];
        for id in (0..self.nodes.len()).rev() {
            let node = &self.nodes[id];
            dead[id] = node.lost
                || match node.child {
                    Some(c) => dead[c],
                    None => node.root.is_none(),
                };
        }
        dead
    }

    /// Relax node positions along each branch.
    fn smooth(&mut self, dead: &[bool]) {
        let ids: Vec<usize> = (0..self.nodes.len())
            .filter(|&id| {
                !dead[id] && !self.nodes[id].tip && self.nodes[id].root != Some(Root::Model)
            })
            .collect();
        for iteration in 0..SMOOTHING_ITERATIONS {
            let forward = iteration % 2 == 0;
            for k in 0..ids.len() {
                let id = if forward {
                    ids[k]
                } else {
                    ids[ids.len() - 1 - k]
                };
                let node = &self.nodes[id];
                if node.parents.is_empty() {
                    continue;
                }
                let above: Pt = {
                    let mut acc = (0.0, 0.0);
                    for &p in &node.parents {
                        acc = add(acc, self.nodes[p].pos);
                    }
                    scale(acc, 1.0 / node.parents.len() as f64)
                };
                let target = match node.child {
                    Some(c) => lerp(above, self.nodes[c].pos, 0.5),
                    None => above,
                };
                let cand = lerp(node.pos, target, 0.5);
                if dist(cand, node.pos) < 1e-4 {
                    continue;
                }
                let layer = node.layer;
                let fits_child = node.child.is_none_or(|c| {
                    dist(cand, self.nodes[c].pos) <= self.volumes.steps[layer] + 1e-6
                });
                let fits_parents = node.parents.iter().all(|&p| {
                    dist(cand, self.nodes[p].pos) <= self.volumes.steps[layer + 1] + 1e-6
                });
                if fits_child && fits_parents && !self.volumes.collides(layer, cand, node.radius) {
                    self.nodes[id].pos = cand;
                }
            }
        }
    }
}

/// Ids of points near a position, by uniform grid.
struct PointGrid {
    cell: f64,
    map: HashMap<(i64, i64), Vec<usize>>,
}

impl PointGrid {
    fn new(cell: f64) -> Self {
        Self {
            cell: cell.max(1e-3),
            map: HashMap::new(),
        }
    }

    fn key(&self, p: Pt) -> (i64, i64) {
        (
            (p.0 / self.cell).floor() as i64,
            (p.1 / self.cell).floor() as i64,
        )
    }

    fn insert(&mut self, p: Pt, id: usize) {
        let key = self.key(p);
        self.map.entry(key).or_default().push(id);
    }

    /// Ids within `r` of `p` — conservatively: every id in the cells the
    /// square around `p` touches.
    fn near(&self, p: Pt, r: f64) -> impl Iterator<Item = usize> + '_ {
        let (cx, cy) = self.key(p);
        let span = (r / self.cell).ceil() as i64;
        (cx - span..=cx + span)
            .flat_map(move |x| (cy - span..=cy + span).map(move |y| (x, y)))
            .filter_map(move |k| self.map.get(&k))
            .flat_map(|v| v.iter().copied())
    }
}

/// Candidate tip positions for one contact pad.
///
/// The edge comes first — a tip every `spacing` along each contour inset by
/// the tip radius, or along the contour itself where the pad is too thin to
/// inset — so the rim of an overhang is never left for the interface to bridge.
/// The interior follows on a staggered lattice fixed to the bed rather than to
/// the pad, so the tips of successive layers stack instead of jittering.  Every
/// island has a rim, so even one the lattice misses entirely gets a tip.
fn sample_pad(pad: &Paths, spacing: f64, tip_radius: f64) -> Vec<Pt> {
    let mut out = Vec::new();
    let inset = poly_inflate(pad, -tip_radius);
    let rim = if inset.is_empty() { pad } else { &inset };
    for contour in rim.iter() {
        let pts: Vec<Pt> = contour.iter().map(|p| (p.x(), p.y())).collect();
        if pts.len() < 2 {
            continue;
        }
        let perimeter: f64 = (0..pts.len())
            .map(|i| dist(pts[i], pts[(i + 1) % pts.len()]))
            .sum();
        let count = (perimeter / spacing).round().max(1.0) as usize;
        let pitch = perimeter / count as f64;
        let mut next = 0.0;
        let mut walked = 0.0;
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            let len = dist(a, b);
            while next <= walked + len && out.len() < 1_000_000 {
                let t = if len > 1e-12 {
                    (next - walked) / len
                } else {
                    0.0
                };
                out.push(lerp(a, b, t));
                next += pitch;
            }
            walked += len;
        }
    }

    let region = Region::new(pad);
    let interior = Region::new(&inset);
    if !interior.is_empty() {
        let dy = spacing * 3f64.sqrt() * 0.5;
        let [x0, y0, x1, y1] = interior.bbox;
        let row0 = (y0 / dy).floor() as i64;
        let row1 = (y1 / dy).ceil() as i64;
        for row in row0..=row1 {
            let y = row as f64 * dy;
            let shift = if row.rem_euclid(2) == 1 {
                spacing * 0.5
            } else {
                0.0
            };
            let col0 = ((x0 - shift) / spacing).floor() as i64;
            let col1 = ((x1 - shift) / spacing).ceil() as i64;
            for col in col0..=col1 {
                let p = (col as f64 * spacing + shift, y);
                if interior.contains(p) {
                    out.push(p);
                }
            }
        }
    }

    if out.is_empty() {
        if let Some(p) = pad.iter().find_map(|c| {
            let pts: Vec<Pt> = c.iter().map(|p| (p.x(), p.y())).collect();
            (pts.len() >= 3 && c.signed_area() > 0.0).then(|| {
                let n = pts.len() as f64;
                let centre = pts
                    .iter()
                    .fold((0.0, 0.0), |acc, &p| add(acc, scale(p, 1.0 / n)));
                if region.contains(centre) {
                    centre
                } else {
                    pts[0]
                }
            })
        }) {
            out.push(p);
        }
    }
    out
}

/// A closed polygon approximating the capsule from `a` to `b` with radius `r`
/// — a disc when the two coincide.
fn capsule(a: Pt, b: Pt, r: f64) -> Path {
    let segments = ((2.0 * std::f64::consts::PI * r / CIRCLE_CHORD_MM).ceil() as usize)
        .clamp(MIN_CIRCLE_SEGMENTS, MAX_CIRCLE_SEGMENTS);
    let half = segments / 2;
    let d = sub(b, a);
    let len = norm(d);
    let base = if len > 1e-9 { d.1.atan2(d.0) } else { 0.0 };
    let mut pts: Vec<(f64, f64)> = Vec::with_capacity(segments + 2);
    // Half circle around `b`, from one side of the segment to the other…
    for k in 0..=half {
        let t = base - std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * k as f64 / half as f64;
        pts.push((b.0 + r * t.cos(), b.1 + r * t.sin()));
    }
    // …and back around `a`.
    for k in 0..=half {
        let t = base + std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * k as f64 / half as f64;
        pts.push((a.0 + r * t.cos(), a.1 + r * t.sin()));
    }
    Path::from(pts)
}

/// Grow the tree.  See the module docs for the steps.
pub(super) fn generate(input: &TreeInput, settings: &TreeSettings) -> TreeOutput {
    let n = input.footprints.len();
    let empty = || TreeOutput {
        branches: vec![Paths::new(vec![]); n],
        supported_contacts: vec![Paths::new(vec![]); n],
    };
    if n < 2 || input.contacts.iter().all(|c| c.is_empty()) {
        return empty();
    }

    let dz = |i: usize| -> f64 {
        if i == 0 {
            0.0
        } else {
            (input.z[i] - input.z[i - 1]).max(1e-4)
        }
    };
    let steps: Vec<f64> = (0..n).map(|i| dz(i) * settings.max_angle.tan()).collect();
    let pref_steps: Vec<f64> = (0..n)
        .map(|i| dz(i) * settings.preferred_angle.min(settings.max_angle).tan())
        .collect();
    let allow_model = settings.rest_on_model || input.enforced.iter().any(|e| !e.is_empty());
    let volumes = Volumes::new(input, settings, steps, allow_model);

    let mut grower = Grower {
        input,
        settings,
        volumes,
        pref_steps,
        nodes: Vec::new(),
        by_layer: vec![Vec::new(); n],
    };
    // No branch can be thicker than one grown from the highest contact all the
    // way to the bed.
    if let Some(top) = (0..n).rev().find(|&i| !input.contacts[i].is_empty()) {
        let thickest = grower.radius_at(input.z[top], input.z[0]);
        grower.volumes.prebuild(thickest);
    }

    for layer in (0..n).rev() {
        grower.seed_tips(layer);
        if layer == 0 {
            for &id in &grower.by_layer[0].clone() {
                if grower.nodes[id].root.is_none() {
                    grower.nodes[id].root = Some(Root::Bed);
                }
            }
            break;
        }
        // Tips seeded straight onto a top surface already stand on it.
        for &id in &grower.by_layer[layer].clone() {
            let node = &grower.nodes[id];
            if node.root.is_none()
                && node.mode == Mode::ToModel
                && grower.volumes.can_rest(layer, node.pos, node.radius)
            {
                grower.nodes[id].root = Some(Root::Model);
            }
        }
        grower.descend(layer);
    }

    let dead = grower.prune();
    grower.smooth(&dead);

    // Every layer is drawn on its own from the finished nodes.
    let grower = &grower;
    let dead = &dead;
    let drawn: Vec<(Paths, Paths)> = super::supports::per_layer(n, |layer| {
        let mut shapes: Vec<Path> = Vec::new();
        let mut live: Vec<(Pt, f64)> = Vec::new();
        for &id in &grower.by_layer[layer] {
            if dead[id] {
                continue;
            }
            let node = &grower.nodes[id];
            live.push((node.pos, node.radius));
            let mut reach: Vec<Pt> = node
                .parents
                .iter()
                .filter(|&&p| !dead[p])
                .map(|&p| lerp(node.pos, grower.nodes[p].pos, 0.5))
                .collect();
            if let Some(c) = node.child {
                reach.push(lerp(node.pos, grower.nodes[c].pos, 0.5));
            }
            if reach.is_empty() {
                shapes.push(capsule(node.pos, node.pos, node.radius));
            }
            for q in reach {
                shapes.push(capsule(node.pos, q, node.radius));
            }
        }
        if shapes.is_empty() {
            return (Paths::new(vec![]), Paths::new(vec![]));
        }
        let union_all =
            union(Paths::new(shapes), Paths::new(vec![]), FillRule::NonZero).unwrap_or_default();
        let branches = poly_difference(
            &simplify(union_all, DRAW_SIMPLIFY_MM, false),
            &poly_inflate(&input.footprints[layer], settings.xy),
        );

        // A pad is held if a surviving branch passes under it.
        let pad = &input.contacts[layer];
        let mut kept: Vec<Path> = Vec::new();
        if !pad.is_empty() {
            for island in super::infill::group_islands(pad) {
                let mut paths = vec![island.0];
                paths.extend(island.1);
                let island = Paths::new(paths);
                let region = Region::new(&island);
                if live.iter().any(|&(p, r)| region.within(p, r)) {
                    kept.extend(island.iter().cloned());
                }
            }
        }
        (branches, Paths::new(kept))
    });

    let (branches, supported_contacts) = drawn.into_iter().unzip();
    TreeOutput {
        branches,
        supported_contacts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(cx: f64, cy: f64, half: f64) -> Paths {
        Paths::new(vec![Path::from(vec![
            (cx - half, cy - half),
            (cx + half, cy - half),
            (cx + half, cy + half),
            (cx - half, cy + half),
        ])])
    }

    fn settings() -> TreeSettings {
        TreeSettings {
            max_angle: 40f64.to_radians(),
            preferred_angle: 25f64.to_radians(),
            tip_radius: 0.4,
            branch_radius: 1.0,
            growth_per_mm: 5f64.to_radians().tan(),
            xy: 0.55,
            z_gap: 1,
            tip_spacing: 2.67,
            rest_on_model: true,
        }
    }

    struct Scene {
        z: Vec<f64>,
        footprints: Vec<Paths>,
        solid: Vec<Paths>,
        contacts: Vec<Paths>,
        enforced: Vec<Paths>,
        obstacles: Vec<Paths>,
    }

    impl Scene {
        fn new(n: usize) -> Self {
            Self {
                z: (0..n).map(|i| 0.1 + 0.2 * i as f64).collect(),
                footprints: vec![Paths::new(vec![]); n],
                solid: vec![Paths::new(vec![]); n],
                contacts: vec![Paths::new(vec![]); n],
                enforced: vec![Paths::new(vec![]); n],
                obstacles: vec![Paths::new(vec![]); n],
            }
        }

        fn model(&mut self, layers: std::ops::Range<usize>, region: Paths) {
            for i in layers {
                self.footprints[i] = poly_union(&self.footprints[i], &region);
                self.solid[i] = poly_inflate(&self.footprints[i], 0.2);
            }
        }

        fn grow(&self, settings: &TreeSettings) -> TreeOutput {
            generate(
                &TreeInput {
                    z: &self.z,
                    footprints: &self.footprints,
                    solid: &self.solid,
                    contacts: &self.contacts,
                    enforced: &self.enforced,
                    obstacles: &self.obstacles,
                },
                settings,
            )
        }
    }

    fn area(paths: &Paths) -> f64 {
        paths.iter().map(|p| p.signed_area()).sum()
    }

    fn islands(paths: &Paths) -> usize {
        paths.iter().filter(|p| p.signed_area() > 0.0).count()
    }

    #[test]
    fn region_queries_agree_with_the_geometry() {
        let mut ring = square(0.0, 0.0, 5.0);
        ring.push(Path::from(vec![
            (-2.0, -2.0),
            (-2.0, 2.0),
            (2.0, 2.0),
            (2.0, -2.0),
        ]));
        let r = Region::new(&ring);
        assert!(r.contains((4.0, 0.0)));
        assert!(!r.contains((0.0, 0.0)), "the hole is outside");
        assert!(!r.contains((6.0, 0.0)));
        let (q, d) = r
            .nearest_boundary((0.0, 0.0), 5.0)
            .expect("the hole's edge is 2 mm away");
        assert!((d - 2.0).abs() < 1e-9 && q.0.abs().max(q.1.abs()) > 1.99);
        let out = r
            .nearest_outside((3.0, 0.0), 2.0)
            .expect("an edge 1 mm away");
        assert!(!r.contains(out) && dist(out, (3.0, 0.0)) < 1.2);
    }

    #[test]
    fn a_floating_pad_grows_a_tree_to_the_bed() {
        // Nothing but a 12 mm pad 6 mm up: tips merge into fewer trunks on
        // the way down, and every branch reaches the bed.
        let mut scene = Scene::new(31);
        scene.contacts[30] = square(0.0, 0.0, 6.0);
        let out = scene.grow(&settings());
        assert!(area(&out.branches[30]) > 0.0, "tips under the pad");
        assert!(area(&out.branches[0]) > 0.0, "branches stand on the bed");
        assert!(
            islands(&out.branches[0]) < islands(&out.branches[29]),
            "branches merge on the way down ({} at the top, {} at the bed)",
            islands(&out.branches[29]),
            islands(&out.branches[0])
        );
        assert!(!out.supported_contacts[30].is_empty(), "the pad is held");
    }

    #[test]
    fn branches_never_enter_the_model_or_its_clearance() {
        // A pad over a block standing in the middle of the bed: the branches
        // under the pad must find their way around the block.
        let mut scene = Scene::new(41);
        scene.model(0..20, square(0.0, 0.0, 3.0));
        scene.contacts[40] = square(0.0, 0.0, 8.0);
        let mut s = settings();
        s.rest_on_model = false;
        let out = scene.grow(&s);
        assert!(area(&out.branches[0]) > 0.0, "the tree reaches the bed");
        for layer in 0..41 {
            let keep_out = poly_inflate(&scene.footprints[layer], s.xy - 0.01);
            let inside = super::super::supports::poly_intersect(&out.branches[layer], &keep_out);
            assert!(
                area(&inside) < 1e-3,
                "layer {layer}: branches overlap the model's clearance by {:.4} mm²",
                area(&inside)
            );
        }
    }

    #[test]
    fn a_branch_with_no_way_down_rests_on_the_model_or_is_dropped() {
        // A pad directly over a wide slab: no branch can reach the bed.
        let mut scene = Scene::new(41);
        scene.model(0..10, square(0.0, 0.0, 30.0));
        scene.contacts[40] = square(0.0, 0.0, 4.0);

        let resting = scene.grow(&settings());
        assert!(
            area(&resting.branches[20]) > 0.0,
            "branches stand on the slab"
        );
        assert!(!resting.supported_contacts[40].is_empty());

        let mut plate_only = settings();
        plate_only.rest_on_model = false;
        let dropped = scene.grow(&plate_only);
        assert!(
            dropped.branches.iter().all(|b| b.is_empty()),
            "with build-plate-only nothing may stand on the slab"
        );
        assert!(
            dropped.supported_contacts[40].is_empty(),
            "an unheld pad is not printed"
        );
    }

    #[test]
    fn branches_thicken_toward_the_bed() {
        let mut scene = Scene::new(101);
        scene.contacts[100] = square(0.0, 0.0, 0.5);
        let out = scene.grow(&settings());
        let top = area(&out.branches[99]);
        let low = area(&out.branches[10]);
        assert!(
            top > 0.0 && low > top * 2.0,
            "trunk {low:.2} mm² vs tip {top:.2} mm²"
        );
    }

    #[test]
    fn the_tree_is_deterministic() {
        let mut scene = Scene::new(31);
        scene.model(0..15, square(4.0, 0.0, 2.0));
        scene.contacts[30] = square(0.0, 0.0, 6.0);
        let a = scene.grow(&settings());
        let b = scene.grow(&settings());
        for layer in 0..31 {
            let pa: Vec<Vec<(f64, f64)>> = a.branches[layer]
                .iter()
                .map(|p| p.iter().map(|q| (q.x(), q.y())).collect())
                .collect();
            let pb: Vec<Vec<(f64, f64)>> = b.branches[layer]
                .iter()
                .map(|p| p.iter().map(|q| (q.x(), q.y())).collect())
                .collect();
            assert_eq!(pa, pb, "layer {layer} differs between runs");
        }
    }
}
