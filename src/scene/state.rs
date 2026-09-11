//! Scene state: objects, transforms, and bed.

use crate::mesh::analysis::{calculate_aabb, face_adjacency, FaceAdjacency};
use crate::mesh::paint::FacetPaint;
use crate::mesh::repair::MeshReport;
use crate::mesh::types::{Mesh, AABB};
use crate::scene::bed::BedConfig;
use crate::scene::transform::{transformed_aabb, Transform};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Monotonically-allocated identifier for a scene object.
///
/// Stable for the lifetime of a [`SceneState`]; not reused after removal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ObjectId(pub u64);

impl std::fmt::Display for ObjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "obj#{}", self.0)
    }
}

/// One transformable mesh placed in the scene.
#[derive(Debug, Clone)]
pub struct SceneObject {
    /// Stable identifier within the owning [`SceneState`].
    pub id: ObjectId,
    /// Display name (typically the source file name).
    pub name: String,
    /// Underlying triangle mesh — shared via `Arc` so transforms are cheap.
    pub mesh: Arc<Mesh>,
    /// Affine transform applied at slice time.
    pub transform: Transform,
    /// Opaque handle to the bytes this object was loaded from.
    ///
    /// The scene engine never interprets this — it only carries it so a
    /// consumer can map an object back to whatever it loaded the mesh from
    /// (the WS server's uploaded-file UUID, a CLI path, …). Duplicated
    /// objects inherit the source of their original, so every instance of a
    /// model resolves to the same bytes.
    ///
    /// Without this, a caller holding N objects and M uploads has no way to
    /// pair them up and has to guess positionally — which silently slices the
    /// wrong mesh once the two lists diverge.
    pub source_id: Option<String>,
    /// Which object *within* the source file this is (0-based, in build order).
    ///
    /// A 3MF holds a whole scene, so one file can back several scene objects.
    /// Re-loading the file at slice time would otherwise hand back every part
    /// for each object and print the whole file once per part; this index says
    /// which one to keep.
    pub source_part: usize,
    /// Health report produced when the mesh was loaded, when one is available.
    ///
    /// Populated by [`SceneOp::Add`](crate::scene::SceneOp::Add); `None` for
    /// meshes handed in directly through [`SceneState::add_mesh`].
    pub report: Option<MeshReport>,
    /// Per-facet support paint — where the user forced or forbade support.
    ///
    /// Empty for an unpainted object, which is the overwhelmingly common case
    /// and costs nothing. Indices address [`Mesh::faces`] of this object's own
    /// mesh, so paint follows the geometry through every transform rather than
    /// being tied to a place on the plate.
    pub paint: FacetPaint,
}

impl SceneObject {
    /// AABB of the object's mesh in its **local** (untransformed) frame.
    pub fn local_aabb(&self) -> AABB {
        calculate_aabb(self.mesh.as_ref())
    }

    /// AABB of the object after applying its current transform.
    pub fn world_aabb(&self) -> AABB {
        transformed_aabb(&self.local_aabb(), &self.transform)
    }
}

/// Top-level scene state owned by the CLI / WS server / WASM handle.
#[derive(Debug, Clone)]
pub struct SceneState {
    /// Objects in insertion order.
    pub objects: Vec<SceneObject>,
    /// Print bed configuration.
    pub bed: BedConfig,
    next_id: u64,
    /// Edge-adjacency graphs, keyed by the identity of the `Arc<Mesh>` they
    /// describe.
    ///
    /// Building one is O(faces log faces), and a held brush fires an op per
    /// pointer sample, so recomputing per stroke would make painting a dense
    /// model unusable. Keying on the `Arc` rather than the object means every
    /// duplicate of a model shares one graph — N instances cost one build, the
    /// same bargain `Arc<Mesh>` already makes for the geometry itself.
    adjacency: HashMap<usize, Arc<FaceAdjacency>>,
}

impl SceneState {
    /// Create an empty scene with the given bed configuration.
    pub fn new(bed: BedConfig) -> Self {
        Self {
            objects: Vec::new(),
            bed,
            next_id: 1,
            adjacency: HashMap::new(),
        }
    }

    /// Edge-adjacency graph for `mesh`, building and caching it on first use.
    ///
    /// Entries are dropped by [`prune_adjacency_cache`](Self::prune_adjacency_cache)
    /// once no object references the mesh any more.
    pub(crate) fn adjacency_for(&mut self, mesh: &Arc<Mesh>) -> Arc<FaceAdjacency> {
        let key = Arc::as_ptr(mesh) as usize;
        if let Some(existing) = self.adjacency.get(&key) {
            return Arc::clone(existing);
        }
        let built = Arc::new(face_adjacency(
            mesh.as_ref(),
            crate::mesh::paint::BRUSH_WELD_TOLERANCE_MM,
        ));
        self.adjacency.insert(key, Arc::clone(&built));
        built
    }

    /// Forget adjacency graphs for meshes no object holds any more.
    ///
    /// The cache is keyed by `Arc` address, and an address can be reused once
    /// the allocation is freed — so a stale entry is not merely wasted memory,
    /// it could hand a *different* mesh the wrong topology. Called after every
    /// removal.
    fn prune_adjacency_cache(&mut self) {
        if self.adjacency.is_empty() {
            return;
        }
        let live: std::collections::HashSet<usize> = self
            .objects
            .iter()
            .map(|o| Arc::as_ptr(&o.mesh) as usize)
            .collect();
        self.adjacency.retain(|key, _| live.contains(key));
    }

    /// Add a mesh to the scene. Returns the assigned [`ObjectId`].
    pub fn add_mesh(&mut self, name: impl Into<String>, mesh: Arc<Mesh>) -> ObjectId {
        self.add_mesh_from(name, mesh, None)
    }

    /// Add a mesh, recording the opaque source handle it was loaded from.
    ///
    /// See [`SceneObject::source_id`] for why the provenance matters.
    pub fn add_mesh_from(
        &mut self,
        name: impl Into<String>,
        mesh: Arc<Mesh>,
        source_id: Option<String>,
    ) -> ObjectId {
        self.add_mesh_part(name, mesh, source_id, 0)
    }

    /// Add one part of a multi-object source file.
    ///
    /// `source_part` is the object's 0-based index within that file, so a
    /// later re-load can pick out the right one.
    pub fn add_mesh_part(
        &mut self,
        name: impl Into<String>,
        mesh: Arc<Mesh>,
        source_id: Option<String>,
        source_part: usize,
    ) -> ObjectId {
        self.add_mesh_part_with_report(name, mesh, source_id, source_part, None)
    }

    /// Add one part of a multi-object source file together with the
    /// [`MeshReport`] produced when it was loaded.
    ///
    /// The bottom of the `add_mesh*` chain — every other variant funnels here.
    #[allow(clippy::too_many_arguments)]
    pub fn add_mesh_part_with_report(
        &mut self,
        name: impl Into<String>,
        mesh: Arc<Mesh>,
        source_id: Option<String>,
        source_part: usize,
        report: Option<MeshReport>,
    ) -> ObjectId {
        let id = ObjectId(self.next_id);
        self.next_id += 1;
        self.objects.push(SceneObject {
            id,
            name: name.into(),
            mesh,
            transform: Transform::IDENTITY,
            source_id,
            source_part,
            report,
            paint: FacetPaint::new(),
        });
        id
    }

    /// Clone an existing object, sharing its mesh.
    ///
    /// The copy keeps the original's transform, name, and source reference;
    /// only the id differs. The mesh is shared via `Arc`, so duplicating a
    /// million-triangle model costs a pointer bump rather than a deep copy.
    /// Returns `None` when `id` is unknown.
    pub fn duplicate(&mut self, id: ObjectId) -> Option<ObjectId> {
        let src = self.get(id)?;
        // A copy is the same geometry, so it inherits the original's health
        // report along with its mesh and provenance — and its paint, which
        // addresses that shared geometry and is just as true of the copy.
        let (name, mesh, transform, source_id, source_part, report, paint) = (
            src.name.clone(),
            Arc::clone(&src.mesh),
            src.transform,
            src.source_id.clone(),
            src.source_part,
            src.report.clone(),
            src.paint.clone(),
        );
        let new_id = ObjectId(self.next_id);
        self.next_id += 1;
        self.objects.push(SceneObject {
            id: new_id,
            name,
            mesh,
            transform,
            source_id,
            source_part,
            report,
            paint,
        });
        Some(new_id)
    }

    /// Remove an object by id. Returns `true` if removed.
    pub fn remove(&mut self, id: ObjectId) -> bool {
        let len = self.objects.len();
        self.objects.retain(|o| o.id != id);
        let removed = self.objects.len() != len;
        if removed {
            self.prune_adjacency_cache();
        }
        removed
    }

    /// Get a reference to an object by id.
    pub fn get(&self, id: ObjectId) -> Option<&SceneObject> {
        self.objects.iter().find(|o| o.id == id)
    }

    /// Number of cached adjacency graphs.
    ///
    /// Exposed for the test that pins the cache being released with the last
    /// object referencing a mesh — a stale entry is a correctness bug, not just
    /// wasted memory, because `Arc` addresses are reused.
    #[cfg(test)]
    pub(crate) fn adjacency_cache_len(&self) -> usize {
        self.adjacency.len()
    }

    /// Get a mutable reference to an object by id.
    pub fn get_mut(&mut self, id: ObjectId) -> Option<&mut SceneObject> {
        self.objects.iter_mut().find(|o| o.id == id)
    }

    /// Placement problems for every object, in `objects` order.
    ///
    /// A plate with several models is easy to get wrong — one object nudged
    /// past the bed edge, or two overlapping after a duplicate — and both
    /// faults are invisible until the print fails. Reporting them from the
    /// scene engine keeps every front-end (viewer, CLI, WS) warning on the
    /// same rules rather than each re-deriving them.
    pub fn placement_report(&self) -> Vec<ObjectPlacement> {
        let boxes: Vec<AABB> = self.objects.iter().map(|o| o.world_aabb()).collect();

        boxes
            .iter()
            .enumerate()
            .map(|(i, aabb)| ObjectPlacement {
                id: self.objects[i].id,
                out_of_bounds: !self.bed.contains_aabb(aabb),
                collides: boxes
                    .iter()
                    .enumerate()
                    .any(|(j, other)| j != i && aabb_overlaps_xy(aabb, other)),
            })
            .collect()
    }
}

/// Whether an object sits somewhere it cannot be printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectPlacement {
    /// Object the report refers to.
    pub id: ObjectId,
    /// Any part of the object falls outside the printable volume.
    pub out_of_bounds: bool,
    /// The object's footprint overlaps another object's.
    pub collides: bool,
}

/// Do two boxes overlap in the XY plane?
///
/// Overlap is judged on the footprint only: two objects sharing XY space are
/// a collision even when currently at different heights, because the nozzle
/// must travel through the lower one to reach the upper one. Touching faces
/// (`max == min`) are not an overlap, so objects packed edge-to-edge by
/// `ArrangeOnBed` stay clean.
fn aabb_overlaps_xy(a: &AABB, b: &AABB) -> bool {
    a.min.x < b.max.x && b.min.x < a.max.x && a.min.y < b.max.y && b.min.y < a.max.y
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::types::Vertex;

    fn unit_cube_mesh() -> Arc<Mesh> {
        let mut m = Mesh::new();
        m.vertices = vec![Vertex::new(0.0, 0.0, 0.0), Vertex::new(1.0, 1.0, 1.0)];
        Arc::new(m)
    }

    #[test]
    fn add_and_remove() {
        let mut s = SceneState::new(BedConfig::default());
        let id = s.add_mesh("cube", unit_cube_mesh());
        assert_eq!(s.objects.len(), 1);
        assert!(s.get(id).is_some());
        assert!(s.remove(id));
        assert_eq!(s.objects.len(), 0);
    }

    #[test]
    fn ids_are_monotonic_and_not_reused() {
        let mut s = SceneState::new(BedConfig::default());
        let a = s.add_mesh("a", unit_cube_mesh());
        let b = s.add_mesh("b", unit_cube_mesh());
        s.remove(a);
        let c = s.add_mesh("c", unit_cube_mesh());
        assert_ne!(a, c);
        assert_ne!(b, c);
        assert!(c.0 > b.0);
    }

    #[test]
    fn duplicate_shares_mesh_and_inherits_source() {
        let mut s = SceneState::new(BedConfig::default());
        let a = s.add_mesh_from("a", unit_cube_mesh(), Some("file-uuid-1".into()));
        s.get_mut(a).unwrap().transform.translation = [5.0, 6.0, 7.0];

        let b = s.duplicate(a).expect("duplicate of a live object");
        assert_ne!(a, b);
        assert_eq!(s.objects.len(), 2);

        let (orig, copy) = (s.get(a).unwrap(), s.get(b).unwrap());
        // The copy must resolve to the same bytes, or slicing it server-side
        // would fetch the wrong file.
        assert_eq!(copy.source_id.as_deref(), Some("file-uuid-1"));
        assert_eq!(copy.name, orig.name);
        assert_eq!(copy.transform.translation, [5.0, 6.0, 7.0]);
        // Sharing, not deep-copying, is the whole point of the Arc.
        assert!(Arc::ptr_eq(&orig.mesh, &copy.mesh));
    }

    #[test]
    fn duplicate_of_unknown_id_is_none() {
        let mut s = SceneState::new(BedConfig::default());
        assert!(s.duplicate(ObjectId(999)).is_none());
    }

    #[test]
    fn placement_report_flags_overlap_and_out_of_bounds() {
        let mut s = SceneState::new(BedConfig::default());
        // Two unit cubes stacked at the same XY -> both collide.
        let a = s.add_mesh("a", unit_cube_mesh());
        let b = s.add_mesh("b", unit_cube_mesh());
        let report = s.placement_report();
        assert!(report.iter().all(|p| p.collides), "{report:?}");
        assert!(report.iter().all(|p| !p.out_of_bounds), "{report:?}");

        // Separate them in X -> no more overlap.
        s.get_mut(b).unwrap().transform.translation = [10.0, 0.0, 0.0];
        let report = s.placement_report();
        assert!(report.iter().all(|p| !p.collides), "{report:?}");

        // Shove one far past the bed edge -> out of bounds, still no overlap.
        s.get_mut(a).unwrap().transform.translation = [5000.0, 0.0, 0.0];
        let report = s.placement_report();
        let flagged = report.iter().find(|p| p.id == a).unwrap();
        assert!(flagged.out_of_bounds);
        assert!(!flagged.collides);
        assert!(!report.iter().find(|p| p.id == b).unwrap().out_of_bounds);
    }

    #[test]
    fn touching_footprints_are_not_a_collision() {
        // ArrangeOnBed packs objects edge-to-edge; abutting boxes must not be
        // reported as overlapping or every arranged plate would warn.
        let mut s = SceneState::new(BedConfig::default());
        s.add_mesh("a", unit_cube_mesh());
        let b = s.add_mesh("b", unit_cube_mesh());
        s.get_mut(b).unwrap().transform.translation = [1.0, 0.0, 0.0];
        assert!(s.placement_report().iter().all(|p| !p.collides));
    }
}
