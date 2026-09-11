//! Auto-orient: find the rotation that puts the largest practical face on the
//! bed without making the print harder than it has to be.
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
//!    - The mesh's own `−Z` leads the list, so an already-well-oriented model
//!      wins every tie and stays where its author put it.
//!
//! 2. **Measurement** ([`score::measure`]) — one pass over the faces per
//!    candidate yields the four quantities that decide printability: the area
//!    actually resting on the plate, the severity-weighted unsupported
//!    overhang area, the footprint, and the height.
//!
//! 3. **Scoring** — the measurements are normalised so the weights below mean
//!    the same thing for a 10 mm trinket and a 300 mm vase, then combined into
//!    one number (lower is better) and the winner is turned into a quaternion.
//!
//! ## Why contact is measured, not inferred
//!
//! Counting every downward-facing face as "contact" — and subtracting it from
//! the overhang penalty — makes a model that rests on *nothing* look ideal:
//! an overhang test is a stack of flat undersides, so tipping it onto a corner
//! reads as both high contact and low overhang while being unprintable in
//! practice.  Contact therefore means faces within one first layer of the
//! bottom plane, and nothing else is treated as supported.

mod candidates;
mod geometry;
pub mod pack;
mod score;
mod types;

pub use types::{ArrangeOptions, AutoOrientOptions};

use crate::mesh::types::Mesh;
use crate::scene::bed::BedConfig;
use glam::{Quat, Vec3};

// ---------------------------------------------------------------------------
// Scoring weights — intentionally not exposed; tune here if needed.
//
// Every term the weights multiply is dimensionless and lives in 0..1, so the
// weights are directly comparable and a model's size cannot change the ranking.
// ---------------------------------------------------------------------------

/// Penalty on the fraction of the surface left unsupported.  Large enough that
/// roughly a fifth of the model hanging in the air overrides a perfect bed
/// face — the "unless it is unprintable" half of the contract.
const OVERHANG_W: f64 = 3.0;
/// Reward for bed-contact area, measured against the best candidate's.  This
/// is the "prefer the biggest area on the bed" term.
const CONTACT_W: f64 = 0.6;
/// Reward for the share of the footprint that actually rests on the plate.
/// Separates a wide flat base from a wide model balanced on a small pad.
const COVERAGE_W: f64 = 0.25;
/// Tiebreaker favouring shorter prints (less time, less wobble).
const HEIGHT_W: f64 = 0.1;
/// Tiny edge given to the orientation the model arrived in, so floating-point
/// noise between two equivalent poses cannot spin a well-placed model.
const STAY_PUT_BONUS: f64 = 0.02;
/// Applied to an orientation taller than the machine can print.  It only ranks
/// such candidates last — if every one of them overflows, the least-bad still
/// wins rather than the caller getting an arbitrary pose.
const EXCEEDS_BUILD_HEIGHT_W: f64 = 100.0;

/// Guards the normalisation divisions against degenerate meshes.
const EPSILON: f64 = 1e-9;

// ---------------------------------------------------------------------------
// Core function
// ---------------------------------------------------------------------------

/// Compute the rotation quaternion that best orients `mesh` for FDM printing.
///
/// The returned quaternion, when applied to the mesh, puts the largest
/// practical face on the plate, keeps unsupported overhangs down, and — as a
/// tiebreaker — prefers shorter prints.
///
/// The caller is responsible for:
/// - Applying the quaternion (e.g. via [`crate::scene::ops::SceneOp::AutoOrient`]).
/// - Dropping the oriented mesh to the floor (`DropToFloor`).
pub fn auto_orient(mesh: &Mesh, options: &AutoOrientOptions) -> Quat {
    auto_orient_in(mesh, options, None)
}

/// [`auto_orient`], additionally rejecting orientations the machine cannot
/// print.  An orientation taller than `bed.height` is ranked below every
/// orientation that fits, however good it otherwise looks.
pub fn auto_orient_in(mesh: &Mesh, options: &AutoOrientOptions, bed: Option<&BedConfig>) -> Quat {
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

    // Build candidate floor-normal directions.  Index 0 is the mesh's own −Z
    // (see `build_candidates`), which is what `STAY_PUT_BONUS` applies to.
    let cands = candidates::build_candidates(mesh, options, &normals, &areas);
    if cands.is_empty() {
        return Quat::IDENTITY;
    }

    let measured: Vec<score::CandidateScore> = cands
        .iter()
        .map(|c| score::measure(mesh, &normals, &areas, *c, options.overhang_threshold_deg))
        .collect();

    // Contact and height are normalised against the best candidate rather than
    // against the model's dimensions: the question is which of *these* poses
    // puts the most on the plate, and the answer must not depend on scale.
    let best_contact = measured
        .iter()
        .map(|m| m.contact_area)
        .fold(0.0_f64, f64::max);
    let tallest = measured.iter().map(|m| m.height).fold(0.0_f64, f64::max);

    let mut best_score = f64::MAX;
    let mut best_candidate = Vec3::NEG_Z;

    for (i, m) in measured.iter().enumerate() {
        let overhang = m.overhang_area / total_area;
        let contact = if best_contact > EPSILON {
            m.contact_area / best_contact
        } else {
            0.0
        };
        let coverage = if m.footprint_area > EPSILON {
            (m.contact_area / m.footprint_area).min(1.0)
        } else {
            0.0
        };
        let height = if tallest > EPSILON {
            m.height / tallest
        } else {
            0.0
        };

        let mut s =
            OVERHANG_W * overhang - CONTACT_W * contact - COVERAGE_W * coverage + HEIGHT_W * height;

        if i == 0 {
            s -= STAY_PUT_BONUS;
        }
        if let Some(bed) = bed {
            if m.height > bed.height {
                s += EXCEEDS_BUILD_HEIGHT_W;
            }
        }

        if s < best_score {
            best_score = s;
            best_candidate = cands[i];
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

    /// Axis-aligned box spanning `min`..`max`, with outward-facing normals.
    fn box_faces(min: [f64; 3], max: [f64; 3]) -> (Vec<Vertex>, Vec<Face>) {
        let v = [
            Vertex::new(min[0], min[1], min[2]), // 0
            Vertex::new(max[0], min[1], min[2]), // 1
            Vertex::new(max[0], max[1], min[2]), // 2
            Vertex::new(min[0], max[1], min[2]), // 3
            Vertex::new(min[0], min[1], max[2]), // 4
            Vertex::new(max[0], min[1], max[2]), // 5
            Vertex::new(max[0], max[1], max[2]), // 6
            Vertex::new(min[0], max[1], max[2]), // 7
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
        let faces = idx
            .iter()
            .map(|i| Face::new([v[i[0]], v[i[1]], v[i[2]]]))
            .collect();
        (v.to_vec(), faces)
    }

    fn box_mesh(min: [f64; 3], max: [f64; 3]) -> Mesh {
        let (vertices, faces) = box_faces(min, max);
        Mesh {
            vertices,
            faces,
            aabb: None,
        }
    }

    /// 10 × 10 × 10 mm axis-aligned cube.
    fn cube_mesh() -> Mesh {
        box_mesh([0.0, 0.0, 0.0], [10.0, 10.0, 10.0])
    }

    /// A tall thin box: 5 × 5 × 50 mm standing upright.
    fn tall_box_mesh() -> Mesh {
        box_mesh([0.0, 0.0, 0.0], [5.0, 5.0, 50.0])
    }

    /// A 50 × 50 × 2 mm slab held 10 mm off the bed by a 10 × 10 × 10 pillar.
    ///
    /// The slab's underside is 2500 mm² of downward-facing area that touches
    /// nothing — the shape of every model the old "any face pointing down is
    /// contact" rule mis-read.
    fn slab_on_pillar_mesh() -> Mesh {
        let (mut vertices, mut faces) = box_faces([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let (slab_v, slab_f) = box_faces([-20.0, -20.0, 10.0], [30.0, 30.0, 12.0]);
        vertices.extend(slab_v);
        faces.extend(slab_f);
        Mesh {
            vertices,
            faces,
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

    /// Measure the pose `q` picks, through the production scorer.
    ///
    /// `q` maps the winning floor direction onto `−Z`, so the direction it
    /// chose is `q⁻¹ * −Z`.
    fn measured(mesh: &Mesh, q: Quat) -> score::CandidateScore {
        let normals: Vec<Vec3> = mesh
            .faces
            .iter()
            .map(|f| face_normal_vec3(f).unwrap_or(Vec3::Z))
            .collect();
        let areas: Vec<f64> = mesh.faces.iter().map(|f| f.area()).collect();
        let dir = (q.inverse() * Vec3::NEG_Z).normalize();
        score::measure(mesh, &normals, &areas, dir, 45.0)
    }

    // -----------------------------------------------------------------------
    // Test: cube
    // -----------------------------------------------------------------------
    #[test]
    fn cube_already_flat() {
        let mesh = cube_mesh();
        let opts = AutoOrientOptions::default();
        let q = auto_orient(&mesh, &opts);
        // Every face of a cube is an equally good floor, so the tie must go to
        // the orientation it arrived in rather than an arbitrary rotation.
        assert_eq!(q, Quat::IDENTITY, "an upright cube must be left alone");
        let m = measured(&mesh, q);
        assert!(
            (m.height - 10.0).abs() < 0.5,
            "cube height after orient should be ~10, got {}",
            m.height
        );
        assert!(
            m.overhang_area < 1e-6,
            "cube should have no overhang after orient, got {}",
            m.overhang_area
        );
        assert!(
            (m.contact_area - 100.0).abs() < 1e-3,
            "a whole cube face should rest on the bed, got {}",
            m.contact_area
        );
    }

    // -----------------------------------------------------------------------
    // Test: a tall thin box lays down on its biggest face
    // -----------------------------------------------------------------------
    #[test]
    fn tall_box_lays_on_its_largest_face() {
        for allow_rotations in [false, true] {
            let mesh = tall_box_mesh();
            let opts = AutoOrientOptions {
                allow_rotations,
                ..Default::default()
            };
            let m = measured(&mesh, auto_orient(&mesh, &opts));
            // Lying down puts a 5 × 50 side on the bed and stands 5 mm tall;
            // upright it rests on 5 × 5 and stands 50 mm.
            assert!(
                m.height <= 10.5,
                "tall box should lay flat (height ≤ 10), got {} (allow_rotations={allow_rotations})",
                m.height
            );
            assert!(
                m.contact_area > 200.0,
                "tall box should rest on a long side, got {} mm² (allow_rotations={allow_rotations})",
                m.contact_area
            );
        }
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
        let m = measured(&mesh, auto_orient(&mesh, &opts));
        assert!(
            m.overhang_area < 1e-6,
            "wedge should have zero overhang after orient (threshold=45°), got {}",
            m.overhang_area
        );
    }

    // -----------------------------------------------------------------------
    // Test: only faces that actually touch the plate count as contact
    // -----------------------------------------------------------------------
    #[test]
    fn a_raised_underside_is_an_overhang_not_bed_contact() {
        let mesh = slab_on_pillar_mesh();
        let m = measured(&mesh, Quat::IDENTITY);
        assert!(
            (m.contact_area - 100.0).abs() < 1e-3,
            "only the pillar's 100 mm² base touches the bed, got {}",
            m.contact_area
        );
        assert!(
            m.overhang_area > 2000.0,
            "the slab's raised underside is an overhang, got {} mm²",
            m.overhang_area
        );
    }

    // -----------------------------------------------------------------------
    // Test: the flat face wins over tipping the model onto an edge
    // -----------------------------------------------------------------------
    #[test]
    fn the_largest_flat_face_goes_on_the_bed() {
        let mesh = slab_on_pillar_mesh();
        let m = measured(&mesh, auto_orient(&mesh, &AutoOrientOptions::default()));
        // Laid on a slab face there is 2500 mm² on the plate and the pillar
        // points up unsupported by nothing; nothing else comes close.
        assert!(
            m.contact_area > 2000.0,
            "auto-orient should put the slab face down, got {} mm² of contact",
            m.contact_area
        );
        assert!(
            m.height <= 12.5,
            "slab-down is the shortest pose, got height {}",
            m.height
        );
    }

    // -----------------------------------------------------------------------
    // Test: an orientation the machine cannot fit is rejected
    // -----------------------------------------------------------------------
    #[test]
    fn an_orientation_taller_than_the_machine_is_rejected() {
        // 5 × 5 × 50 standing up is 50 mm tall; a 20 mm-tall machine can only
        // print it lying down.
        let mesh = tall_box_mesh();
        let bed = BedConfig {
            height: 20.0,
            ..Default::default()
        };
        let opts = AutoOrientOptions::default();
        let m = measured(&mesh, auto_orient_in(&mesh, &opts, Some(&bed)));
        assert!(
            m.height <= bed.height,
            "orientation must fit the build height, got {}",
            m.height
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
