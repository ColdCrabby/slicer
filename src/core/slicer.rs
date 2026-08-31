use clipper2::*;

use crate::mesh::types::{Mesh, Vertex};

use super::types::{ExtrusionRole, SliceLayer};

/// Vertical offset (mm) added to every layer's sampling plane so it never
/// coincides with a model's horizontal faces.  See the note at the sampling
/// call in [`slice_mesh`]: intersecting exactly through a flat deck/floor makes
/// many vertices land on the plane at once and yields a degenerate,
/// order-dependent cross-section.  ~1 µm is far below any printable feature yet
/// large enough to clear f32 vertex quantisation.
const SLICE_EPSILON: f64 = 1e-3;

/// Interpolate the XY intersection point of a triangle edge with a Z plane.
///
/// Given two vertices `a` and `b` that straddle the plane `z`, returns the XY
/// point where the edge crosses that plane.
fn edge_intersect(a: Vertex, b: Vertex, z: f64) -> (f64, f64) {
    let t = (z - a.z) / (b.z - a.z);
    (a.x + t * (b.x - a.x), a.y + t * (b.y - a.y))
}

/// Slice a mesh into layers separated by `layer_height` millimeters.
///
/// For each layer plane the function intersects every triangle with the plane
/// and chains the resulting line segments into closed contour paths.  The
/// contours are stored in a [`SliceLayer`] using Clipper2's [`Paths`] type so
/// they can be used directly with Boolean or offset operations.
///
/// This function only generates perimeter paths. Use [`add_infill_to_layers`]
/// to add infill patterns to the layers after slicing.
///
/// # Arguments
/// * `mesh`         – triangle mesh in millimetres
/// * `layer_height` – distance between layer planes in mm (must be > 0)
///
/// # Returns
/// A `Vec<SliceLayer>` ordered from bottom to top.  Empty if the mesh has no
/// faces or `layer_height` is not positive.
///
/// # Example
/// ```
/// use slicer_engine::mesh::types::{Face, Mesh, Vertex};
/// use slicer_engine::core::slice_mesh;
///
/// let v = [
///     Vertex::new(0.0, 0.0, 0.0),
///     Vertex::new(10.0, 0.0, 0.0),
///     Vertex::new(0.0, 10.0, 0.0),
///     Vertex::new(0.0, 0.0, 10.0),
/// ];
/// let mesh = Mesh {
///     vertices: v.to_vec(),
///     faces: vec![Face::new([v[0], v[1], v[3]]), Face::new([v[0], v[2], v[3]])],
///     aabb: None,
/// };
/// let layers = slice_mesh(&mesh, 2.0);
/// assert!(!layers.is_empty());
/// ```
pub fn slice_mesh(mesh: &Mesh, layer_height: f64) -> Vec<SliceLayer> {
    slice_mesh_with_first_layer(mesh, layer_height, layer_height)
}

/// Slice a mesh whose **first layer** is a different thickness from the rest.
///
/// A thicker first layer is the standard remedy for an imperfect bed: the extra
/// material absorbs the variation a mesh bed levels out only approximately, so
/// almost every shipped profile sets one. Only the bottom-most layer is
/// affected; everything above it is spaced by `layer_height` as usual.
///
/// Layer planes are sampled at the **middle** of the material each layer
/// deposits, which is what keeps the cross-section representative of the whole
/// slab rather than of one of its faces. That convention is preserved exactly:
/// passing `first_layer_height == layer_height` reproduces [`slice_mesh`]
/// plane-for-plane.
///
/// ```text
///   first_layer_height = 0.24, layer_height = 0.20
///
///   0.00 ─────────────────  bed
///                    · 0.12 ← layer 0 sampled here, prints 0.24mm
///   0.24 ─────────────────
///                    · 0.34 ← layer 1 sampled here, prints 0.20mm
///   0.44 ─────────────────
/// ```
///
/// # Arguments
/// * `mesh`               – triangle mesh in millimetres
/// * `layer_height`       – distance between layer planes in mm (must be > 0)
/// * `first_layer_height` – thickness of the bottom layer in mm; values `<= 0`
///   fall back to `layer_height`
///
/// # Returns
/// A `Vec<SliceLayer>` ordered from bottom to top. Empty if the mesh has no
/// faces or `layer_height` is not positive.
pub fn slice_mesh_with_first_layer(
    mesh: &Mesh,
    layer_height: f64,
    first_layer_height: f64,
) -> Vec<SliceLayer> {
    if mesh.faces.is_empty() || layer_height <= 0.0 {
        return Vec::new();
    }

    // Determine Z extent from vertices
    let z_min = mesh
        .vertices
        .iter()
        .map(|v| v.z)
        .fold(f64::INFINITY, f64::min);
    let z_max = mesh
        .vertices
        .iter()
        .map(|v| v.z)
        .fold(f64::NEG_INFINITY, f64::max);

    if z_min >= z_max {
        return Vec::new();
    }

    // Layer planes are sampled at the middle of the material each layer lays
    // down, so the first plane sits half a *first* layer above the mesh bottom
    // and the step into layer 1 spans half of each.
    let first_h = if first_layer_height > 0.0 {
        first_layer_height
    } else {
        layer_height
    };
    let first_z = z_min + first_h * 0.5;
    let layer_count = ((z_max - first_z) / layer_height).ceil() as usize + 1;

    let mut layers = Vec::with_capacity(layer_count);

    let mut z = first_z;
    let mut index = 0usize;
    while z < z_max {
        // Sample the cross-section a hair above the nominal layer plane.  A
        // model's horizontal faces (decks, floors) frequently sit *exactly* on a
        // layer plane; intersecting straight through such a face makes hundreds
        // of vertices land on the plane at once, and the resulting cross-section
        // is degenerate and precision/order-dependent — the source of a phantom,
        // left/right-asymmetric island on the 3DBenchy cabin deck.  A ~1 µm
        // offset makes every vertex fall strictly above or below the plane
        // without measurably moving the contour.
        let segments = collect_segments(mesh, z + SLICE_EPSILON);
        let contours = chain_segments(segments);

        let mut layer = SliceLayer::new(z);
        for contour in contours {
            if contour.len() >= 3 {
                let path: Path = contour.into();
                layer.paths.push(path);
                layer.path_roles.push(ExtrusionRole::OuterWall);
            }
        }

        layers.push(layer);
        // Stepping off layer 0 crosses half of it and half of layer 1; every
        // step after that is a whole `layer_height`. When the two heights are
        // equal this is exactly `z += layer_height` throughout.
        z += if index == 0 {
            (first_h + layer_height) * 0.5
        } else {
            layer_height
        };
        index += 1;
    }

    layers
}

/// Collect the XY line segments produced by intersecting `mesh` with the
/// horizontal plane at height `z`, **oriented** by triangle winding.
///
/// Each crossed triangle contributes one directed segment `enter → exit`, where
/// `enter` is the intersection on the edge running from below the plane to
/// on/above it and `exit` the edge running from on/above back to below.  Because
/// every segment is emitted in this consistent direction, [`chain_segments`] can
/// trace a contour by simply following the segment whose start matches the
/// current segment's end — a deterministic walk that cannot mis-turn where
/// several segments meet, unlike a coordinate-matched greedy search.
///
/// A vertex exactly on the plane counts as *above* (`z_coord ≥ z`); a triangle
/// that merely grazes the plane at a single vertex therefore collapses to a
/// zero-length segment (dropped in [`chain_segments`]) instead of a spurious
/// stub, and each grazing *edge* is emitted once, by the triangle below it.
fn collect_segments(mesh: &Mesh, z: f64) -> Vec<[(f64, f64); 2]> {
    let mut segments = Vec::new();

    for face in &mesh.faces {
        let [v0, v1, v2] = face.vertices;

        let mut enter: Option<(f64, f64)> = None;
        let mut exit: Option<(f64, f64)> = None;
        for (a, b) in [(v0, v1), (v1, v2), (v2, v0)] {
            let below_a = a.z < z;
            let below_b = b.z < z;
            if below_a && !below_b {
                enter = Some(edge_intersect(a, b, z));
            } else if below_b && !below_a {
                exit = Some(edge_intersect(a, b, z));
            }
        }

        if let (Some(enter), Some(exit)) = (enter, exit) {
            segments.push([enter, exit]);
        }
    }

    segments
}

/// Chain **oriented** line segments into closed contour polygons.
///
/// [`collect_segments`] emits every segment directed `start → end` by triangle
/// winding, so a contour is traced deterministically: the next segment is the
/// (unused) one whose start coincides with the current segment's end.  Endpoints
/// are snapped to a 0.1 µm grid (`SCALE`) before comparison to absorb the tiny
/// floating-point differences between adjacent triangles.
///
/// Zero-length segments — produced by near-horizontal triangles grazing the
/// plane — are dropped up front.  Where a cross-section *self-touches* (a pinch
/// point whose rounded key is shared by more than one segment, e.g. two cabin
/// lobes meeting at a corner), the next segment is chosen as the **straightest
/// continuation** (smallest turn) rather than by list order.  Both make the
/// walk order-independent: the old coordinate-matched greedy chainer treated
/// grazing stubs and pinch junctions as real and mis-turned into phantom loops
/// whose resolution flipped with the face order — the source of the spurious,
/// left/right-asymmetric triangular island beside the 3DBenchy cabin.
fn chain_segments(segments: Vec<[(f64, f64); 2]>) -> Vec<Vec<(f64, f64)>> {
    if segments.is_empty() {
        return Vec::new();
    }

    // Represent coordinates as (i64, i64) keyed at 10 000× precision (0.1 µm).
    const SCALE: f64 = 10_000.0;
    let key = |p: (f64, f64)| -> (i64, i64) {
        ((p.0 * SCALE).round() as i64, (p.1 * SCALE).round() as i64)
    };

    // Drop zero-length (grazing) segments, then index the rest by start key.
    let segments: Vec<[(f64, f64); 2]> = segments
        .into_iter()
        .filter(|s| key(s[0]) != key(s[1]))
        .collect();

    let mut start_map: std::collections::HashMap<(i64, i64), Vec<usize>> =
        std::collections::HashMap::new();
    for (i, seg) in segments.iter().enumerate() {
        start_map.entry(key(seg[0])).or_default().push(i);
    }

    let mut used = vec![false; segments.len()];
    let mut contours: Vec<Vec<(f64, f64)>> = Vec::new();

    for s0 in 0..segments.len() {
        if used[s0] {
            continue;
        }

        let mut chain: Vec<(f64, f64)> = Vec::new();
        let mut current = s0;
        loop {
            if used[current] {
                break;
            }
            used[current] = true;
            let [a, b] = segments[current];
            chain.push(a);

            // Among unused segments starting where this one ends, follow the
            // straightest (smallest-turn) continuation.  With a single candidate
            // this is just "the next segment"; at a self-touch it keeps the walk
            // on the same contour instead of hopping across the pinch.
            let din = (b.0 - a.0, b.1 - a.1);
            let mut next: Option<usize> = None;
            let mut best_turn = f64::INFINITY;
            if let Some(cands) = start_map.get(&key(b)) {
                for &i in cands {
                    if used[i] {
                        continue;
                    }
                    let [c0, c1] = segments[i];
                    let dout = (c1.0 - c0.0, c1.1 - c0.1);
                    let cross = din.0 * dout.1 - din.1 * dout.0;
                    let dot = din.0 * dout.0 + din.1 * dout.1;
                    let turn = cross.atan2(dot).abs();
                    if turn < best_turn {
                        best_turn = turn;
                        next = Some(i);
                    }
                }
            }
            match next {
                Some(n) => current = n,
                None => break,
            }
        }

        if chain.len() >= 3 {
            contours.push(chain);
        }
    }

    contours
}
