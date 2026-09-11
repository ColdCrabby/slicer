//! Spatial analysis functions: AABB, volume, surface area, coplanar face groups.

use crate::mesh::types::{Mesh, Vertex, AABB};

// ---------------------------------------------------------------------------
// Coplanar face groups
// ---------------------------------------------------------------------------

/// Compute the geometric (unnormalised) normal of a triangle from three vertices.
#[inline]
fn geometric_normal(a: &Vertex, b: &Vertex, c: &Vertex) -> [f64; 3] {
    let ux = b.x - a.x;
    let uy = b.y - a.y;
    let uz = b.z - a.z;
    let vx = c.x - a.x;
    let vy = c.y - a.y;
    let vz = c.z - a.z;
    [uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx]
}

#[inline]
fn normalise(n: [f64; 3]) -> [f64; 3] {
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len < 1e-12 {
        [0.0, 0.0, 0.0]
    } else {
        [n[0] / len, n[1] / len, n[2] / len]
    }
}

// Union–find with path compression and union by rank.
fn uf_find(parent: &mut [u32], mut i: u32) -> u32 {
    while parent[i as usize] != i {
        parent[i as usize] = parent[parent[i as usize] as usize]; // path halving
        i = parent[i as usize];
    }
    i
}

fn uf_union(parent: &mut [u32], rank: &mut [u8], a: u32, b: u32) {
    let ra = uf_find(parent, a);
    let rb = uf_find(parent, b);
    if ra == rb {
        return;
    }
    match rank[ra as usize].cmp(&rank[rb as usize]) {
        std::cmp::Ordering::Less => parent[ra as usize] = rb,
        std::cmp::Ordering::Greater => parent[rb as usize] = ra,
        std::cmp::Ordering::Equal => {
            parent[rb as usize] = ra;
            rank[ra as usize] += 1;
        }
    }
}

/// Assign each raw corner (`face_index * 3 + corner`) a canonical vertex id, so
/// positions within `merge_distance_mm` of each other collapse to one id.
///
/// A mesh's corners are stored per-face and floating-point near-duplicates are
/// common, so topology can only be read after this quantisation. Two corners
/// share an id exactly when they are the same point of the surface, which is
/// what makes an edge key comparable between faces.
///
/// The representative chosen for a group is its **lowest raw index**, so the
/// mapping is deterministic — [`crate::mesh::paint`] relies on that, because
/// the browser and the engine each derive topology independently and must
/// agree.
fn canonical_vertex_ids(mesh: &Mesh, merge_distance_mm: f64) -> Vec<u32> {
    let n = mesh.faces.len();
    let quant = 1.0 / merge_distance_mm.max(1e-9);

    let mut qverts: Vec<([i64; 3], u32)> = mesh
        .faces
        .iter()
        .flat_map(|f| f.vertices.iter())
        .enumerate()
        .map(|(raw_idx, v)| {
            let q = [
                (v.x * quant).round() as i64,
                (v.y * quant).round() as i64,
                (v.z * quant).round() as i64,
            ];
            (q, raw_idx as u32)
        })
        .collect();
    qverts.sort_unstable_by_key(|(q, _)| *q);

    let mut canonical_id: Vec<u32> = vec![0; n * 3];
    let mut group_start = 0usize;
    while group_start < qverts.len() {
        let key = qverts[group_start].0;
        let mut group_end = group_start + 1;
        while group_end < qverts.len() && qverts[group_end].0 == key {
            group_end += 1;
        }
        let rep = qverts[group_start].1;
        for &(_, raw) in &qverts[group_start..group_end] {
            canonical_id[raw as usize] = rep;
        }
        group_start = group_end;
    }
    canonical_id
}

/// Every `(edge key, face)` pair in the mesh, sorted by key, so the faces
/// sharing an edge are contiguous.
fn sorted_half_edges(canonical_id: &[u32], face_count: usize) -> Vec<([u32; 2], u32)> {
    let mut half_edges: Vec<([u32; 2], u32)> = Vec::with_capacity(face_count * 3);
    for face_idx in 0..face_count {
        for edge in 0..3usize {
            let va = canonical_id[face_idx * 3 + edge];
            let vb = canonical_id[face_idx * 3 + (edge + 1) % 3];
            let key = if va < vb { [va, vb] } else { [vb, va] };
            half_edges.push((key, face_idx as u32));
        }
    }
    half_edges.sort_unstable_by_key(|(k, _)| *k);
    half_edges
}

/// Which faces touch which, across shared edges.
///
/// Stored compressed (an offset table plus one flat neighbour list) because a
/// brush stroke walks it once per pointer sample and a per-face `Vec` would
/// make that allocation-bound.
#[derive(Debug, Clone, Default)]
pub struct FaceAdjacency {
    offsets: Vec<u32>,
    neighbours: Vec<u32>,
}

impl FaceAdjacency {
    /// Faces sharing an edge with `face`.
    pub fn neighbours_of(&self, face: usize) -> &[u32] {
        if face + 1 >= self.offsets.len() {
            return &[];
        }
        let start = self.offsets[face] as usize;
        let end = self.offsets[face + 1] as usize;
        &self.neighbours[start..end]
    }

    /// Number of faces covered.
    pub fn face_count(&self) -> usize {
        self.offsets.len().saturating_sub(1)
    }
}

/// Build the edge-adjacency graph of a mesh.
///
/// Two faces are neighbours when they share an edge whose endpoints agree
/// within `vertex_merge_distance_mm`. Used by the support-paint brush to grow a
/// stroke across the surface rather than through it: without it, painting the
/// front of a thin wall also paints the back, which the user cannot see and did
/// not ask for.
pub fn face_adjacency(mesh: &Mesh, vertex_merge_distance_mm: f64) -> FaceAdjacency {
    let n = mesh.faces.len();
    if n == 0 {
        return FaceAdjacency::default();
    }
    let canonical_id = canonical_vertex_ids(mesh, vertex_merge_distance_mm);
    let half_edges = sorted_half_edges(&canonical_id, n);

    // Two passes: count neighbours per face to size the table, then fill it.
    let mut counts: Vec<u32> = vec![0; n];
    let mut pairs: Vec<(u32, u32)> = Vec::new();
    let mut i = 0usize;
    while i < half_edges.len() {
        let key = half_edges[i].0;
        let mut j = i;
        while j < half_edges.len() && half_edges[j].0 == key {
            j += 1;
        }
        for a in i..j {
            for b in (a + 1)..j {
                let (fa, fb) = (half_edges[a].1, half_edges[b].1);
                if fa == fb {
                    continue;
                }
                counts[fa as usize] += 1;
                counts[fb as usize] += 1;
                pairs.push((fa, fb));
            }
        }
        i = j;
    }

    let mut offsets: Vec<u32> = Vec::with_capacity(n + 1);
    offsets.push(0);
    let mut running = 0u32;
    for &c in &counts {
        running += c;
        offsets.push(running);
    }

    let mut cursor = offsets.clone();
    let mut neighbours: Vec<u32> = vec![0; running as usize];
    for (fa, fb) in pairs {
        neighbours[cursor[fa as usize] as usize] = fb;
        cursor[fa as usize] += 1;
        neighbours[cursor[fb as usize] as usize] = fa;
        cursor[fb as usize] += 1;
    }

    FaceAdjacency {
        offsets,
        neighbours,
    }
}

/// Assign each face to a coplanar group and return a `Vec<u32>` of length
/// `mesh.faces.len()` where `result[face_index]` is the canonical group id
/// (the root of its union–find tree, renumbered 0…N-1).
///
/// Two triangles are placed in the same group when:
/// 1. They share an edge (two vertices within `vertex_merge_distance_mm`), **and**
/// 2. Their geometric normals agree within `angle_threshold_deg`.
///
/// The returned group ids are contiguous starting from 0 and are ordered by
/// the first face that belongs to each group (i.e. `result[0]` is always 0).
pub fn compute_coplanar_groups(
    mesh: &Mesh,
    angle_threshold_deg: f32,
    vertex_merge_distance_mm: f64,
) -> Vec<u32> {
    let n = mesh.faces.len();
    if n == 0 {
        return Vec::new();
    }

    // --- 1. Compute unit normals for every face. ---------------------------
    let normals: Vec<[f64; 3]> = mesh
        .faces
        .iter()
        .map(|f| {
            normalise(geometric_normal(
                &f.vertices[0],
                &f.vertices[1],
                &f.vertices[2],
            ))
        })
        .collect();

    let cos_threshold = (angle_threshold_deg as f64).to_radians().cos();

    // --- 2. Build an edge → face adjacency map. ----------------------------
    // Quantise vertex positions so floating-point near-duplicates collapse to
    // one id, then sort every (edge, face) pair so the faces sharing an edge
    // are contiguous.
    let canonical_id = canonical_vertex_ids(mesh, vertex_merge_distance_mm);
    let half_edges = sorted_half_edges(&canonical_id, n);

    // --- 3. Union-find: merge adjacent coplanar faces. ---------------------
    let mut parent: Vec<u32> = (0..n as u32).collect();
    let mut rank: Vec<u8> = vec![0; n];

    let mut i = 0usize;
    while i < half_edges.len() {
        let key = half_edges[i].0;
        // Collect all faces sharing this edge key.
        let mut j = i;
        while j < half_edges.len() && half_edges[j].0 == key {
            j += 1;
        }
        // Pairwise-check every face pair sharing this edge.
        for a in i..j {
            for b in (a + 1)..j {
                let fa = half_edges[a].1 as usize;
                let fb = half_edges[b].1 as usize;
                let na = normals[fa];
                let nb = normals[fb];
                let dot = na[0] * nb[0] + na[1] * nb[1] + na[2] * nb[2];
                // Dot product of unit normals ≥ cos(threshold) → same plane.
                if dot >= cos_threshold {
                    uf_union(&mut parent, &mut rank, fa as u32, fb as u32);
                }
            }
        }
        i = j;
    }

    // --- 4. Compact group ids so they are contiguous 0…G-1. ---------------
    let mut root_to_compact: std::collections::HashMap<u32, u32> =
        std::collections::HashMap::with_capacity(n / 2);
    let mut next_id: u32 = 0;
    let mut result: Vec<u32> = Vec::with_capacity(n);
    for face_idx in 0..n as u32 {
        let root = uf_find(&mut parent, face_idx);
        let compact = *root_to_compact.entry(root).or_insert_with(|| {
            let id = next_id;
            next_id += 1;
            id
        });
        result.push(compact);
    }

    result
}

/// Compute the Axis-Aligned Bounding Box for the given mesh.
///
/// Returns an `AABB` covering all vertices. Panics if the mesh has no vertices.
pub fn calculate_aabb(mesh: &Mesh) -> AABB {
    AABB::new_from_vertices(&mesh.vertices)
        .expect("Mesh must have at least one vertex to calculate AABB")
}

/// Compute the signed volume of a closed mesh using the divergence theorem (mm³).
///
/// Each triangle contributes `(v0 · (v1 × v2)) / 6` to the total, which equals
/// the volume of the signed tetrahedron between the triangle and the origin.
/// Summing over all faces and taking the absolute value gives the mesh volume.
///
/// Returns an error string if the mesh appears to be open (the raw sum is
/// suspiciously small relative to the surface area, which would not be the case
/// for a geometrically valid closed shell). Note: the current implementation
/// simply checks if the mesh has any faces; full open-mesh detection is deferred.
pub fn calculate_volume(mesh: &Mesh) -> Result<f64, String> {
    if mesh.faces.is_empty() {
        return Err("Mesh has no faces; cannot calculate volume".to_string());
    }

    let mut signed_volume = 0.0_f64;
    for face in &mesh.faces {
        let v0 = face.vertices[0];
        let v1 = face.vertices[1];
        let v2 = face.vertices[2];

        // Scalar triple product: v0 · (v1 × v2)
        let cross_x = v1.y * v2.z - v1.z * v2.y;
        let cross_y = v1.z * v2.x - v1.x * v2.z;
        let cross_z = v1.x * v2.y - v1.y * v2.x;
        signed_volume += v0.x * cross_x + v0.y * cross_y + v0.z * cross_z;
    }

    Ok((signed_volume / 6.0).abs())
}

/// Compute the total surface area of the mesh (mm²) by summing the area of
/// every triangle face.
pub fn calculate_surface_area(mesh: &Mesh) -> f64 {
    mesh.faces.iter().map(|f| f.area()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::types::{Face, Mesh, Vertex};

    /// Build a 10×10×10 axis-aligned unit cube mesh (12 triangles).
    fn make_cube_mesh() -> Mesh {
        let v = [
            Vertex::new(0.0, 0.0, 0.0),    // 0
            Vertex::new(10.0, 0.0, 0.0),   // 1
            Vertex::new(10.0, 10.0, 0.0),  // 2
            Vertex::new(0.0, 10.0, 0.0),   // 3
            Vertex::new(0.0, 0.0, 10.0),   // 4
            Vertex::new(10.0, 0.0, 10.0),  // 5
            Vertex::new(10.0, 10.0, 10.0), // 6
            Vertex::new(0.0, 10.0, 10.0),  // 7
        ];

        // 12 faces (2 triangles per side × 6 sides), wound outward
        let face_indices: [[usize; 3]; 12] = [
            [0, 2, 1],
            [0, 3, 2], // bottom  (-Z)
            [4, 5, 6],
            [4, 6, 7], // top     (+Z)
            [0, 1, 5],
            [0, 5, 4], // front   (-Y)
            [2, 3, 7],
            [2, 7, 6], // back    (+Y)
            [0, 4, 7],
            [0, 7, 3], // left    (-X)
            [1, 2, 6],
            [1, 6, 5], // right   (+X)
        ];

        let faces: Vec<Face> = face_indices
            .iter()
            .map(|idx| Face::new([v[idx[0]], v[idx[1]], v[idx[2]]]))
            .collect();

        Mesh {
            vertices: v.to_vec(),
            faces,
            aabb: None,
        }
    }

    #[test]
    fn test_aabb_on_cube() {
        let mesh = make_cube_mesh();
        let aabb = calculate_aabb(&mesh);
        assert_eq!(aabb.min.x, 0.0);
        assert_eq!(aabb.min.y, 0.0);
        assert_eq!(aabb.min.z, 0.0);
        assert_eq!(aabb.max.x, 10.0);
        assert_eq!(aabb.max.y, 10.0);
        assert_eq!(aabb.max.z, 10.0);
    }

    #[test]
    fn test_volume_on_cube() {
        let mesh = make_cube_mesh();
        let vol = calculate_volume(&mesh).unwrap();
        // 10^3 = 1000 mm³, allow 1% tolerance
        assert!((vol - 1000.0).abs() < 10.0, "Volume was {vol}");
    }

    #[test]
    fn test_surface_area_on_cube() {
        let mesh = make_cube_mesh();
        let area = calculate_surface_area(&mesh);
        // 6 sides × 10×10 = 600 mm², allow 1% tolerance
        assert!((area - 600.0).abs() < 6.0, "Surface area was {area}");
    }

    #[test]
    fn test_volume_empty_mesh_returns_error() {
        let mesh = Mesh::new();
        assert!(calculate_volume(&mesh).is_err());
    }

    // -----------------------------------------------------------------------
    // compute_coplanar_groups
    // -----------------------------------------------------------------------

    /// A cube has 6 flat faces, each made of 2 coplanar triangles → 6 groups.
    #[test]
    fn test_coplanar_groups_cube_has_six_groups() {
        let mesh = make_cube_mesh();
        let groups = compute_coplanar_groups(&mesh, 1.0, 0.001);
        assert_eq!(groups.len(), 12);
        let mut unique: Vec<u32> = groups.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            6,
            "expected 6 coplanar groups, got {unique:?}"
        );
    }

    /// The two triangles that form each face of the cube must be in the same group.
    #[test]
    fn test_coplanar_groups_cube_face_pairs_are_merged() {
        let mesh = make_cube_mesh();
        let groups = compute_coplanar_groups(&mesh, 1.0, 0.001);
        // Face pairs per side (see make_cube_mesh winding order): 0+1, 2+3, etc.
        for pair_start in (0..12).step_by(2) {
            assert_eq!(
                groups[pair_start],
                groups[pair_start + 1],
                "face {} and {} should be in the same group",
                pair_start,
                pair_start + 1,
            );
        }
    }

    /// Adjacent faces on different sides of the cube must NOT merge.
    #[test]
    fn test_coplanar_groups_cube_cross_side_faces_are_separate() {
        let mesh = make_cube_mesh();
        let groups = compute_coplanar_groups(&mesh, 1.0, 0.001);
        // Bottom (0,1) vs top (2,3): different planes.
        assert_ne!(groups[0], groups[2]);
    }

    /// Empty mesh returns an empty vec without panicking.
    #[test]
    fn test_coplanar_groups_empty_mesh() {
        let mesh = Mesh::new();
        let groups = compute_coplanar_groups(&mesh, 1.0, 0.001);
        assert!(groups.is_empty());
    }
}
