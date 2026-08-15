//! Scene operations — the only way to mutate object placement.
//!
//! Every CLI flag and every UI gesture must be expressed as a [`SceneOp`].
//! Each successfully-applied op returns an [`OpReceipt`] containing the
//! inverse op, so undo can be added later without redesigning the API.

use crate::mesh::types::Vertex;
use crate::scene::loader::{self, MeshFormat};
use crate::scene::state::{ObjectId, SceneState};
use crate::scene::transform::Transform;
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// A unit operation on a [`SceneState`].
///
/// All variants must be reversible from the current state plus the returned
/// [`OpReceipt`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", content = "args")]
pub enum SceneOp {
    /// Add a mesh from raw bytes.
    Add {
        name: String,
        format: MeshFormat,
        #[serde(with = "serde_bytes")]
        bytes: Vec<u8>,
    },
    /// Remove an object.
    Remove { id: ObjectId },
    /// Translate by a delta in scene millimeters.
    Translate { id: ObjectId, delta: [f64; 3] },
    /// Replace the entire transform.
    SetTransform { id: ObjectId, transform: Transform },
    /// Rotate around `axis` by `radians`, composing with the current rotation.
    Rotate {
        id: ObjectId,
        axis: [f32; 3],
        radians: f32,
    },
    /// Multiply the per-axis scale by `factors`.
    Scale { id: ObjectId, factors: [f32; 3] },
    /// Translate so the object's transformed-AABB center matches the bed center
    /// in XY. Z is preserved.
    CenterOnBed { id: ObjectId },
    /// Translate so the object's transformed-AABB sits with min.z = 0.
    DropToFloor { id: ObjectId },
    /// Rotate so the chosen face's outward normal points along `-Z`, then
    /// translate so that the **selected face itself** sits on z = 0.
    ///
    /// Unlike a plain drop-to-floor, this lands the chosen face on the bed
    /// even when the face is in the middle of the object (the rest of the
    /// mesh extends upward from that face). Picking a top or bottom face
    /// behaves identically to drop-to-floor.
    PlaceFaceOnFloor { id: ObjectId, face_index: usize },
    /// Automatically rotate the object to minimise overhangs, maximise flat
    /// bed-contact area, and — as a tiebreaker — prefer shorter print heights.
    ///
    /// The result is equivalent to calling `SetTransform` with the optimal
    /// rotation followed by `DropToFloor`. The inverse is `SetTransform` back
    /// to the original transform.
    AutoOrient {
        id: ObjectId,
        options: crate::orient::AutoOrientOptions,
    },
    /// Auto-orient and arrange multiple objects on the bed without overlap.
    ///
    /// For each id in `ids`:
    /// 1. If `options.auto_orient` is `true`, run [`SceneOp::AutoOrient`] on
    ///    the object (preserving individual orientation quality).
    /// 2. After all objects are oriented, compute each object's XY footprint
    ///    and run a shelf-first-fit packing algorithm to place them without
    ///    overlap, separated by `options.spacing_mm`.
    /// 3. Center the entire arrangement on the bed.
    ///
    /// The inverse is a [`SceneOp::BatchSetTransform`] that restores every
    /// affected object to its pre-op transform.
    ArrangeOnBed {
        ids: Vec<ObjectId>,
        #[serde(default)]
        options: crate::orient::ArrangeOptions,
    },
    /// Restore multiple object transforms atomically.
    ///
    /// Used as the inverse of [`SceneOp::ArrangeOnBed`] so undo works in one
    /// step.  Silently skips ids that no longer exist in the scene.
    BatchSetTransform {
        transforms: Vec<(ObjectId, crate::scene::Transform)>,
    },
}

/// Optional modifiers that alter how a [`SceneOp`] is applied.
///
/// Applied via [`SceneState::apply_with_options`]. Plain [`SceneState::apply`]
/// uses [`SceneOptions::default`] (all modifiers off).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct SceneOptions {
    /// "Heavy gravity": after applying a transform op, drop the affected
    /// object so its world-AABB min.z lands on 0. No effect on `Add`,
    /// `Remove`, `DropToFloor`, or `PlaceFaceOnFloor` (those already control
    /// the Z position themselves).
    #[serde(default)]
    pub gravity: bool,
}

/// Receipt returned by a successful [`SceneState::apply`] call.
#[derive(Debug, Clone)]
pub struct OpReceipt {
    /// Op that, when applied to the post-state, restores the pre-state.
    pub inverse: SceneOp,
}

/// Errors that can arise when applying a [`SceneOp`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum SceneError {
    #[error("object {0} not found")]
    NotFound(ObjectId),
    #[error("face index {face} out of range (mesh has {count} faces)")]
    FaceOutOfRange { face: usize, count: usize },
    #[error("face {0} has degenerate (zero-area) normal")]
    DegenerateFace(usize),
    #[error("mesh load failed: {0}")]
    Load(String),
}

impl SceneState {
    /// Apply a [`SceneOp`] to this scene with [`SceneOptions::default`].
    pub fn apply(&mut self, op: SceneOp) -> Result<OpReceipt, SceneError> {
        self.apply_inner(op)
    }

    /// Apply a [`SceneOp`] with optional modifiers.
    ///
    /// When [`SceneOptions::gravity`] is set, the affected object is dropped
    /// to the floor (`world_aabb().min.z = 0`) immediately after the op runs.
    /// Gravity is skipped for `Add`, `Remove`, `DropToFloor`, and
    /// `PlaceFaceOnFloor` (those ops either don't move an object's Z or
    /// already put it on the floor themselves).
    ///
    /// The returned receipt's inverse restores the pre-op transform — undoing
    /// both the original op and the gravity drop in one step.
    pub fn apply_with_options(
        &mut self,
        op: SceneOp,
        options: SceneOptions,
    ) -> Result<OpReceipt, SceneError> {
        let gravity_target = options
            .gravity
            .then(|| affected_id_for_gravity(&op))
            .flatten();
        let receipt = self.apply_inner(op)?;
        if let Some(id) = gravity_target {
            if let Some(obj) = self.get(id) {
                let world = obj.world_aabb();
                let mut new_t = obj.transform;
                new_t.translation[2] -= world.min.z as f32;
                self.get_mut(id).unwrap().transform = new_t;
            }
        }
        Ok(receipt)
    }

    fn apply_inner(&mut self, op: SceneOp) -> Result<OpReceipt, SceneError> {
        match op {
            SceneOp::Add {
                name,
                format,
                bytes,
            } => {
                let mesh = loader::load_bytes(&bytes, format).map_err(SceneError::Load)?;
                let id = self.add_mesh(name, Arc::new(mesh));
                Ok(OpReceipt {
                    inverse: SceneOp::Remove { id },
                })
            }

            SceneOp::Remove { id } => {
                let obj = self.get(id).ok_or(SceneError::NotFound(id))?.clone();
                self.remove(id);
                // Pure removal cannot be inverted without re-uploading the mesh
                // bytes; record a SetTransform stub so the receipt is at least
                // shaped like the others. True undo of a Remove will require
                // a higher-level history that retains the mesh.
                Ok(OpReceipt {
                    inverse: SceneOp::SetTransform {
                        id,
                        transform: obj.transform,
                    },
                })
            }

            SceneOp::Translate { id, delta } => {
                let prev = self.get(id).ok_or(SceneError::NotFound(id))?.transform;
                let mut new_t = prev;
                new_t.translation[0] += delta[0] as f32;
                new_t.translation[1] += delta[1] as f32;
                new_t.translation[2] += delta[2] as f32;
                self.get_mut(id).unwrap().transform = new_t;
                Ok(OpReceipt {
                    inverse: SceneOp::SetTransform {
                        id,
                        transform: prev,
                    },
                })
            }

            SceneOp::SetTransform { id, transform } => {
                let prev = self.get(id).ok_or(SceneError::NotFound(id))?.transform;
                self.get_mut(id).unwrap().transform = transform;
                Ok(OpReceipt {
                    inverse: SceneOp::SetTransform {
                        id,
                        transform: prev,
                    },
                })
            }

            SceneOp::Rotate { id, axis, radians } => {
                let prev = self.get(id).ok_or(SceneError::NotFound(id))?.transform;
                let axis_v = Vec3::from(axis).normalize_or_zero();
                let q = Quat::from_axis_angle(axis_v, radians);
                let mut new_t = prev;
                new_t.set_quat(q * prev.quat());
                self.get_mut(id).unwrap().transform = new_t;
                Ok(OpReceipt {
                    inverse: SceneOp::SetTransform {
                        id,
                        transform: prev,
                    },
                })
            }

            SceneOp::Scale { id, factors } => {
                let prev = self.get(id).ok_or(SceneError::NotFound(id))?.transform;
                let mut new_t = prev;
                new_t.scale[0] *= factors[0];
                new_t.scale[1] *= factors[1];
                new_t.scale[2] *= factors[2];
                self.get_mut(id).unwrap().transform = new_t;
                Ok(OpReceipt {
                    inverse: SceneOp::SetTransform {
                        id,
                        transform: prev,
                    },
                })
            }

            SceneOp::CenterOnBed { id } => {
                let obj = self.get(id).ok_or(SceneError::NotFound(id))?;
                let prev = obj.transform;
                let world = obj.world_aabb();
                let world_center = world.center();
                let (bx, by) = self.bed.center_xy();
                let mut new_t = prev;
                new_t.translation[0] += (bx - world_center.x) as f32;
                new_t.translation[1] += (by - world_center.y) as f32;
                self.get_mut(id).unwrap().transform = new_t;
                Ok(OpReceipt {
                    inverse: SceneOp::SetTransform {
                        id,
                        transform: prev,
                    },
                })
            }

            SceneOp::DropToFloor { id } => {
                let obj = self.get(id).ok_or(SceneError::NotFound(id))?;
                let prev = obj.transform;
                let world = obj.world_aabb();
                let mut new_t = prev;
                new_t.translation[2] -= world.min.z as f32;
                self.get_mut(id).unwrap().transform = new_t;
                Ok(OpReceipt {
                    inverse: SceneOp::SetTransform {
                        id,
                        transform: prev,
                    },
                })
            }

            SceneOp::PlaceFaceOnFloor { id, face_index } => {
                let obj = self.get(id).ok_or(SceneError::NotFound(id))?;
                let prev = obj.transform;
                let mesh = obj.mesh.clone();
                if face_index >= mesh.faces.len() {
                    return Err(SceneError::FaceOutOfRange {
                        face: face_index,
                        count: mesh.faces.len(),
                    });
                }
                let face = &mesh.faces[face_index];
                let local_normal =
                    face_normal(face).ok_or(SceneError::DegenerateFace(face_index))?;
                // Apply current rotation to the local normal to get its current world direction.
                let world_normal = (prev.quat() * local_normal).normalize_or_zero();
                if world_normal.length_squared() < 1e-12 {
                    return Err(SceneError::DegenerateFace(face_index));
                }
                // Quaternion that rotates the world normal to point along -Z.
                let down = Vec3::new(0.0, 0.0, -1.0);
                let align = Quat::from_rotation_arc(world_normal, down);
                let mut new_t = prev;
                new_t.set_quat(align * prev.quat());
                self.get_mut(id).unwrap().transform = new_t;
                // Land the *selected face itself* on z = 0 (not the AABB min).
                // For a top/bottom face this is identical to drop-to-floor; for
                // a mid-object face the rest of the mesh extends upward from
                // that face instead of clipping through the bed.
                let matrix = new_t.to_matrix();
                let face_min_z = face
                    .vertices
                    .iter()
                    .map(|v| {
                        matrix
                            .transform_point3(Vec3::new(v.x as f32, v.y as f32, v.z as f32))
                            .z
                    })
                    .fold(f32::INFINITY, f32::min);
                new_t.translation[2] -= face_min_z;
                self.get_mut(id).unwrap().transform = new_t;
                Ok(OpReceipt {
                    inverse: SceneOp::SetTransform {
                        id,
                        transform: prev,
                    },
                })
            }

            SceneOp::AutoOrient { id, options } => {
                let obj = self.get(id).ok_or(SceneError::NotFound(id))?;
                let prev = obj.transform;
                let mesh = obj.mesh.clone();
                // Compute the optimal rotation quaternion.
                let q = crate::orient::auto_orient(&mesh, &options);
                // Apply the rotation, preserving the existing scale.
                let mut new_t = prev;
                new_t.set_quat(q);
                self.get_mut(id).unwrap().transform = new_t;
                // Drop to floor: shift Z so the world-AABB min sits at z = 0.
                let world = self.get(id).unwrap().world_aabb();
                self.get_mut(id).unwrap().transform.translation[2] -= world.min.z as f32;
                // Center on bed: shift XY so the world-AABB center aligns with
                // the bed centre.  Mirrors what CenterOnBed does so the object
                // lands in a sensible position regardless of where it started.
                let world = self.get(id).unwrap().world_aabb();
                let world_center = world.center();
                let (bx, by) = self.bed.center_xy();
                let t = &mut self.get_mut(id).unwrap().transform;
                t.translation[0] += (bx - world_center.x) as f32;
                t.translation[1] += (by - world_center.y) as f32;
                Ok(OpReceipt {
                    inverse: SceneOp::SetTransform {
                        id,
                        transform: prev,
                    },
                })
            }

            SceneOp::ArrangeOnBed { ids, options } => {
                // Snapshot pre-op transforms for the undo inverse.
                let prev_transforms: Vec<(ObjectId, crate::scene::Transform)> = ids
                    .iter()
                    .filter_map(|&id| self.get(id).map(|o| (id, o.transform)))
                    .collect();

                // ----------------------------------------------------------------
                // Step 1: Auto-orient each object (if requested).
                // ----------------------------------------------------------------
                if options.auto_orient {
                    for &id in &ids {
                        if self.get(id).is_none() {
                            continue;
                        }
                        let mesh = self.get(id).unwrap().mesh.clone();
                        let q = crate::orient::auto_orient(&mesh, &options.orient_options);
                        let prev = self.get(id).unwrap().transform;
                        let mut new_t = prev;
                        new_t.set_quat(q);
                        self.get_mut(id).unwrap().transform = new_t;
                        // Drop to floor after rotation.
                        let world = self.get(id).unwrap().world_aabb();
                        self.get_mut(id).unwrap().transform.translation[2] -= world.min.z as f32;
                    }
                }

                // ----------------------------------------------------------------
                // Step 2: Collect XY footprints (world-AABB width × depth).
                // ----------------------------------------------------------------
                // We pair each footprint with its corresponding id so we can
                // look up results by index after packing.
                let footprints: Vec<(ObjectId, (f64, f64))> = ids
                    .iter()
                    .filter_map(|&id| {
                        self.get(id).map(|o| {
                            let aabb = o.world_aabb();
                            let w = (aabb.max.x - aabb.min.x).max(0.0);
                            let d = (aabb.max.y - aabb.min.y).max(0.0);
                            (id, (w, d))
                        })
                    })
                    .collect();

                let fp_values: Vec<(f64, f64)> = footprints.iter().map(|&(_, fp)| fp).collect();
                let packed =
                    crate::orient::pack::pack_footprints(&fp_values, &self.bed, options.spacing_mm);

                // ----------------------------------------------------------------
                // Step 3: Move each object to its packed position.
                //
                // The packing result gives the object's desired min-X / min-Y
                // in scene coordinates.  We shift the translation so the
                // world-AABB min-X and min-Y land there.
                // ----------------------------------------------------------------
                for item in &packed {
                    let (obj_id, _) = footprints[item.index];
                    if let Some(obj) = self.get(obj_id) {
                        let world = obj.world_aabb();
                        let dx = item.x - world.min.x;
                        let dy = item.y - world.min.y;
                        let t = &mut self.get_mut(obj_id).unwrap().transform;
                        t.translation[0] += dx as f32;
                        t.translation[1] += dy as f32;
                    }
                }

                Ok(OpReceipt {
                    inverse: SceneOp::BatchSetTransform {
                        transforms: prev_transforms,
                    },
                })
            }

            SceneOp::BatchSetTransform { transforms } => {
                // Save the current transforms before overwriting them.
                let prev_transforms: Vec<(ObjectId, crate::scene::Transform)> = transforms
                    .iter()
                    .filter_map(|&(id, _)| self.get(id).map(|o| (id, o.transform)))
                    .collect();

                for (id, transform) in transforms {
                    if let Some(obj) = self.get_mut(id) {
                        obj.transform = transform;
                    }
                }
                Ok(OpReceipt {
                    inverse: SceneOp::BatchSetTransform {
                        transforms: prev_transforms,
                    },
                })
            }
        }
    }
}

/// Compute a face's geometric normal from its three vertices.
fn face_normal(face: &crate::mesh::types::Face) -> Option<Vec3> {
    if let Some(n) = face.normal {
        let v = Vec3::new(n.x as f32, n.y as f32, n.z as f32);
        if v.length_squared() > 1e-12 {
            return Some(v.normalize());
        }
    }
    let a = vertex_to_vec3(&face.vertices[0]);
    let b = vertex_to_vec3(&face.vertices[1]);
    let c = vertex_to_vec3(&face.vertices[2]);
    let n = (b - a).cross(c - a);
    if n.length_squared() < 1e-12 {
        None
    } else {
        Some(n.normalize())
    }
}

fn vertex_to_vec3(v: &Vertex) -> Vec3 {
    Vec3::new(v.x as f32, v.y as f32, v.z as f32)
}

/// Returns the object id that gravity should act on after `op`, or `None` if
/// gravity should be skipped for that op.
fn affected_id_for_gravity(op: &SceneOp) -> Option<ObjectId> {
    match op {
        SceneOp::Translate { id, .. }
        | SceneOp::SetTransform { id, .. }
        | SceneOp::Rotate { id, .. }
        | SceneOp::Scale { id, .. }
        | SceneOp::CenterOnBed { id } => Some(*id),
        SceneOp::Add { .. }
        | SceneOp::Remove { .. }
        | SceneOp::DropToFloor { .. }
        | SceneOp::PlaceFaceOnFloor { .. }
        | SceneOp::AutoOrient { .. }
        | SceneOp::ArrangeOnBed { .. }
        | SceneOp::BatchSetTransform { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::types::{Face, Mesh};
    use crate::scene::bed::BedConfig;

    fn cube_mesh(origin: [f64; 3], size: f64) -> Arc<Mesh> {
        let [x, y, z] = origin;
        let s = size;
        let v: Vec<Vertex> = vec![
            Vertex::new(x, y, z),
            Vertex::new(x + s, y, z),
            Vertex::new(x + s, y + s, z),
            Vertex::new(x, y + s, z),
            Vertex::new(x, y, z + s),
            Vertex::new(x + s, y, z + s),
            Vertex::new(x + s, y + s, z + s),
            Vertex::new(x, y + s, z + s),
        ];
        let idx: [[usize; 3]; 12] = [
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [2, 3, 7],
            [2, 7, 6],
            [0, 4, 7],
            [0, 7, 3],
            [1, 2, 6],
            [1, 6, 5],
        ];
        let faces: Vec<Face> = idx
            .iter()
            .map(|i| Face::new([v[i[0]], v[i[1]], v[i[2]]]))
            .collect();
        Arc::new(Mesh {
            vertices: v,
            faces,
            aabb: None,
        })
    }

    fn small_bed() -> BedConfig {
        BedConfig {
            width: 100.0,
            depth: 100.0,
            height: 100.0,
            origin_offset_x: 0.0,
            origin_offset_y: 0.0,
        }
    }

    #[test]
    fn translate_updates_transform() {
        let mut s = SceneState::new(small_bed());
        let id = s.add_mesh("c", cube_mesh([0.0, 0.0, 0.0], 10.0));
        s.apply(SceneOp::Translate {
            id,
            delta: [5.0, 6.0, 7.0],
        })
        .unwrap();
        let t = s.get(id).unwrap().transform.translation;
        assert!((t[0] - 5.0).abs() < 1e-5);
        assert!((t[1] - 6.0).abs() < 1e-5);
        assert!((t[2] - 7.0).abs() < 1e-5);
    }

    #[test]
    fn center_on_bed_centers_xy() {
        let mut s = SceneState::new(small_bed());
        let id = s.add_mesh("c", cube_mesh([0.0, 0.0, 0.0], 10.0));
        s.apply(SceneOp::CenterOnBed { id }).unwrap();
        let world = s.get(id).unwrap().world_aabb();
        assert!((world.center().x - 50.0).abs() < 1e-4);
        assert!((world.center().y - 50.0).abs() < 1e-4);
    }

    #[test]
    fn center_on_bed_is_idempotent() {
        let mut s = SceneState::new(small_bed());
        let id = s.add_mesh("c", cube_mesh([3.0, 7.0, 0.0], 10.0));
        s.apply(SceneOp::CenterOnBed { id }).unwrap();
        let after_once = s.get(id).unwrap().transform;
        s.apply(SceneOp::CenterOnBed { id }).unwrap();
        let after_twice = s.get(id).unwrap().transform;
        for i in 0..3 {
            assert!(
                (after_once.translation[i] - after_twice.translation[i]).abs() < 1e-4,
                "axis {i}"
            );
        }
    }

    #[test]
    fn drop_to_floor_lands_on_zero() {
        let mut s = SceneState::new(small_bed());
        let id = s.add_mesh("c", cube_mesh([0.0, 0.0, 12.0], 10.0));
        s.apply(SceneOp::DropToFloor { id }).unwrap();
        let world = s.get(id).unwrap().world_aabb();
        assert!(world.min.z.abs() < 1e-4, "min.z={}", world.min.z);
    }

    #[test]
    fn translate_inverse_restores_transform() {
        let mut s = SceneState::new(small_bed());
        let id = s.add_mesh("c", cube_mesh([0.0, 0.0, 0.0], 10.0));
        let receipt = s
            .apply(SceneOp::Translate {
                id,
                delta: [11.0, 22.0, 33.0],
            })
            .unwrap();
        s.apply(receipt.inverse).unwrap();
        let t = s.get(id).unwrap().transform.translation;
        assert!(t[0].abs() < 1e-5);
        assert!(t[1].abs() < 1e-5);
        assert!(t[2].abs() < 1e-5);
    }

    #[test]
    fn place_face_on_floor_lands_face_at_z0() {
        // Cube faces 8 and 9 are the +X-facing pair (vertices (1,*,*) and (1,*,*)).
        // Pick face index 4: triangle (0,1,5) — the -Y face (normal points -Y).
        // We want a face with non-Z normal so the alignment actually rotates.
        let mut s = SceneState::new(small_bed());
        let id = s.add_mesh("c", cube_mesh([0.0, 0.0, 0.0], 10.0));
        // Face 4 normal: (b-a)x(c-a) where a=(0,0,0), b=(10,0,0), c=(10,0,10) → (0,-100,0) → -Y.
        s.apply(SceneOp::PlaceFaceOnFloor { id, face_index: 4 })
            .unwrap();
        let world = s.get(id).unwrap().world_aabb();
        assert!(
            world.min.z.abs() < 1e-3,
            "min.z={} after place-face",
            world.min.z
        );
    }

    #[test]
    fn place_face_on_floor_lands_mid_object_face() {
        // Build a tall cuboid (10x10x40) and pick a side face that is *not*
        // the bottom. The selected face must end up at z ≈ 0 — the rest of the
        // mesh extends upward from there. With the old AABB-based drop, a side
        // face would not actually touch the bed: the object would be sitting
        // on its (newly rotated) bottom edge instead.
        let mut s = SceneState::new(small_bed());
        let id = s.add_mesh("c", cube_mesh([0.0, 0.0, 0.0], 10.0));
        // Face 4 is on the -Y side; its three vertices span z = 0..10 in the
        // local mesh. After a rotation that aligns -Y with -Z and then lands
        // the face at z = 0, all three vertices of face 4 must have world z = 0.
        s.apply(SceneOp::PlaceFaceOnFloor { id, face_index: 4 })
            .unwrap();
        let obj = s.get(id).unwrap();
        let matrix = obj.transform.to_matrix();
        let face = &obj.mesh.faces[4];
        for v in &face.vertices {
            let p = matrix.transform_point3(Vec3::new(v.x as f32, v.y as f32, v.z as f32));
            assert!(p.z.abs() < 1e-3, "face vertex z = {} (expected 0)", p.z);
        }
    }

    #[test]
    fn gravity_drops_after_translate() {
        let mut s = SceneState::new(small_bed());
        let id = s.add_mesh("c", cube_mesh([0.0, 0.0, 0.0], 10.0));
        s.apply_with_options(
            SceneOp::Translate {
                id,
                delta: [5.0, 6.0, 25.0],
            },
            SceneOptions { gravity: true },
        )
        .unwrap();
        let world = s.get(id).unwrap().world_aabb();
        // XY translation respected, but Z is pulled back to the floor.
        assert!(world.min.z.abs() < 1e-4, "min.z={}", world.min.z);
        let t = s.get(id).unwrap().transform.translation;
        assert!((t[0] - 5.0).abs() < 1e-4);
        assert!((t[1] - 6.0).abs() < 1e-4);
    }

    #[test]
    fn gravity_inverse_restores_pre_op_transform() {
        let mut s = SceneState::new(small_bed());
        let id = s.add_mesh("c", cube_mesh([0.0, 0.0, 0.0], 10.0));
        // Start with the cube floating at z=20.
        s.apply(SceneOp::Translate {
            id,
            delta: [0.0, 0.0, 20.0],
        })
        .unwrap();
        let pre = s.get(id).unwrap().transform;
        let receipt = s
            .apply_with_options(
                SceneOp::Translate {
                    id,
                    delta: [1.0, 0.0, 5.0],
                },
                SceneOptions { gravity: true },
            )
            .unwrap();
        // After op + gravity, it sits on the floor.
        assert!(s.get(id).unwrap().world_aabb().min.z.abs() < 1e-4);
        // Inverse must restore the pre-op floating state, not the post-gravity state.
        s.apply(receipt.inverse).unwrap();
        let restored = s.get(id).unwrap().transform;
        for i in 0..3 {
            assert!(
                (restored.translation[i] - pre.translation[i]).abs() < 1e-4,
                "axis {i}"
            );
        }
    }

    #[test]
    fn missing_object_returns_not_found() {
        let mut s = SceneState::new(small_bed());
        let err = s
            .apply(SceneOp::Translate {
                id: ObjectId(999),
                delta: [0.0; 3],
            })
            .unwrap_err();
        matches!(err, SceneError::NotFound(_));
    }

    #[test]
    fn arrange_on_bed_no_overlap() {
        // Four 10×10×10 cubes on a 100×100 bed; ArrangeOnBed should place them
        // without overlap (with spacing_mm = 2).
        let mut s = SceneState::new(small_bed());
        let ids: Vec<ObjectId> = (0..4)
            .map(|i| s.add_mesh(format!("c{i}"), cube_mesh([0.0, 0.0, 0.0], 10.0)))
            .collect();

        s.apply(SceneOp::ArrangeOnBed {
            ids: ids.clone(),
            options: crate::orient::ArrangeOptions {
                spacing_mm: 2.0,
                auto_orient: false, // skip orient so the test is deterministic
                ..Default::default()
            },
        })
        .unwrap();

        // Check every pair for non-overlap (world-AABB test in XY).
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                let ai = s.get(ids[i]).unwrap().world_aabb();
                let aj = s.get(ids[j]).unwrap().world_aabb();
                let overlap_x = ai.min.x < aj.max.x && ai.max.x > aj.min.x;
                let overlap_y = ai.min.y < aj.max.y && ai.max.y > aj.min.y;
                assert!(
                    !(overlap_x && overlap_y),
                    "objects {i} and {j} overlap after ArrangeOnBed"
                );
            }
        }
    }

    #[test]
    fn arrange_on_bed_undo_restores_transforms() {
        let mut s = SceneState::new(small_bed());
        let ids: Vec<ObjectId> = (0..2)
            .map(|i| {
                s.add_mesh(
                    format!("c{i}"),
                    cube_mesh([f64::from(i) * 5.0, 0.0, 0.0], 10.0),
                )
            })
            .collect();

        let pre: Vec<_> = ids.iter().map(|&id| s.get(id).unwrap().transform).collect();

        let receipt = s
            .apply(SceneOp::ArrangeOnBed {
                ids: ids.clone(),
                options: crate::orient::ArrangeOptions {
                    spacing_mm: 2.0,
                    auto_orient: false,
                    ..Default::default()
                },
            })
            .unwrap();

        // Apply the inverse.
        s.apply(receipt.inverse).unwrap();

        for (i, &id) in ids.iter().enumerate() {
            let restored = s.get(id).unwrap().transform.translation;
            let axes = ["x", "y", "z"];
            for (axis, &axis_name) in axes.iter().enumerate() {
                assert!(
                    (restored[axis] - pre[i].translation[axis]).abs() < 1e-4,
                    "obj {i} {axis_name}: got {}, expected {}",
                    restored[axis],
                    pre[i].translation[axis]
                );
            }
        }
    }
}
