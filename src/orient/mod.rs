//! Auto-orient: find the rotation that minimises overhangs and maximises
//! flat bed-contact area for a given mesh.
//!
//! ## Algorithm
//!
//! 1. **Candidate generation** — collect unique "floor direction" candidates:
//!    - Snap all face normals to a coarse grid (≈6° resolution) and accumulate
//!      area per bucket; keep the top [`candidates::MAX_FLAT_CANDIDATES`]
//!      directions by total area.  This is O(faces) and merges near-duplicate
//!      normals on curved surfaces into a single representative direction.
//!    - If [`AutoOrientOptions::allow_rotations`] is `true`, additionally
//!      sample ~128 directions on a Fibonacci sphere (covers organic shapes
//!      with no prominent flat regions).
//!
//! 2. **Scoring** — for each candidate direction `d` (the direction that will
//!    be rotated to face down / align with `−Z`):
//!    - `rz(n) = −dot(d, n)` (equivalent to `(q * n).z` where
//!      `q = from_rotation_arc(d, −Z)`, but computed with a single dot product
//!      — no quaternion construction or matrix multiply needed).
//!    - Score = `OVERHANG_W × net_overhang_area − CONTACT_W × contact_area + HEIGHT_W × height`
//!      (lower is better).
//!    - `net_overhang = overhang_area − contact_area`: bed-contact faces are
//!      supported and must not be counted as overhangs.
//!    - `height = max_v(dot(d, v)) − min_v(dot(d, v))` across all vertices
//!      (no AABB transform needed).
//!
//! 3. **Result** — build `Quat::from_rotation_arc(best_candidate, −Z)` **once**
//!    for the winner, then optionally compose with a preferred Z-rotation.

mod candidates;
mod geometry;
pub mod pack;
mod types;

pub use types::{ArrangeOptions, AutoOrientOptions};

use crate::mesh::types::Mesh;
use glam::{Quat, Vec3};

// ---------------------------------------------------------------------------
// Scoring weights — intentionally not exposed; tune here if needed.
// ---------------------------------------------------------------------------

/// Weight applied to overhang area (dominant term — matches Cura's approach).
const OVERHANG_W: f64 = 1.0;
/// Reward for large flat contact area with the bed (OrcaSlicer heuristic).
const CONTACT_W: f64 = 0.5;
/// Small height penalty so identical-overhang candidates prefer shorter prints.
const HEIGHT_W: f64 = 0.01;
/// Half-angle (degrees) for "essentially flat on bed" contact detection.
const CONTACT_ANGLE_DEG: f64 = 10.0;

// ---------------------------------------------------------------------------
// Core function
// ---------------------------------------------------------------------------

/// Compute the rotation quaternion that best orients `mesh` for FDM printing.
///
/// The returned quaternion, when applied to the mesh, minimises unsupported
/// overhangs, maximises flat bed-contact area, and — as a tiebreaker — prefers
/// shorter print heights.
///
/// The caller is responsible for:
/// - Applying the quaternion (e.g. via [`crate::scene::ops::SceneOp::AutoOrient`]).
/// - Dropping the oriented mesh to the floor (`DropToFloor`).
pub fn auto_orient(mesh: &Mesh, options: &AutoOrientOptions) -> Quat {
    if mesh.faces.is_empty() {
        return Quat::IDENTITY;
    }

    // Pre-compute per-face normals (unit vectors) and areas once.
    let normals: Vec<Vec3> = mesh
        .faces
        .iter()
        .map(|f| geometry::face_normal_vec3(f).unwrap_or(Vec3::Z))
        .collect();

    let areas: Vec<f64> = mesh.faces.iter().map(|f| f.area()).collect();
    let total_area: f64 = areas.iter().sum();
    if total_area < 1e-10 {
        return Quat::IDENTITY;
    }

    // Collect all mesh vertices once for height computation.
    let vertices: Vec<Vec3> = mesh.vertices.iter().map(geometry::vertex_to_vec3).collect();

    // Build candidate floor-normal directions.
    let cands = candidates::build_candidates(mesh, options, &normals, &areas);
    if cands.is_empty() {
        return Quat::IDENTITY;
    }

    // Pre-compute scoring thresholds as f32 for the hot dot-product loop.
    //
    // `overhang_z_threshold` = sin(overhang_threshold_deg):
    //   rz = -dot(candidate, n); rz < -threshold  →  face is an overhang.
    // `contact_z_threshold` = cos(CONTACT_ANGLE_DEG):
    //   rz < -threshold  →  face is essentially flat on the bed.
    let overhang_z_threshold = options.overhang_threshold_deg.to_radians().sin() as f32;
    let contact_z_threshold = CONTACT_ANGLE_DEG.to_radians().cos() as f32;

    let mut best_score = f64::MAX;
    let mut best_candidate = Vec3::NEG_Z; // default: already pointing down

    for candidate in &cands {
        // ---------------------------------------------------------------------------
        // Overhang + contact scoring
        //
        // Mathematical identity: (q * n).z  where  q = from_rotation_arc(c, -Z)
        //   = -dot(c, n)
        //
        // Proof: q maps c → -Z, so q⁻¹ maps -Z → c and maps +Z → -c.
        //   (q * n).z = n · (q⁻¹ * Z) = n · (-c) = -dot(c, n).
        //
        // This replaces a quaternion construction + multiplication per face with
        // a single dot product — the dominant cost for large meshes.
        // ---------------------------------------------------------------------------
        let mut overhang_area = 0.0_f64;
        let mut contact_area = 0.0_f64;

        for (i, n) in normals.iter().enumerate() {
            let rz = -candidate.dot(*n);
            if rz < -overhang_z_threshold {
                overhang_area += areas[i];
            }
            if rz < -contact_z_threshold {
                contact_area += areas[i];
            }
        }

        // ---------------------------------------------------------------------------
        // Height scoring
        //
        // After rotating so that `candidate` points to -Z, the print height equals
        // the range of vertex projections onto `candidate`:
        //   height = max_v(dot(c, v)) - min_v(dot(c, v))
        //
        // This avoids building a Transform + running transformed_aabb per candidate.
        // ---------------------------------------------------------------------------
        let mut min_proj = f32::INFINITY;
        let mut max_proj = f32::NEG_INFINITY;
        for v in &vertices {
            let p = candidate.dot(*v);
            min_proj = min_proj.min(p);
            max_proj = max_proj.max(p);
        }
        let height = (max_proj - min_proj) as f64;

        // Bed-contact faces (rz ≈ -1) are supported and must NOT be counted as
        // overhangs.  Net unsupported overhang = total downward area minus the
        // portion that rests on the bed.
        let net_overhang = overhang_area - contact_area;
        let score = OVERHANG_W * net_overhang - CONTACT_W * contact_area + HEIGHT_W * height;

        if score < best_score {
            best_score = score;
            best_candidate = *candidate;
        }
    }

    // Build the winning quaternion exactly once.
    let mut best_quat = Quat::from_rotation_arc(best_candidate, Vec3::NEG_Z);

    // Optionally compose with a Z-rotation preference (CoreXY 45°, etc.).
    if options.preferred_z_rotation_deg.abs() > 1e-6 {
        let z_rot = Quat::from_axis_angle(
            Vec3::Z,
            (options.preferred_z_rotation_deg as f32).to_radians(),
        );
        best_quat = z_rot * best_quat;
    }

    best_quat
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::geometry::face_normal_vec3;
    use super::*;
    use crate::mesh::types::{Face, Mesh, Vertex};

    /// 10 × 10 × 10 mm axis-aligned cube with outward-facing normals.
    fn cube_mesh() -> Mesh {
        let v = [
            Vertex::new(0.0, 0.0, 0.0),    // 0 bottom corners
            Vertex::new(10.0, 0.0, 0.0),   // 1
            Vertex::new(10.0, 10.0, 0.0),  // 2
            Vertex::new(0.0, 10.0, 0.0),   // 3
            Vertex::new(0.0, 0.0, 10.0),   // 4 top corners
            Vertex::new(10.0, 0.0, 10.0),  // 5
            Vertex::new(10.0, 10.0, 10.0), // 6
            Vertex::new(0.0, 10.0, 10.0),  // 7
        ];
        let idx: [[usize; 3]; 12] = [
            [0, 2, 1],
            [0, 3, 2], // bottom −Z
            [4, 5, 6],
            [4, 6, 7], // top +Z
            [0, 1, 5],
            [0, 5, 4], // front −Y
            [2, 3, 7],
            [2, 7, 6], // back +Y
            [0, 4, 7],
            [0, 7, 3], // left −X
            [1, 2, 6],
            [1, 6, 5], // right +X
        ];
        Mesh {
            vertices: v.to_vec(),
            faces: idx
                .iter()
                .map(|i| Face::new([v[i[0]], v[i[1]], v[i[2]]]))
                .collect(),
            aabb: None,
        }
    }

    /// A tall thin box: 5 × 5 × 50 mm standing upright.
    fn tall_box_mesh() -> Mesh {
        let v = [
            Vertex::new(0.0, 0.0, 0.0),  // 0
            Vertex::new(5.0, 0.0, 0.0),  // 1
            Vertex::new(5.0, 5.0, 0.0),  // 2
            Vertex::new(0.0, 5.0, 0.0),  // 3
            Vertex::new(0.0, 0.0, 50.0), // 4
            Vertex::new(5.0, 0.0, 50.0), // 5
            Vertex::new(5.0, 5.0, 50.0), // 6
            Vertex::new(0.0, 5.0, 50.0), // 7
        ];
        let idx: [[usize; 3]; 12] = [
            [0, 2, 1],
            [0, 3, 2], // bottom
            [4, 5, 6],
            [4, 6, 7], // top
            [0, 1, 5],
            [0, 5, 4], // front
            [2, 3, 7],
            [2, 7, 6], // back
            [0, 4, 7],
            [0, 7, 3], // left
            [1, 2, 6],
            [1, 6, 5], // right
        ];
        Mesh {
            vertices: v.to_vec(),
            faces: idx
                .iter()
                .map(|i| Face::new([v[i[0]], v[i[1]], v[i[2]]]))
                .collect(),
            aabb: None,
        }
    }

    /// A triangular wedge prism: cross-section is a right triangle.
    /// Vertices: two triangular end caps and three rectangular faces.
    /// The slanted face has a 45° angle relative to horizontal.
    fn wedge_mesh() -> Mesh {
        // Right-triangle cross-section in XZ plane, extruded along Y.
        // Base: (0,0,0)→(10,0,0), Height: 10 at x=0.
        // Slanted face: normal = normalise([10,0,10]) = [1/√2, 0, 1/√2]
        let v = [
            Vertex::new(0.0, 0.0, 0.0),  // 0
            Vertex::new(10.0, 0.0, 0.0), // 1
            Vertex::new(0.0, 0.0, 10.0), // 2  ← apex (x=0, z=10)
            Vertex::new(0.0, 5.0, 0.0),  // 3
            Vertex::new(10.0, 5.0, 0.0), // 4
            Vertex::new(0.0, 5.0, 10.0), // 5
        ];
        // Front cap (y=0): v0,v2,v1 → outward normal −Y
        // Back cap  (y=5): v3,v4,v5 → outward normal +Y
        // Bottom face (z=0): v0,v1,v4,v3 → normal −Z (two triangles)
        // Left face (x=0): v0,v3,v5,v2 → normal −X (two triangles)
        // Slanted face: v1,v2,v5,v4 → normal [1,0,1]/√2 (two triangles)
        let faces: Vec<Face> = vec![
            // front cap
            Face::new([v[0], v[2], v[1]]),
            // back cap
            Face::new([v[3], v[4], v[5]]),
            // bottom (z=0, outward = -Z)
            Face::new([v[0], v[1], v[4]]),
            Face::new([v[0], v[4], v[3]]),
            // left (x=0, outward = -X)
            Face::new([v[0], v[3], v[5]]),
            Face::new([v[0], v[5], v[2]]),
            // slanted face (outward ≈ [1,0,1]/√2, i.e. pointing up-right)
            Face::new([v[1], v[2], v[5]]),
            Face::new([v[1], v[5], v[4]]),
        ];
        Mesh {
            vertices: v.to_vec(),
            faces,
            aabb: None,
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Height of the bounding box after applying `q` to `mesh`.
    /// Uses the same vertex-projection formula as the production code:
    ///   height = max_v(dot(candidate, v)) − min_v(dot(candidate, v))
    fn oriented_height(mesh: &Mesh, q: Quat) -> f64 {
        // Recover the candidate direction: q maps it to -Z, so candidate = q⁻¹ * (-Z)
        // Equivalently, the height direction after rotation is (q * v).z → use dot(q⁻¹*Z_neg, v)
        // Simpler: just project vertices through the quaternion and measure Z span.
        let (mut min_z, mut max_z) = (f32::INFINITY, f32::NEG_INFINITY);
        for f in &mesh.faces {
            for v in &f.vertices {
                let world = q * Vec3::new(v.x as f32, v.y as f32, v.z as f32);
                min_z = min_z.min(world.z);
                max_z = max_z.max(world.z);
            }
        }
        (max_z - min_z) as f64
    }

    /// Net unsupported overhang area after rotation `q`:
    /// faces pointing >threshold below horizontal, minus faces essentially flat
    /// on the bed (within `CONTACT_ANGLE_DEG` of -Z). The bed-contact face
    /// is always supported, so it must not be counted as an overhang.
    fn net_overhang_area(mesh: &Mesh, q: Quat, threshold_deg: f64) -> f64 {
        let overhang_z_threshold = threshold_deg.to_radians().sin() as f32;
        let contact_z_threshold = CONTACT_ANGLE_DEG.to_radians().cos() as f32;
        let mut overhang = 0.0f64;
        let mut contact = 0.0f64;
        for f in &mesh.faces {
            let n = face_normal_vec3(f).unwrap_or(Vec3::Z);
            let rz = (q * n).z;
            let area = f.area();
            if rz < -overhang_z_threshold {
                overhang += area;
            }
            if rz < -contact_z_threshold {
                contact += area;
            }
        }
        overhang - contact
    }

    // -----------------------------------------------------------------------
    // Test: cube
    // -----------------------------------------------------------------------
    #[test]
    fn cube_already_flat() {
        let mesh = cube_mesh();
        let opts = AutoOrientOptions::default();
        let q = auto_orient(&mesh, &opts);
        // The cube is already flat. After applying q, the height should be ~10
        // (the cube's side length) — any axis-aligned face can be chosen.
        let h = oriented_height(&mesh, q);
        assert!(
            (h - 10.0).abs() < 0.5,
            "cube height after orient should be ~10, got {h}"
        );
        // Net unsupported overhangs should be zero: the bottom face is
        // supported by the bed (it's the bed-contact face), so it cancels out.
        let oa = net_overhang_area(&mesh, q, 45.0);
        assert!(
            oa < 1e-6,
            "cube should have zero net overhang after orient, got {oa}"
        );
    }

    // -----------------------------------------------------------------------
    // Test: tall thin box with allow_rotations=true → should lay flat
    // -----------------------------------------------------------------------
    #[test]
    fn tall_box_lays_flat_with_rotations() {
        let mesh = tall_box_mesh();
        let opts = AutoOrientOptions {
            allow_rotations: true,
            ..Default::default()
        };
        let q = auto_orient(&mesh, &opts);
        let h = oriented_height(&mesh, q);
        // Laying flat: height ≈ 5 (the short side). Upright: height = 50.
        // Accept anything ≤ 10 as "laid flat".
        assert!(
            h <= 10.0 + 0.5,
            "tall box should lay flat (height ≤ 10), got {h}"
        );
    }

    // -----------------------------------------------------------------------
    // Test: wedge — auto-orient without allow_rotations should choose the
    // orientation with zero net overhangs (slanted face down wins because it
    // has the largest flat contact area among zero-net-overhang candidates).
    // -----------------------------------------------------------------------
    #[test]
    fn wedge_no_overhang() {
        let mesh = wedge_mesh();
        let opts = AutoOrientOptions::default();
        let q = auto_orient(&mesh, &opts);
        let oa = net_overhang_area(&mesh, q, 45.0);
        assert!(
            oa < 1e-6,
            "wedge should have zero net overhang after orient (threshold=45°), got {oa}"
        );
    }

    // -----------------------------------------------------------------------
    // Test: preferred_z_rotation_deg is composed into the result
    // -----------------------------------------------------------------------
    #[test]
    fn preferred_z_rotation_applied() {
        let mesh = cube_mesh();
        let opts_no_rot = AutoOrientOptions::default();
        let opts_z45 = AutoOrientOptions {
            preferred_z_rotation_deg: 45.0,
            ..Default::default()
        };
        let q_no = auto_orient(&mesh, &opts_no_rot);
        let q_z45 = auto_orient(&mesh, &opts_z45);
        // The two results should differ (the Z rotation changes the quaternion).
        let dot = (q_no.x * q_z45.x + q_no.y * q_z45.y + q_no.z * q_z45.z + q_no.w * q_z45.w).abs();
        assert!(
            dot < 0.999,
            "preferred_z_rotation should change the quaternion (dot={dot})"
        );
    }
}
