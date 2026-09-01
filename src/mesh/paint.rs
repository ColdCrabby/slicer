//! Per-facet paint annotations — the data behind paint-on support.
//!
//! A [`FacetPaint`] tags each triangle of a mesh as an **enforcer** (put support
//! here even if the overhang rule found none), a **blocker** (never put support
//! here), or nothing at all.  It is a plain parallel array over
//! [`Mesh::faces`](crate::mesh::Mesh), which is what makes it cheap to apply and
//! trivial to keep in step with the mesh.
//!
//! # Why a facet index is a safe key
//!
//! The browser picks a triangle with a Three.js raycast and the engine has to
//! agree which triangle that was.  That already holds:
//! `SceneHandle::getRenderBuffer` emits one independent triangle per face at
//! `base = face_idx * 3`, so a raycast's `faceIndex` **is** the index into
//! `mesh.faces`; `apply_transform` maps faces in order; `merge_meshes` appends
//! them in order; and every runtime loads through the same deterministic
//! repair pass.  Paint is the second consumer of that invariant — the first is
//! `SceneOp::PlaceFaceOnFloor`.
//!
//! The invariant is still *checked* rather than trusted: [`FacetPaint::decode`]
//! is handed the face count of the mesh it is about to be applied to and
//! refuses a payload that disagrees.  OrcaSlicer keys paint the same way and
//! documents silent corruption when a mesh changes underneath it; a loud
//! refusal is worth the one comparison.
//!
//! # Wire format
//!
//! Paint rides along with every slice request, so it is encoded compactly:
//!
//! ```text
//! version : u8   = 1
//! faces   : varint
//! for each state in [Enforcer, Blocker]:
//!     count   : varint
//!     indices : varint delta from the previous index, ascending
//! ```
//!
//! then base64url without padding, so it survives JSON and a query string
//! unescaped.
//!
//! Deltas rather than runs: STL facet order has no spatial locality, so a
//! contiguous painted patch is scattered through the index space and
//! run-length encoding degrades to two entries per face.  Ascending deltas
//! stay small for a clustered selection (most fit one byte) and degrade
//! gracefully when it is not.
//!
//! # Granularity
//!
//! Paint is **whole-facet**: a triangle is entirely enforced, entirely blocked,
//! or untouched.  Sub-triangle painting means subdividing facets under the
//! brush, which is a much larger algorithm; the `version` byte above exists so
//! it can be added without invalidating paint saved by this build.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::mesh::analysis::FaceAdjacency;
use crate::mesh::types::{Face, Mesh};

/// What the user painted onto a facet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum PaintState {
    /// Untouched — the automatic overhang rule decides.
    #[default]
    None = 0,
    /// Force support beneath this facet regardless of its overhang angle.
    Enforcer = 1,
    /// Never generate support beneath this facet.
    Blocker = 2,
}

impl PaintState {
    /// The two states that are actually stored, in encoding order.
    pub const PAINTED: [PaintState; 2] = [PaintState::Enforcer, PaintState::Blocker];

    fn from_u8(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(PaintState::None),
            1 => Some(PaintState::Enforcer),
            2 => Some(PaintState::Blocker),
            _ => None,
        }
    }
}

/// Per-facet paint for one mesh.
///
/// Empty until something is painted: an untouched object carries no allocation
/// and encodes to nothing, so the overwhelmingly common case is free.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FacetPaint {
    /// One [`PaintState`] per face, or empty when nothing is painted.
    states: Vec<u8>,
    /// Number of entries in `states` that are not [`PaintState::None`].
    painted: usize,
}

impl FacetPaint {
    /// An empty annotation — nothing painted.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether nothing at all is painted.
    ///
    /// The whole feature is skipped on this basis, so it must stay exact:
    /// painting a facet and erasing it again returns `true`.
    pub fn is_empty(&self) -> bool {
        self.painted == 0
    }

    /// How many facets carry a state other than [`PaintState::None`].
    pub fn painted_count(&self) -> usize {
        self.painted
    }

    /// Number of facets this annotation covers, or `0` when nothing is painted.
    pub fn face_count(&self) -> usize {
        self.states.len()
    }

    /// State of one facet. Out-of-range indices read as [`PaintState::None`].
    pub fn get(&self, face: usize) -> PaintState {
        self.states
            .get(face)
            .and_then(|&raw| PaintState::from_u8(raw))
            .unwrap_or(PaintState::None)
    }

    /// Paint one facet, growing the backing array on first use.
    ///
    /// `face_count` is the mesh's face count and is used only to size that
    /// first allocation. Painting out of range is ignored rather than
    /// panicking — a stale index from the UI must not take the engine down.
    pub fn set(&mut self, face: usize, state: PaintState, face_count: usize) {
        if face >= face_count {
            return;
        }
        if self.states.is_empty() {
            if state == PaintState::None {
                return;
            }
            self.states = vec![PaintState::None as u8; face_count];
        }
        let Some(slot) = self.states.get_mut(face) else {
            return;
        };
        let previous = *slot;
        if previous == state as u8 {
            return;
        }
        if previous != PaintState::None as u8 {
            self.painted -= 1;
        }
        if state != PaintState::None {
            self.painted += 1;
        }
        *slot = state as u8;
        if self.painted == 0 {
            // Erased back to nothing: drop the allocation so `is_empty` and the
            // encoding agree with a never-painted object.
            self.states.clear();
            self.states.shrink_to_fit();
        }
    }

    /// Erase everything.
    pub fn clear(&mut self) {
        self.states.clear();
        self.states.shrink_to_fit();
        self.painted = 0;
    }

    /// Facet indices carrying `state`, ascending.
    pub fn faces_with(&self, state: PaintState) -> impl Iterator<Item = usize> + '_ {
        let wanted = state as u8;
        self.states
            .iter()
            .enumerate()
            .filter(move |(_, &raw)| raw == wanted)
            .map(|(index, _)| index)
    }

    /// Append `other`'s facets after this annotation's, as
    /// [`merge_meshes`](crate::core::merge_meshes) concatenates faces.
    ///
    /// `self_faces` is this annotation's mesh face count, which may exceed
    /// [`face_count`](Self::face_count) when nothing is painted here yet — the
    /// offset has to come from the mesh, not from the (possibly empty) array.
    pub fn append(&mut self, self_faces: usize, other: &FacetPaint, other_faces: usize) {
        if other.is_empty() {
            if !self.states.is_empty() {
                self.states
                    .resize(self_faces + other_faces, PaintState::None as u8);
            }
            return;
        }
        if self.states.is_empty() {
            self.states = vec![PaintState::None as u8; self_faces];
        } else {
            self.states.resize(self_faces, PaintState::None as u8);
        }
        self.states
            .reserve(other_faces.max(other.states.len()));
        for face in 0..other_faces {
            self.states
                .push(other.states.get(face).copied().unwrap_or(0));
        }
        self.painted += other.painted;
    }

    /// Encode for the wire, or `None` when nothing is painted.
    ///
    /// The mesh's face count is embedded so [`decode`](Self::decode) can refuse
    /// a payload that no longer matches its mesh.
    pub fn encode(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut bytes = vec![FORMAT_VERSION];
        write_varint(&mut bytes, self.states.len() as u64);
        for state in PaintState::PAINTED {
            let indices: Vec<usize> = self.faces_with(state).collect();
            write_varint(&mut bytes, indices.len() as u64);
            let mut previous = 0usize;
            for index in indices {
                write_varint(&mut bytes, (index - previous) as u64);
                previous = index;
            }
        }
        Some(base64url_encode(&bytes))
    }

    /// Decode a payload produced by [`encode`](Self::encode).
    ///
    /// `expected_faces` is the face count of the mesh the paint is about to be
    /// applied to; a mismatch is an error rather than a silent misapplication
    /// onto whatever triangles happen to share those indices.
    pub fn decode(encoded: &str, expected_faces: usize) -> Result<Self, PaintDecodeError> {
        let bytes = base64url_decode(encoded).ok_or(PaintDecodeError::Malformed)?;
        let mut cursor = 0usize;

        let version = *bytes.first().ok_or(PaintDecodeError::Malformed)?;
        cursor += 1;
        if version != FORMAT_VERSION {
            return Err(PaintDecodeError::UnsupportedVersion(version));
        }

        let faces = read_varint(&bytes, &mut cursor).ok_or(PaintDecodeError::Malformed)? as usize;
        if faces != expected_faces {
            return Err(PaintDecodeError::FaceCountMismatch {
                encoded: faces,
                mesh: expected_faces,
            });
        }

        let mut paint = FacetPaint {
            states: vec![PaintState::None as u8; faces],
            painted: 0,
        };
        for state in PaintState::PAINTED {
            let count =
                read_varint(&bytes, &mut cursor).ok_or(PaintDecodeError::Malformed)? as usize;
            let mut index = 0usize;
            for step in 0..count {
                let delta =
                    read_varint(&bytes, &mut cursor).ok_or(PaintDecodeError::Malformed)? as usize;
                // The first index is an absolute offset from 0; every later one
                // is a strictly positive step, so a zero delta mid-run means a
                // duplicate index and the payload is not what we wrote.
                if step > 0 && delta == 0 {
                    return Err(PaintDecodeError::Malformed);
                }
                index += delta;
                if index >= faces {
                    return Err(PaintDecodeError::Malformed);
                }
                if paint.states[index] == PaintState::None as u8 {
                    paint.painted += 1;
                }
                paint.states[index] = state as u8;
            }
        }

        if cursor != bytes.len() {
            return Err(PaintDecodeError::Malformed);
        }
        if paint.painted == 0 {
            paint.clear();
        }
        Ok(paint)
    }
}

/// Version byte leading every encoded payload.
const FORMAT_VERSION: u8 = 1;

/// Default radius, in millimetres, used to weld near-duplicate vertices when
/// deriving mesh topology for the brush.
///
/// Matches the tolerance the coplanar-group pass uses for the face highlight,
/// so the brush and the highlight agree about which facets touch.
pub const BRUSH_WELD_TOLERANCE_MM: f64 = 0.001;

/// Paint every facet the brush covers, spreading across the surface from a seed.
///
/// `center` and `radius` are in the mesh's **local** frame — the caller
/// un-transforms the world-space hit point, so a scaled or rotated object needs
/// no special handling here and the paint stays attached to the geometry rather
/// than to a position on the plate.
///
/// The stroke starts at `seed_face` and grows only through
/// [`FaceAdjacency`], which is what keeps it on the surface the user can see.
/// A plain "every facet within the radius" test would paint straight through a
/// thin wall and out the other side — invisible from the camera, and impossible
/// to erase without rotating the model.
///
/// Returns the number of facets whose state actually changed.
pub fn paint_sphere(
    mesh: &Mesh,
    adjacency: &FaceAdjacency,
    paint: &mut FacetPaint,
    seed_face: usize,
    center: [f64; 3],
    radius: f64,
    state: PaintState,
) -> usize {
    let face_count = mesh.faces.len();
    if seed_face >= face_count || radius <= 0.0 {
        return 0;
    }

    let radius_sq = radius * radius;
    let mut changed = 0usize;
    let mut visited = vec![false; face_count];
    // The seed is painted whatever the radius, so a tap always marks the facet
    // under the cursor even when the brush is smaller than the triangle.
    let mut queue = vec![seed_face];
    visited[seed_face] = true;

    while let Some(face) = queue.pop() {
        if paint.get(face) != state {
            paint.set(face, state, face_count);
            changed += 1;
        }
        for &neighbour in adjacency.neighbours_of(face) {
            let neighbour = neighbour as usize;
            if visited[neighbour] {
                continue;
            }
            visited[neighbour] = true;
            if triangle_within_sphere(&mesh.faces[neighbour], center, radius_sq) {
                queue.push(neighbour);
            }
        }
    }
    changed
}

/// Whether any point of the triangle lies within `radius` of `center`.
///
/// Tests the true closest point on the triangle, not just its corners and
/// edges: on a coarse mesh a single facet can be far larger than the brush, and
/// a corner/edge test would report "no hit" for a stroke landing squarely in
/// the middle of one — the brush would do nothing exactly where the model is
/// simplest.
fn triangle_within_sphere(face: &Face, center: [f64; 3], radius_sq: f64) -> bool {
    let [a, b, c] = &face.vertices;
    let a = [a.x, a.y, a.z];
    let b = [b.x, b.y, b.z];
    let c = [c.x, c.y, c.z];
    let closest = closest_point_on_triangle(center, a, b, c);
    let d = sub(closest, center);
    dot(d, d) <= radius_sq
}

/// Closest point on triangle `abc` to `p` (Ericson, *Real-Time Collision
/// Detection*, §5.1.5) — a Voronoi-region walk over the three corners, the
/// three edges and the interior.
fn closest_point_on_triangle(
    p: [f64; 3],
    a: [f64; 3],
    b: [f64; 3],
    c: [f64; 3],
) -> [f64; 3] {
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }

    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }

    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let denom = d1 - d3;
        let v = if denom.abs() < 1e-20 { 0.0 } else { d1 / denom };
        return add(a, scale(ab, v));
    }

    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }

    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let denom = d2 - d6;
        let w = if denom.abs() < 1e-20 { 0.0 } else { d2 / denom };
        return add(a, scale(ac, w));
    }

    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let denom = (d4 - d3) + (d5 - d6);
        let w = if denom.abs() < 1e-20 { 0.0 } else { (d4 - d3) / denom };
        return add(b, scale(sub(c, b), w));
    }

    let denom = va + vb + vc;
    if denom.abs() < 1e-20 {
        return a;
    }
    let v = vb / denom;
    let w = vc / denom;
    add(add(a, scale(ab, v)), scale(ac, w))
}

#[inline]
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
#[inline]
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Why a paint payload could not be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaintDecodeError {
    /// The payload is not a paint encoding at all.
    Malformed,
    /// Written by a build using a newer format than this one understands.
    UnsupportedVersion(u8),
    /// The payload was painted onto a mesh with a different number of facets.
    FaceCountMismatch {
        /// Face count recorded when the paint was made.
        encoded: usize,
        /// Face count of the mesh it is being applied to.
        mesh: usize,
    },
}

impl fmt::Display for PaintDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PaintDecodeError::Malformed => write!(f, "support paint data is malformed"),
            PaintDecodeError::UnsupportedVersion(v) => write!(
                f,
                "support paint data uses format version {v}, which this build does not understand"
            ),
            PaintDecodeError::FaceCountMismatch { encoded, mesh } => write!(
                f,
                "support paint was painted on a mesh with {encoded} triangles but is being \
                 applied to one with {mesh}; the model changed since it was painted"
            ),
        }
    }
}

impl std::error::Error for PaintDecodeError {}

// ── varint + base64url ────────────────────────────────────────────────────
//
// Both are written out here rather than pulled in: this module ships in the
// plain wasm scene bundle, where the `base64` crate is not a dependency.

fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn read_varint(bytes: &[u8], cursor: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    let mut shift = 0u32;
    loop {
        let byte = *bytes.get(*cursor)?;
        *cursor += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn base64url_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64[(triple >> 18) as usize & 0x3f] as char);
        out.push(B64[(triple >> 12) as usize & 0x3f] as char);
        if chunk.len() > 1 {
            out.push(B64[(triple >> 6) as usize & 0x3f] as char);
        }
        if chunk.len() > 2 {
            out.push(B64[triple as usize & 0x3f] as char);
        }
    }
    out
}

fn base64url_decode(text: &str) -> Option<Vec<u8>> {
    fn value(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a') as u32 + 26),
            b'0'..=b'9' => Some((c - b'0') as u32 + 52),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    }
    let raw = text.as_bytes();
    let mut out = Vec::with_capacity(raw.len() / 4 * 3);
    for chunk in raw.chunks(4) {
        if chunk.len() == 1 {
            return None;
        }
        let mut triple = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            triple |= value(c)? << (18 - 6 * i);
        }
        out.push((triple >> 16) as u8);
        if chunk.len() > 2 {
            out.push((triple >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(triple as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn painted(pairs: &[(usize, PaintState)], faces: usize) -> FacetPaint {
        let mut paint = FacetPaint::new();
        for &(face, state) in pairs {
            paint.set(face, state, faces);
        }
        paint
    }

    #[test]
    fn an_untouched_annotation_is_empty_and_encodes_to_nothing() {
        let paint = FacetPaint::new();
        assert!(paint.is_empty());
        assert_eq!(paint.encode(), None);
        assert_eq!(paint.face_count(), 0, "no allocation until something is painted");
    }

    #[test]
    fn painting_and_erasing_returns_to_empty() {
        // The whole feature is skipped on `is_empty`, so an erased object has to
        // be indistinguishable from one that was never painted — otherwise it
        // keeps paying for a paint pass that can only produce nothing.
        let mut paint = painted(&[(3, PaintState::Enforcer)], 10);
        assert!(!paint.is_empty());
        paint.set(3, PaintState::None, 10);
        assert!(paint.is_empty());
        assert_eq!(paint.encode(), None);
    }

    #[test]
    fn repainting_a_facet_does_not_double_count_it() {
        let mut paint = painted(&[(1, PaintState::Enforcer)], 10);
        paint.set(1, PaintState::Blocker, 10);
        assert_eq!(paint.painted_count(), 1);
        assert_eq!(paint.get(1), PaintState::Blocker);
    }

    #[test]
    fn a_round_trip_preserves_every_facet() {
        let paint = painted(
            &[
                (0, PaintState::Enforcer),
                (5, PaintState::Blocker),
                (6, PaintState::Enforcer),
                (299, PaintState::Blocker),
            ],
            300,
        );
        let encoded = paint.encode().expect("something is painted");
        let decoded = FacetPaint::decode(&encoded, 300).expect("round trip");
        assert_eq!(decoded, paint);
    }

    #[test]
    fn a_payload_from_a_different_mesh_is_refused() {
        // The failure this guards against is silent: the indices are all in
        // range for the new mesh, they just mean different triangles. Orca
        // documents exactly this as a source of corrupted paint.
        let paint = painted(&[(2, PaintState::Enforcer)], 10);
        let encoded = paint.encode().expect("painted");
        assert_eq!(
            FacetPaint::decode(&encoded, 12),
            Err(PaintDecodeError::FaceCountMismatch {
                encoded: 10,
                mesh: 12
            })
        );
    }

    #[test]
    fn garbage_is_refused_rather_than_guessed_at() {
        assert_eq!(FacetPaint::decode("!!!!", 10), Err(PaintDecodeError::Malformed));
        assert_eq!(FacetPaint::decode("", 10), Err(PaintDecodeError::Malformed));
        // Valid base64, but a version this build does not know.
        let future = base64url_encode(&[99, 10]);
        assert_eq!(
            FacetPaint::decode(&future, 10),
            Err(PaintDecodeError::UnsupportedVersion(99))
        );
    }

    #[test]
    fn out_of_range_paint_is_ignored_not_fatal() {
        // A stale face index from the UI must not take the engine down.
        let mut paint = FacetPaint::new();
        paint.set(50, PaintState::Enforcer, 10);
        assert!(paint.is_empty());
    }

    #[test]
    fn appending_shifts_the_second_mesh_by_the_first_face_count() {
        // `merge_meshes` concatenates faces, so paint has to concatenate the
        // same way or a merged plate paints the wrong triangles.
        let a = painted(&[(1, PaintState::Enforcer)], 4);
        let b = painted(&[(0, PaintState::Blocker)], 3);
        let mut merged = a.clone();
        merged.append(4, &b, 3);
        assert_eq!(merged.get(1), PaintState::Enforcer);
        assert_eq!(merged.get(4), PaintState::Blocker);
        assert_eq!(merged.painted_count(), 2);
        assert_eq!(merged.face_count(), 7);
    }

    #[test]
    fn appending_an_unpainted_mesh_still_reserves_its_facets() {
        // Otherwise a third mesh appended afterwards lands on top of the
        // second one's indices.
        let a = painted(&[(0, PaintState::Enforcer)], 2);
        let mut merged = a.clone();
        merged.append(2, &FacetPaint::new(), 3);
        merged.append(5, &painted(&[(0, PaintState::Blocker)], 2), 2);
        assert_eq!(merged.get(5), PaintState::Blocker);
        assert_eq!(merged.face_count(), 7);
    }

    #[test]
    fn appending_onto_an_unpainted_mesh_offsets_correctly() {
        let mut merged = FacetPaint::new();
        merged.append(4, &painted(&[(1, PaintState::Enforcer)], 3), 3);
        assert_eq!(merged.get(5), PaintState::Enforcer);
        assert_eq!(merged.painted_count(), 1);
    }

    #[test]
    fn a_clustered_selection_encodes_compactly() {
        // The point of delta+varint: a contiguous run costs about one byte per
        // facet, so paint stays small enough to ride on every slice request.
        let faces = 200_000;
        let mut paint = FacetPaint::new();
        for face in 1000..6000 {
            paint.set(face, PaintState::Enforcer, faces);
        }
        let encoded = paint.encode().expect("painted");
        assert!(
            encoded.len() < 8 * 1024,
            "5000 painted facets encoded to {} bytes",
            encoded.len()
        );
        assert_eq!(FacetPaint::decode(&encoded, faces).unwrap(), paint);
    }

    #[test]
    fn base64_round_trips_every_input_length() {
        // Lengths 1, 2 and 3 mod 3 all take different branches.
        for len in 0..12usize {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            let text = base64url_encode(&bytes);
            assert!(!text.contains('='), "padding would need escaping in a URL");
            assert_eq!(base64url_decode(&text).as_deref(), Some(bytes.as_slice()));
        }
    }

    #[test]
    fn varints_round_trip_across_byte_boundaries() {
        for value in [0u64, 1, 127, 128, 300, 16_383, 16_384, u32::MAX as u64] {
            let mut bytes = Vec::new();
            write_varint(&mut bytes, value);
            let mut cursor = 0;
            assert_eq!(read_varint(&bytes, &mut cursor), Some(value));
            assert_eq!(cursor, bytes.len());
        }
    }

    // ── brush ────────────────────────────────────────────────────────────

    use crate::mesh::analysis::face_adjacency;
    use crate::mesh::types::Vertex;

    fn tri(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Face {
        Face::new([
            Vertex::new(a[0], a[1], a[2]),
            Vertex::new(b[0], b[1], b[2]),
            Vertex::new(c[0], c[1], c[2]),
        ])
    }

    /// A flat `n × n` grid of unit quads in the z = 0 plane, two triangles each.
    fn grid(n: usize) -> Mesh {
        let mut mesh = Mesh::new();
        for row in 0..n {
            for col in 0..n {
                let (x, y) = (col as f64, row as f64);
                mesh.faces.push(tri(
                    [x, y, 0.0],
                    [x + 1.0, y, 0.0],
                    [x + 1.0, y + 1.0, 0.0],
                ));
                mesh.faces.push(tri(
                    [x, y, 0.0],
                    [x + 1.0, y + 1.0, 0.0],
                    [x, y + 1.0, 0.0],
                ));
            }
        }
        mesh
    }

    #[test]
    fn the_brush_paints_the_facet_under_the_cursor() {
        let mesh = grid(4);
        let adjacency = face_adjacency(&mesh, BRUSH_WELD_TOLERANCE_MM);
        let mut paint = FacetPaint::new();
        let changed = paint_sphere(
            &mesh,
            &adjacency,
            &mut paint,
            0,
            [0.6, 0.3, 0.0],
            0.1,
            PaintState::Enforcer,
        );
        assert_eq!(changed, 1);
        assert_eq!(paint.get(0), PaintState::Enforcer);
    }

    #[test]
    fn a_brush_smaller_than_the_facet_still_paints_it() {
        // On a coarse mesh a single triangle can dwarf the brush. Painting the
        // middle of one must not come out as "nothing was hit" — that is the
        // whole-facet granularity working, not a miss.
        let mut mesh = Mesh::new();
        mesh.faces.push(tri(
            [0.0, 0.0, 0.0],
            [100.0, 0.0, 0.0],
            [0.0, 100.0, 0.0],
        ));
        let adjacency = face_adjacency(&mesh, BRUSH_WELD_TOLERANCE_MM);
        let mut paint = FacetPaint::new();
        paint_sphere(
            &mesh,
            &adjacency,
            &mut paint,
            0,
            [20.0, 20.0, 0.0],
            0.5,
            PaintState::Enforcer,
        );
        assert_eq!(paint.get(0), PaintState::Enforcer);
    }

    #[test]
    fn a_wide_brush_spreads_across_neighbouring_facets() {
        let mesh = grid(6);
        let adjacency = face_adjacency(&mesh, BRUSH_WELD_TOLERANCE_MM);
        let mut paint = FacetPaint::new();
        paint_sphere(
            &mesh,
            &adjacency,
            &mut paint,
            0,
            [1.0, 1.0, 0.0],
            1.5,
            PaintState::Blocker,
        );
        assert!(
            paint.painted_count() > 4,
            "a 1.5mm brush on a 1mm grid should cover several facets, got {}",
            paint.painted_count()
        );
        // …but not the whole sheet: a 6×6 grid is 72 facets.
        assert!(paint.painted_count() < 30);
    }

    #[test]
    fn the_stroke_stays_on_the_surface_it_started_on() {
        // Two parallel sheets a hair apart, as the two skins of a thin wall are.
        // A radius test alone would paint both; only the adjacency walk keeps
        // the stroke on the side the user is looking at. Painting through a
        // wall is invisible from the camera and cannot be erased without
        // rotating the model.
        let mut mesh = grid(4);
        let front_faces = mesh.faces.len();
        for face in 0..front_faces {
            let f = &mesh.faces[face];
            let lift = |v: &Vertex| [v.x, v.y, v.z + 0.05];
            let moved = tri(
                lift(&f.vertices[0]),
                lift(&f.vertices[1]),
                lift(&f.vertices[2]),
            );
            mesh.faces.push(moved);
        }
        let adjacency = face_adjacency(&mesh, BRUSH_WELD_TOLERANCE_MM);
        let mut paint = FacetPaint::new();
        paint_sphere(
            &mesh,
            &adjacency,
            &mut paint,
            0,
            [1.0, 1.0, 0.0],
            2.0,
            PaintState::Enforcer,
        );
        assert!(paint.painted_count() > 1, "the stroke should spread at all");
        for face in front_faces..mesh.faces.len() {
            assert_eq!(
                paint.get(face),
                PaintState::None,
                "facet {face} is on the far skin and must not be painted through the wall"
            );
        }
    }

    #[test]
    fn erasing_with_the_brush_reports_the_facets_it_cleared() {
        let mesh = grid(4);
        let adjacency = face_adjacency(&mesh, BRUSH_WELD_TOLERANCE_MM);
        let mut paint = FacetPaint::new();
        paint_sphere(
            &mesh,
            &adjacency,
            &mut paint,
            0,
            [1.0, 1.0, 0.0],
            1.5,
            PaintState::Enforcer,
        );
        let before = paint.painted_count();
        assert!(before > 0);
        let cleared = paint_sphere(
            &mesh,
            &adjacency,
            &mut paint,
            0,
            [1.0, 1.0, 0.0],
            1.5,
            PaintState::None,
        );
        assert_eq!(cleared, before);
        assert!(paint.is_empty());
    }

    #[test]
    fn repainting_the_same_spot_changes_nothing_the_second_time() {
        // The UI fires a stroke per pointer sample, so an idempotent repaint is
        // what stops a held brush pushing a history entry per frame.
        let mesh = grid(4);
        let adjacency = face_adjacency(&mesh, BRUSH_WELD_TOLERANCE_MM);
        let mut paint = FacetPaint::new();
        let args = ([1.0, 1.0, 0.0], 1.5);
        let first = paint_sphere(
            &mesh,
            &adjacency,
            &mut paint,
            0,
            args.0,
            args.1,
            PaintState::Enforcer,
        );
        let second = paint_sphere(
            &mesh,
            &adjacency,
            &mut paint,
            0,
            args.0,
            args.1,
            PaintState::Enforcer,
        );
        assert!(first > 0);
        assert_eq!(second, 0);
    }

    #[test]
    fn an_out_of_range_seed_is_a_no_op() {
        let mesh = grid(2);
        let adjacency = face_adjacency(&mesh, BRUSH_WELD_TOLERANCE_MM);
        let mut paint = FacetPaint::new();
        assert_eq!(
            paint_sphere(
                &mesh,
                &adjacency,
                &mut paint,
                999,
                [0.0, 0.0, 0.0],
                1.0,
                PaintState::Enforcer
            ),
            0
        );
        assert!(paint.is_empty());
    }

    #[test]
    fn closest_point_lands_in_the_right_voronoi_region() {
        let (a, b, c) = ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        // Above the interior → straight down onto the face.
        assert_eq!(
            closest_point_on_triangle([0.25, 0.25, 5.0], a, b, c),
            [0.25, 0.25, 0.0]
        );
        // Beyond a corner → the corner itself.
        assert_eq!(closest_point_on_triangle([-3.0, -3.0, 0.0], a, b, c), a);
        // Off one edge → onto that edge.
        let on_edge = closest_point_on_triangle([0.5, -2.0, 0.0], a, b, c);
        assert!((on_edge[0] - 0.5).abs() < 1e-9 && on_edge[1].abs() < 1e-9);
    }
}
