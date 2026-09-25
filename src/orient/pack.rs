//! Shape-aware nesting: pack objects on the bed by the space they really take.
//!
//! ## Why not bounding boxes, and why not shadows either
//!
//! A bounding box is a bad description of a printed part — an L-bracket, a
//! bracket arm, anything posed at 45° is mostly air inside its box, so a packer
//! that reasons about boxes reserves that air and leaves the plate half empty.
//!
//! Packing the part's *shadow* — the outline it casts on the plate — fixes
//! that, and then stops one step short. On a plate printed a layer at a time,
//! the nozzle is always at the height of the tallest thing printed so far, so
//! two parts may share plate area outright as long as they do not want the same
//! height there. A part leaning at 45° hangs over its neighbour's foot; a small
//! part tucks in under a flared rim. That is the packing this module does, and
//! it is why parts here fit where a shadow packer says they cannot.
//!
//! ## What an object is, here
//!
//! Each object is rasterised into a grid of the bed, and every occupied column
//! keeps the **vertical span** the part wants there: `base` (its lowest
//! surface) to `top` (its highest). Two parts may share a column when their
//! spans are apart by the clearance; otherwise they collide. The span is kept
//! in millimetres rather than voxels, so nothing is lost to layering, and it is
//! read from the piece of each triangle that stands over the cell — an upright
//! wall of a leaning part claims only the heights it really spans there.
//!
//! Two rules keep that honest:
//!
//! - **A column is only free below the part when the underside there holds
//!   itself up.** A 45° underside prints over thin air, so the space under it is
//!   genuinely empty; a flat shelf grows support columns down to the plate, and
//!   those are as solid as the part. Undersides shallower than
//!   [`PackOptions::overhang_threshold_deg`] therefore reserve everything below
//!   them, exactly as a shadow would.
//! - **Two parts that meet overhead keep one order.** A part under its
//!   neighbour on one side and over it on the other prints fine and comes off
//!   the plate as one interlocked lump, so it is refused — as is any loop of
//!   parts each over the next. The top part can always be lifted off first.
//!
//! ## How a part is placed
//!
//! Everything about the plate is bitwise, which is what makes trying every
//! position affordable:
//!
//! 1. **The bed is an occupancy map.** Every cell not fully on the plate starts
//!    occupied, so "does it fit on the bed" is the same test as "does it hit
//!    something", and a round bed needs no inscribed square.
//! 2. **Largest first**, so the parts with the fewest choices choose first.
//! 3. **Every rotation, every position.** Positions are visited in rings
//!    outwards, and the search stops once the next ring cannot beat the best
//!    hit — which makes "nearest" exact rather than first-fit.
//! 4. **A clear outline is an instant yes.** A candidate that overlaps
//!    something is next asked two bitmap questions — do both stand on the plate
//!    there, and for a part with no headroom, is every underside it meets high
//!    enough — and only an irregular part that neither settles pays for the
//!    column-by-column height test.
//! 5. **The corner is a window's corner.** The plate is packed inside the
//!    smallest copy of its own outline, shrunk about the centre, that still
//!    takes every part — see [`pack_objects`]. A light plate becomes one compact
//!    block mid-plate instead of a row along one edge, and a tight window is
//!    what pushes small parts in under their neighbours' overhangs.
//!
//! Rasterisation is conservative: a cell is occupied if a triangle touches it,
//! and a bed cell is printable only if all of it is on the plate.
//!
//! ## Rotation
//!
//! [`PackOptions::rotation_step_deg`] defaults to 90°, which is free: a quarter
//! turn cannot undo an auto-orient result, and it preserves a printer's
//! preferred Z-rotation — a machine asking for 45° still gets a diagonal part
//! afterwards. Finer steps nest tighter at the cost of turning parts off the
//! angle the user chose.

use crate::mesh::types::Mesh;
use crate::scene::bed::BedConfig;
use crate::scene::transform::Transform;
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

/// Longest bed axis is divided into at most this many cells.
///
/// 512 cells across a 220 mm bed is a 0.43 mm cell — finer than the placement
/// error anyone can see, and a grid row still fits in 8 machine words, which is
/// what keeps the collision test a handful of `AND`s.
const MAX_GRID_CELLS: usize = 512;

/// Cells never get finer than this, so a 1 m bed does not turn the search into
/// a million-cell sweep for no visible gain.
const MIN_CELL_MM: f64 = 0.4;

/// How much more a cell costs for sitting deep in the plate than wide across
/// it, when the packer measures a candidate spot.
///
/// Anything above 1 makes the nest fill in rows rather than grow as a quarter
/// disc from the corner, and rows leave fewer unusable slivers. Pushed much
/// higher it degenerates into strict bottom-left fill and gives the gain back,
/// so this is the middle of the range that measured best over a corpus of real
/// parts — not a derived number.
const DEPTH_BIAS: f64 = 1.5;

/// Least vertical clearance between two parts sharing plate area (mm).
///
/// The gap the user asks for is a horizontal one, and on a plate it can
/// sensibly be zero; overhead is different. A part passing over another wants
/// room for the lower one to curl, and for the higher one's first unsupported
/// layer to droop, before the two touch.
const MIN_VERTICAL_GAP_MM: f64 = 1.0;

/// Below this (mm) a part counts as standing on the plate rather than hanging.
const ON_PLATE_EPS_MM: f32 = 0.05;

/// One placed object returned by [`pack_objects`].
#[derive(Debug, Clone)]
pub struct Placement {
    /// Index into the input slice.
    pub index: usize,
    /// Rotation about Z applied to the footprint, in degrees.
    pub angle_deg: f64,
    /// Where the footprint's pivot must land, in scene millimetres.
    ///
    /// The pivot is the centre of the object's world XY bounds *before* the
    /// rotation, and the rotation turns the object about it — so a caller turns
    /// the object about that point and then moves the point here.
    pub pivot: [f64; 2],
    /// `false` when the object did not fit on the plate and was parked beside
    /// it. Nothing is discarded; the caller decides how to say so.
    pub on_bed: bool,
}

/// Knobs for [`pack_objects`].
#[derive(Debug, Clone, Copy)]
pub struct PackOptions {
    /// Gap left between two parts that share a height, in millimetres.
    pub spacing_mm: f64,
    /// Rotation step tried per object, in degrees. `0` (or ≥ 360) packs every
    /// object at the angle it arrived in.
    pub rotation_step_deg: f64,
    /// May a part take plate area above or below another part?
    ///
    /// True for a plate printed a layer at a time, where the nozzle never
    /// visits a height below what is already printed. False for sequential
    /// printing, where the gantry drives past finished parts and every part
    /// needs its own column of air from the plate upwards.
    pub vertical_nesting: bool,
    /// Underside angle, in degrees from vertical, past which a part is taken to
    /// need support down to the plate — and so to own the space beneath it.
    /// Same convention and default as the process's support threshold.
    pub overhang_threshold_deg: f64,
}

impl Default for PackOptions {
    fn default() -> Self {
        Self {
            spacing_mm: 2.0,
            rotation_step_deg: 90.0,
            vertical_nesting: true,
            overhang_threshold_deg: 45.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Footprint
// ---------------------------------------------------------------------------

/// An object's triangles in plate coordinates, ready to be rasterised.
///
/// Built once per object from the mesh and its current transform, then
/// rasterised again per rotation candidate — transforming is the expensive part
/// and it does not depend on the angle.
#[derive(Debug, Clone)]
pub struct Footprint {
    /// Triangles: XY relative to [`pivot`](Self::pivot), Z above the plate.
    tris: Vec<[[f32; 3]; 3]>,
    /// Centre of the object's world XY bounds, in scene millimetres.
    pub pivot: [f64; 2],
    /// Width and depth of the footprint's bounds (mm), used to order objects.
    pub extent: [f64; 2],
    /// Height above the plate (mm).
    pub height: f64,
}

impl Footprint {
    /// Project `mesh` through `transform` into plate coordinates.
    pub fn new(mesh: &Mesh, transform: &Transform) -> Self {
        let matrix = transform.to_matrix();
        let mut points: Vec<[f32; 3]> = Vec::with_capacity(mesh.faces.len() * 3);
        for face in &mesh.faces {
            for v in &face.vertices {
                let p =
                    matrix.transform_point3(glam::Vec3::new(v.x as f32, v.y as f32, v.z as f32));
                points.push([p.x, p.y, p.z]);
            }
        }

        let (mut min_x, mut min_y, mut min_z) = (f32::INFINITY, f32::INFINITY, f32::INFINITY);
        let (mut max_x, mut max_y, mut max_z) =
            (f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for p in &points {
            min_x = min_x.min(p[0]);
            min_y = min_y.min(p[1]);
            min_z = min_z.min(p[2]);
            max_x = max_x.max(p[0]);
            max_y = max_y.max(p[1]);
            max_z = max_z.max(p[2]);
        }
        if !min_x.is_finite() {
            return Self {
                tris: Vec::new(),
                pivot: [0.0, 0.0],
                extent: [0.0, 0.0],
                height: 0.0,
            };
        }

        let pivot = [
            (min_x as f64 + max_x as f64) / 2.0,
            (min_y as f64 + max_y as f64) / 2.0,
        ];
        let (px, py) = (pivot[0] as f32, pivot[1] as f32);
        // Heights are measured from the part's own floor, so a model resting on
        // the plate reads as base 0 whatever noise its coordinates carry.
        let floor = min_z;
        let tris = points
            .as_chunks::<3>()
            .0
            .iter()
            .filter_map(|t| {
                let shift = |p: [f32; 3]| [p[0] - px, p[1] - py, p[2] - floor];
                let (a, b, c) = (shift(t[0]), shift(t[1]), shift(t[2]));
                if a == b && b == c {
                    None
                } else {
                    Some([a, b, c])
                }
            })
            .collect();

        Self {
            tris,
            pivot,
            extent: [max_x as f64 - min_x as f64, max_y as f64 - min_y as f64],
            height: max_z as f64 - min_z as f64,
        }
    }

    fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    /// Identity of the *shape*, ignoring where it sits.
    ///
    /// A plate is usually one model many times over, and rasterising the same
    /// geometry once per copy per angle is the bulk of the work. Two footprints
    /// with the same fingerprint rasterise identically, so the work is done
    /// once and shared.
    fn fingerprint(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.tris.len().hash(&mut hasher);
        for t in &self.tris {
            for p in t {
                p[0].to_bits().hash(&mut hasher);
                p[1].to_bits().hash(&mut hasher);
                p[2].to_bits().hash(&mut hasher);
            }
        }
        hasher.finish()
    }
}
// ---------------------------------------------------------------------------
// Bit grid
// ---------------------------------------------------------------------------

/// A row-major bitmap, one bit per grid cell, packed into 64-bit words.
#[derive(Debug, Clone)]
struct BitGrid {
    cols: usize,
    rows: usize,
    /// Words per row.
    stride: usize,
    bits: Vec<u64>,
}

impl BitGrid {
    fn new(cols: usize, rows: usize) -> Self {
        let stride = cols.div_ceil(64).max(1);
        Self {
            cols,
            rows,
            stride,
            bits: vec![0; stride * rows.max(1)],
        }
    }

    #[inline]
    fn row(&self, y: usize) -> &[u64] {
        &self.bits[y * self.stride..(y + 1) * self.stride]
    }

    #[inline]
    fn set(&mut self, x: usize, y: usize) {
        self.bits[y * self.stride + (x >> 6)] |= 1u64 << (x & 63);
    }

    #[inline]
    fn get(&self, x: usize, y: usize) -> bool {
        self.bits[y * self.stride + (x >> 6)] >> (x & 63) & 1 == 1
    }

    /// Number of set cells.
    fn count(&self) -> usize {
        self.bits.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// Does `other`, placed with its cell (0,0) at `(ox, oy)`, hit a set bit?
    ///
    /// `other` must fit inside `self` at that offset; callers check that when
    /// they enumerate positions.
    fn collides(&self, other: &BitGrid, ox: usize, oy: usize) -> bool {
        let shift = ox & 63;
        let word0 = ox >> 6;
        for y in 0..other.rows {
            let src = other.row(y);
            let dst = self.row(oy + y);
            for (i, &m) in src.iter().enumerate() {
                if m == 0 {
                    continue;
                }
                let lo = dst[word0 + i];
                let window = if shift == 0 {
                    lo
                } else {
                    let hi = dst.get(word0 + i + 1).copied().unwrap_or(0);
                    (lo >> shift) | (hi << (64 - shift))
                };
                if window & m != 0 {
                    return true;
                }
            }
        }
        false
    }

    /// OR `other` into `self` with its cell (0,0) at signed `(ox, oy)`,
    /// dropping whatever falls outside.
    ///
    /// Signed and clipping because this is how a *dilated* mask is committed:
    /// its spacing halo routinely hangs off the edge of the plate, which is
    /// harmless — those cells were never printable in the first place.
    fn or_clipped(&mut self, other: &BitGrid, ox: isize, oy: isize) {
        for y in 0..other.rows {
            let ty = oy + y as isize;
            if ty < 0 || ty >= self.rows as isize {
                continue;
            }
            let ty = ty as usize;
            for (i, &word) in other.row(y).iter().enumerate() {
                let mut w = word;
                while w != 0 {
                    let bit = w.trailing_zeros() as usize;
                    w &= w - 1;
                    let tx = ox + (i * 64 + bit) as isize;
                    if tx >= 0 && tx < self.cols as isize {
                        self.set(tx as usize, ty);
                    }
                }
            }
        }
    }
}

/// Shift a packed row left (towards higher x) by one bit.
fn shl1(row: &mut [u64]) {
    let mut carry = 0u64;
    for word in row.iter_mut() {
        let next = *word >> 63;
        *word = (*word << 1) | carry;
        carry = next;
    }
}

/// Shift a packed row right (towards lower x) by one bit.
fn shr1(row: &mut [u64]) {
    let mut carry = 0u64;
    for word in row.iter_mut().rev() {
        let next = *word << 63;
        *word = (*word >> 1) | carry;
        carry = next;
    }
}

/// Grow `mask` by a disc of `k` cells, returning the padded result.
///
/// The padded mask is `k` cells larger on every side, so its cell (0,0) sits
/// at the original's `(-k, -k)`.
fn dilate(mask: &BitGrid, k: usize) -> BitGrid {
    if k == 0 || mask.rows == 0 {
        return mask.clone();
    }
    let mut out = BitGrid::new(mask.cols + 2 * k, mask.rows + 2 * k);
    let mut smear = vec![0u64; out.stride];
    for y in 0..mask.rows {
        // Seed the smear with the source row shifted right by k cells.
        smear.iter_mut().for_each(|w| *w = 0);
        let mut any = false;
        for (i, &word) in mask.row(y).iter().enumerate() {
            let mut w = word;
            any |= w != 0;
            while w != 0 {
                let bit = w.trailing_zeros() as usize;
                w &= w - 1;
                let x = i * 64 + bit + k;
                smear[x >> 6] |= 1u64 << (x & 63);
            }
        }
        if !any {
            continue;
        }
        // Widen the smear one cell at a time, walking dy inwards so the disc
        // radius — and with it the required width — only ever grows.
        let mut width = 0usize;
        for dy in (0..=k).rev() {
            let target = ((k * k - dy * dy) as f64).sqrt().floor() as usize;
            while width < target {
                let mut left = smear.clone();
                let mut right = smear.clone();
                shl1(&mut left);
                shr1(&mut right);
                for i in 0..smear.len() {
                    smear[i] |= left[i] | right[i];
                }
                width += 1;
            }
            for ty in [y + k + dy, y + k - dy] {
                let row = &mut out.bits[ty * out.stride..(ty + 1) * out.stride];
                for (i, &w) in smear.iter().enumerate() {
                    row[i] |= w;
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Shapes — the grid, plus the height each column claims
// ---------------------------------------------------------------------------

/// A footprint rasterised at one angle: which columns it occupies, and the
/// vertical span it wants in each.
#[derive(Clone)]
struct Shape {
    grid: BitGrid,
    /// Lowest surface per cell (mm above the plate). `0` where the part reaches
    /// the plate, and where it reserves its way down for support.
    base: Vec<f32>,
    /// Highest surface per cell (mm above the plate).
    top: Vec<f32>,
    /// Offset of cell (0,0)'s corner from the footprint pivot, in mm.
    offset: [f64; 2],
    /// Does any column start above the plate? Only a part with headroom can
    /// nest over or under another, which is what lets the common case skip the
    /// per-column test entirely.
    has_headroom: bool,
    /// Columns that stand on the plate — `base` 0. Two of those can never
    /// share a column whatever their heights, so this is the bitmap that turns
    /// most candidate spots away before a single height is read.
    floor: BitGrid,
    /// Identity of the rasterised shape, ignoring where its grid sits. A part
    /// that looks the same after a quarter turn — a cube, a cylinder — is
    /// searched once rather than four times.
    form: u64,
    /// Lowest and highest column top (mm). For a part with no headroom they
    /// are all the height test needs — see [`Plate::probe`].
    tops: [f32; 2],
    /// `grid` and `floor` as runs of set cells per row, which is what lets a
    /// search test a whole row of positions at once — see [`row_hits`].
    runs: Runs,
    floor_runs: Runs,
}

/// Per row, each run of set cells as `(first cell, length - 1)`.
type Runs = Vec<Vec<(u32, u32)>>;

/// `grid`'s rows as runs of set cells.
fn runs_of(grid: &BitGrid) -> Runs {
    (0..grid.rows)
        .map(|y| {
            let mut out = Vec::new();
            let mut start: Option<usize> = None;
            for x in 0..=grid.cols {
                let on = x < grid.cols && grid.get(x, y);
                match (on, start) {
                    (true, None) => start = Some(x),
                    (false, Some(a)) => {
                        out.push((a as u32, (x - 1 - a) as u32));
                        start = None;
                    }
                    _ => {}
                }
            }
            out
        })
        .collect()
}

impl Shape {
    /// The solid box around this shape, from the plate to its highest point.
    fn boxed(&self) -> Self {
        let (cols, rows) = (self.grid.cols, self.grid.rows);
        let height = self
            .top
            .iter()
            .copied()
            .filter(|t| t.is_finite())
            .fold(0.0_f32, f32::max);
        let mut grid = BitGrid::new(cols, rows);
        for y in 0..rows {
            for x in 0..cols {
                grid.set(x, y);
            }
        }
        Shape {
            grid,
            base: vec![0.0; cols * rows],
            top: vec![height; cols * rows],
            offset: self.offset,
            has_headroom: false,
            floor: BitGrid::new(0, 0),
            form: 0,
            tops: [0.0, 0.0],
            runs: Vec::new(),
            floor_runs: Vec::new(),
        }
        .seal()
    }

    /// Fill in what is derived from the columns: `floor` and `form`.
    fn seal(mut self) -> Self {
        let (cols, rows) = (self.grid.cols, self.grid.rows);
        let mut floor = BitGrid::new(cols, rows);
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (cols, rows).hash(&mut hasher);
        self.grid.bits.hash(&mut hasher);
        for y in 0..rows {
            for x in 0..cols {
                if !self.grid.get(x, y) {
                    continue;
                }
                let i = self.idx(x, y);
                if self.base[i] <= ON_PLATE_EPS_MM {
                    floor.set(x, y);
                }
                self.base[i].to_bits().hash(&mut hasher);
                self.top[i].to_bits().hash(&mut hasher);
            }
        }
        self.floor = floor;
        self.form = hasher.finish();
        self.tops = self
            .top
            .iter()
            .filter(|t| t.is_finite())
            .fold([f32::INFINITY, f32::NEG_INFINITY], |[lo, hi], &t| {
                [lo.min(t), hi.max(t)]
            });
        self.runs = runs_of(&self.grid);
        self.floor_runs = runs_of(&self.floor);
        self
    }

    #[inline]
    fn idx(&self, x: usize, y: usize) -> usize {
        y * self.grid.cols + x
    }
}

/// Rasterise `fp` rotated by `angle_deg` into cells of `cell` mm.
fn rasterise(fp: &Footprint, angle_deg: f64, cell: f64, options: &PackOptions) -> Shape {
    let (sin, cos) = angle_deg.to_radians().sin_cos();
    let (sin, cos) = (sin as f32, cos as f32);
    let rotate = |p: [f32; 3]| [p[0] * cos - p[1] * sin, p[0] * sin + p[1] * cos, p[2]];

    let tris: Vec<[[f32; 3]; 3]> = fp
        .tris
        .iter()
        .map(|t| [rotate(t[0]), rotate(t[1]), rotate(t[2])])
        .collect();

    let (mut min_x, mut min_y) = (f32::INFINITY, f32::INFINITY);
    let (mut max_x, mut max_y) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for t in &tris {
        for p in t {
            min_x = min_x.min(p[0]);
            min_y = min_y.min(p[1]);
            max_x = max_x.max(p[0]);
            max_y = max_y.max(p[1]);
        }
    }
    if !min_x.is_finite() {
        return Shape {
            grid: BitGrid::new(0, 0),
            base: Vec::new(),
            top: Vec::new(),
            offset: [0.0, 0.0],
            has_headroom: false,
            floor: BitGrid::new(0, 0),
            form: 0,
            tops: [0.0, 0.0],
            runs: Vec::new(),
            floor_runs: Vec::new(),
        };
    }

    // Snap the origin to a cell boundary below the footprint so a part's
    // rasterisation never depends on where it happened to sit.
    let origin = [
        (min_x as f64 / cell).floor() * cell,
        (min_y as f64 / cell).floor() * cell,
    ];
    let cols = (((max_x as f64 - origin[0]) / cell).ceil() as usize + 1).max(1);
    let rows = (((max_y as f64 - origin[1]) / cell).ceil() as usize + 1).max(1);
    let mut shape = Shape {
        grid: BitGrid::new(cols, rows),
        base: vec![f32::INFINITY; cols * rows],
        top: vec![f32::NEG_INFINITY; cols * rows],
        offset: origin,
        has_headroom: false,
        floor: BitGrid::new(0, 0),
        form: 0,
        tops: [0.0, 0.0],
        runs: Vec::new(),
        floor_runs: Vec::new(),
    };

    // A face leaning further from vertical than this needs support under it.
    // Measured on the face's own normal, which is what tells a 45° flank from
    // a flat shelf beside a post: the height step from post to shelf looks
    // steep, the shelf is not. A face exactly on the threshold counts as
    // printable, so a 45° flank against the stock 45° does not turn on the last
    // bit of rounding.
    let steepest_flat = options
        .overhang_threshold_deg
        .clamp(1.0, 89.0)
        .to_radians()
        .sin() as f32
        + 1e-4;
    let mut flat = vec![false; cols * rows];

    let half = (cell / 2.0) as f32;
    for t in &tris {
        let tmin_x = t[0][0].min(t[1][0]).min(t[2][0]);
        let tmax_x = t[0][0].max(t[1][0]).max(t[2][0]);
        let tmin_y = t[0][1].min(t[1][1]).min(t[2][1]);
        let tmax_y = t[0][1].max(t[1][1]).max(t[2][1]);
        let x0 = (((tmin_x as f64 - origin[0]) / cell).floor().max(0.0)) as usize;
        let x1 = ((((tmax_x as f64 - origin[0]) / cell).floor()) as usize).min(cols - 1);
        let y0 = (((tmin_y as f64 - origin[1]) / cell).floor().max(0.0)) as usize;
        let y1 = ((((tmax_y as f64 - origin[1]) / cell).floor()) as usize).min(rows - 1);
        let needs_support = face_tilt_cos(t) > steepest_flat;
        for cy in y0..=y1 {
            let center_y = (origin[1] + (cy as f64 + 0.5) * cell) as f32;
            for cx in x0..=x1 {
                let center_x = (origin[0] + (cx as f64 + 0.5) * cell) as f32;
                if !triangle_hits_cell(t, [center_x, center_y], half) {
                    continue;
                }
                // How high this triangle reaches over this one cell: the
                // piece of it standing over the cell, not its whole range. An
                // upright wall of a leaning part is the case that matters — it
                // is a slanted parallelogram, and claiming its full height in
                // every column would fill in the very space its lean leaves.
                let Some((lo, hi)) = z_over_cell(
                    t,
                    [center_x - half, center_y - half],
                    [center_x + half, center_y + half],
                ) else {
                    continue;
                };
                let i = cy * cols + cx;
                let lo = lo.max(0.0);
                shape.grid.set(cx, cy);
                // The column's underside is whichever face is lowest over it.
                // On a tie the steeper one decides: two faces meet at the same
                // height only along an edge, and at the rim of a flared part
                // that edge has the flat top on it — which is not an underside.
                // A flat underside still cannot free anything this way, because
                // `reserve_support_space` also wants a chain of steep columns
                // down to the plate.
                if lo < shape.base[i] - ON_PLATE_EPS_MM || !shape.base[i].is_finite() {
                    flat[i] = needs_support;
                } else if lo <= shape.base[i] + ON_PLATE_EPS_MM {
                    flat[i] &= needs_support;
                }
                shape.base[i] = shape.base[i].min(lo);
                shape.top[i] = shape.top[i].max(hi.max(0.0));
            }
        }
    }

    reserve_support_space(&mut shape, &flat, options);
    shape.seal()
}

/// How close to horizontal a triangle lies: `|cos|` of the angle between its
/// normal and the vertical. `1` for a flat face, `0` for a wall — and so, for a
/// face tilted `θ` from vertical, `sin θ`, the number the support threshold is
/// compared against. Unsigned, because winding is not trusted to say which
/// side is down; the lowest face over a column is the underside either way.
fn face_tilt_cos(t: &[[f32; 3]; 3]) -> f32 {
    let u = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
    let v = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len <= f32::EPSILON {
        return 0.0;
    }
    n[2].abs() / len
}

/// Lowest and highest point of the part of `tri` standing over the square
/// `lo..hi`, or `None` if none of it does.
///
/// The triangle is clipped to the square's four sides and the heights read off
/// what is left — exact for a face at any angle, upright walls included.
fn z_over_cell(tri: &[[f32; 3]; 3], lo: [f32; 2], hi: [f32; 2]) -> Option<(f32, f32)> {
    // A triangle clipped by four lines has at most seven corners.
    let mut poly = [[0f32; 3]; 8];
    let mut next = [[0f32; 3]; 8];
    poly[..3].copy_from_slice(tri);
    let mut len = 3;
    // Each side as (axis, bound, keep points on the low side?).
    for (axis, bound, below) in [
        (0, lo[0], false),
        (0, hi[0], true),
        (1, lo[1], false),
        (1, hi[1], true),
    ] {
        let inside = |p: &[f32; 3]| {
            if below {
                p[axis] <= bound
            } else {
                p[axis] >= bound
            }
        };
        let mut n = 0;
        for i in 0..len {
            let (a, b) = (poly[i], poly[(i + 1) % len]);
            let (ia, ib) = (inside(&a), inside(&b));
            if ia {
                next[n] = a;
                n += 1;
            }
            if ia != ib {
                let t = (bound - a[axis]) / (b[axis] - a[axis]);
                next[n] = [
                    a[0] + (b[0] - a[0]) * t,
                    a[1] + (b[1] - a[1]) * t,
                    a[2] + (b[2] - a[2]) * t,
                ];
                n += 1;
            }
        }
        if n == 0 {
            return None;
        }
        std::mem::swap(&mut poly, &mut next);
        len = n;
    }
    let (mut lo_z, mut hi_z) = (f32::INFINITY, f32::NEG_INFINITY);
    for p in &poly[..len] {
        lo_z = lo_z.min(p[2]);
        hi_z = hi_z.max(p[2]);
    }
    Some((lo_z, hi_z))
}

/// Separating-axis test between a triangle's XY projection and a cell square.
///
/// The two axis-aligned axes are already covered by the caller's bbox walk, so
/// only the triangle's three edge normals are left.
fn triangle_hits_cell(tri: &[[f32; 3]; 3], center: [f32; 2], half: f32) -> bool {
    for i in 0..3 {
        let a = tri[i];
        let b = tri[(i + 1) % 3];
        let axis = [-(b[1] - a[1]), b[0] - a[0]];
        if axis[0] == 0.0 && axis[1] == 0.0 {
            continue;
        }
        let mut tmin = f32::INFINITY;
        let mut tmax = f32::NEG_INFINITY;
        for p in tri {
            let d = p[0] * axis[0] + p[1] * axis[1];
            tmin = tmin.min(d);
            tmax = tmax.max(d);
        }
        let c = center[0] * axis[0] + center[1] * axis[1];
        let r = half * (axis[0].abs() + axis[1].abs());
        if c + r < tmin || c - r > tmax {
            return false;
        }
    }
    true
}

/// Pull `base` down to the plate wherever the part cannot hold itself up there.
///
/// Support material is as solid as the part it carries, so a column that will
/// grow a support tower owns the space under it and nothing may nest there. A
/// column holds itself up when two things are true: the face under it is steep
/// enough to print over air (`flat` is false), and it steps down to a lower
/// column that holds itself up too, and so on to the plate. The second half is
/// what catches the lowest tip of a hanging part, which always needs support
/// however steep its sides.
///
/// The walk runs low columns first, so each one is decided against neighbours
/// that are already settled; an isolated flat shelf never reaches the plate and
/// keeps nothing.
///
/// With `vertical_nesting` off, no column keeps its headroom: every part gets
/// its own air from the plate to its roof.
fn reserve_support_space(shape: &mut Shape, flat: &[bool], options: &PackOptions) {
    let (cols, rows) = (shape.grid.cols, shape.grid.rows);
    if cols == 0 || rows == 0 {
        return;
    }

    if !options.vertical_nesting {
        for slot in shape.base.iter_mut() {
            if slot.is_finite() {
                *slot = 0.0;
            }
        }
        shape.has_headroom = false;
        return;
    }

    let mut order: Vec<u32> = (0..(cols * rows) as u32)
        .filter(|&i| shape.base[i as usize].is_finite())
        .collect();
    order.sort_unstable_by(|&a, &b| {
        shape.base[a as usize]
            .partial_cmp(&shape.base[b as usize])
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut grounded = vec![false; cols * rows];
    for &cell_index in &order {
        let i = cell_index as usize;
        let (x, y) = (i % cols, i / cols);
        let here = shape.base[i];
        if here <= ON_PLATE_EPS_MM {
            grounded[i] = true;
            continue;
        }
        if flat[i] {
            continue;
        }
        let mut neighbours = [None; 4];
        if x > 0 {
            neighbours[0] = Some(i - 1);
        }
        if x + 1 < cols {
            neighbours[1] = Some(i + 1);
        }
        if y > 0 {
            neighbours[2] = Some(i - cols);
        }
        if y + 1 < rows {
            neighbours[3] = Some(i + cols);
        }
        grounded[i] = neighbours
            .iter()
            .flatten()
            .any(|&n| grounded[n] && shape.base[n] < here);
    }

    let mut headroom = false;
    for (base, &held) in shape.base.iter_mut().zip(&grounded) {
        if !base.is_finite() {
            continue;
        }
        if held && *base > ON_PLATE_EPS_MM {
            headroom = true;
        } else {
            *base = 0.0;
        }
    }
    shape.has_headroom = headroom;
}

// ---------------------------------------------------------------------------
// The plate
// ---------------------------------------------------------------------------

/// A part already on the plate, in plate-grid coordinates and already grown by
/// the spacing, so a candidate can be tested against it as it stands.
struct Placed {
    /// Columns within the spacing of the part.
    grid: BitGrid,
    /// Lowest surface over that neighbourhood (mm), per cell of `grid`.
    base: Vec<f32>,
    /// Highest surface over that neighbourhood (mm), per cell of `grid`.
    top: Vec<f32>,
    /// Plate cell the grid's (0,0) sits on. Negative near the plate's edge.
    ox: isize,
    oy: isize,
    has_headroom: bool,
}

impl Placed {
    /// Could a candidate of this size at `(ix, iy)` touch this part at all?
    ///
    /// A plate has tens of parts and a search visits thousands of positions, so
    /// the boxes are worth asking before the columns.
    #[inline]
    fn near(&self, ix: isize, iy: isize, cols: usize, rows: usize) -> bool {
        ix < self.ox + self.grid.cols as isize
            && ix + cols as isize > self.ox
            && iy < self.oy + self.grid.rows as isize
            && iy + rows as isize > self.oy
    }
}

/// The plate under construction: what is off the bed, what is taken, and by what.
struct Plate {
    /// Cells not fully on the printable bed.
    off_bed: BitGrid,
    /// Off the bed, or within spacing of something placed. A candidate clear of
    /// this needs no further checking — which is the common case, and the
    /// reason the height test costs nothing on an ordinary plate.
    blocked: BitGrid,
    placed: Vec<Placed>,
    /// Columns within the spacing of a placed part's own floor — its foot, and
    /// wherever it reserves the space down to the plate for support. Nothing
    /// that also stands on the plate may go there.
    floor: BitGrid,
    /// Columns where a placed part's underside is below a height, per height
    /// in tenths of a millimetre. Built on demand and dropped whenever a part
    /// is placed; see [`Plate::fits`].
    below: RefCell<HashMap<u32, Rc<BitGrid>>>,
    /// Does anything placed have headroom? Until something does, a part with
    /// none of its own can only pass something by missing it.
    any_headroom: bool,
    /// `over[a]` lists the parts that pass over part `a` somewhere. Kept free
    /// of cycles, so there is always a part that can be lifted off first.
    over: Vec<Vec<usize>>,
    /// Vertical clearance demanded of two parts sharing a column (mm).
    vertical_gap: f32,
}

impl Plate {
    fn new(off_bed: BitGrid, vertical_gap: f32) -> Self {
        Self {
            blocked: off_bed.clone(),
            floor: BitGrid::new(off_bed.cols, off_bed.rows),
            below: RefCell::new(HashMap::new()),
            any_headroom: false,
            off_bed,
            placed: Vec::new(),
            over: Vec::new(),
            vertical_gap,
        }
    }

    /// Can `shape` sit with its cell (0,0) on plate cell `(ix, iy)`?
    #[cfg(test)]
    fn fits(&self, shape: &Shape, ix: usize, iy: usize) -> bool {
        self.probe(shape).fits(ix, iy)
    }

    /// Everything about the plate that one search for `shape` reads over and
    /// over, fetched once.
    fn probe<'a>(&'a self, shape: &'a Shape) -> Probe<'a> {
        // A part with no headroom stands on the plate everywhere, so it can
        // only ever pass under its neighbours — and then two bitmaps settle
        // it. Under a column whose underside is lower than its shortest column
        // plus the gap, it cannot go; if every underside it meets clears its
        // tallest column, it fits. Only a part of uneven height falls between
        // the two and pays for the column-by-column test.
        let levels = (!shape.has_headroom).then(|| {
            let [short, tall] = shape.tops;
            (
                self.undersides_below(short + self.vertical_gap, false),
                self.undersides_below(tall + self.vertical_gap, true),
            )
        });
        Probe {
            plate: self,
            shape,
            levels,
        }
    }

    /// The placed parts `shape` at `(ix, iy)` would pass under, and those it
    /// would pass over — or `None` if it cannot sit there at all.
    ///
    /// Two parts that share columns keep one order across all of them. A part
    /// under its neighbour on one side and over it on the other prints fine and
    /// comes off the plate as one interlocked lump; so does a loop of three
    /// parts each over the next. Refusing both means the top part can always
    /// be lifted straight off, then the next, and so on down.
    fn stacking(&self, shape: &Shape, ix: usize, iy: usize) -> Option<(Vec<usize>, Vec<usize>)> {
        let (ix, iy) = (ix as isize, iy as isize);
        let (mut under, mut over) = (Vec::new(), Vec::new());
        for (i, p) in self.placed.iter().enumerate() {
            if !p.near(ix, iy, shape.grid.cols, shape.grid.rows) {
                continue;
            }
            match meet(p, shape, ix, iy, self.vertical_gap) {
                Meet::Apart => {}
                Meet::Below => under.push(i),
                Meet::Above => over.push(i),
                Meet::Clash => return None,
            }
        }
        // Everything in `under` sits over the candidate, and the candidate sits
        // over everything in `over`. Climbing from a part above the candidate
        // and arriving at a part below it would close a loop.
        if !under.is_empty() && !over.is_empty() && self.climbs(&under, &over) {
            return None;
        }
        Some((under, over))
    }

    /// Every column where some placed part's underside is lower than `height`.
    ///
    /// Heights are kept to a tenth of a millimetre; `round_up` says which way
    /// to round so the answer errs towards a collision either way it is used.
    fn undersides_below(&self, height: f32, round_up: bool) -> Rc<BitGrid> {
        let tenths = height * 10.0;
        let key = if round_up {
            tenths.ceil()
        } else {
            tenths.floor()
        }
        .max(0.0) as u32;
        if let Some(grid) = self.below.borrow().get(&key) {
            return grid.clone();
        }
        let limit = key as f32 / 10.0;
        let mut grid = BitGrid::new(self.off_bed.cols, self.off_bed.rows);
        for p in &self.placed {
            let mut low = BitGrid::new(p.grid.cols, p.grid.rows);
            for (i, b) in p.base.iter().enumerate() {
                if *b < limit {
                    low.set(i % p.grid.cols, i / p.grid.cols);
                }
            }
            grid.or_clipped(&low, p.ox, p.oy);
        }
        let grid = Rc::new(grid);
        self.below.borrow_mut().insert(key, grid.clone());
        grid
    }

    /// Can you get from any part in `from` to any part in `to` by stepping,
    /// again and again, to a part that passes over the one you are on?
    fn climbs(&self, from: &[usize], to: &[usize]) -> bool {
        let mut seen = vec![false; self.placed.len()];
        let mut stack: Vec<usize> = from.to_vec();
        while let Some(a) = stack.pop() {
            if to.contains(&a) {
                return true;
            }
            if std::mem::replace(&mut seen[a], true) {
                continue;
            }
            stack.extend(self.over[a].iter().copied());
        }
        false
    }

    /// Record `shape` at `(ix, iy)`, grown by `halo` cells of spacing.
    fn commit(&mut self, shape: &Shape, ix: usize, iy: usize, halo: usize) {
        let (under, over) = if self.blocked.collides(&shape.grid, ix, iy) {
            self.stacking(shape, ix, iy).unwrap_or_default()
        } else {
            (Vec::new(), Vec::new())
        };
        self.below.borrow_mut().clear();
        self.any_headroom |= shape.has_headroom;
        let me = self.placed.len();
        // Parts the new one passes under are over it; it is over the rest.
        self.over.push(under);
        for &o in &over {
            self.over[o].push(me);
        }

        let grid = dilate(&shape.grid, halo);
        let (ox, oy) = (ix as isize - halo as isize, iy as isize - halo as isize);
        self.blocked.or_clipped(&grid, ox, oy);

        // Spread each column's span across the halo, so a candidate tested at
        // its own columns still keeps its distance from this part's material.
        // A disc is rows of different widths, so each width is swept along the
        // rows once with a sliding window, and every cell then takes the pick
        // of the rows above and below it at the width the disc has there —
        // work per cell that grows with the halo, not with its area.
        let (cols, rows) = (grid.cols, grid.rows);
        let k = halo;
        let half_width: Vec<usize> = (0..=k)
            .map(|dy| ((k * k - dy * dy) as f64).sqrt().floor() as usize)
            .collect();
        let mut base = vec![f32::INFINITY; cols * rows];
        let mut top = vec![f32::NEG_INFINITY; cols * rows];
        for sy in 0..shape.grid.rows {
            for sx in 0..shape.grid.cols {
                if shape.grid.get(sx, sy) {
                    let (si, ti) = (shape.idx(sx, sy), (sy + k) * cols + sx + k);
                    base[ti] = shape.base[si];
                    top[ti] = shape.top[si];
                }
            }
        }
        for (values, keep_low) in [(&mut base, true), (&mut top, false)] {
            let pick = |a: f32, b: f32| if keep_low { a.min(b) } else { a.max(b) };
            // Each row swept at every width the disc takes.
            let mut widths: Vec<usize> = half_width.clone();
            widths.dedup();
            let swept: Vec<Vec<f32>> = widths
                .iter()
                .map(|&w| {
                    let mut out = vec![0f32; cols * rows];
                    for y in 0..rows {
                        let row = y * cols..(y + 1) * cols;
                        sliding(&values[row.clone()], w, &pick, &mut out[row]);
                    }
                    out
                })
                .collect();
            let sweeps: Vec<&[f32]> = half_width
                .iter()
                .map(|w| swept[widths.iter().position(|v| v == w).unwrap_or(0)].as_slice())
                .collect();
            for y in 0..rows {
                for x in 0..cols {
                    if !grid.get(x, y) {
                        values[y * cols + x] = if keep_low {
                            f32::INFINITY
                        } else {
                            f32::NEG_INFINITY
                        };
                        continue;
                    }
                    let mut v = if keep_low {
                        f32::INFINITY
                    } else {
                        f32::NEG_INFINITY
                    };
                    for (dy, sweep) in sweeps.iter().enumerate() {
                        if y + dy < rows {
                            v = pick(v, sweep[(y + dy) * cols + x]);
                        }
                        if dy > 0 && y >= dy {
                            v = pick(v, sweep[(y - dy) * cols + x]);
                        }
                    }
                    values[y * cols + x] = v;
                }
            }
        }

        let mut floor = BitGrid::new(grid.cols, grid.rows);
        for (i, b) in base.iter().enumerate() {
            if *b <= ON_PLATE_EPS_MM {
                floor.set(i % grid.cols, i / grid.cols);
            }
        }
        self.floor.or_clipped(&floor, ox, oy);

        self.placed.push(Placed {
            grid,
            base,
            top,
            ox,
            oy,
            has_headroom: shape.has_headroom,
        });
    }
}

/// One shape's view of the plate, for the length of one search.
struct Probe<'a> {
    plate: &'a Plate,
    shape: &'a Shape,
    /// For a part with no headroom: the columns whose underside is too low
    /// for its shortest column, and those too low for its tallest.
    levels: Option<(Rc<BitGrid>, Rc<BitGrid>)>,
}

impl Probe<'_> {
    /// Can the shape sit with its cell (0,0) on plate cell `(ix, iy)`?
    fn fits(&self, ix: usize, iy: usize) -> bool {
        let (plate, shape) = (self.plate, self.shape);
        if !plate.blocked.collides(&shape.grid, ix, iy) {
            return true;
        }
        // Two columns that both start at the plate overlap at the plate.
        if plate.floor.collides(&shape.floor, ix, iy) {
            return false;
        }
        // Off the plate is off the plate, whatever height it is at.
        if plate.off_bed.collides(&shape.grid, ix, iy) {
            return false;
        }
        // A part that passes under everything it meets cannot close a loop,
        // so the fast answer needs nothing more.
        if let Some((too_low, low)) = &self.levels {
            if too_low.collides(&shape.grid, ix, iy) {
                return false;
            }
            if !low.collides(&shape.grid, ix, iy) {
                return true;
            }
        }
        // Another part is in the way in plan view. That is only a collision if
        // the two also want the same height there — or if passing each other
        // would lock them together.
        plate.stacking(shape, ix, iy).is_some()
    }
}

/// `out[i]` = `pick` over `values[i - k..=i + k]`, clipped to the ends —
/// a sliding window min or max in constant work per value, whatever `k`.
///
/// The line is cut into blocks of `2k + 1`: every window spans at most two
/// blocks, so it is the running pick from its start to the end of one block
/// combined with the running pick from the start of the next block to its end.
fn sliding(values: &[f32], k: usize, pick: &dyn Fn(f32, f32) -> f32, out: &mut [f32]) {
    let n = values.len();
    if k == 0 || n == 0 {
        out[..n].copy_from_slice(values);
        return;
    }
    let w = 2 * k + 1;
    let mut ahead = values.to_vec(); // running pick from each block's start
    let mut behind = values.to_vec(); // running pick to each block's end
    for i in 1..n {
        if i % w != 0 {
            ahead[i] = pick(ahead[i - 1], values[i]);
        }
    }
    for i in (0..n.saturating_sub(1)).rev() {
        if (i + 1) % w != 0 {
            behind[i] = pick(behind[i + 1], values[i]);
        }
    }
    for (i, slot) in out.iter_mut().enumerate().take(n) {
        let lo = i.saturating_sub(k);
        let hi = (i + k).min(n - 1);
        // A window inside one block — clipped at an end, or exactly one
        // block — is read directly; it happens at most once per block.
        *slot = if lo / w == hi / w {
            values[lo + 1..=hi]
                .iter()
                .fold(values[lo], |a, &b| pick(a, b))
        } else {
            pick(behind[lo], ahead[hi])
        };
    }
}

/// Read 64 bits of `row` starting at bit `start`, which may sit outside it.
///
/// Bits off either end read as empty, which is what makes a candidate hanging
/// off the side of a placed part's neighbourhood free there rather than a
/// special case.
#[inline]
fn window(row: &[u64], start: isize) -> u64 {
    let word = start >> 6;
    let shift = start.rem_euclid(64) as u32;
    let at = |i: isize| -> u64 {
        if i < 0 || i as usize >= row.len() {
            0
        } else {
            row[i as usize]
        }
    };
    let lo = at(word);
    if shift == 0 {
        lo
    } else {
        (lo >> shift) | (at(word + 1) << (64 - shift))
    }
}

/// How a candidate stands towards a part already on the plate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Meet {
    /// No column in common.
    Apart,
    /// The candidate passes under the part wherever the two share a column.
    Below,
    /// The candidate passes over the part wherever the two share a column.
    Above,
    /// They want the same space somewhere, or pass each other both ways.
    Clash,
}

/// How a candidate at `(ix, iy)` meets a placed part.
///
/// Only the columns the two actually share are read, found a machine word at a
/// time, so a candidate that clips a corner of its neighbour costs a few
/// comparisons rather than a walk over its own area.
fn meet(placed: &Placed, shape: &Shape, ix: isize, iy: isize, vertical_gap: f32) -> Meet {
    // Without headroom on one side or the other there is no way for two parts
    // to pass each other, so any shared column is a collision and the heights
    // need not be read at all.
    let nesting_possible = placed.has_headroom || shape.has_headroom;
    let x_offset = ix - placed.ox;
    let (mut below, mut above) = (false, false);

    // Only the rows the two have in common.
    let first = (placed.oy - iy).max(0) as usize;
    let last =
        (placed.oy + placed.grid.rows as isize - iy).clamp(0, shape.grid.rows as isize) as usize;
    for sy in first..last {
        let py = (iy + sy as isize - placed.oy) as usize;
        let placed_row = placed.grid.row(py);
        for (i, &word) in shape.grid.row(sy).iter().enumerate() {
            if word == 0 {
                continue;
            }
            let mut shared = word & window(placed_row, x_offset + (i * 64) as isize);
            if shared != 0 && !nesting_possible {
                return Meet::Clash;
            }
            while shared != 0 {
                let bit = shared.trailing_zeros() as usize;
                shared &= shared - 1;
                let sx = i * 64 + bit;
                let px = (x_offset + sx as isize) as usize;
                let si = shape.idx(sx, sy);
                let pi = py * placed.grid.cols + px;
                if shape.top[si] + vertical_gap <= placed.base[pi] {
                    below = true;
                } else if placed.top[pi] + vertical_gap <= shape.base[si] {
                    above = true;
                } else {
                    return Meet::Clash;
                }
                if below && above {
                    return Meet::Clash;
                }
            }
        }
    }
    match (below, above) {
        (false, false) => Meet::Apart,
        (true, false) => Meet::Below,
        (false, true) => Meet::Above,
        (true, true) => Meet::Clash,
    }
}

// ---------------------------------------------------------------------------
// Packing
// ---------------------------------------------------------------------------

/// Grid cell size for `bed`, in millimetres.
pub fn cell_size(bed: &BedConfig) -> f64 {
    let longest = bed.width.max(bed.depth).max(1.0);
    (longest / MAX_GRID_CELLS as f64).max(MIN_CELL_MM)
}

/// Build the occupancy map: everything not fully inside the plate, shrunk by
/// `scale` about its centre, starts occupied.
///
/// At `scale` 1 that is the plate itself. Below it, it is a smaller plate of
/// the same shape in the middle of the real one — the window
/// [`pack_objects`] narrows to keep a light plate in one compact group.
fn bed_occupancy(bed: &BedConfig, cell: f64, scale: f64) -> (BitGrid, [f64; 2]) {
    let origin = [bed.origin_offset_x, bed.origin_offset_y];
    let cols = ((bed.width / cell).floor() as usize).max(1);
    let rows = ((bed.depth / cell).floor() as usize).max(1);
    let (bx, by) = bed.center_xy();
    let inside = |x: f64, y: f64| {
        bed.contains_xy(x, y) && bed.contains_xy(bx + (x - bx) / scale, by + (y - by) / scale)
    };
    // Every cell corner is shared by four cells, so the corners are tested
    // once each and the cells read them.
    let lattice: Vec<bool> = (0..=rows)
        .flat_map(|y| (0..=cols).map(move |x| (x, y)))
        .map(|(x, y)| inside(origin[0] + x as f64 * cell, origin[1] + y as f64 * cell))
        .collect();
    let corner = |x: usize, y: usize| lattice[y * (cols + 1) + x];
    let mut grid = BitGrid::new(cols, rows);
    for y in 0..rows {
        for x in 0..cols {
            let printable =
                corner(x, y) && corner(x + 1, y) && corner(x, y + 1) && corner(x + 1, y + 1);
            if !printable {
                grid.set(x, y);
            }
        }
    }
    (grid, origin)
}

/// Bounds of the clear cells of `grid`, as inclusive `[min, max]` per axis.
fn free_bounds(grid: &BitGrid) -> Option<([usize; 2], [usize; 2])> {
    let mut lo = [usize::MAX; 2];
    let mut hi = [0usize; 2];
    for y in 0..grid.rows {
        for x in 0..grid.cols {
            if !grid.get(x, y) {
                lo = [lo[0].min(x), lo[1].min(y)];
                hi = [hi[0].max(x), hi[1].max(y)];
            }
        }
    }
    (lo[0] != usize::MAX).then_some((lo, hi))
}

/// Angles tried per object, in degrees.
fn rotation_candidates(step_deg: f64) -> Vec<f64> {
    if !step_deg.is_finite() || step_deg <= 0.0 || step_deg >= 360.0 {
        return vec![0.0];
    }
    let step = step_deg.max(1.0);
    let count = (360.0 / step).floor() as usize;
    (0..count.max(1)).map(|i| i as f64 * step).collect()
}

/// How closely the window search in [`pack_objects`] homes in on the smallest
/// window that takes every part, as a fraction of the plate.
///
/// Two per cent of a 256 mm plate is about 5 mm — less than the gap most
/// people leave between parts, so a finer search would move nothing anyone
/// could see, and each halving costs a full packing run.
const WINDOW_TOLERANCE: f64 = 0.02;

/// Objects sorted by `key`, largest first, ties by index so a run repeats.
fn by_key(footprints: &[Footprint], key: &dyn Fn(usize) -> f64) -> Vec<usize> {
    let mut order: Vec<usize> = (0..footprints.len()).collect();
    order.sort_by(|&a, &b| {
        key(b)
            .partial_cmp(&key(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });
    order
}

/// Orders the packer tries on a plate that does not fit at the first go.
///
/// Which part goes first decides the whole arrangement, and no single rule wins
/// on every plate: biggest-first is the classic and it strands a tall part with
/// headroom behind a wall of small ones, while tallest-first buries a wide flat
/// part. Running several orders and keeping the best is what turns a plate that
/// nearly fits into one that does.
fn candidate_orders(footprints: &[Footprint]) -> Vec<Vec<usize>> {
    let area = |i: usize| footprints[i].extent[0] * footprints[i].extent[1];
    let mut shapes: Vec<u64> = footprints.iter().map(|f| f.fingerprint()).collect();
    shapes.sort_unstable();
    shapes.dedup();
    if shapes.len() < 2 {
        // One model, many copies: every order is the same order, and trying
        // five of them is five times the work for the same plate.
        return vec![by_key(footprints, &area)];
    }
    let by = |key: &dyn Fn(usize) -> f64| by_key(footprints, key);

    let mut orders = vec![
        // The classic: whatever needs the most room chooses first.
        by(&area),
        // Tall first, so a part with headroom claims its place before the
        // small parts that could live under it are scattered across the plate.
        by(&|i| footprints[i].height),
        // Longest edge first, which beats area on plates of long thin parts.
        by(&|i| footprints[i].extent[0].max(footprints[i].extent[1])),
    ];
    // Two shuffles from a fixed seed: cheap, and the arrange stays repeatable.
    let mut seed = 0x9E3779B97F4A7C15u64;
    let mut shuffle = || -> Vec<usize> {
        let mut order: Vec<usize> = (0..footprints.len()).collect();
        for i in (1..order.len()).rev() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            order.swap(i, (seed % (i as u64 + 1)) as usize);
        }
        order
    };
    if footprints.len() > 2 {
        orders.push(shuffle());
        orders.push(shuffle());
    }
    orders.dedup();
    orders
}

/// How a packing run places parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Way {
    /// Pack each part's solid box instead of its shape.
    boxed: bool,
    /// Send each part to the next corner of the window in turn, instead of
    /// every part to the same one.
    corners: bool,
}

impl Way {
    /// The shape-true run, one corner. The one that wins on almost every plate.
    const SHAPE: Way = Way {
        boxed: false,
        corners: false,
    };
    /// Each part to the next corner in turn. Placement is greedy, and one
    /// corner lets round parts settle into a honeycomb — the third in the
    /// dip between the first two — that needs more width than two by two, so
    /// the fourth has nowhere to go. Taking the corners in turn builds the two
    /// by two instead.
    const CORNERS: Way = Way {
        boxed: false,
        corners: true,
    };
    /// Boxes, not shapes: never fits fewer parts than the box packer this
    /// module replaced, and is only tried when the shapes left a part over.
    const BOXES: Way = Way {
        boxed: true,
        corners: false,
    };

    /// Corner for the `n`th part placed: low, high, then the other diagonal.
    fn corner(self, n: usize) -> u8 {
        if self.corners {
            [0, 3, 1, 2][n % 4]
        } else {
            0
        }
    }
}

/// Everything a packing run shares with the next: the grid, and the shapes
/// already rasterised — the expensive half, and the same for every run.
struct Packer<'a> {
    footprints: &'a [Footprint],
    bed: &'a BedConfig,
    options: &'a PackOptions,
    angles: Vec<f64>,
    cell: f64,
    halo: usize,
    /// The whole plate, for centring a finished arrangement on it.
    plate: BitGrid,
    shapes: HashMap<(u64, u64, bool), Rc<Shape>>,
}

/// Nest `footprints` on `bed` and return where each one goes.
///
/// Packing fills a corner of whatever it is given, which on a whole plate
/// spreads a handful of parts along one edge and up a staircase — a long tour
/// for the nozzle from part to part on every layer. So a plate is packed inside
/// the smallest window, the plate's own shape shrunk about its centre, that
/// still takes every part: a light plate comes out as one compact block in the
/// middle, and a full one is packed exactly as densely as the whole plate
/// allows. A tight window is also what makes parts use each other's headroom:
/// the smallest block is the one where they pass over and under each other.
///
/// Objects are never dropped: one that cannot fit is parked in a column beside
/// the plate with `on_bed: false`, so the caller can tell the user rather than
/// quietly losing their part.
pub fn pack_objects(
    footprints: &[Footprint],
    bed: &BedConfig,
    options: &PackOptions,
) -> Vec<Placement> {
    if footprints.is_empty() {
        return Vec::new();
    }
    let cell = cell_size(bed);
    let mut packer = Packer {
        footprints,
        bed,
        options,
        angles: rotation_candidates(options.rotation_step_deg),
        cell,
        halo: if options.spacing_mm > 0.0 {
            (options.spacing_mm / cell).ceil() as usize
        } else {
            0
        },
        plate: bed_occupancy(bed, cell, 1.0).0,
        shapes: HashMap::new(),
    };
    let wanted = footprints.iter().filter(|fp| !fp.is_empty()).count();
    let on_bed = |run: &[Placement]| run.iter().filter(|p| p.on_bed).count();

    // First the whole plate. One shape-true run in the classic order settles
    // almost every plate, and it parks whatever it cannot place, so there is
    // always an answer.
    let orders = candidate_orders(footprints);
    let Some(first) = packer.nest(&orders[0], 1.0, false, Way::SHAPE) else {
        unreachable!("a run that may park parts always finishes");
    };
    let mut fitted =
        (on_bed(&first) == wanted).then(|| (orders[0].clone(), Way::SHAPE, first.clone()));

    // A part left over: try the other ways and orders. These runs only need to
    // say whether everything fits, so each gives up at its first miss rather
    // than parking the rest.
    if fitted.is_none() {
        let tries = orders
            .iter()
            .flat_map(|order| [(order, Way::SHAPE), (order, Way::CORNERS)])
            .skip(1)
            .chain(orders.iter().map(|order| (order, Way::BOXES)));
        for (order, way) in tries {
            if let Some(run) = packer.nest(order, 1.0, true, way) {
                fitted = Some((order.clone(), way, run));
                break;
            }
        }
    }

    // Nothing takes every part. Keep whichever of the shapes and the boxes
    // parks fewer — boxes never fit fewer parts than the box packer this module
    // replaced.
    let Some((order, way, whole)) = fitted else {
        let Some(boxes) = packer.nest(&orders[0], 1.0, false, Way::BOXES) else {
            unreachable!("a run that may park parts always finishes");
        };
        return if on_bed(&boxes) > on_bed(&first) {
            boxes
        } else {
            first
        };
    };

    // Then the smallest window that still takes everything. Nothing smaller
    // than the parts' own area can hold them, which puts a floor under the
    // window before a single run.
    let free = (packer.plate.cols * packer.plate.rows - packer.plate.count()).max(1) as f64;
    let needed: f64 = footprints
        .iter()
        .filter(|fp| !fp.is_empty())
        .map(|fp| packer.shape(fp, fp.fingerprint(), 0.0, false).grid.count() as f64)
        .sum();
    // Shapes that fitted the whole plate get both corner rules at each size —
    // a window tight enough to matter is exactly where the honeycomb bites.
    let ways: &[Way] = if way.boxed {
        &[Way::BOXES]
    } else {
        &[Way::SHAPE, Way::CORNERS]
    };
    // Whether a window fits is not strictly monotone in its size — packing is
    // greedy — but it nearly is, and every run kept here is a complete, valid
    // plate, so the worst a wrong turn costs is a slightly larger block. The
    // floor is loose on purpose: parts that pass over each other can share
    // area, so the true minimum may sit below their summed shadows.
    let mut lo = (0.5 * needed / free).sqrt().min(1.0);
    let mut hi = 1.0;
    let mut best = whole;
    let mut first = true;
    while hi - lo > WINDOW_TOLERANCE {
        // A plate that barely fits is the common expensive case, and one run
        // just short of the full plate says so; bisecting down to it would
        // spend every step on a window that cannot work.
        let mid = if std::mem::take(&mut first) {
            (hi - 2.0 * WINDOW_TOLERANCE).max(lo)
        } else {
            (lo + hi) / 2.0
        };
        match ways
            .iter()
            .find_map(|&way| packer.nest(&order, mid, true, way))
        {
            Some(run) => {
                best = run;
                hi = mid;
            }
            None => lo = mid,
        }
    }
    best
}

impl Packer<'_> {
    /// `fp` rasterised at `angle`, from the cache.
    ///
    /// `boxed` swaps the part for the solid box around it, standing on the
    /// plate — see [`pack_objects`] for when that is worth it.
    fn shape(&mut self, fp: &Footprint, key: u64, angle: f64, boxed: bool) -> Rc<Shape> {
        let (cell, options) = (self.cell, self.options);
        self.shapes
            .entry((key, angle.to_bits(), boxed))
            .or_insert_with(|| {
                let shape = rasterise(fp, angle, cell, options);
                Rc::new(if boxed { shape.boxed() } else { shape })
            })
            .clone()
    }

    /// One packing run in `order`, into the plate shrunk to `scale` about its
    /// centre.
    ///
    /// With `all_or_nothing` the run gives up — `None` — at the first part that
    /// does not fit, which is all the window search needs to know. Otherwise a
    /// part that does not fit is parked beside the plate and the run goes on.
    /// `way` says how each part is placed.
    fn nest(
        &mut self,
        order: &[usize],
        scale: f64,
        all_or_nothing: bool,
        way: Way,
    ) -> Option<Vec<Placement>> {
        let (footprints, bed, options, cell) = (self.footprints, self.bed, self.options, self.cell);
        let (window, origin) = bed_occupancy(bed, cell, scale);
        // No free cell at all — a plate smaller than one grid cell. An empty
        // range turns every part away, so a run that may park parks them all.
        let (lo, hi) = match free_bounds(&window) {
            Some(bounds) => bounds,
            None if all_or_nothing => return None,
            None => ([1, 1], [0, 0]),
        };
        let mut plate = Plate::new(window, options.spacing_mm.max(MIN_VERTICAL_GAP_MM) as f32);

        let mut placements: Vec<Placement> = footprints
            .iter()
            .enumerate()
            .map(|(index, fp)| Placement {
                index,
                angle_deg: 0.0,
                pivot: fp.pivot,
                on_bed: false,
            })
            .collect();
        // Where each placed object sits on the grid, for the recentring pass.
        let mut placed_cells: Vec<(usize, BitGrid, usize, usize)> = Vec::new();

        let mut overflow_y = 0.0_f64;
        // Shapes that found nowhere to go. The plate only ever gains parts, so
        // a shape that did not fit cannot fit later either — and a plate is
        // usually one model many times over, which is exactly when the hopeless
        // search would otherwise be the whole cost of the arrange.
        let mut hopeless: std::collections::HashSet<u64> = std::collections::HashSet::new();
        for &idx in order {
            let fp = &footprints[idx];
            if fp.is_empty() {
                continue;
            }
            let key = fp.fingerprint();

            // cost, angle, shape, ix, iy
            let mut best: Option<(f64, f64, Rc<Shape>, usize, usize)> = None;
            if !hopeless.contains(&key) {
                let mut tried: Vec<u64> = Vec::with_capacity(self.angles.len());
                for angle in self.angles.clone() {
                    let shape = self.shape(fp, key, angle, way.boxed);
                    if tried.contains(&shape.form) {
                        continue;
                    }
                    tried.push(shape.form);
                    let (w, h) = (shape.grid.cols, shape.grid.rows);
                    if lo[0] + w > hi[0] + 1 || lo[1] + h > hi[1] + 1 {
                        continue;
                    }
                    if let Some((cost, ix, iy)) = search(
                        &plate.probe(&shape),
                        lo,
                        [hi[0] + 1 - w, hi[1] + 1 - h],
                        way.corner(placed_cells.len()),
                        cell,
                        best.as_ref().map(|b| b.0),
                    ) {
                        if best.as_ref().is_none_or(|b| cost < b.0) {
                            best = Some((cost, angle, shape, ix, iy));
                        }
                    }
                }
            }

            match best {
                Some((_, angle, shape, ix, iy)) => {
                    let placed = &mut placements[idx];
                    placed.angle_deg = angle;
                    placed.pivot = [
                        origin[0] + ix as f64 * cell - shape.offset[0],
                        origin[1] + iy as f64 * cell - shape.offset[1],
                    ];
                    placed.on_bed = true;
                    plate.commit(&shape, ix, iy, self.halo);
                    placed_cells.push((idx, shape.grid.clone(), ix, iy));
                }
                None if all_or_nothing => return None,
                None => {
                    hopeless.insert(key);
                    // Park it to the right of the plate, stacked downwards. The
                    // object stays visible and out of bounds instead of
                    // vanishing or silently overlapping a part that did fit.
                    let gap = options.spacing_mm.max(1.0);
                    let placed = &mut placements[idx];
                    placed.angle_deg = 0.0;
                    placed.pivot = [
                        bed.origin_offset_x + bed.width + gap + fp.extent[0] / 2.0,
                        bed.origin_offset_y + overflow_y + fp.extent[1] / 2.0,
                    ];
                    placed.on_bed = false;
                    overflow_y += fp.extent[1] + gap;
                }
            }
        }

        // Centred on the whole plate, not the window: on a round plate the
        // window is the smaller disc, and the pile may fit the big one better
        // centred. The pile moves as one, so nothing that passed over or under
        // a neighbour stops doing so.
        let (shift_x, shift_y) = recentring_shift(&self.plate, &placed_cells);
        for (idx, ..) in &placed_cells {
            let placed = &mut placements[*idx];
            placed.pivot[0] += shift_x as f64 * cell;
            placed.pivot[1] += shift_y as f64 * cell;
        }

        Some(placements)
    }
}

/// How far the finished arrangement may move to sit centred on the plate.
///
/// The arrangement is packed into a corner, so it needs moving back; the move
/// is whatever puts its bounds in the middle, shortened one cell at a time
/// until every object is still fully on the plate. On a rectangular bed the
/// full move always survives that check; on a round one it is the difference
/// between a centred plate and parts pushed over the rim.
fn recentring_shift(
    bed_mask: &BitGrid,
    placed: &[(usize, BitGrid, usize, usize)],
) -> (isize, isize) {
    if placed.is_empty() {
        return (0, 0);
    }
    let min_x = placed.iter().map(|(_, _, ix, _)| *ix).min().unwrap_or(0);
    let min_y = placed.iter().map(|(_, _, _, iy)| *iy).min().unwrap_or(0);
    let max_x = placed
        .iter()
        .map(|(_, m, ix, _)| ix + m.cols)
        .max()
        .unwrap_or(0);
    let max_y = placed
        .iter()
        .map(|(_, m, _, iy)| iy + m.rows)
        .max()
        .unwrap_or(0);

    let mut dx = ((bed_mask.cols as isize - (max_x + min_x) as isize) / 2).max(0);
    let mut dy = ((bed_mask.rows as isize - (max_y + min_y) as isize) / 2).max(0);

    loop {
        let fits = placed.iter().all(|(_, mask, ix, iy)| {
            let x = *ix as isize + dx;
            let y = *iy as isize + dy;
            x >= 0
                && y >= 0
                && x as usize + mask.cols <= bed_mask.cols
                && y as usize + mask.rows <= bed_mask.rows
                && !bed_mask.collides(mask, x as usize, y as usize)
        });
        if fits || (dx == 0 && dy == 0) {
            return (dx, dy);
        }
        if dx.abs() >= dy.abs() {
            dx -= dx.signum();
        } else {
            dy -= dy.signum();
        }
    }
}

/// Find the free cell offset nearest one corner of `lo..=hi`.
///
/// `corner` picks which: bit 0 set counts from the high-x side, bit 1 from the
/// high-y side, so `0` is the low corner. Distance is measured with depth
/// counting `DEPTH_BIAS` times width, so within one row the nearest spot is
/// simply the first that fits, and a row cannot hold anything nearer than its
/// own depth — once that passes the best spot found, the search is provably
/// finished, and the result is the true nearest placement rather than the first
/// acceptable one.
///
/// A row is tested all at once: [`row_hits`] turns the plate and the shape's
/// runs into the set of positions along the row that touch something, a
/// handful of word operations instead of one bitmap test per position. Only a
/// position that touches something and is not settled by the floor, the plate
/// edge or the undersides pays for the column-by-column test.
///
/// Nesting into the corner is deliberate. Gravity towards the plate's middle
/// sounds right and packs badly: the first big part lands squarely on the space
/// every later part needs. The whole arrangement is moved back to the centre
/// once it is done, and `DEPTH_BIAS` is what makes the pile grow in rows on the
/// way there.
fn search(
    probe: &Probe,
    lo: [usize; 2],
    hi: [usize; 2],
    corner: u8,
    cell: f64,
    ceiling: Option<f64>,
) -> Option<(f64, usize, usize)> {
    let (plate, shape) = (probe.plate, probe.shape);
    let span = [hi[0] - lo[0], hi[1] - lo[1]];
    let at = |i: usize, axis: usize| {
        if corner >> axis & 1 == 1 {
            hi[axis] - i
        } else {
            lo[axis] + i
        }
    };
    let words = plate.blocked.stride;
    let mut scratch = vec![0u64; words];
    let mut touching = vec![0u64; words];
    // Which positions along the row the cheaper tests rule in or out, built
    // only for a row that needs them.
    let mut settled: Vec<Vec<u64>> = Vec::new();

    let mut best: Option<(f64, usize, usize)> = None;
    // A placement is only interesting while it beats this. It starts at the
    // best the previous rotation found, so a hopeless angle is abandoned early.
    let mut bound = ceiling.unwrap_or(f64::INFINITY);
    for j in 0..=span[1] {
        let depth = j as f64 * DEPTH_BIAS;
        if depth * cell >= bound {
            break;
        }
        let y = at(j, 1);
        // How far along this row a spot could still beat the best. When that
        // is only a few positions, testing them one by one is cheaper than
        // working out the whole row.
        let reach = if bound.is_finite() {
            let r = bound / cell;
            (r * r - depth * depth).max(0.0).sqrt() as usize
        } else {
            span[0]
        };
        if reach < BATCH_ROW {
            for i in 0..=reach.min(span[0]) {
                let cost = ((i * i) as f64 + depth * depth).sqrt() * cell;
                if cost >= bound {
                    break;
                }
                let (x, y) = (at(i, 0), y);
                if probe.fits(x, y) {
                    best = Some((cost, x, y));
                    bound = cost;
                    break;
                }
            }
            continue;
        }
        row_hits(&plate.blocked, &shape.runs, y, &mut touching, &mut scratch);
        settled.clear();
        for i in 0..=span[0] {
            let cost = ((i * i) as f64 + depth * depth).sqrt() * cell;
            if cost >= bound {
                break;
            }
            let x = at(i, 0);
            if !bit(&touching, x) || probe.nests(x, y, &mut settled, &mut scratch) {
                best = Some((cost, x, y));
                bound = cost;
                break;
            }
        }
    }

    best
}

impl Probe<'_> {
    /// Can the shape, touching something in plan view at `(x, y)`, sit there
    /// anyway by passing over or under it?
    ///
    /// `sets` caches, for row `y`, the positions ruled out by the floor and the
    /// plate edge and — for a part with no headroom — those the undersides rule
    /// out or in; it is filled on first use and belongs to one row.
    fn nests(&self, x: usize, y: usize, sets: &mut Vec<Vec<u64>>, scratch: &mut [u64]) -> bool {
        let (plate, shape) = (self.plate, self.shape);
        if !shape.has_headroom && !plate.any_headroom {
            return false;
        }
        if sets.is_empty() {
            let mut row = |grid: &BitGrid, runs: &Runs| {
                let mut out = vec![0u64; grid.stride];
                row_hits(grid, runs, y, &mut out, scratch);
                out
            };
            sets.push(row(&plate.floor, &shape.floor_runs));
            sets.push(row(&plate.off_bed, &shape.runs));
            if let Some((too_low, low)) = &self.levels {
                sets.push(row(too_low, &shape.runs));
                sets.push(row(low, &shape.runs));
            }
        }
        // Two columns that both start at the plate overlap at the plate, and
        // off the plate is off the plate at any height.
        if bit(&sets[0], x) || bit(&sets[1], x) {
            return false;
        }
        if self.levels.is_some() {
            if bit(&sets[2], x) {
                return false;
            }
            if !bit(&sets[3], x) {
                // It passes under everything it meets, which cannot close a
                // loop either.
                return true;
            }
        }
        plate.stacking(shape, x, y).is_some()
    }
}

/// Positions along a row below which [`search`] tests spots one at a time
/// rather than working out the whole row. A row costs about as much as a few
/// dozen single tests of a mid-sized part; the exact figure barely matters.
const BATCH_ROW: usize = 48;

#[inline]
fn bit(words: &[u64], x: usize) -> bool {
    words[x >> 6] >> (x & 63) & 1 == 1
}

/// Positions along row `y` at which a shape whose rows are `runs` touches a
/// set cell of `grid`: bit `x` of `out` is set when the shape with its cell
/// (0,0) on `(x, y)` covers something.
///
/// Row `r` of the shape meets row `y + r` of the grid, and a run covering cells
/// `a..=a + n` of it touches a set cell `c` from every `x` in `c - a - n..=c - a`
/// — the grid row shifted down by `a`, then smeared down across `n` more.
fn row_hits(grid: &BitGrid, runs: &Runs, y: usize, out: &mut [u64], scratch: &mut [u64]) {
    out.iter_mut().for_each(|w| *w = 0);
    for (r, row_runs) in runs.iter().enumerate() {
        if row_runs.is_empty() || y + r >= grid.rows {
            continue;
        }
        let src = grid.row(y + r);
        if src.iter().all(|&w| w == 0) {
            continue;
        }
        for &(a, n) in row_runs {
            shift_down(src, a as usize, scratch);
            smear_down(scratch, n as usize);
            for (o, s) in out.iter_mut().zip(scratch.iter()) {
                *o |= *s;
            }
        }
    }
}

/// `dst[x] = src[x + n]`, zero past the end.
fn shift_down(src: &[u64], n: usize, dst: &mut [u64]) {
    let (w, b) = (n >> 6, (n & 63) as u32);
    for (i, slot) in dst.iter_mut().enumerate() {
        let lo = src.get(i + w).copied().unwrap_or(0);
        *slot = if b == 0 {
            lo
        } else {
            let hi = src.get(i + w + 1).copied().unwrap_or(0);
            (lo >> b) | (hi << (64 - b))
        };
    }
}

/// `bits[x] |= bits[x + 1] | … | bits[x + n]`, by doubling.
fn smear_down(bits: &mut [u64], n: usize) {
    let mut covered = 1;
    while covered <= n {
        let step = covered.min(n + 1 - covered);
        let (w, b) = (step >> 6, (step & 63) as u32);
        // Ascending, so every word read further on is still unchanged.
        for i in 0..bits.len() {
            let lo = bits.get(i + w).copied().unwrap_or(0);
            let shifted = if b == 0 {
                lo
            } else {
                let hi = bits.get(i + w + 1).copied().unwrap_or(0);
                (lo >> b) | (hi << (64 - b))
            };
            bits[i] |= shifted;
        }
        covered += step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::types::{Face, Vertex};
    use crate::scene::bed::BedShape;

    fn default_bed() -> BedConfig {
        BedConfig {
            width: 220.0,
            depth: 220.0,
            height: 250.0,
            origin_offset_x: 0.0,
            origin_offset_y: 0.0,
            shape: BedShape::Rectangular,
        }
    }

    /// Triangles from a ring of 3D corners, fanned from the first.
    fn fan(poly: &[[f64; 3]], mesh: &mut Mesh) {
        for i in 1..poly.len() - 1 {
            mesh.faces.push(Face::new([
                Vertex::new(poly[0][0], poly[0][1], poly[0][2]),
                Vertex::new(poly[i][0], poly[i][1], poly[i][2]),
                Vertex::new(poly[i + 1][0], poly[i + 1][1], poly[i + 1][2]),
            ]));
        }
    }

    /// A flat sheet spanning `poly` at height `z` — enough geometry to claim
    /// the columns it covers, at the height it covers them.
    fn sheet(poly: &[[f64; 2]], z: f64) -> Mesh {
        let mut mesh = Mesh::new();
        let corners: Vec<[f64; 3]> = poly.iter().map(|p| [p[0], p[1], z]).collect();
        mesh.vertices = corners
            .iter()
            .map(|p| Vertex::new(p[0], p[1], p[2]))
            .collect();
        fan(&corners, &mut mesh);
        mesh
    }

    fn square(w: f64, d: f64) -> Vec<[f64; 2]> {
        vec![[0.0, 0.0], [w, 0.0], [w, d], [0.0, d]]
    }

    /// A 10 mm slab: flat on the plate, flat on top.
    fn box_mesh(w: f64, d: f64) -> Mesh {
        let mut mesh = sheet(&square(w, d), 0.0);
        mesh.faces.extend(sheet(&square(w, d), 10.0).faces);
        mesh
    }

    /// An L: a `size × size` square with the top-right quarter cut away.
    fn l_mesh(size: f64) -> Mesh {
        let h = size / 2.0;
        sheet(
            &[
                [0.0, 0.0],
                [size, 0.0],
                [size, h],
                [h, h],
                [h, size],
                [0.0, size],
            ],
            0.0,
        )
    }

    /// A table: a flat `span × span` top at height `h` on four thin legs. The
    /// top is horizontal, so it needs support and owns the space beneath it.
    fn table_mesh(span: f64, h: f64) -> Mesh {
        let mut mesh = sheet(&square(span, span), h);
        for (lx, ly) in [
            (0.0, 0.0),
            (span - 6.0, 0.0),
            (0.0, span - 6.0),
            (span - 6.0, span - 6.0),
        ] {
            let leg: Vec<[f64; 2]> = vec![
                [lx, ly],
                [lx + 6.0, ly],
                [lx + 6.0, ly + 6.0],
                [lx, ly + 6.0],
            ];
            for z in [0.0, h] {
                mesh.faces.extend(sheet(&leg, z).faces);
            }
        }
        mesh
    }

    /// An arch: two 45° flanks meeting at a ridge `h` above the plate, resting
    /// on their outer edges with nothing underneath. Every underside descends
    /// at 45°, so the part holds itself up and the air under the ridge is real.
    fn arch_mesh(span: f64, h: f64) -> Mesh {
        let mut mesh = Mesh::new();
        let mid = span / 2.0;
        for (near, far) in [(0.0_f64, mid), (span, mid)] {
            fan(
                &[
                    [near, 0.0, 0.0],
                    [near, span, 0.0],
                    [far, span, h],
                    [far, 0.0, h],
                ],
                &mut mesh,
            );
        }
        mesh
    }

    fn footprint(mesh: &Mesh) -> Footprint {
        Footprint::new(mesh, &Transform::IDENTITY)
    }

    /// Minimum distance between two placed parts, by brute force over their
    /// rasterised outlines. Slow, but it measures the thing the packer promises
    /// instead of the boxes around it.
    fn min_distance(a: &Footprint, pa: &Placement, b: &Footprint, pb: &Placement) -> f64 {
        let cell = 0.5;
        let options = PackOptions::default();
        let sample = |fp: &Footprint, p: &Placement| -> Vec<[f64; 2]> {
            let shape = rasterise(fp, p.angle_deg, cell, &options);
            let mut out = Vec::new();
            for y in 0..shape.grid.rows {
                for x in 0..shape.grid.cols {
                    if shape.grid.get(x, y) {
                        out.push([
                            p.pivot[0] + shape.offset[0] + (x as f64 + 0.5) * cell,
                            p.pivot[1] + shape.offset[1] + (y as f64 + 0.5) * cell,
                        ]);
                    }
                }
            }
            out
        };
        let (sa, sb) = (sample(a, pa), sample(b, pb));
        let mut best = f64::INFINITY;
        for p in &sa {
            for q in &sb {
                let d = ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt();
                best = best.min(d);
            }
        }
        best
    }

    #[test]
    fn single_object_lands_on_the_bed_centre() {
        let bed = default_bed();
        let fps = vec![footprint(&box_mesh(50.0, 50.0))];
        let out = pack_objects(&fps, &bed, &PackOptions::default());
        assert_eq!(out.len(), 1);
        assert!(out[0].on_bed);
        let cell = cell_size(&bed);
        assert!(
            (out[0].pivot[0] - 110.0).abs() <= cell,
            "{:?}",
            out[0].pivot
        );
        assert!(
            (out[0].pivot[1] - 110.0).abs() <= cell,
            "{:?}",
            out[0].pivot
        );
    }

    #[test]
    fn empty_input() {
        assert!(pack_objects(&[], &default_bed(), &PackOptions::default()).is_empty());
    }

    #[test]
    fn objects_keep_the_requested_spacing() {
        let bed = default_bed();
        let fps: Vec<Footprint> = (0..4).map(|_| footprint(&box_mesh(40.0, 40.0))).collect();
        let opts = PackOptions {
            spacing_mm: 5.0,
            ..PackOptions::default()
        };
        let out = pack_objects(&fps, &bed, &opts);
        assert!(out.iter().all(|p| p.on_bed));
        for i in 0..out.len() {
            for j in (i + 1)..out.len() {
                let d = min_distance(&fps[i], &out[i], &fps[j], &out[j]);
                assert!(
                    d >= opts.spacing_mm - 1.5,
                    "objects {i},{j} are {d:.2} mm apart, want ≥ {}",
                    opts.spacing_mm
                );
            }
        }
    }

    #[test]
    fn concave_parts_nest_closer_than_their_boxes_allow() {
        // Two 100 mm Ls on a 160 × 110 mm plate. Their boxes need 201 mm side
        // by side and 201 mm stacked, so a box-packer cannot place the second
        // one at all; their outlines interlock into 151 × 101 mm.
        let bed = BedConfig {
            width: 160.0,
            depth: 110.0,
            ..default_bed()
        };
        let fps: Vec<Footprint> = (0..2).map(|_| footprint(&l_mesh(100.0))).collect();
        let opts = PackOptions {
            spacing_mm: 1.0,
            ..PackOptions::default()
        };
        let out = pack_objects(&fps, &bed, &opts);
        assert!(
            out.iter().all(|p| p.on_bed),
            "two 100 mm Ls must interlock onto a 160 x 110 mm plate: {out:?}"
        );
    }

    #[test]
    fn a_part_slides_under_a_self_supporting_overhang() {
        // The tent's flanks are 45° slopes: they print over thin air, so the
        // space beside its ridge is genuinely empty and a short part belongs
        // there. No packer working from outlines would ever put it there.
        let bed = BedConfig {
            width: 90.0,
            depth: 90.0,
            ..default_bed()
        };
        let arch = footprint(&arch_mesh(80.0, 40.0));
        let small = footprint(&box_mesh(20.0, 20.0));
        let out = pack_objects(&[arch, small], &bed, &PackOptions::default());
        assert!(out.iter().all(|p| p.on_bed), "{out:?}");
    }

    #[test]
    fn a_flat_shelf_keeps_the_space_beneath_it() {
        // The same plate, with the overhang a flat table top: support columns
        // would fill the space under it, so nothing may share those columns and
        // on a plate this tight the second part stays off the bed.
        let bed = BedConfig {
            width: 90.0,
            depth: 90.0,
            ..default_bed()
        };
        let table = footprint(&table_mesh(80.0, 40.0));
        let small = footprint(&box_mesh(20.0, 20.0));
        let out = pack_objects(&[table, small], &bed, &PackOptions::default());
        assert!(out[0].on_bed, "the table itself fits");
        assert!(
            !out[1].on_bed,
            "nothing may share a column with support material: {out:?}"
        );
    }

    #[test]
    fn sequential_printing_gives_every_part_its_own_column() {
        // Printing one part at a time drives the gantry past finished parts, so
        // nothing may pass over anything, however self-supporting it is.
        let bed = BedConfig {
            width: 90.0,
            depth: 90.0,
            ..default_bed()
        };
        let arch = footprint(&arch_mesh(80.0, 40.0));
        let small = footprint(&box_mesh(20.0, 20.0));
        let out = pack_objects(
            &[arch, small],
            &bed,
            &PackOptions {
                vertical_nesting: false,
                ..PackOptions::default()
            },
        );
        assert!(out[0].on_bed);
        assert!(!out[1].on_bed, "{out:?}");
    }

    #[test]
    fn a_light_plate_gathers_into_a_block() {
        // Nine parts on a plate with room for far more. Filled from a corner
        // they spread along the first row and step up in a staircase; the
        // nozzle then tours the whole spread on every layer. They belong in a
        // three-by-three block in the middle.
        let bed = BedConfig {
            width: 256.0,
            depth: 256.0,
            ..default_bed()
        };
        let fps: Vec<Footprint> = (0..9).map(|_| footprint(&box_mesh(30.0, 30.0))).collect();
        let opts = PackOptions {
            spacing_mm: 4.0,
            ..PackOptions::default()
        };
        let out = pack_objects(&fps, &bed, &opts);
        assert!(out.iter().all(|p| p.on_bed));

        let span = |axis: usize| {
            let v = out.iter().map(|p| p.pivot[axis]);
            v.clone().fold(f64::MIN, f64::max) - v.fold(f64::MAX, f64::min) + 30.0
        };
        // Three boxes and two gaps, plus a cell of rasterisation slack each.
        let block = 3.0 * 30.0 + 2.0 * opts.spacing_mm + 3.0 * cell_size(&bed);
        assert!(
            span(0) <= block && span(1) <= block,
            "nine boxes spread {:.0} x {:.0} mm, want a {block:.0} mm block",
            span(0),
            span(1)
        );
        let (cx, cy) = bed.center_xy();
        let mid = |axis: usize| out.iter().map(|p| p.pivot[axis]).sum::<f64>() / 9.0;
        assert!(
            (mid(0) - cx).abs() < 5.0 && (mid(1) - cy).abs() < 5.0,
            "the block should sit mid-plate, got ({:.0}, {:.0})",
            mid(0),
            mid(1)
        );
    }

    #[test]
    fn a_part_too_big_for_the_plate_is_parked_not_dropped() {
        let bed = BedConfig {
            width: 100.0,
            depth: 100.0,
            ..default_bed()
        };
        let fps = vec![
            footprint(&box_mesh(40.0, 40.0)),
            footprint(&box_mesh(150.0, 40.0)),
        ];
        let out = pack_objects(&fps, &bed, &PackOptions::default());
        assert_eq!(out.len(), 2);
        assert!(out[0].on_bed);
        assert!(!out[1].on_bed, "the oversized part cannot claim the plate");
        assert!(out[1].pivot[0] > bed.width, "parked beside the plate");
    }

    #[test]
    fn everything_stays_on_a_circular_plate() {
        let bed = BedConfig {
            width: 200.0,
            depth: 200.0,
            height: 250.0,
            origin_offset_x: 0.0,
            origin_offset_y: 0.0,
            shape: BedShape::Circular,
        };
        let fps: Vec<Footprint> = (0..6).map(|_| footprint(&box_mesh(30.0, 30.0))).collect();
        let out = pack_objects(&fps, &bed, &PackOptions::default());
        let (bx, by) = bed.center_xy();
        let radius = bed.width.min(bed.depth) / 2.0;
        for (fp, p) in fps.iter().zip(out.iter()) {
            assert!(p.on_bed);
            let (hw, hd) = (fp.extent[0] / 2.0, fp.extent[1] / 2.0);
            for (x, y) in [
                (p.pivot[0] - hw, p.pivot[1] - hd),
                (p.pivot[0] + hw, p.pivot[1] - hd),
                (p.pivot[0] - hw, p.pivot[1] + hd),
                (p.pivot[0] + hw, p.pivot[1] + hd),
            ] {
                let dist = ((x - bx).powi(2) + (y - by).powi(2)).sqrt();
                assert!(dist <= radius + 1e-6, "corner ({x:.1},{y:.1}) off the disk");
            }
        }
    }

    #[test]
    fn rotation_can_be_switched_off() {
        let bed = default_bed();
        let fps: Vec<Footprint> = (0..3).map(|_| footprint(&l_mesh(40.0))).collect();
        let out = pack_objects(
            &fps,
            &bed,
            &PackOptions {
                rotation_step_deg: 0.0,
                ..PackOptions::default()
            },
        );
        assert!(out.iter().all(|p| p.angle_deg == 0.0));
    }

    #[test]
    fn a_long_part_is_turned_to_fit() {
        // 150 × 20 on a 60 × 200 bed only fits standing up.
        let bed = BedConfig {
            width: 60.0,
            depth: 200.0,
            ..default_bed()
        };
        let fps = vec![footprint(&box_mesh(150.0, 20.0))];
        let out = pack_objects(&fps, &bed, &PackOptions::default());
        assert!(out[0].on_bed);
        assert!(
            out[0].angle_deg == 90.0 || out[0].angle_deg == 270.0,
            "expected a quarter turn, got {}",
            out[0].angle_deg
        );
    }

    #[test]
    fn only_a_self_supporting_underside_keeps_its_headroom() {
        // Read the middle column of each part: it is under the ridge of one and
        // under the centre of the other's flat top, which is where the two
        // disagree about whether the space below belongs to the part.
        let centre = |mesh: &Mesh| -> f32 {
            let shape = rasterise(&footprint(mesh), 0.0, 0.5, &PackOptions::default());
            let (x, y) = (shape.grid.cols / 2, shape.grid.rows / 2);
            assert!(
                shape.grid.get(x, y),
                "the middle column is part of the shape"
            );
            shape.base[shape.idx(x, y)]
        };

        assert!(
            centre(&arch_mesh(40.0, 20.0)) > 15.0,
            "a 45° flank prints over thin air, so the space under the ridge is free"
        );
        assert_eq!(
            centre(&table_mesh(40.0, 20.0)),
            0.0,
            "a flat top grows support to the plate, so it owns the space below"
        );
    }

    /// A mushroom: a flat `span` × `span` cap, 4 mm thick, standing `h` above
    /// the plate on a 10 mm post in its middle.
    fn mushroom_mesh(span: f64, h: f64) -> Mesh {
        let mut mesh = sheet(&square(span, span), h);
        mesh.faces.extend(sheet(&square(span, span), h + 4.0).faces);
        let (a, b) = (span / 2.0 - 5.0, span / 2.0 + 5.0);
        mesh.faces
            .extend(sheet(&[[a, a], [b, a], [b, b], [a, b]], 0.0).faces);
        for (p, q) in [
            ([a, a], [b, a]),
            ([b, a], [b, b]),
            ([b, b], [a, b]),
            ([a, b], [a, a]),
        ] {
            fan(
                &[
                    [p[0], p[1], 0.0],
                    [q[0], q[1], 0.0],
                    [q[0], q[1], h],
                    [p[0], p[1], h],
                ],
                &mut mesh,
            );
        }
        mesh
    }

    #[test]
    fn a_flat_cap_on_a_post_keeps_all_the_space_beneath_it() {
        // Step from the post's foot to the cap's underside and the height
        // jumps by the whole post — steep, if all you look at is heights. The
        // cap itself is flat and grows support right up to the post, so not a
        // single column under it may be lent to another part.
        let shape = rasterise(
            &footprint(&mushroom_mesh(60.0, 30.0)),
            0.0,
            0.5,
            &PackOptions::default(),
        );
        let freed = shape
            .base
            .iter()
            .filter(|b| b.is_finite() && **b > 0.0)
            .count();
        assert_eq!(freed, 0, "{freed} columns under a flat cap were left free");
        assert!(!shape.has_headroom);
    }

    /// A one-row shape whose columns claim `spans`; `None` leaves a hole.
    fn columns(spans: &[Option<(f32, f32)>]) -> Shape {
        let mut shape = Shape {
            grid: BitGrid::new(spans.len(), 1),
            base: vec![f32::INFINITY; spans.len()],
            top: vec![f32::NEG_INFINITY; spans.len()],
            offset: [0.0, 0.0],
            has_headroom: true,
            floor: BitGrid::new(0, 0),
            form: 0,
            tops: [0.0, 0.0],
            runs: Vec::new(),
            floor_runs: Vec::new(),
        };
        for (x, span) in spans.iter().enumerate() {
            if let Some((lo, hi)) = *span {
                shape.grid.set(x, 0);
                shape.base[x] = lo;
                shape.top[x] = hi;
            }
        }
        shape.seal()
    }

    #[test]
    fn parts_never_pass_each_other_both_ways() {
        // Every column taken alone has room: the candidate clears the placed
        // part overhead on the left and underneath on the right. Together they
        // would hook into each other, and neither could be lifted off first.
        let mut plate = Plate::new(BitGrid::new(2, 1), 1.0);
        plate.commit(&columns(&[Some((0.0, 10.0)), Some((20.0, 30.0))]), 0, 0, 0);
        let hook = columns(&[Some((15.0, 25.0)), Some((0.0, 10.0))]);
        assert!(!plate.fits(&hook, 0, 0));

        // Under the placed part in both columns is fine.
        let under = columns(&[None, Some((0.0, 10.0))]);
        assert!(plate.fits(&under, 0, 0));
    }

    #[test]
    fn stacking_never_closes_a_loop() {
        // P0 passes over P1 in column 1. A candidate over P0 in column 0 and
        // under P1 in column 2 is consistent with each of them — and closes
        // the loop P0 over P1 over candidate over P0, which nobody can unload.
        let mut plate = Plate::new(BitGrid::new(3, 1), 1.0);
        plate.commit(&columns(&[Some((0.0, 5.0)), Some((20.0, 25.0))]), 0, 0, 0);
        plate.commit(&columns(&[Some((0.0, 10.0)), Some((30.0, 35.0))]), 1, 0, 0);
        let looped = columns(&[Some((10.0, 15.0)), None, Some((0.0, 20.0))]);
        assert!(!plate.fits(&looped, 0, 0));

        // Over P0 and clear of P1 is fine.
        let open = columns(&[Some((10.0, 15.0))]);
        assert!(plate.fits(&open, 0, 0));
    }

    /// A closed solid between two rings of corners — `bottom` and `top`, the
    /// same length, corner `i` of one joined to corner `i` of the other.
    fn loft(bottom: &[[f64; 3]], top: &[[f64; 3]]) -> Mesh {
        let mut mesh = Mesh::new();
        let v = |p: [f64; 3]| Vertex::new(p[0], p[1], p[2]);
        let n = bottom.len();
        for i in 1..n - 1 {
            mesh.faces
                .push(Face::new([v(bottom[0]), v(bottom[i + 1]), v(bottom[i])]));
            mesh.faces
                .push(Face::new([v(top[0]), v(top[i]), v(top[i + 1])]));
        }
        for i in 0..n {
            let j = (i + 1) % n;
            mesh.faces
                .push(Face::new([v(bottom[i]), v(bottom[j]), v(top[j])]));
            mesh.faces
                .push(Face::new([v(bottom[i]), v(top[j]), v(top[i])]));
        }
        mesh
    }

    fn ring(r: f64, z: f64) -> Vec<[f64; 3]> {
        (0..32)
            .map(|i| {
                let a = i as f64 / 32.0 * std::f64::consts::TAU;
                [r * a.cos(), r * a.sin(), z]
            })
            .collect()
    }

    fn square_at(w: f64, z: f64, dx: f64) -> Vec<[f64; 3]> {
        vec![[dx, 0.0, z], [dx + w, 0.0, z], [dx + w, w, z], [dx, w, z]]
    }

    /// An 18 mm square post, 45 mm tall, leaning 35° from vertical.
    fn leaning_mesh() -> Mesh {
        let h = 45.0;
        loft(
            &square_at(18.0, 0.0, 0.0),
            &square_at(18.0, h, h * 35f64.to_radians().tan()),
        )
    }

    /// A funnel on its narrow end: 8 mm radius on the plate flaring to 30 mm
    /// at 30 mm up — 36° from vertical, so it prints without support.
    fn funnel_mesh() -> Mesh {
        loft(&ring(8.0, 0.0), &ring(30.0, 30.0))
    }

    #[test]
    fn leaning_parts_stand_like_books() {
        // Side by side, three leaning posts need three shadows — well over the
        // plate's 120 mm. Each one's foot fits under the next one's lean, like
        // books leaning on a shelf, and then they take half that. The posts'
        // upright side walls are slanted parallelograms, which is what this
        // guards: read as their whole height, they fill in the lean.
        let bed = BedConfig {
            width: 120.0,
            depth: 30.0,
            ..default_bed()
        };
        let fps: Vec<Footprint> = (0..3).map(|_| footprint(&leaning_mesh())).collect();
        let out = pack_objects(&fps, &bed, &PackOptions::default());
        assert!(out.iter().all(|p| p.on_bed), "{out:?}");
    }

    #[test]
    fn small_parts_tuck_under_a_flared_rim() {
        // A funnel fills a 64 mm plate edge to edge in plan view, leaving only
        // the corners of its box. The cubes fit there only by reaching in
        // under the rim, which is high enough over them to print.
        let bed = BedConfig {
            width: 64.0,
            depth: 64.0,
            ..default_bed()
        };
        let mut fps = vec![footprint(&funnel_mesh())];
        fps.extend((0..4).map(|_| footprint(&box_mesh(10.0, 10.0))));
        let out = pack_objects(&fps, &bed, &PackOptions::default());
        assert!(out.iter().all(|p| p.on_bed), "{out:?}");

        let flat = pack_objects(
            &fps,
            &bed,
            &PackOptions {
                vertical_nesting: false,
                ..PackOptions::default()
            },
        );
        assert!(
            flat.iter().any(|p| !p.on_bed),
            "the plate should be too small without nesting, or this proves nothing"
        );
    }

    #[test]
    fn round_parts_fall_back_to_two_by_two() {
        // Four 60 mm discs fit a 126 mm plate two by two and in no other way.
        // Placed greedily, the third settles in the dip between the first two
        // and the fourth has nowhere left to go.
        let bed = BedConfig {
            width: 126.0,
            depth: 126.0,
            ..default_bed()
        };
        let disc = || footprint(&loft(&ring(30.0, 0.0), &ring(30.0, 10.0)));
        let fps: Vec<Footprint> = (0..4).map(|_| disc()).collect();
        let out = pack_objects(&fps, &bed, &PackOptions::default());
        assert!(out.iter().all(|p| p.on_bed), "{out:?}");
    }

    #[test]
    fn dilation_grows_by_a_disc() {
        let mut g = BitGrid::new(9, 9);
        g.set(4, 4);
        let out = dilate(&g, 3);
        // Padded by 3 on every side, so the seed sits at (7,7).
        assert!(out.get(7, 7));
        assert!(out.get(7 + 3, 7), "3 cells right is inside the disc");
        assert!(out.get(7, 7 - 3), "3 cells up is inside the disc");
        assert!(!out.get(7 + 3, 7 + 3), "the corner is outside the disc");
    }
}
