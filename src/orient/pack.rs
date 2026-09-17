//! Shape-aware nesting: pack objects on the bed by their real outline.
//!
//! ## Why not bounding boxes
//!
//! A bounding box is a bad description of a printed part. An L-bracket, a
//! bracket arm, anything printed at 45° — each one wastes most of its box on
//! air, and a packer that reasons about boxes leaves that air unusable. Two
//! parts whose boxes overlap heavily can still sit side by side without
//! touching, and that is exactly the arrangement that fits a plate no
//! box-packer can.
//!
//! So this module packs the **footprint** — the projection of the object's
//! own triangles onto the plate — and lets concave parts nest into each
//! other's hollows.
//!
//! ## How
//!
//! The footprint is rasterised into a bitmask over a grid of the bed
//! (sub-millimetre cells, see [`cell_size`]). Everything after that is
//! bitwise:
//!
//! 1. **The bed is the occupancy map.** The grid starts with every cell that
//!    is *not* fully on the plate already marked occupied, so "does it fit on
//!    the bed" and "does it hit another part" are the same test.
//! 2. **Largest first.** Objects are placed in descending footprint order, so
//!    the parts with the fewest choices choose first.
//! 3. **Each object tries every rotation** in `rotation_step_deg` steps and
//!    every position, and takes the one closest to the bed centre. Positions
//!    are visited in rings outwards from the ideal spot, and the search stops
//!    as soon as the next ring cannot beat the best placement found — which
//!    makes "closest to centre" exact rather than a heuristic.
//! 4. **Spacing is applied when a part is committed**, by dilating its mask by
//!    a disc of `spacing_mm` before OR-ing it into the occupancy map. The next
//!    part is then tested with its *true* outline, so the gap between any two
//!    parts is the requested spacing measured the honest way — between the
//!    outlines, not between the boxes.
//!
//! Rasterisation is **conservative**: a cell is occupied if a triangle touches
//! it at all, and a bed cell counts as printable only if the whole cell is on
//! the plate. Parts can therefore end up up to one cell further apart than
//! asked, but never closer, and never off the plate.
//!
//! ## Rotation
//!
//! The default step is 90°, which is free: it cannot undo an auto-orient
//! result, and it preserves a printer's preferred Z-rotation (a CoreXY machine
//! asking for 45° still gets a diagonal part after a quarter turn). Finer
//! steps nest better and are available through
//! [`ArrangeOptions::rotation_step_deg`](super::ArrangeOptions), at the cost of
//! turning parts off whatever angle the user chose.

use crate::mesh::types::Mesh;
use crate::scene::bed::BedConfig;
use crate::scene::transform::Transform;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Longest bed axis is divided into at most this many cells.
///
/// 512 cells across a 220 mm bed is a 0.43 mm cell — finer than the placement
/// error anyone can see, and a grid row still fits in 8 machine words, which
/// is what keeps the collision test a handful of `AND`s.
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

/// Triangles with a projected area below this (mm²) contribute nothing but
/// their outline — kept, because a mesh with no bottom face relies on its
/// vertical walls to describe the footprint at all.
const DEGENERATE_AREA_MM2: f64 = 1e-9;

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
    /// rotation, and the rotation turns the object about it — so a caller
    /// turns the object about that point and then moves the point here.
    pub pivot: [f64; 2],
    /// `false` when the object did not fit on the plate and was parked beside
    /// it. Nothing is discarded; the caller decides how to say so.
    pub on_bed: bool,
}

/// Knobs for [`pack_objects`].
#[derive(Debug, Clone, Copy)]
pub struct PackOptions {
    /// Gap left between two outlines, in millimetres.
    pub spacing_mm: f64,
    /// Rotation step tried per object, in degrees. `0` (or ≥ 360) packs every
    /// object at the angle it arrived in.
    pub rotation_step_deg: f64,
}

impl Default for PackOptions {
    fn default() -> Self {
        Self {
            spacing_mm: 2.0,
            rotation_step_deg: 90.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Footprint
// ---------------------------------------------------------------------------

/// The shadow an object casts on the plate: its triangles projected to XY.
///
/// Built once per object from the mesh and its current transform, then
/// rasterised again per rotation candidate — projecting is the expensive part
/// and it does not depend on the angle.
#[derive(Debug, Clone)]
pub struct Footprint {
    /// Projected triangles, relative to [`pivot`](Self::pivot).
    tris: Vec<[[f32; 2]; 3]>,
    /// Centre of the object's world XY bounds, in scene millimetres.
    pub pivot: [f64; 2],
    /// Width and depth of the footprint's bounds (mm), used to order objects.
    pub extent: [f64; 2],
}

impl Footprint {
    /// Project `mesh` through `transform` onto the plate.
    pub fn new(mesh: &Mesh, transform: &Transform) -> Self {
        let matrix = transform.to_matrix();
        let mut points: Vec<[f32; 2]> = Vec::with_capacity(mesh.faces.len() * 3);
        for face in &mesh.faces {
            for v in &face.vertices {
                let p =
                    matrix.transform_point3(glam::Vec3::new(v.x as f32, v.y as f32, v.z as f32));
                points.push([p.x, p.y]);
            }
        }

        let (mut min_x, mut min_y) = (f32::INFINITY, f32::INFINITY);
        let (mut max_x, mut max_y) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for p in &points {
            min_x = min_x.min(p[0]);
            min_y = min_y.min(p[1]);
            max_x = max_x.max(p[0]);
            max_y = max_y.max(p[1]);
        }
        if !min_x.is_finite() {
            return Self {
                tris: Vec::new(),
                pivot: [0.0, 0.0],
                extent: [0.0, 0.0],
            };
        }

        let pivot = [
            (min_x as f64 + max_x as f64) / 2.0,
            (min_y as f64 + max_y as f64) / 2.0,
        ];
        let (px, py) = (pivot[0] as f32, pivot[1] as f32);
        let tris = points
            .as_chunks::<3>()
            .0
            .iter()
            .filter_map(|t| {
                let a = [t[0][0] - px, t[0][1] - py];
                let b = [t[1][0] - px, t[1][1] - py];
                let c = [t[2][0] - px, t[2][1] - py];
                let area = ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs();
                let span = (b[0] - a[0])
                    .abs()
                    .max((c[0] - a[0]).abs())
                    .max((b[1] - a[1]).abs())
                    .max((c[1] - a[1]).abs());
                // A triangle that projects to a single point says nothing; one
                // that projects to a line is a wall, and walls are the outline.
                if (area as f64) < DEGENERATE_AREA_MM2 && span < f32::EPSILON {
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
        }
    }

    fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    /// Identity of the *shape*, ignoring where it sits.
    ///
    /// A plate is usually one model many times over, and rasterising the same
    /// outline once per copy per angle is the bulk of the work. Two footprints
    /// with the same fingerprint rasterise identically, so the mask is built
    /// once and shared.
    fn fingerprint(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.tris.len().hash(&mut hasher);
        for t in &self.tris {
            for p in t {
                p[0].to_bits().hash(&mut hasher);
                p[1].to_bits().hash(&mut hasher);
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

// ---------------------------------------------------------------------------
// Rasterisation
// ---------------------------------------------------------------------------

/// A footprint rasterised at one angle, with where its grid sits.
#[derive(Clone)]
struct Mask {
    grid: BitGrid,
    /// Offset of mask cell (0,0)'s corner from the footprint pivot, in mm.
    offset: [f64; 2],
}

/// Rasterise `fp` rotated by `angle_deg` into cells of `cell` mm.
///
/// Conservative: every cell a triangle touches is set.
fn rasterise(fp: &Footprint, angle_deg: f64, cell: f64) -> Mask {
    let (sin, cos) = angle_deg.to_radians().sin_cos();
    let (sin, cos) = (sin as f32, cos as f32);
    let rotate = |p: [f32; 2]| [p[0] * cos - p[1] * sin, p[0] * sin + p[1] * cos];

    let tris: Vec<[[f32; 2]; 3]> = fp
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
        return Mask {
            grid: BitGrid::new(0, 0),
            offset: [0.0, 0.0],
        };
    }

    // Snap the mask origin to a cell boundary below the footprint so the
    // rasterisation of a part never depends on where it happened to sit.
    let origin = [
        (min_x as f64 / cell).floor() * cell,
        (min_y as f64 / cell).floor() * cell,
    ];
    let cols = (((max_x as f64 - origin[0]) / cell).ceil() as usize + 1).max(1);
    let rows = (((max_y as f64 - origin[1]) / cell).ceil() as usize + 1).max(1);
    let mut grid = BitGrid::new(cols, rows);

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
        for cy in y0..=y1 {
            let center_y = (origin[1] + (cy as f64 + 0.5) * cell) as f32;
            for cx in x0..=x1 {
                if grid.get(cx, cy) {
                    continue;
                }
                let center_x = (origin[0] + (cx as f64 + 0.5) * cell) as f32;
                if triangle_hits_cell(t, [center_x, center_y], half) {
                    grid.set(cx, cy);
                }
            }
        }
    }

    Mask {
        grid,
        offset: origin,
    }
}

/// Separating-axis test between a triangle and an axis-aligned square.
///
/// The two axis-aligned axes are already covered by the caller's bbox walk, so
/// only the triangle's three edge normals are left.
fn triangle_hits_cell(tri: &[[f32; 2]; 3], center: [f32; 2], half: f32) -> bool {
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
// Packing
// ---------------------------------------------------------------------------

/// Grid cell size for `bed`, in millimetres.
pub fn cell_size(bed: &BedConfig) -> f64 {
    let longest = bed.width.max(bed.depth).max(1.0);
    (longest / MAX_GRID_CELLS as f64).max(MIN_CELL_MM)
}

/// Build the occupancy map: everything not fully on the plate starts occupied.
fn bed_occupancy(bed: &BedConfig, cell: f64) -> (BitGrid, [f64; 2]) {
    let origin = [bed.origin_offset_x, bed.origin_offset_y];
    let cols = ((bed.width / cell).floor() as usize).max(1);
    let rows = ((bed.depth / cell).floor() as usize).max(1);
    let mut grid = BitGrid::new(cols, rows);
    for y in 0..rows {
        let y0 = origin[1] + y as f64 * cell;
        for x in 0..cols {
            let x0 = origin[0] + x as f64 * cell;
            let printable = bed.contains_xy(x0, y0)
                && bed.contains_xy(x0 + cell, y0)
                && bed.contains_xy(x0, y0 + cell)
                && bed.contains_xy(x0 + cell, y0 + cell);
            if !printable {
                grid.set(x, y);
            }
        }
    }
    (grid, origin)
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

/// Nest `footprints` on `bed` and return where each one goes.
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
    let (bed_mask, origin) = bed_occupancy(bed, cell);
    let mut occupancy = bed_mask.clone();
    let halo = if options.spacing_mm > 0.0 {
        (options.spacing_mm / cell).ceil() as usize
    } else {
        0
    };
    let angles = rotation_candidates(options.rotation_step_deg);

    // Largest footprint first: the parts with the fewest places to go choose
    // before the small ones fill those places in.
    let mut order: Vec<usize> = (0..footprints.len()).collect();
    order.sort_by(|&a, &b| {
        let area_a = footprints[a].extent[0] * footprints[a].extent[1];
        let area_b = footprints[b].extent[0] * footprints[b].extent[1];
        area_b
            .partial_cmp(&area_a)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });

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
    let mut masks: HashMap<(u64, u64), Mask> = HashMap::new();
    for &idx in &order {
        let fp = &footprints[idx];
        if fp.is_empty() {
            continue;
        }
        let shape = fp.fingerprint();

        let mut best: Option<(f64, f64, Mask, usize, usize)> = None; // cost, angle, mask, ix, iy
        for &angle in &angles {
            let mask = masks
                .entry((shape, angle.to_bits()))
                .or_insert_with(|| rasterise(fp, angle, cell))
                .clone();
            if mask.grid.cols > occupancy.cols || mask.grid.rows > occupancy.rows {
                continue;
            }
            let limit_x = occupancy.cols - mask.grid.cols;
            let limit_y = occupancy.rows - mask.grid.rows;

            // Pack into the plate's low corner, not its middle. Gravity
            // towards the centre sounds right and packs badly: the first big
            // part lands squarely on the space every later part needs. The
            // whole arrangement is moved back to the centre once it is done,
            // and `DEPTH_BIAS` is what makes the pile grow in rows on the way.
            if let Some((cost, ix, iy)) = search(
                &occupancy,
                &mask.grid,
                0.0,
                0.0,
                limit_x,
                limit_y,
                cell,
                best.as_ref().map(|b| b.0),
            ) {
                if best.as_ref().is_none_or(|b| cost < b.0) {
                    best = Some((cost, angle, mask, ix, iy));
                }
            }
        }

        match best {
            Some((_, angle, mask, ix, iy)) => {
                let placed = &mut placements[idx];
                placed.angle_deg = angle;
                placed.pivot = [
                    origin[0] + ix as f64 * cell - mask.offset[0],
                    origin[1] + iy as f64 * cell - mask.offset[1],
                ];
                placed.on_bed = true;
                let halo_mask = dilate(&mask.grid, halo);
                occupancy.or_clipped(
                    &halo_mask,
                    ix as isize - halo as isize,
                    iy as isize - halo as isize,
                );
                placed_cells.push((idx, mask.grid, ix, iy));
            }
            None => {
                // Park it to the right of the plate, stacked downwards. The
                // object stays visible and out of bounds instead of vanishing
                // or silently overlapping a part that did fit.
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

    let (shift_x, shift_y) = recentring_shift(&bed_mask, &placed_cells);
    for (idx, ..) in &placed_cells {
        let placed = &mut placements[*idx];
        placed.pivot[0] += shift_x as f64 * cell;
        placed.pivot[1] += shift_y as f64 * cell;
    }

    placements
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

/// Find the feasible cell offset closest to `(ideal_x, ideal_y)`.
///
/// Positions are visited in square rings outwards. A ring `r` cannot hold
/// anything closer than `r * cell` to the ideal spot, so once that exceeds the
/// best distance found the search is provably finished — the result is the
/// true nearest placement, not the first acceptable one.
#[allow(clippy::too_many_arguments)]
fn search(
    occupancy: &BitGrid,
    mask: &BitGrid,
    ideal_x: f64,
    ideal_y: f64,
    limit_x: usize,
    limit_y: usize,
    cell: f64,
    ceiling: Option<f64>,
) -> Option<(f64, usize, usize)> {
    let cx = ideal_x.round() as isize;
    let cy = ideal_y.round() as isize;
    let max_ring = cx
        .abs()
        .max((limit_x as isize - cx).abs())
        .max(cy.abs())
        .max((limit_y as isize - cy).abs());

    let mut best: Option<(f64, usize, usize)> = None;
    // A placement is only interesting while it beats this. It starts at the
    // best the previous rotation found, so a hopeless angle is abandoned early.
    let mut bound = ceiling.unwrap_or(f64::INFINITY);
    for r in 0..=max_ring {
        // Nothing in ring `r` can be nearer than this, so once the floor
        // reaches the bound the answer is already in hand — which is what
        // makes "nearest the centre" exact rather than first-fit.
        if (r as f64 - 1.0).max(0.0) * cell >= bound {
            break;
        }

        for dy in -r..=r {
            let on_y_edge = dy.abs() == r;
            let mut dx = -r;
            while dx <= r {
                if !on_y_edge && dx.abs() != r {
                    dx = r; // interior of the ring was covered by earlier rings
                    continue;
                }
                let x = cx + dx;
                let y = cy + dy;
                dx += 1;
                if x < 0 || y < 0 || x as usize > limit_x || y as usize > limit_y {
                    continue;
                }
                let (ux, uy) = (x as usize, y as usize);
                let ddx = ux as f64 - ideal_x;
                let ddy = uy as f64 - ideal_y;
                let cost = (ddx * ddx + ddy * ddy * DEPTH_BIAS * DEPTH_BIAS).sqrt() * cell;
                if cost >= bound {
                    continue;
                }
                if !occupancy.collides(mask, ux, uy) {
                    best = Some((cost, ux, uy));
                    bound = cost;
                }
            }
        }
    }

    best
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

    /// A flat plate spanning `poly` (a closed CCW outline) at z = 0, fanned
    /// from its first vertex — enough geometry to cast the footprint we want.
    fn plate(poly: &[[f64; 2]]) -> Mesh {
        let mut mesh = Mesh::new();
        mesh.vertices = poly.iter().map(|p| Vertex::new(p[0], p[1], 0.0)).collect();
        for i in 1..poly.len() - 1 {
            mesh.faces.push(Face::new([
                Vertex::new(poly[0][0], poly[0][1], 0.0),
                Vertex::new(poly[i][0], poly[i][1], 0.0),
                Vertex::new(poly[i + 1][0], poly[i + 1][1], 0.0),
            ]));
        }
        mesh
    }

    fn box_mesh(w: f64, d: f64) -> Mesh {
        plate(&[[0.0, 0.0], [w, 0.0], [w, d], [0.0, d]])
    }

    /// An L: a `size × size` square with the top-right quarter cut away.
    fn l_mesh(size: f64) -> Mesh {
        let h = size / 2.0;
        plate(&[
            [0.0, 0.0],
            [size, 0.0],
            [size, h],
            [h, h],
            [h, size],
            [0.0, size],
        ])
    }

    fn footprint(mesh: &Mesh) -> Footprint {
        Footprint::new(mesh, &Transform::IDENTITY)
    }

    /// Minimum distance between two placed footprints, by brute force over
    /// their rasterised outlines. Slow, but it measures the thing the packer
    /// promises instead of the boxes around it.
    fn min_distance(a: &Footprint, pa: &Placement, b: &Footprint, pb: &Placement) -> f64 {
        let cell = 0.5;
        let sample = |fp: &Footprint, p: &Placement| -> Vec<[f64; 2]> {
            let mask = rasterise(fp, p.angle_deg, cell);
            let mut out = Vec::new();
            for y in 0..mask.grid.rows {
                for x in 0..mask.grid.cols {
                    if mask.grid.get(x, y) {
                        out.push([
                            p.pivot[0] + mask.offset[0] + (x as f64 + 0.5) * cell,
                            p.pivot[1] + mask.offset[1] + (y as f64 + 0.5) * cell,
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
        // The pivot is the footprint centre, so it should sit on the bed centre
        // to within a grid cell.
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
            rotation_step_deg: 90.0,
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
        // Two 100 mm Ls on a 160 × 110 mm plate. Their boxes need 201 mm
        // side by side and 201 mm stacked, so a box-packer cannot place the
        // second one at all; their outlines interlock into 151 × 101 mm.
        let bed = BedConfig {
            width: 160.0,
            depth: 110.0,
            ..default_bed()
        };
        let fps: Vec<Footprint> = (0..2).map(|_| footprint(&l_mesh(100.0))).collect();
        let opts = PackOptions {
            spacing_mm: 1.0,
            rotation_step_deg: 90.0,
        };
        let out = pack_objects(&fps, &bed, &opts);
        assert!(
            out.iter().all(|p| p.on_bed),
            "two 100 mm Ls must interlock onto a 160 x 110 mm plate: {out:?}"
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
                spacing_mm: 2.0,
                rotation_step_deg: 0.0,
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
