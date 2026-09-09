//! Affine transforms for scene objects.
//!
//! Internally uses quaternions for rotation; Euler-XYZ degrees are exposed
//! at protocol/CLI boundaries via [`Transform::from_euler_xyz_deg`] /
//! [`Transform::to_euler_xyz_deg`].

use crate::mesh::types::{Face, Mesh, Vertex, AABB};
use glam::{DVec3, EulerRot, Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};

/// Affine transform: scale → rotate → translate (TRS), applied in that order.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    /// Translation in millimeters.
    pub translation: [f32; 3],
    /// Rotation as a unit quaternion `[x, y, z, w]`.
    pub rotation: [f32; 4],
    /// Per-axis scale factors.
    pub scale: [f32; 3],
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    /// The identity transform (no translation, no rotation, unit scale).
    pub const IDENTITY: Self = Self {
        translation: [0.0, 0.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0, 1.0, 1.0],
    };

    /// Construct from Euler-XYZ angles in degrees (intrinsic XYZ rotation order).
    pub fn from_euler_xyz_deg(translation: [f32; 3], euler_deg: [f32; 3], scale: [f32; 3]) -> Self {
        let q = Quat::from_euler(
            EulerRot::XYZ,
            euler_deg[0].to_radians(),
            euler_deg[1].to_radians(),
            euler_deg[2].to_radians(),
        );
        Self {
            translation,
            rotation: [q.x, q.y, q.z, q.w],
            scale,
        }
    }

    /// Decompose rotation back into Euler-XYZ degrees (intrinsic XYZ).
    ///
    /// Note: Euler decomposition is not unique; do not round-trip beyond the
    /// boundary layer.
    pub fn to_euler_xyz_deg(&self) -> [f32; 3] {
        let q = self.quat();
        let (x, y, z) = q.to_euler(EulerRot::XYZ);
        [x.to_degrees(), y.to_degrees(), z.to_degrees()]
    }

    /// Quaternion view of the rotation.
    pub fn quat(&self) -> Quat {
        Quat::from_xyzw(
            self.rotation[0],
            self.rotation[1],
            self.rotation[2],
            self.rotation[3],
        )
    }

    /// Set the rotation from a glam [`Quat`].
    pub fn set_quat(&mut self, q: Quat) {
        let q = q.normalize();
        self.rotation = [q.x, q.y, q.z, q.w];
    }

    /// 4×4 matrix in column-major order (TRS composition).
    pub fn to_matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(
            Vec3::from(self.scale),
            self.quat(),
            Vec3::from(self.translation),
        )
    }

    /// Map a point from world space back into the object's local frame.
    ///
    /// The inverse of [`to_matrix`](Self::to_matrix) applied to a point. Used by
    /// support painting, where the cursor lands somewhere in world millimetres
    /// but the paint has to be recorded against the geometry, so it survives
    /// every later move, rotate and scale.
    ///
    /// Returns the point unchanged if the transform is singular (a zero scale
    /// on some axis), which is the only sensible answer — there is no local
    /// point to speak of once an axis has been collapsed.
    pub fn inverse_point(&self, world: [f64; 3]) -> [f64; 3] {
        let matrix = self.to_matrix();
        let inverse = matrix.inverse();
        if !inverse.is_finite() {
            return world;
        }
        let p =
            inverse.transform_point3(Vec3::new(world[0] as f32, world[1] as f32, world[2] as f32));
        [p.x as f64, p.y as f64, p.z as f64]
    }

    /// Compose `self` with `other`: `result = self ∘ other` (apply `other` first).
    ///
    /// Lossy when scale is non-uniform combined with rotation; sufficient for
    /// the simple op stack used by the scene engine.
    pub fn compose(&self, other: &Transform) -> Transform {
        let combined = self.to_matrix() * other.to_matrix();
        Transform::from_matrix(combined)
    }

    /// Decompose a 4×4 matrix back into a [`Transform`].
    pub fn from_matrix(m: Mat4) -> Transform {
        let (scale, rot, translation) = m.to_scale_rotation_translation();
        Self {
            translation: translation.into(),
            rotation: [rot.x, rot.y, rot.z, rot.w],
            scale: scale.into(),
        }
    }

    /// Inverse transform.
    pub fn inverse(&self) -> Transform {
        Transform::from_matrix(self.to_matrix().inverse())
    }
}

/// Apply a transform to a [`Vertex`] (in mm).
fn transform_vertex(m: &Mat4, v: &Vertex) -> Vertex {
    let p = m.transform_point3(Vec3::new(v.x as f32, v.y as f32, v.z as f32));
    Vertex::new(p.x as f64, p.y as f64, p.z as f64)
}

/// Apply a [`Transform`] to a [`Vertex`] direction (rotation+scale only — no translation).
fn transform_normal(m: &Mat4, n: &Vertex) -> Vertex {
    let v = m.transform_vector3(Vec3::new(n.x as f32, n.y as f32, n.z as f32));
    Vertex::new(v.x as f64, v.y as f64, v.z as f64)
}

/// Bake a [`Transform`] into a fresh [`Mesh`].
///
/// Returns a new mesh with all vertex positions transformed. Cached AABB is
/// cleared. Normals (when present) are rotated/scaled but not re-normalised —
/// downstream code should re-normalise if it depends on unit-length normals.
pub fn apply_transform(mesh: &Mesh, transform: &Transform) -> Mesh {
    let mat = transform.to_matrix();
    let vertices: Vec<Vertex> = mesh
        .vertices
        .iter()
        .map(|v| transform_vertex(&mat, v))
        .collect();
    let faces: Vec<Face> = mesh
        .faces
        .iter()
        .map(|f| Face {
            vertices: [
                transform_vertex(&mat, &f.vertices[0]),
                transform_vertex(&mat, &f.vertices[1]),
                transform_vertex(&mat, &f.vertices[2]),
            ],
            normal: f.normal.as_ref().map(|n| transform_normal(&mat, n)),
        })
        .collect();
    Mesh {
        vertices,
        faces,
        aabb: None,
    }
}

/// Compute the AABB of a mesh after applying `transform`, without baking
/// every vertex.
///
/// Transforms the eight corners of the mesh's local AABB and returns the
/// AABB enclosing them. This is conservative for non-rotated transforms and
/// exact for axis-aligned rotations; it is sufficient for placement ops
/// like center-on-bed and drop-to-floor.
pub fn transformed_aabb(local_aabb: &AABB, transform: &Transform) -> AABB {
    let mat = transform.to_matrix();
    let mn = &local_aabb.min;
    let mx = &local_aabb.max;
    let corners = [
        Vertex::new(mn.x, mn.y, mn.z),
        Vertex::new(mx.x, mn.y, mn.z),
        Vertex::new(mn.x, mx.y, mn.z),
        Vertex::new(mx.x, mx.y, mn.z),
        Vertex::new(mn.x, mn.y, mx.z),
        Vertex::new(mx.x, mn.y, mx.z),
        Vertex::new(mn.x, mx.y, mx.z),
        Vertex::new(mx.x, mx.y, mx.z),
    ];
    let transformed: Vec<Vertex> = corners.iter().map(|c| transform_vertex(&mat, c)).collect();
    AABB::new_from_vertices(&transformed).expect("eight corners produce a non-empty AABB")
}

/// Helpers for double-precision math at the boundary.
pub fn dvec3_from(v: &Vertex) -> DVec3 {
    DVec3::new(v.x, v.y, v.z)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn identity_is_neutral() {
        let t = Transform::IDENTITY;
        let m = t.to_matrix();
        assert_eq!(m, Mat4::IDENTITY);
    }

    #[test]
    fn euler_xyz_roundtrip() {
        let t = Transform::from_euler_xyz_deg([0.0; 3], [30.0, 45.0, 15.0], [1.0; 3]);
        let e = t.to_euler_xyz_deg();
        assert!(approx(e[0], 30.0), "x={}", e[0]);
        assert!(approx(e[1], 45.0), "y={}", e[1]);
        assert!(approx(e[2], 15.0), "z={}", e[2]);
    }

    #[test]
    fn compose_with_inverse_is_identity() {
        let t = Transform::from_euler_xyz_deg([5.0, 6.0, 7.0], [10.0, 20.0, 30.0], [2.0, 2.0, 2.0]);
        let composed = t.compose(&t.inverse());
        let m = composed.to_matrix();
        for i in 0..4 {
            for j in 0..4 {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!(
                    approx(m.col(i)[j], expected),
                    "M[{},{}]={} != {}",
                    i,
                    j,
                    m.col(i)[j],
                    expected
                );
            }
        }
    }

    #[test]
    fn apply_transform_translates_vertices() {
        let mut mesh = Mesh::new();
        mesh.vertices = vec![
            Vertex::new(0.0, 0.0, 0.0),
            Vertex::new(1.0, 0.0, 0.0),
            Vertex::new(0.0, 1.0, 0.0),
        ];
        mesh.faces = vec![Face::new([
            mesh.vertices[0],
            mesh.vertices[1],
            mesh.vertices[2],
        ])];
        let t = Transform {
            translation: [10.0, 20.0, 30.0],
            ..Transform::IDENTITY
        };
        let out = apply_transform(&mesh, &t);
        assert!((out.vertices[0].x - 10.0).abs() < 1e-5);
        assert!((out.vertices[0].y - 20.0).abs() < 1e-5);
        assert!((out.vertices[0].z - 30.0).abs() < 1e-5);
        assert!(out.aabb.is_none());
    }

    #[test]
    fn transformed_aabb_matches_baked() {
        let aabb = AABB {
            min: Vertex::new(-1.0, -1.0, -1.0),
            max: Vertex::new(1.0, 1.0, 1.0),
        };
        let t = Transform {
            translation: [10.0, 0.0, 0.0],
            scale: [2.0, 2.0, 2.0],
            ..Transform::IDENTITY
        };
        let out = transformed_aabb(&aabb, &t);
        assert!((out.min.x - 8.0).abs() < 1e-5);
        assert!((out.max.x - 12.0).abs() < 1e-5);
        assert!((out.min.y + 2.0).abs() < 1e-5);
        assert!((out.max.y - 2.0).abs() < 1e-5);
    }
}
