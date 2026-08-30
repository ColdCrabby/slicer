//! Mesh validation and repair — the health check every model passes on import.
//!
//! Real-world STLs are frequently defective: vertices that should be shared are
//! duplicated (so the surface is "cracked"), triangles are missing (holes),
//! normals point inward, and zero-area or repeated triangles ride along. The
//! slicer chains open contour segments defensively, which *hides* those defects
//! behind symptoms — missing top/bottom surfaces, broken islands — that are
//! very hard to diagnose downstream.
//!
//! This module builds a welded, indexed view of a [`Mesh`], reports its
//! topological health ([`MeshDiagnostics`]), and optionally repairs it
//! ([`repair`]).
//!
//! # Contract
//!
//! - **A clean mesh is never touched.** [`repair`] returns
//!   [`Cow::Borrowed`] whenever the diagnostics come back clean, so a
//!   well-formed model is bit-for-bit unchanged and costs one analysis pass.
//! - **Repair is deterministic.** The same bytes always produce the same
//!   vertex and face ordering. Face indices are part of the wire protocol
//!   (`SceneOp::PlaceFaceOnFloor { face_index }` is picked in the browser and
//!   re-resolved by the engine), so this is a correctness requirement, not a
//!   nicety.
//! - **Pure `std`.** No new dependencies, so the wasm build is unaffected.
//!
//! # Non-goals
//!
//! Non-manifold edges (more than two incident faces) are *reported* but not
//! repaired — splitting them changes the surface in ways that need their own
//! design. Self-intersections, T-junctions and shell separation are likewise
//! out of scope.

use crate::mesh::types::{Face, Mesh, Vertex};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// Triangles with an area at or below this (mm²) are treated as degenerate.
const DEGENERATE_AREA_MM2: f64 = 1e-10;

/// Default distance below which two vertices are considered the same point.
///
/// STL stores coordinates as `f32`, so a 100 mm model already carries ~1e-5 mm
/// of representation error; 1e-4 mm sits just above that and far below any
/// intentional geometry.
pub const DEFAULT_WELD_TOLERANCE_MM: f64 = 1e-4;

/// Default upper bound on the size of a hole that will be capped.
///
/// A boundary loop longer than this is far more likely to be an intentionally
/// open surface than a defect, so it is reported and left alone.
pub const DEFAULT_MAX_HOLE_EDGES: usize = 512;

// ---------------------------------------------------------------------------
// Fast hashing
// ---------------------------------------------------------------------------

/// FxHash-style hasher — the edge and vertex maps below are hot enough that
/// SipHash shows up in load times on a 225k-triangle model.
#[derive(Default)]
struct FxHasher {
    hash: u64,
}

const FX_SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl FxHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(FX_SEED);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.add(*b as u64);
        }
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }
    #[inline]
    fn write_i64(&mut self, i: i64) {
        self.add(i as u64);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
}

type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;

fn fx_map<K, V>(capacity: usize) -> FxMap<K, V> {
    FxMap::with_capacity_and_hasher(capacity, BuildHasherDefault::<FxHasher>::default())
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Topological health of a mesh, measured on its welded triangle graph.
///
/// All counts describe the mesh *as loaded* (after exact-position welding);
/// [`repair`] reports a second set measured after its work.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MeshDiagnostics {
    /// Triangle count.
    pub triangles: usize,
    /// Unique vertex positions after welding.
    pub vertices: usize,
    /// Connected components, joined across shared edges.
    pub shells: usize,
    /// Triangles with a repeated corner or effectively zero area.
    pub degenerate_faces: usize,
    /// Triangles that repeat another triangle's corner set.
    pub duplicate_faces: usize,
    /// Edges with more than two incident triangles.
    pub non_manifold_edges: usize,
    /// Edges with exactly one incident triangle that bound a region of real
    /// area (the rim of an actual hole).
    pub boundary_edges: usize,
    /// Edges with exactly one incident triangle whose loop encloses **no**
    /// area — a zero-width slit, not a hole.
    ///
    /// These are what a zero-area sliver leaves behind: the sliver itself is
    /// excluded from the edge graph as degenerate, so its rim reads as
    /// "boundary" even though the surface encloses exactly the same solid with
    /// or without it. They are extremely common in exported STLs, cannot be
    /// meaningfully capped (any patch would itself be zero-area), and are
    /// therefore reported for information but never treated as a defect.
    pub slit_boundary_edges: usize,
    /// Closed loops formed by the boundary edges — i.e. the number of holes.
    /// Zero-area slit loops are excluded.
    pub holes: usize,
    /// Edge count of the largest hole, or 0 when there are none.
    pub largest_hole_edges: usize,
    /// Shared edges traversed in the *same* direction by both triangles, which
    /// means the two disagree about which side is outside.
    pub inconsistent_winding_edges: usize,
    /// Closed shells whose signed volume is negative — their normals point in.
    pub inverted_shells: usize,
}

impl MeshDiagnostics {
    /// No holes: every edge bounding real area has at least two incident
    /// triangles. Zero-area slits do not count — they enclose nothing, so a
    /// surface carrying them still bounds the same solid.
    pub fn is_watertight(&self) -> bool {
        self.boundary_edges == 0
    }

    /// Every edge is shared by exactly two consistently-wound triangles.
    pub fn is_manifold(&self) -> bool {
        self.non_manifold_edges == 0 && self.inconsistent_winding_edges == 0
    }

    /// Nothing to fix and nothing to warn about.
    pub fn is_clean(&self) -> bool {
        self.degenerate_faces == 0
            && self.duplicate_faces == 0
            && self.inverted_shells == 0
            && self.is_watertight()
            && self.is_manifold()
    }

    /// Comma-separated list of the defects present, or `None` when clean.
    pub fn defect_summary(&self) -> Option<String> {
        let mut parts = Vec::new();
        if self.degenerate_faces > 0 {
            parts.push(format!(
                "{} degenerate {}",
                self.degenerate_faces,
                plural(self.degenerate_faces, "triangle")
            ));
        }
        if self.duplicate_faces > 0 {
            parts.push(format!(
                "{} duplicate {}",
                self.duplicate_faces,
                plural(self.duplicate_faces, "triangle")
            ));
        }
        if self.holes > 0 {
            parts.push(format!(
                "{} {} ({} boundary {})",
                self.holes,
                plural(self.holes, "hole"),
                self.boundary_edges,
                plural(self.boundary_edges, "edge")
            ));
        } else if self.boundary_edges > 0 {
            parts.push(format!(
                "{} boundary {}",
                self.boundary_edges,
                plural(self.boundary_edges, "edge")
            ));
        }
        if self.non_manifold_edges > 0 {
            parts.push(format!(
                "{} non-manifold {}",
                self.non_manifold_edges,
                plural(self.non_manifold_edges, "edge")
            ));
        }
        if self.inconsistent_winding_edges > 0 {
            parts.push(format!(
                "{} inconsistently wound {}",
                self.inconsistent_winding_edges,
                plural(self.inconsistent_winding_edges, "edge")
            ));
        }
        if self.inverted_shells > 0 {
            parts.push(format!(
                "{} inverted {}",
                self.inverted_shells,
                plural(self.inverted_shells, "shell")
            ));
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(", "))
        }
    }
}

/// What [`repair`] actually changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RepairActions {
    /// Vertices merged by the tolerance pass (beyond exact-position welding).
    pub welded_vertices: usize,
    /// Zero-area or repeated-corner triangles dropped.
    pub removed_degenerate_faces: usize,
    /// Redundant repeats of an existing triangle dropped.
    pub removed_duplicate_faces: usize,
    /// Triangles whose winding was reversed to agree with their neighbours.
    pub flipped_faces: usize,
    /// Holes capped.
    pub filled_holes: usize,
    /// Triangles added to cap those holes.
    pub added_fill_triangles: usize,
    /// Holes left open because they exceeded `max_hole_edges`.
    pub unfilled_holes: usize,
}

impl RepairActions {
    /// `true` when the repair pass changed nothing.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Comma-separated list of what changed, or `None` when nothing did.
    pub fn summary(&self) -> Option<String> {
        let mut parts = Vec::new();
        if self.welded_vertices > 0 {
            parts.push(format!(
                "welded {} {}",
                self.welded_vertices,
                plural(self.welded_vertices, "vertex")
            ));
        }
        if self.removed_degenerate_faces > 0 {
            parts.push(format!(
                "removed {} degenerate {}",
                self.removed_degenerate_faces,
                plural(self.removed_degenerate_faces, "triangle")
            ));
        }
        if self.removed_duplicate_faces > 0 {
            parts.push(format!(
                "removed {} duplicate {}",
                self.removed_duplicate_faces,
                plural(self.removed_duplicate_faces, "triangle")
            ));
        }
        if self.flipped_faces > 0 {
            parts.push(format!(
                "flipped {} {}",
                self.flipped_faces,
                plural(self.flipped_faces, "triangle")
            ));
        }
        if self.filled_holes > 0 {
            parts.push(format!(
                "filled {} {} with {} {}",
                self.filled_holes,
                plural(self.filled_holes, "hole"),
                self.added_fill_triangles,
                plural(self.added_fill_triangles, "triangle")
            ));
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(", "))
        }
    }
}

/// Which repairs to attempt, and how aggressively.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RepairOptions {
    /// Master switch. When `false`, [`repair`] only measures.
    pub enabled: bool,
    /// Distance below which two vertices are merged (mm).
    pub weld_tolerance_mm: f64,
    /// Drop zero-area and repeated-corner triangles.
    pub remove_degenerate: bool,
    /// Drop redundant repeats of an existing triangle.
    pub remove_duplicates: bool,
    /// Make winding consistent per shell and orient normals outward.
    pub unify_winding: bool,
    /// Cap boundary loops with new triangles.
    pub fill_holes: bool,
    /// Largest boundary loop (in edges) that will be capped.
    pub max_hole_edges: usize,
}

impl Default for RepairOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            weld_tolerance_mm: DEFAULT_WELD_TOLERANCE_MM,
            remove_degenerate: true,
            remove_duplicates: true,
            unify_winding: true,
            fill_holes: true,
            max_hole_edges: DEFAULT_MAX_HOLE_EDGES,
        }
    }
}

impl RepairOptions {
    /// Analysis only — measure the mesh but change nothing.
    pub fn analysis_only() -> Self {
        Self {
            enabled: false,
            ..Self::default()
        }
    }
}

/// The outcome of a [`repair`] call: health before, health after, and the
/// actions taken in between.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MeshReport {
    /// Health of the mesh as loaded.
    pub before: MeshDiagnostics,
    /// Health of the mesh handed on to the rest of the engine.
    pub after: MeshDiagnostics,
    /// What the repair pass changed.
    pub actions: RepairActions,
    /// `true` when the mesh was rewritten.
    pub repaired: bool,
    /// One-line human summary suitable for a log line or a toast.
    pub summary: String,
}

impl MeshReport {
    /// `true` when the incoming mesh had nothing wrong with it.
    pub fn was_clean(&self) -> bool {
        self.before.is_clean()
    }

    /// `true` when the mesh still has defects the repair pass could not fix.
    pub fn has_remaining_defects(&self) -> bool {
        !self.after.is_clean()
    }

    /// `true` when the report is worth putting in front of a user.
    pub fn is_noteworthy(&self) -> bool {
        !self.was_clean() || self.has_remaining_defects()
    }

    fn build(before: MeshDiagnostics, after: MeshDiagnostics, actions: RepairActions) -> Self {
        let summary = compose_summary(&before, &after, &actions);
        Self {
            before,
            after,
            actions,
            repaired: !actions.is_empty(),
            summary,
        }
    }
}

fn compose_summary(
    before: &MeshDiagnostics,
    after: &MeshDiagnostics,
    actions: &RepairActions,
) -> String {
    let head = format!(
        "{} {}",
        after.triangles,
        plural(after.triangles, "triangle")
    );
    match (before.defect_summary(), actions.summary()) {
        (None, _) => format!("{head}: clean, watertight and manifold"),
        (Some(found), taken) => {
            let mut s = format!("{head}: found {found}");
            if let Some(taken) = taken {
                s.push_str(&format!("; {taken}"));
            }
            match after.defect_summary() {
                Some(left) => s.push_str(&format!("; {left} remain")),
                None => s.push_str("; mesh is now clean"),
            }
            s
        }
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        word.to_string()
    } else if word == "vertex" {
        "vertices".to_string()
    } else {
        format!("{word}s")
    }
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Emit a report through a process logger.
///
/// Defects are a warning (the user needs to know their model was patched, or
/// that it still isn't sound); a clean bill of health is debug-only noise.
/// Shared by the CLI, the WS server and the desktop bridge so every runtime
/// words it identically.
pub fn log_report(logger: &dyn crate::logging::ProcessLogger, label: &str, report: &MeshReport) {
    if report.is_noteworthy() {
        logger.log_warn(&format!("[mesh] {label}: {}", report.summary));
    } else {
        logger.log_debug(&format!("[mesh] {label}: {}", report.summary));
    }
}

/// Measure a mesh's topological health without changing it.
pub fn analyze(mesh: &Mesh) -> MeshDiagnostics {
    let indexed = Indexed::from_mesh(mesh);
    indexed.diagnose()
}

/// Validate and (optionally) repair a mesh.
///
/// Returns the mesh to use downstream together with a [`MeshReport`]. When the
/// incoming mesh is already clean — or when `options.enabled` is `false` — the
/// original mesh is borrowed and returned untouched.
///
/// # Example
/// ```
/// use slicer_engine::mesh::repair::{repair, RepairOptions};
/// use slicer_engine::mesh::types::Mesh;
///
/// let mesh = Mesh::new();
/// let (mesh, report) = repair(&mesh, &RepairOptions::default());
/// assert!(!report.repaired);
/// assert_eq!(mesh.faces.len(), 0);
/// ```
pub fn repair<'a>(mesh: &'a Mesh, options: &RepairOptions) -> (Cow<'a, Mesh>, MeshReport) {
    let mut indexed = Indexed::from_mesh(mesh);
    let before = indexed.diagnose();

    if !options.enabled || before.is_clean() {
        return (
            Cow::Borrowed(mesh),
            MeshReport::build(before, before, RepairActions::default()),
        );
    }

    let mut actions = RepairActions::default();

    // Weld against *any* open edge, slits included: a crack narrow enough to
    // read as zero-area is exactly the kind welding exists to close.
    if before.boundary_edges + before.slit_boundary_edges > 0 {
        actions.welded_vertices = indexed.weld_within(options.weld_tolerance_mm);
    }
    if options.remove_degenerate {
        actions.removed_degenerate_faces = indexed.drop_degenerate();
    }
    if options.remove_duplicates {
        actions.removed_duplicate_faces = indexed.drop_duplicates();
    }
    if options.unify_winding {
        actions.flipped_faces = indexed.unify_winding();
    }
    if options.fill_holes {
        let fill = indexed.fill_holes(options.max_hole_edges);
        actions.filled_holes = fill.filled;
        actions.added_fill_triangles = fill.triangles;
        actions.unfilled_holes = fill.skipped;
        // Capping a hole can leave a shell inverted (a mesh that was open
        // cannot have a meaningful signed volume until it is closed), so give
        // the winding pass a second look once the surface is watertight.
        if options.unify_winding && fill.filled > 0 {
            actions.flipped_faces += indexed.unify_winding();
        }
    }

    if actions.is_empty() {
        return (
            Cow::Borrowed(mesh),
            MeshReport::build(before, before, actions),
        );
    }

    let after = indexed.diagnose();
    (
        Cow::Owned(indexed.to_mesh()),
        MeshReport::build(before, after, actions),
    )
}

// ---------------------------------------------------------------------------
// Indexed mesh
// ---------------------------------------------------------------------------

/// A welded, indexed view of a mesh: unique positions plus triangles that
/// reference them. Every repair step operates on this representation.
struct Indexed {
    positions: Vec<Vertex>,
    tris: Vec<[u32; 3]>,
}

impl Indexed {
    /// Build from a mesh by welding exactly-coincident face corners.
    ///
    /// `Face` carries vertex *copies*, so the faces — not `Mesh::vertices` —
    /// are the authoritative geometry.
    fn from_mesh(mesh: &Mesh) -> Self {
        let mut map: FxMap<[u64; 3], u32> = fx_map(mesh.faces.len() * 3 / 2 + 1);
        let mut positions: Vec<Vertex> = Vec::with_capacity(mesh.faces.len() / 2 + 1);
        let mut tris: Vec<[u32; 3]> = Vec::with_capacity(mesh.faces.len());

        for face in &mesh.faces {
            let mut idx = [0u32; 3];
            for (slot, v) in idx.iter_mut().zip(face.vertices.iter()) {
                let key = position_key(v);
                *slot = *map.entry(key).or_insert_with(|| {
                    positions.push(*v);
                    (positions.len() - 1) as u32
                });
            }
            tris.push(idx);
        }

        Self { positions, tris }
    }

    fn area_of(&self, tri: &[u32; 3]) -> f64 {
        let a = self.positions[tri[0] as usize];
        let b = self.positions[tri[1] as usize];
        let c = self.positions[tri[2] as usize];
        let n = cross(sub(&b, &a), sub(&c, &a));
        0.5 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt()
    }

    fn is_degenerate(&self, tri: &[u32; 3]) -> bool {
        tri[0] == tri[1]
            || tri[1] == tri[2]
            || tri[2] == tri[0]
            || self.area_of(tri) <= DEGENERATE_AREA_MM2
    }

    /// Measure health. Degenerate triangles are counted but excluded from the
    /// edge graph, where their repeated corners would fabricate defects.
    fn diagnose(&self) -> MeshDiagnostics {
        let mut d = MeshDiagnostics {
            triangles: self.tris.len(),
            vertices: self.positions.len(),
            ..Default::default()
        };
        if self.tris.is_empty() {
            return d;
        }

        let live: Vec<u32> = (0..self.tris.len() as u32)
            .filter(|i| !self.is_degenerate(&self.tris[*i as usize]))
            .collect();
        d.degenerate_faces = self.tris.len() - live.len();
        d.duplicate_faces = self.count_duplicates(&live);

        let graph = EdgeGraph::build(&self.tris, &live);
        d.non_manifold_edges = graph.non_manifold_edges;
        d.inconsistent_winding_edges = graph.inconsistent_edges;

        let loops = graph.boundary_loops();
        // A boundary loop enclosing no area is a slit, not a hole: it is what a
        // zero-area sliver leaves behind once the sliver itself is excluded as
        // degenerate. Capping it could only ever produce more zero-area
        // triangles, so it is counted separately and never treated as a defect.
        for boundary in &loops {
            let edges = boundary.len();
            if self.loop_area(boundary) > DEGENERATE_AREA_MM2 {
                d.holes += 1;
                d.boundary_edges += edges;
                d.largest_hole_edges = d.largest_hole_edges.max(edges);
            } else {
                d.slit_boundary_edges += edges;
            }
        }
        // Boundary half-edges that never formed a closed loop (a dangling
        // chain) are still genuine open edges; attribute them to holes so they
        // are not silently dropped from the count.
        let looped: usize = loops.iter().map(|l| l.len()).sum();
        d.boundary_edges += graph.boundary.len().saturating_sub(looped);

        let shells = graph.shells(self.tris.len());
        d.shells = shells.count;
        if d.boundary_edges == 0 && d.non_manifold_edges == 0 && d.inconsistent_winding_edges == 0 {
            let mut volumes = vec![0.0f64; shells.count];
            for (face, tri) in self.tris.iter().enumerate() {
                if let Some(shell) = shells.of_face[face] {
                    volumes[shell as usize] += self.signed_volume(tri);
                }
            }
            d.inverted_shells = volumes.iter().filter(|v| **v < 0.0).count();
        }

        d
    }

    fn count_duplicates(&self, live: &[u32]) -> usize {
        let mut seen: FxMap<[u32; 3], u32> = fx_map(live.len());
        let mut dupes = 0;
        for face in live {
            let key = sorted_key(&self.tris[*face as usize]);
            let slot = seen.entry(key).or_insert(0);
            if *slot > 0 {
                dupes += 1;
            }
            *slot += 1;
        }
        dupes
    }

    /// Area enclosed by a boundary loop, via Newell's method so it is valid
    /// for a non-planar rim too. Zero means the loop is a slit.
    fn loop_area(&self, boundary: &[u32]) -> f64 {
        let mut n = [0.0f64; 3];
        for i in 0..boundary.len() {
            let p = self.positions[boundary[i] as usize];
            let q = self.positions[boundary[(i + 1) % boundary.len()] as usize];
            n[0] += (p.y - q.y) * (p.z + q.z);
            n[1] += (p.z - q.z) * (p.x + q.x);
            n[2] += (p.x - q.x) * (p.y + q.y);
        }
        0.5 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt()
    }

    fn signed_volume(&self, tri: &[u32; 3]) -> f64 {
        let a = self.positions[tri[0] as usize];
        let b = self.positions[tri[1] as usize];
        let c = self.positions[tri[2] as usize];
        dot([a.x, a.y, a.z], cross([b.x, b.y, b.z], [c.x, c.y, c.z])) / 6.0
    }

    /// Merge vertices that lie within `tolerance` of one another.
    ///
    /// Only vertices touching a boundary edge are considered: a crack is
    /// exactly where welding matters, the candidate set is tiny compared with
    /// the whole model, and restricting it makes accidental merges of
    /// genuinely distinct interior geometry impossible.
    ///
    /// Returns the number of vertices merged away.
    fn weld_within(&mut self, tolerance: f64) -> usize {
        if tolerance <= 0.0 || self.positions.is_empty() {
            return 0;
        }

        let live: Vec<u32> = (0..self.tris.len() as u32)
            .filter(|i| !self.is_degenerate(&self.tris[*i as usize]))
            .collect();
        let graph = EdgeGraph::build(&self.tris, &live);
        if graph.boundary.is_empty() {
            return 0;
        }

        let mut candidate = vec![false; self.positions.len()];
        for (a, b, _) in &graph.boundary {
            candidate[*a as usize] = true;
            candidate[*b as usize] = true;
        }
        let candidates: Vec<u32> = (0..self.positions.len() as u32)
            .filter(|i| candidate[*i as usize])
            .collect();

        // Spatial hash on a grid of `tolerance`; probing the 27 surrounding
        // cells makes the result independent of where the grid lines fall.
        let inv = 1.0 / tolerance;
        let mut buckets: FxMap<[i64; 3], Vec<u32>> = fx_map(candidates.len());
        for v in &candidates {
            let p = self.positions[*v as usize];
            buckets.entry(cell_of(&p, inv)).or_default().push(*v);
        }

        let mut parent: Vec<u32> = (0..self.positions.len() as u32).collect();
        let tol_sq = tolerance * tolerance;
        for v in &candidates {
            let p = self.positions[*v as usize];
            let base = cell_of(&p, inv);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        let cell = [base[0] + dx, base[1] + dy, base[2] + dz];
                        let Some(others) = buckets.get(&cell) else {
                            continue;
                        };
                        for other in others {
                            if other <= v {
                                continue;
                            }
                            let q = self.positions[*other as usize];
                            if distance_sq(&p, &q) <= tol_sq {
                                uf_union(&mut parent, *v, *other);
                            }
                        }
                    }
                }
            }
        }

        // Rewrite triangles through the union-find roots, then compact.
        let mut merged = 0usize;
        for i in 0..self.positions.len() as u32 {
            if uf_find(&mut parent, i) != i {
                merged += 1;
            }
        }
        if merged == 0 {
            return 0;
        }
        for tri in &mut self.tris {
            for slot in tri.iter_mut() {
                *slot = uf_find(&mut parent, *slot);
            }
        }
        self.compact();
        merged
    }

    /// Drop zero-area and repeated-corner triangles. Returns the count removed.
    fn drop_degenerate(&mut self) -> usize {
        let before = self.tris.len();
        let mut kept = Vec::with_capacity(before);
        for tri in &self.tris {
            if !self.is_degenerate(tri) {
                kept.push(*tri);
            }
        }
        let removed = before - kept.len();
        if removed > 0 {
            self.tris = kept;
            self.compact();
        }
        removed
    }

    /// Drop redundant repeats of a triangle.
    ///
    /// Triangles sharing a corner set are grouped. When one orientation
    /// outnumbers the other, a single triangle in the majority orientation
    /// survives. When they cancel exactly — the classic zero-volume "flap" —
    /// the whole group goes, because keeping one would tear three edges open.
    fn drop_duplicates(&mut self) -> usize {
        let mut groups: FxMap<[u32; 3], i32> = fx_map(self.tris.len());
        for tri in self.tris.iter() {
            let net = groups.entry(sorted_key(tri)).or_insert(0);
            *net += if is_even_permutation(tri) { 1 } else { -1 };
        }
        if groups.len() == self.tris.len() {
            return 0;
        }

        let mut emitted: FxMap<[u32; 3], bool> = fx_map(groups.len());
        let mut kept = Vec::with_capacity(groups.len());
        for tri in &self.tris {
            let key = sorted_key(tri);
            let net = groups[&key];
            if net == 0 {
                continue;
            }
            let wanted_even = net > 0;
            if is_even_permutation(tri) != wanted_even {
                continue;
            }
            if emitted.insert(key, true).is_some() {
                continue;
            }
            kept.push(*tri);
        }

        let removed = self.tris.len() - kept.len();
        if removed > 0 {
            self.tris = kept;
            self.compact();
        }
        removed
    }

    /// Make winding consistent across each shell, then orient every closed
    /// shell so its normals point outward. Returns the number of triangles
    /// whose winding was reversed.
    fn unify_winding(&mut self) -> usize {
        if self.tris.is_empty() {
            return 0;
        }
        let live: Vec<u32> = (0..self.tris.len() as u32)
            .filter(|i| !self.is_degenerate(&self.tris[*i as usize]))
            .collect();
        let graph = EdgeGraph::build(&self.tris, &live);
        let adjacency = graph.face_adjacency(self.tris.len());

        // Signed volume is the divergence-theorem cone volume about the
        // origin: it equals the enclosed volume only for a **closed** shell.
        // On an open one the sum is dominated by the cone over the missing
        // region and its sign depends on where the model happens to sit
        // relative to the origin — orienting by it would flip a correctly
        // wound open surface inside out. So track which faces disqualify their
        // shell: any face on a boundary or non-manifold edge, and any face the
        // edge graph never saw (degenerate ones).
        let mut open = vec![true; self.tris.len()];
        for face in &live {
            open[*face as usize] = false;
        }
        for (_, _, face) in &graph.boundary {
            open[*face as usize] = true;
        }
        for face in &graph.non_manifold_faces {
            open[*face as usize] = true;
        }

        let mut flip = vec![false; self.tris.len()];
        let mut visited = vec![false; self.tris.len()];
        let mut stack: Vec<u32> = Vec::new();
        let mut shell_members: Vec<u32> = Vec::new();

        for seed in 0..self.tris.len() as u32 {
            if visited[seed as usize] {
                continue;
            }
            visited[seed as usize] = true;
            stack.push(seed);
            shell_members.clear();

            while let Some(face) = stack.pop() {
                shell_members.push(face);
                for (neighbour, same_direction) in adjacency.neighbours(face) {
                    let (neighbour, same_direction) = (*neighbour, *same_direction);
                    if visited[neighbour as usize] {
                        continue;
                    }
                    visited[neighbour as usize] = true;
                    // Two triangles agree when they traverse their shared edge
                    // in opposite directions; `same_direction` means they
                    // disagree, so the neighbour needs the opposite flip state.
                    flip[neighbour as usize] = flip[face as usize] ^ same_direction;
                    stack.push(neighbour);
                }
            }

            // Orient outward — but only for a shell that is actually closed.
            // An open one keeps whatever the BFS produced; if `fill_holes`
            // later seals it, `repair` runs this pass again and the shell is
            // oriented then.
            if shell_members.iter().any(|f| open[*f as usize]) {
                continue;
            }
            let volume: f64 = shell_members
                .iter()
                .map(|f| {
                    let tri = self.tris[*f as usize];
                    let tri = if flip[*f as usize] {
                        flipped(&tri)
                    } else {
                        tri
                    };
                    self.signed_volume(&tri)
                })
                .sum();
            if volume < 0.0 {
                for f in &shell_members {
                    flip[*f as usize] = !flip[*f as usize];
                }
            }
        }

        let mut flipped_count = 0;
        for (i, tri) in self.tris.iter_mut().enumerate() {
            if flip[i] {
                *tri = flipped(tri);
                flipped_count += 1;
            }
        }
        flipped_count
    }

    /// Cap boundary loops no longer than `max_edges`.
    fn fill_holes(&mut self, max_edges: usize) -> FillOutcome {
        let live: Vec<u32> = (0..self.tris.len() as u32)
            .filter(|i| !self.is_degenerate(&self.tris[*i as usize]))
            .collect();
        let graph = EdgeGraph::build(&self.tris, &live);
        let loops = graph.boundary_loops();

        let mut outcome = FillOutcome::default();
        for boundary in &loops {
            if boundary.len() < 3 {
                continue;
            }
            // A loop enclosing no area is a slit left by a zero-area sliver,
            // not a hole. Any patch across it would itself be zero-area — it
            // would close nothing (`diagnose` excludes degenerate triangles
            // from the edge graph, so the rim would still read as boundary)
            // while adding junk geometry to every model that has one, which
            // real-world STLs very often do. Leave it alone and don't count it
            // as unfilled either: there was never a hole to fill.
            if self.loop_area(boundary) <= DEGENERATE_AREA_MM2 {
                continue;
            }
            if boundary.len() > max_edges {
                outcome.skipped += 1;
                continue;
            }
            // Each cap triangle reuses a boundary half-edge in reverse, so the
            // patch is consistently wound with the surface it closes.
            if boundary.len() == 3 {
                self.tris.push([boundary[0], boundary[2], boundary[1]]);
                outcome.triangles += 1;
            } else {
                let centre = centroid(
                    &boundary
                        .iter()
                        .map(|v| self.positions[*v as usize])
                        .collect::<Vec<_>>(),
                );
                self.positions.push(centre);
                let c = (self.positions.len() - 1) as u32;
                for i in 0..boundary.len() {
                    let a = boundary[i];
                    let b = boundary[(i + 1) % boundary.len()];
                    self.tris.push([c, b, a]);
                    outcome.triangles += 1;
                }
            }
            outcome.filled += 1;
        }
        outcome
    }

    /// Drop unreferenced positions and renumber, preserving first-seen order.
    fn compact(&mut self) {
        let mut remap = vec![u32::MAX; self.positions.len()];
        let mut positions = Vec::with_capacity(self.positions.len());
        for tri in &mut self.tris {
            for slot in tri.iter_mut() {
                let old = *slot as usize;
                if remap[old] == u32::MAX {
                    positions.push(self.positions[old]);
                    remap[old] = (positions.len() - 1) as u32;
                }
                *slot = remap[old];
            }
        }
        self.positions = positions;
    }

    /// Rebuild a [`Mesh`], recomputing every face normal from the geometry.
    fn to_mesh(&self) -> Mesh {
        let faces = self
            .tris
            .iter()
            .map(|tri| {
                let vertices = [
                    self.positions[tri[0] as usize],
                    self.positions[tri[1] as usize],
                    self.positions[tri[2] as usize],
                ];
                let n = unit_normal(&vertices);
                Face {
                    vertices,
                    normal: n.map(|n| Vertex::new(n[0], n[1], n[2])),
                }
            })
            .collect();
        Mesh {
            vertices: self.positions.clone(),
            faces,
            aabb: None,
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct FillOutcome {
    filled: usize,
    triangles: usize,
    skipped: usize,
}

// ---------------------------------------------------------------------------
// Edge graph
// ---------------------------------------------------------------------------

/// One (undirected edge, incident triangle) record. `forward` is `true` when
/// the triangle traverses the edge from the low index to the high one.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct EdgeRef {
    lo: u32,
    hi: u32,
    face: u32,
    forward: bool,
}

/// Edge-centric view of the triangle graph, built once and queried for every
/// diagnostic and repair step.
struct EdgeGraph {
    /// Directed boundary half-edges `(from, to, face)`, in edge-sorted order.
    boundary: Vec<(u32, u32, u32)>,
    /// Manifold edges as `(face_a, face_b, same_direction)`.
    pairs: Vec<(u32, u32, bool)>,
    /// Faces incident to at least one non-manifold edge.
    non_manifold_faces: Vec<u32>,
    non_manifold_edges: usize,
    inconsistent_edges: usize,
}

impl EdgeGraph {
    fn build(tris: &[[u32; 3]], live: &[u32]) -> Self {
        let mut refs: Vec<EdgeRef> = Vec::with_capacity(live.len() * 3);
        for face in live {
            let tri = &tris[*face as usize];
            for k in 0..3 {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                refs.push(EdgeRef {
                    lo: a.min(b),
                    hi: a.max(b),
                    face: *face,
                    forward: a < b,
                });
            }
        }
        refs.sort_unstable();

        let mut graph = Self {
            boundary: Vec::new(),
            pairs: Vec::new(),
            non_manifold_faces: Vec::new(),
            non_manifold_edges: 0,
            inconsistent_edges: 0,
        };

        let mut i = 0;
        while i < refs.len() {
            let mut j = i + 1;
            while j < refs.len() && refs[j].lo == refs[i].lo && refs[j].hi == refs[i].hi {
                j += 1;
            }
            match j - i {
                1 => {
                    let e = refs[i];
                    let (from, to) = if e.forward {
                        (e.lo, e.hi)
                    } else {
                        (e.hi, e.lo)
                    };
                    graph.boundary.push((from, to, e.face));
                }
                2 => {
                    let (a, b) = (refs[i], refs[i + 1]);
                    let same_direction = a.forward == b.forward;
                    if same_direction {
                        graph.inconsistent_edges += 1;
                    }
                    graph.pairs.push((a.face, b.face, same_direction));
                }
                _ => {
                    graph.non_manifold_edges += 1;
                    graph
                        .non_manifold_faces
                        .extend(refs[i..j].iter().map(|e| e.face));
                }
            }
            i = j;
        }

        graph
    }

    /// Chain the boundary half-edges into closed loops — one per hole.
    fn boundary_loops(&self) -> Vec<Vec<u32>> {
        if self.boundary.is_empty() {
            return Vec::new();
        }
        let mut successors: FxMap<u32, Vec<usize>> = fx_map(self.boundary.len());
        for (i, (from, _, _)) in self.boundary.iter().enumerate() {
            successors.entry(*from).or_default().push(i);
        }

        let mut used = vec![false; self.boundary.len()];
        let mut loops = Vec::new();
        for start in 0..self.boundary.len() {
            if used[start] {
                continue;
            }
            let mut chain = Vec::new();
            let mut edge = start;
            loop {
                used[edge] = true;
                let (from, to, _) = self.boundary[edge];
                chain.push(from);
                if to == self.boundary[start].0 {
                    break;
                }
                let Some(next) = successors
                    .get(&to)
                    .and_then(|c| c.iter().copied().find(|i| !used[*i]))
                else {
                    // Dangling chain — the boundary is not a closed loop, so
                    // there is nothing safe to cap. Drop it.
                    chain.clear();
                    break;
                };
                edge = next;
            }
            if chain.len() >= 3 {
                loops.push(chain);
            }
        }
        loops
    }

    /// Union–find over the manifold edges: which shell each triangle is in.
    fn shells(&self, face_count: usize) -> Shells {
        let mut parent: Vec<u32> = (0..face_count as u32).collect();
        for (a, b, _) in &self.pairs {
            uf_union(&mut parent, *a, *b);
        }
        // Non-manifold edges still connect their faces materially; join them
        // through the boundary-free path so shell counts stay meaningful.
        let mut of_face = vec![None; face_count];
        let mut labels: FxMap<u32, u32> = fx_map(16);
        let mut count = 0;
        let mut touched = vec![false; face_count];
        for (a, b, _) in &self.pairs {
            touched[*a as usize] = true;
            touched[*b as usize] = true;
        }
        for (_, _, face) in &self.boundary {
            touched[*face as usize] = true;
        }
        for face in 0..face_count {
            if !touched[face] {
                continue;
            }
            let root = uf_find(&mut parent, face as u32);
            let label = *labels.entry(root).or_insert_with(|| {
                count += 1;
                count - 1
            });
            of_face[face] = Some(label);
        }
        Shells {
            count: count as usize,
            of_face,
        }
    }

    /// CSR adjacency over the manifold edges, for the winding BFS.
    fn face_adjacency(&self, face_count: usize) -> Adjacency {
        let mut degree = vec![0u32; face_count + 1];
        for (a, b, _) in &self.pairs {
            degree[*a as usize] += 1;
            degree[*b as usize] += 1;
        }
        let mut offsets = Vec::with_capacity(face_count + 1);
        let mut total = 0u32;
        for d in degree.iter().take(face_count) {
            offsets.push(total);
            total += *d;
        }
        offsets.push(total);

        let mut cursor = offsets.clone();
        let mut entries = vec![(0u32, false); total as usize];
        for (a, b, same) in &self.pairs {
            entries[cursor[*a as usize] as usize] = (*b, *same);
            cursor[*a as usize] += 1;
            entries[cursor[*b as usize] as usize] = (*a, *same);
            cursor[*b as usize] += 1;
        }
        Adjacency { offsets, entries }
    }
}

struct Shells {
    count: usize,
    of_face: Vec<Option<u32>>,
}

struct Adjacency {
    offsets: Vec<u32>,
    entries: Vec<(u32, bool)>,
}

impl Adjacency {
    fn neighbours(&self, face: u32) -> &[(u32, bool)] {
        let start = self.offsets[face as usize] as usize;
        let end = self.offsets[face as usize + 1] as usize;
        &self.entries[start..end]
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// Bit-exact position key, with `-0.0` and NaN normalised so they hash stably.
#[inline]
fn position_key(v: &Vertex) -> [u64; 3] {
    [norm_bits(v.x), norm_bits(v.y), norm_bits(v.z)]
}

#[inline]
fn norm_bits(x: f64) -> u64 {
    if x == 0.0 {
        0.0f64.to_bits()
    } else if x.is_nan() {
        f64::NAN.to_bits()
    } else {
        x.to_bits()
    }
}

#[inline]
fn cell_of(v: &Vertex, inv: f64) -> [i64; 3] {
    [
        (v.x * inv).floor() as i64,
        (v.y * inv).floor() as i64,
        (v.z * inv).floor() as i64,
    ]
}

#[inline]
fn sorted_key(tri: &[u32; 3]) -> [u32; 3] {
    let mut k = *tri;
    k.sort_unstable();
    k
}

/// `true` when the triangle's corner order is an even permutation of its
/// sorted order — i.e. the two triples describe the same winding.
#[inline]
fn is_even_permutation(tri: &[u32; 3]) -> bool {
    let mut swaps = 0;
    let mut k = *tri;
    for i in 0..3 {
        for j in 0..2 - i {
            if k[j] > k[j + 1] {
                k.swap(j, j + 1);
                swaps += 1;
            }
        }
    }
    swaps % 2 == 0
}

#[inline]
fn flipped(tri: &[u32; 3]) -> [u32; 3] {
    [tri[0], tri[2], tri[1]]
}

#[inline]
fn sub(a: &Vertex, b: &Vertex) -> [f64; 3] {
    [a.x - b.x, a.y - b.y, a.z - b.z]
}

#[inline]
fn cross(u: [f64; 3], v: [f64; 3]) -> [f64; 3] {
    [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ]
}

#[inline]
fn dot(u: [f64; 3], v: [f64; 3]) -> f64 {
    u[0] * v[0] + u[1] * v[1] + u[2] * v[2]
}

#[inline]
fn distance_sq(a: &Vertex, b: &Vertex) -> f64 {
    let d = sub(a, b);
    dot(d, d)
}

fn unit_normal(v: &[Vertex; 3]) -> Option<[f64; 3]> {
    let n = cross(sub(&v[1], &v[0]), sub(&v[2], &v[0]));
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len < 1e-12 {
        None
    } else {
        Some([n[0] / len, n[1] / len, n[2] / len])
    }
}

fn centroid(points: &[Vertex]) -> Vertex {
    let n = points.len() as f64;
    let mut sum = [0.0f64; 3];
    for p in points {
        sum[0] += p.x;
        sum[1] += p.y;
        sum[2] += p.z;
    }
    Vertex::new(sum[0] / n, sum[1] / n, sum[2] / n)
}

fn uf_find(parent: &mut [u32], mut i: u32) -> u32 {
    while parent[i as usize] != i {
        parent[i as usize] = parent[parent[i as usize] as usize];
        i = parent[i as usize];
    }
    i
}

fn uf_union(parent: &mut [u32], a: u32, b: u32) {
    let ra = uf_find(parent, a);
    let rb = uf_find(parent, b);
    if ra == rb {
        return;
    }
    // Always attach the larger root to the smaller one: the result no longer
    // depends on insertion order, which keeps repair deterministic.
    if ra < rb {
        parent[rb as usize] = ra;
    } else {
        parent[ra as usize] = rb;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> Vertex {
        Vertex::new(x, y, z)
    }

    fn mesh_from(tris: &[[Vertex; 3]]) -> Mesh {
        let mut mesh = Mesh::new();
        for t in tris {
            mesh.faces.push(Face::new(*t));
        }
        mesh.vertices = mesh
            .faces
            .iter()
            .flat_map(|f| f.vertices.iter().copied())
            .collect();
        mesh
    }

    /// Unit cube from (0,0,0) to (s,s,s), outward-facing, watertight.
    fn cube(s: f64) -> Vec<[Vertex; 3]> {
        let p = [
            v(0.0, 0.0, 0.0),
            v(s, 0.0, 0.0),
            v(s, s, 0.0),
            v(0.0, s, 0.0),
            v(0.0, 0.0, s),
            v(s, 0.0, s),
            v(s, s, s),
            v(0.0, s, s),
        ];
        let quads = [
            [0, 3, 2, 1], // bottom (-Z)
            [4, 5, 6, 7], // top (+Z)
            [0, 1, 5, 4], // front (-Y)
            [1, 2, 6, 5], // right (+X)
            [2, 3, 7, 6], // back (+Y)
            [3, 0, 4, 7], // left (-X)
        ];
        let mut tris = Vec::new();
        for q in quads {
            tris.push([p[q[0]], p[q[1]], p[q[2]]]);
            tris.push([p[q[0]], p[q[2]], p[q[3]]]);
        }
        tris
    }

    #[test]
    fn clean_cube_is_reported_clean() {
        let mesh = mesh_from(&cube(10.0));
        let d = analyze(&mesh);
        assert_eq!(d.triangles, 12);
        assert_eq!(d.vertices, 8);
        assert_eq!(d.shells, 1);
        assert_eq!(d.boundary_edges, 0);
        assert_eq!(d.non_manifold_edges, 0);
        assert_eq!(d.inconsistent_winding_edges, 0);
        assert_eq!(d.inverted_shells, 0);
        assert!(d.is_clean());
    }

    #[test]
    fn clean_mesh_is_borrowed_not_rebuilt() {
        let mesh = mesh_from(&cube(10.0));
        let (out, report) = repair(&mesh, &RepairOptions::default());
        assert!(matches!(out, Cow::Borrowed(_)));
        assert!(!report.repaired);
        assert!(report.was_clean());
        assert!(!report.is_noteworthy());
        assert!(report.summary.contains("clean"));
    }

    #[test]
    fn analysis_only_measures_without_changing() {
        let mut tris = cube(10.0);
        tris.pop();
        let mesh = mesh_from(&tris);
        let (out, report) = repair(&mesh, &RepairOptions::analysis_only());
        assert!(matches!(out, Cow::Borrowed(_)));
        assert!(!report.repaired);
        assert_eq!(report.before, report.after);
        assert_eq!(report.before.holes, 1);
    }

    #[test]
    fn triangular_hole_is_capped() {
        let mut tris = cube(10.0);
        tris.pop(); // remove one triangle → a 3-edge hole
        let mesh = mesh_from(&tris);

        let before = analyze(&mesh);
        assert_eq!(before.boundary_edges, 3);
        assert_eq!(before.holes, 1);
        assert_eq!(before.largest_hole_edges, 3);
        assert!(!before.is_watertight());

        let (out, report) = repair(&mesh, &RepairOptions::default());
        assert_eq!(report.actions.filled_holes, 1);
        assert_eq!(report.actions.added_fill_triangles, 1);
        assert_eq!(out.faces.len(), 12);
        assert!(report.after.is_clean());
    }

    #[test]
    fn quad_hole_is_capped_with_a_centroid_fan() {
        let mut tris = cube(10.0);
        // Drop both triangles of the top face → a 4-edge hole.
        tris.remove(3);
        tris.remove(2);
        let mesh = mesh_from(&tris);

        let before = analyze(&mesh);
        assert_eq!(before.holes, 1);
        assert_eq!(before.largest_hole_edges, 4);

        let (out, report) = repair(&mesh, &RepairOptions::default());
        assert_eq!(report.actions.filled_holes, 1);
        assert_eq!(report.actions.added_fill_triangles, 4);
        assert!(report.after.is_clean());
        assert_eq!(out.faces.len(), 14);
    }

    #[test]
    fn oversized_hole_is_reported_but_left_open() {
        let mut tris = cube(10.0);
        tris.pop();
        let mesh = mesh_from(&tris);
        let options = RepairOptions {
            max_hole_edges: 2,
            ..RepairOptions::default()
        };
        let (_, report) = repair(&mesh, &options);
        assert_eq!(report.actions.filled_holes, 0);
        assert_eq!(report.actions.unfilled_holes, 1);
        assert!(!report.after.is_watertight());
        assert!(report.has_remaining_defects());
    }

    #[test]
    fn flipped_face_is_rewound() {
        let mut tris = cube(10.0);
        tris[0].swap(1, 2);
        let mesh = mesh_from(&tris);

        let before = analyze(&mesh);
        assert_eq!(before.inconsistent_winding_edges, 3);
        assert!(!before.is_manifold());

        let (out, report) = repair(&mesh, &RepairOptions::default());
        assert_eq!(report.actions.flipped_faces, 1);
        assert!(report.after.is_clean());
        assert_eq!(out.faces.len(), 12);
    }

    #[test]
    fn fully_inverted_mesh_is_turned_outward() {
        let tris: Vec<[Vertex; 3]> = cube(10.0)
            .into_iter()
            .map(|mut t| {
                t.swap(1, 2);
                t
            })
            .collect();
        let mesh = mesh_from(&tris);

        let before = analyze(&mesh);
        assert_eq!(before.inconsistent_winding_edges, 0);
        assert_eq!(before.inverted_shells, 1);
        assert!(!before.is_clean());

        let (_, report) = repair(&mesh, &RepairOptions::default());
        assert_eq!(report.actions.flipped_faces, 12);
        assert_eq!(report.after.inverted_shells, 0);
        assert!(report.after.is_clean());
    }

    #[test]
    fn degenerate_triangles_are_dropped() {
        let mut tris = cube(10.0);
        tris.push([v(1.0, 1.0, 0.0), v(2.0, 2.0, 0.0), v(3.0, 3.0, 0.0)]); // collinear
        tris.push([v(1.0, 1.0, 0.0), v(1.0, 1.0, 0.0), v(2.0, 2.0, 0.0)]); // repeated corner
        let mesh = mesh_from(&tris);

        let before = analyze(&mesh);
        assert_eq!(before.degenerate_faces, 2);

        let (out, report) = repair(&mesh, &RepairOptions::default());
        assert_eq!(report.actions.removed_degenerate_faces, 2);
        assert_eq!(out.faces.len(), 12);
        assert!(report.after.is_clean());
    }

    #[test]
    fn duplicate_triangle_is_dropped() {
        let mut tris = cube(10.0);
        tris.push(tris[0]);
        let mesh = mesh_from(&tris);

        let before = analyze(&mesh);
        assert_eq!(before.duplicate_faces, 1);

        let (out, report) = repair(&mesh, &RepairOptions::default());
        assert_eq!(report.actions.removed_duplicate_faces, 1);
        assert_eq!(out.faces.len(), 12);
        assert!(report.after.is_clean());
    }

    #[test]
    fn zero_volume_flap_removes_both_triangles() {
        let mut tris = cube(10.0);
        let mut flap = tris[0];
        flap.swap(1, 2);
        tris.push(tris[0]);
        tris.push(flap);
        let mesh = mesh_from(&tris);

        let (out, report) = repair(&mesh, &RepairOptions::default());
        // The original triangle plus one repeat outnumber the single reversed
        // copy, so exactly one survives and the cube stays closed.
        assert_eq!(report.actions.removed_duplicate_faces, 2);
        assert_eq!(out.faces.len(), 12);
        assert!(report.after.is_clean());
    }

    #[test]
    fn cracked_vertices_are_welded() {
        // Nudge one corner of one triangle by less than the weld tolerance so
        // the surface splits along two edges.
        let mut tris = cube(10.0);
        tris[0][0] = v(1e-6, 1e-6, 0.0);
        let mesh = mesh_from(&tris);

        let before = analyze(&mesh);
        assert_eq!(before.vertices, 9);
        assert!(before.boundary_edges > 0);

        let (out, report) = repair(&mesh, &RepairOptions::default());
        assert_eq!(report.actions.welded_vertices, 1);
        assert!(report.after.is_clean());
        assert_eq!(out.faces.len(), 12);
        assert_eq!(out.vertices.len(), 8);
    }

    #[test]
    fn welding_never_merges_beyond_the_tolerance() {
        let mut tris = cube(10.0);
        tris[0][0] = v(0.5, 0.5, 0.0);
        let mesh = mesh_from(&tris);
        let (_, report) = repair(&mesh, &RepairOptions::default());
        assert_eq!(report.actions.welded_vertices, 0);
    }

    #[test]
    fn repair_is_deterministic() {
        let mut tris = cube(10.0);
        tris[0].swap(1, 2);
        tris.pop();
        tris.push([v(1.0, 1.0, 0.0), v(2.0, 2.0, 0.0), v(3.0, 3.0, 0.0)]);
        let mesh = mesh_from(&tris);

        let (a, ra) = repair(&mesh, &RepairOptions::default());
        let (b, rb) = repair(&mesh, &RepairOptions::default());
        assert_eq!(ra, rb);
        assert_eq!(a.faces, b.faces);
        assert_eq!(a.vertices, b.vertices);
    }

    #[test]
    fn repaired_mesh_carries_recomputed_outward_normals() {
        let mut tris = cube(10.0);
        tris.pop();
        let mesh = mesh_from(&tris);
        let (out, _) = repair(&mesh, &RepairOptions::default());
        for face in &out.faces {
            let n = face.normal.expect("normal recomputed");
            let len = (n.x * n.x + n.y * n.y + n.z * n.z).sqrt();
            assert!((len - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn an_uncappable_hole_never_inverts_the_surface() {
        // Regression: `signed_volume` is a cone volume about the **origin**, so
        // on an open shell its sign says nothing about which way is out. A
        // correctly wound cube sitting away from the origin with one face
        // missing sums to a *negative* pseudo-volume — orienting by it would
        // turn the whole surface inside out, and because `fill_holes` cannot
        // cap the hole there is no second pass to undo it.
        let mut tris = cube(1.0);
        for t in tris.iter_mut() {
            for p in t.iter_mut() {
                p.x += 100.0;
                p.y += 100.0;
                p.z += 100.0;
            }
        }
        tris.remove(3);
        tris.remove(2); // drop the top face → a 4-edge hole

        let mesh = mesh_from(&tris);
        let options = RepairOptions {
            max_hole_edges: 2, // too small to cap this hole
            ..RepairOptions::default()
        };
        let (out, report) = repair(&mesh, &options);

        assert_eq!(report.actions.unfilled_holes, 1);
        assert_eq!(
            report.actions.flipped_faces, 0,
            "an open shell must keep the winding it came with"
        );
        for (before, after) in tris.iter().zip(out.faces.iter()) {
            assert_eq!(&after.vertices, before, "winding was silently reversed");
        }
    }

    #[test]
    fn an_open_shell_is_oriented_once_its_hole_is_capped() {
        // The mirror of the case above: as soon as the surface is sealed the
        // volume test becomes meaningful again, so an inside-out open mesh
        // still comes out facing the right way.
        let mut tris: Vec<[Vertex; 3]> = cube(1.0)
            .into_iter()
            .map(|mut t| {
                for p in t.iter_mut() {
                    p.x += 100.0;
                    p.y += 100.0;
                    p.z += 100.0;
                }
                t.swap(1, 2);
                t
            })
            .collect();
        tris.remove(3);
        tris.remove(2);

        let mesh = mesh_from(&tris);
        let (out, report) = repair(&mesh, &RepairOptions::default());
        assert_eq!(report.actions.filled_holes, 1);
        assert!(report.after.is_clean());

        let volume: f64 = out
            .faces
            .iter()
            .map(|f| {
                let [a, b, c] = f.vertices;
                (a.x * (b.y * c.z - b.z * c.y)
                    + a.y * (b.z * c.x - b.x * c.z)
                    + a.z * (b.x * c.y - b.y * c.x))
                    / 6.0
            })
            .sum();
        assert!(
            volume > 0.0,
            "sealed shell should face outward, got {volume}"
        );
    }

    #[test]
    fn a_zero_area_slit_is_not_a_hole_and_is_never_patched() {
        // Regression: a T-junction leaves three collinear half-edges with one
        // incident face each. That reads as a boundary loop, but it encloses
        // *no area* — patching it could only ever add zero-area triangles,
        // which close nothing (they are excluded from the edge graph as
        // degenerate) while polluting the mesh. Measured on a real 225 706-tri
        // Benchy export: 9 such loops, 15 junk triangles added, and the mesh
        // then reported as still defective.
        let s = 10.0;
        let p = [
            v(0.0, 0.0, 0.0),
            v(s, 0.0, 0.0),
            v(s, s, 0.0),
            v(0.0, s, 0.0),
        ];
        let mut tris = cube(s);
        // Split the bottom face at the midpoint of the p0–p1 edge while the
        // front face keeps spanning the whole edge.
        let mid = v(s / 2.0, 0.0, 0.0);
        tris.retain(|t| *t != [p[0], p[2], p[1]]);
        tris.push([p[2], p[1], mid]);
        tris.push([p[2], mid, p[0]]);

        let mesh = mesh_from(&tris);
        let d = analyze(&mesh);
        assert_eq!(d.slit_boundary_edges, 3, "the T-junction rim");
        assert_eq!(d.holes, 0, "a zero-area loop is not a hole");
        assert_eq!(d.boundary_edges, 0);
        assert!(d.is_watertight(), "a slit encloses nothing");
        assert!(d.is_clean());

        let (out, report) = repair(&mesh, &RepairOptions::default());
        assert!(
            matches!(out, Cow::Borrowed(_)),
            "nothing to fix — must not rebuild"
        );
        assert_eq!(report.actions.added_fill_triangles, 0);
        assert_eq!(report.actions.unfilled_holes, 0);
        assert!(!report.is_noteworthy(), "must not warn about a slit");
    }

    #[test]
    fn a_slit_alongside_a_real_hole_is_told_apart() {
        // The slit must not mask a genuine void, nor vice versa.
        let s = 10.0;
        let p = [
            v(0.0, 0.0, 0.0),
            v(s, 0.0, 0.0),
            v(s, s, 0.0),
            v(0.0, s, 0.0),
        ];
        let mut tris = cube(s);
        // Punch out a real hole first (both triangles of the top face), while
        // the indices still match the pristine cube ordering.
        tris.remove(3);
        tris.remove(2);
        // Then introduce the T-junction on the bottom face.
        let mid = v(s / 2.0, 0.0, 0.0);
        tris.retain(|t| *t != [p[0], p[2], p[1]]);
        tris.push([p[2], p[1], mid]);
        tris.push([p[2], mid, p[0]]);

        let mesh = mesh_from(&tris);
        let d = analyze(&mesh);
        assert_eq!(d.slit_boundary_edges, 3);
        assert_eq!(d.holes, 1, "the top face is a genuine void");
        assert_eq!(d.boundary_edges, 4);
        assert!(!d.is_watertight());

        let (_, report) = repair(&mesh, &RepairOptions::default());
        assert_eq!(report.actions.filled_holes, 1, "real hole capped");
        assert_eq!(report.actions.added_fill_triangles, 4);
        assert_eq!(
            report.after.slit_boundary_edges, 3,
            "the slit is left exactly as it was"
        );
        assert!(report.after.is_clean());
    }

    #[test]
    fn empty_mesh_is_handled() {
        let mesh = Mesh::new();
        let d = analyze(&mesh);
        assert_eq!(d.triangles, 0);
        assert!(d.is_clean());
        let (out, report) = repair(&mesh, &RepairOptions::default());
        assert!(matches!(out, Cow::Borrowed(_)));
        assert!(!report.repaired);
    }

    #[test]
    fn non_manifold_edge_is_reported_not_repaired() {
        // Two cube faces sharing one edge, plus a third fin on that same edge.
        let tris = vec![
            [v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0), v(10.0, 10.0, 0.0)],
            [v(0.0, 0.0, 0.0), v(10.0, 10.0, 0.0), v(0.0, 10.0, 0.0)],
            [v(0.0, 0.0, 0.0), v(10.0, 10.0, 0.0), v(0.0, 0.0, 10.0)],
        ];
        let mesh = mesh_from(&tris);
        let d = analyze(&mesh);
        assert_eq!(d.non_manifold_edges, 1);
        assert!(!d.is_manifold());

        let (_, report) = repair(&mesh, &RepairOptions::default());
        assert_eq!(report.after.non_manifold_edges, 1);
        assert!(report.has_remaining_defects());
        assert!(report.is_noteworthy());
    }

    #[test]
    fn two_shells_are_counted_separately() {
        let mut tris = cube(10.0);
        tris.extend(cube(4.0).into_iter().map(|mut t| {
            for p in t.iter_mut() {
                p.x += 100.0;
            }
            t
        }));
        let mesh = mesh_from(&tris);
        let d = analyze(&mesh);
        assert_eq!(d.shells, 2);
        assert!(d.is_clean());
    }

    #[test]
    fn defect_summary_reads_naturally() {
        let d = MeshDiagnostics {
            triangles: 10,
            holes: 1,
            boundary_edges: 3,
            degenerate_faces: 2,
            ..Default::default()
        };
        let s = d.defect_summary().unwrap();
        assert!(s.contains("2 degenerate triangles"));
        assert!(s.contains("1 hole (3 boundary edges)"));
    }
}
