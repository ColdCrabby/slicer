//! Per-candidate printability measurements.
//!
//! Everything [`super::auto_orient`] needs to rank one floor direction is
//! gathered in a single pass over the faces, expressed in the frame the
//! candidate defines: rotating the mesh so `dir` points at the bed puts a
//! point `v` at printed height `−dot(dir, v)` and gives a face normal `n` a
//! vertical component `rz = −dot(dir, n)`.

use glam::Vec3;

use super::geometry::vertex_to_vec3;
use crate::mesh::types::Mesh;

/// Half-angle (degrees) within which a face counts as lying flat on the bed.
const CONTACT_ANGLE_DEG: f64 = 10.0;

/// A face is resting on the plate only when its *highest* vertex is within
/// this many millimetres of the bottom plane — roughly one first layer.
///
/// This is the whole point of measuring contact instead of counting
/// downward-facing normals: an overhang test is a stack of flat undersides,
/// none of which touch anything.
const BED_CONTACT_BAND_MM: f32 = 0.2;

/// What one candidate floor direction costs to print.
#[derive(Debug, Clone, Copy)]
pub(super) struct CandidateScore {
    /// Area of the faces that actually rest on the plate.
    pub contact_area: f64,
    /// Area of the unsupported downward-facing faces, each weighted by how
    /// far past the overhang threshold it leans (0 at the threshold, 1 for a
    /// flat ceiling).
    pub overhang_area: f64,
    /// Area of the model's shadow on the bed.
    pub footprint_area: f64,
    /// Print height along the candidate direction.
    pub height: f64,
}

/// Measure `dir` as a floor direction for `mesh`.
///
/// `normals` and `areas` are the per-face values, computed once by the caller
/// and shared across every candidate.
pub(super) fn measure(
    mesh: &Mesh,
    normals: &[Vec3],
    areas: &[f64],
    dir: Vec3,
    overhang_threshold_deg: f64,
) -> CandidateScore {
    let (mut min_h, mut max_h) = (f32::INFINITY, f32::NEG_INFINITY);
    for v in &mesh.vertices {
        let h = -dir.dot(vertex_to_vec3(v));
        min_h = min_h.min(h);
        max_h = max_h.max(h);
    }
    let bed_plane = min_h + BED_CONTACT_BAND_MM;

    let sin_threshold = overhang_threshold_deg.to_radians().sin() as f32;
    let contact_cos = CONTACT_ANGLE_DEG.to_radians().cos() as f32;

    let mut contact_area = 0.0_f64;
    let mut overhang_area = 0.0_f64;
    // The shadow of a closed surface is exactly half its area-weighted
    // |cos| sum: every vertical ray leaves the mesh as often as it enters.
    let mut shadow = 0.0_f64;

    for (i, n) in normals.iter().enumerate() {
        let rz = -dir.dot(*n);
        shadow += areas[i] * rz.abs() as f64;

        // Faces pointing up, or leaning down less than either threshold, are
        // free.  Taking the *smaller* threshold matters: a caller asking for an
        // 89° overhang angle would otherwise skip past the bed faces before
        // they could be counted as contact.
        if rz >= -contact_cos.min(sin_threshold) {
            continue;
        }

        if rz < -contact_cos && face_ceiling(mesh, i, dir) <= bed_plane {
            contact_area += areas[i];
            continue; // resting on the plate, so supported by definition
        }

        if rz < -sin_threshold {
            let severity = ((-rz - sin_threshold) / (1.0 - sin_threshold)).clamp(0.0, 1.0);
            overhang_area += areas[i] * severity as f64;
        }
    }

    CandidateScore {
        contact_area,
        overhang_area,
        footprint_area: shadow * 0.5,
        height: (max_h - min_h) as f64,
    }
}

/// Printed height of a face's highest vertex.
fn face_ceiling(mesh: &Mesh, face: usize, dir: Vec3) -> f32 {
    mesh.faces[face]
        .vertices
        .iter()
        .map(|v| -dir.dot(vertex_to_vec3(v)))
        .fold(f32::NEG_INFINITY, f32::max)
}
