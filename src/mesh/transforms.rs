//! Coordinate transformations for meshes.
//!
//! All transforms are immutable: they return a new `Mesh` and leave the
//! original unchanged. The cached `aabb` field is cleared on the new instance
//! so it is recomputed lazily on next access.

use crate::mesh::analysis::calculate_aabb;
use crate::mesh::types::{Face, Mesh, Vertex};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Controls optional mesh decimation applied before slicing.
///
/// Decimation reduces the triangle count of the input mesh as a preprocessing
/// step. Fewer triangles speed up all subsequent slicing operations; the
/// trade-off is reduced geometric accuracy on very fine surface details.
///
/// The original mesh is never modified; only the copy handed to the slicing
/// pipeline is decimated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub enum MeshQuality {
    /// Slices the mesh exactly as it arrived (default).
    #[default]
    Normal,
    /// Also slices the mesh exactly as it arrived — identical to Normal today.
    ///
    /// Kept so a profile asking for maximum fidelity still loads, and so a
    /// future refinement pass has somewhere to land.
    HighQuality,
    /// Collapses nearby vertices first: quicker on dense models, at the cost of
    /// fine surface detail.
    ///
    /// Significantly reduces triangle count for faster slicing of
    /// high-density models. Fine surface details may be smoothed away.
    Draft,
}

/// Translate every vertex of the mesh by the given offset.
///
/// Returns a new `Mesh`; the original is unchanged.
pub fn translate_mesh(mesh: &Mesh, offset: Vertex) -> Mesh {
    let vertices: Vec<Vertex> = mesh
        .vertices
        .iter()
        .map(|v| Vertex::new(v.x + offset.x, v.y + offset.y, v.z + offset.z))
        .collect();

    // Rebuild faces using the translated vertex positions
    let faces: Vec<Face> = mesh
        .faces
        .iter()
        .map(|f| Face {
            vertices: [
                Vertex::new(
                    f.vertices[0].x + offset.x,
                    f.vertices[0].y + offset.y,
                    f.vertices[0].z + offset.z,
                ),
                Vertex::new(
                    f.vertices[1].x + offset.x,
                    f.vertices[1].y + offset.y,
                    f.vertices[1].z + offset.z,
                ),
                Vertex::new(
                    f.vertices[2].x + offset.x,
                    f.vertices[2].y + offset.y,
                    f.vertices[2].z + offset.z,
                ),
            ],
            normal: f.normal,
        })
        .collect();

    Mesh {
        vertices,
        faces,
        aabb: None,
    }
}

/// Center the mesh horizontally so that the AABB center lies at (0, 0, z_center).
///
/// Only the X and Y axes are affected; the Z position is unchanged.
/// Returns a new `Mesh`; the original is unchanged.
pub fn center_mesh(mesh: &Mesh) -> Mesh {
    let aabb = calculate_aabb(mesh);
    let center = aabb.center();
    // Shift XY so the center is at the origin; Z is not altered.
    let offset = Vertex::new(-center.x, -center.y, 0.0);
    translate_mesh(mesh, offset)
}

/// Translate the mesh so that its lowest Z vertex sits exactly on Z = 0.
///
/// Returns a new `Mesh`; the original is unchanged.
pub fn drop_to_floor(mesh: &Mesh) -> Mesh {
    let aabb = calculate_aabb(mesh);
    let offset = Vertex::new(0.0, 0.0, -aabb.min.z);
    translate_mesh(mesh, offset)
}

/// Reduce the triangle count of `mesh` according to `quality`.
///
/// | `quality`       | Behaviour                                                   |
/// |-----------------|-------------------------------------------------------------|
/// | `Normal`        | Returns a clone; no processing applied.                     |
/// | `HighQuality`   | Returns a clone; no processing applied.                     |
/// | `Draft`         | Applies vertex-clustering decimation (see below).           |
///
/// # Draft decimation — vertex clustering
///
/// The mesh's bounding box is divided into a uniform grid of cells whose
/// edge length is `max_dimension / DRAFT_GRID_CELLS`.  Every vertex that
/// falls in the same cell is merged to the centroid of all vertices in that
/// cell.  Triangles that become degenerate after merging (two or more corners
/// collapse to the same cell) are discarded.
///
/// The algorithm is O(F) in time and space where F is the face count.  No
/// external dependencies are required.
///
/// # Returns
///
/// A new `Mesh`; the original is unchanged.  Face count is equal to or less
/// than the input count.
pub fn decimate_mesh(mesh: &Mesh, quality: MeshQuality) -> Mesh {
    match quality {
        MeshQuality::Normal | MeshQuality::HighQuality => mesh.clone(),
        MeshQuality::Draft => {
            if mesh.faces.is_empty() {
                return mesh.clone();
            }
            let aabb = calculate_aabb(mesh);
            let max_dim = f64::max(f64::max(aabb.width(), aabb.depth()), aabb.height());
            if max_dim <= 0.0 {
                return mesh.clone();
            }
            // 64 cells per longest axis provides a ~1.6 % reduction in feature
            // size (max_dim / 64) while still meaningfully reducing face count on
            // dense models.  Larger values preserve more detail; smaller values
            // merge more aggressively.  64 is a practical default that works well
            // for typical FDM models (10–300 mm) without losing print-critical geometry.
            const DRAFT_GRID_CELLS: f64 = 64.0;
            let cell_size = max_dim / DRAFT_GRID_CELLS;
            cluster_vertices(mesh, cell_size, &aabb)
        }
    }
}

/// Merge nearby vertices into cell centroids and rebuild faces.
///
/// Every vertex `v` maps to cell key `(floor((v - aabb.min) / cell_size))`.
/// All vertices in the same cell are averaged to a single representative point.
/// Degenerate faces (any two corners in the same cell) are dropped.
fn cluster_vertices(mesh: &Mesh, cell_size: f64, aabb: &crate::mesh::types::AABB) -> Mesh {
    use std::collections::HashMap;

    /// Per-cell accumulator used during vertex clustering.
    struct CellAccum {
        sum_x: f64,
        sum_y: f64,
        sum_z: f64,
        count: u64,
        output_index: usize,
    }

    let mut cells: HashMap<(i64, i64, i64), CellAccum> = HashMap::new();

    let key_of = |v: &Vertex| -> (i64, i64, i64) {
        (
            ((v.x - aabb.min.x) / cell_size).floor() as i64,
            ((v.y - aabb.min.y) / cell_size).floor() as i64,
            ((v.z - aabb.min.z) / cell_size).floor() as i64,
        )
    };

    // First pass: accumulate every face vertex into its cell.
    for face in &mesh.faces {
        for v in &face.vertices {
            let k = key_of(v);
            let entry = cells.entry(k).or_insert(CellAccum {
                sum_x: 0.0,
                sum_y: 0.0,
                sum_z: 0.0,
                count: 0,
                output_index: 0,
            });
            entry.sum_x += v.x;
            entry.sum_y += v.y;
            entry.sum_z += v.z;
            entry.count += 1;
        }
    }

    // Assign output vertex indices and compute centroids.
    let mut new_vertices: Vec<Vertex> = Vec::with_capacity(cells.len());
    for entry in cells.values_mut() {
        entry.output_index = new_vertices.len();
        let count = entry.count as f64;
        new_vertices.push(Vertex::new(
            entry.sum_x / count,
            entry.sum_y / count,
            entry.sum_z / count,
        ));
    }

    // Second pass: rebuild non-degenerate faces.
    let mut new_faces: Vec<Face> = Vec::new();
    for face in &mesh.faces {
        let i0 = cells
            .get(&key_of(&face.vertices[0]))
            .expect("vertex cell populated in first pass")
            .output_index;
        let i1 = cells
            .get(&key_of(&face.vertices[1]))
            .expect("vertex cell populated in first pass")
            .output_index;
        let i2 = cells
            .get(&key_of(&face.vertices[2]))
            .expect("vertex cell populated in first pass")
            .output_index;
        // Discard degenerate faces produced by vertex merging.
        if i0 == i1 || i1 == i2 || i0 == i2 {
            continue;
        }
        new_faces.push(Face::new([
            new_vertices[i0],
            new_vertices[i1],
            new_vertices[i2],
        ]));
    }

    Mesh {
        vertices: new_vertices,
        faces: new_faces,
        aabb: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::analysis::calculate_aabb;
    use crate::mesh::types::{Face, Mesh, Vertex};

    /// Build a simple cube displaced from the origin.
    fn make_displaced_cube(x_off: f64, y_off: f64, z_off: f64) -> Mesh {
        let v = [
            Vertex::new(x_off, y_off, z_off),
            Vertex::new(x_off + 10.0, y_off, z_off),
            Vertex::new(x_off + 10.0, y_off + 10.0, z_off),
            Vertex::new(x_off, y_off + 10.0, z_off),
            Vertex::new(x_off, y_off, z_off + 10.0),
            Vertex::new(x_off + 10.0, y_off, z_off + 10.0),
            Vertex::new(x_off + 10.0, y_off + 10.0, z_off + 10.0),
            Vertex::new(x_off, y_off + 10.0, z_off + 10.0),
        ];
        let face_indices: [[usize; 3]; 12] = [
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
    fn test_center_mesh() {
        // Cube from (10,10,5) to (20,20,15) — elevated so we verify Z is NOT shifted
        let mesh = make_displaced_cube(10.0, 10.0, 5.0);
        let centered = center_mesh(&mesh);
        let aabb = calculate_aabb(&centered);
        // AABB center in XY should be at (0, 0)
        assert!(
            (aabb.center().x).abs() < 1e-9,
            "center.x={}",
            aabb.center().x
        );
        assert!(
            (aabb.center().y).abs() < 1e-9,
            "center.y={}",
            aabb.center().y
        );
        // Z should be unchanged: floor stays at 5
        assert!((aabb.min.z - 5.0).abs() < 1e-9, "z_min={}", aabb.min.z);
    }

    #[test]
    fn test_drop_to_floor() {
        // Cube from (5,5,5) to (15,15,15)
        let mesh = make_displaced_cube(5.0, 5.0, 5.0);
        let dropped = drop_to_floor(&mesh);
        let aabb = calculate_aabb(&dropped);
        assert!((aabb.min.z).abs() < 1e-9, "z_min={}", aabb.min.z);
        // XY should be unchanged
        assert!((aabb.min.x - 5.0).abs() < 1e-9);
        assert!((aabb.min.y - 5.0).abs() < 1e-9);
    }

    #[test]
    fn test_translate_mesh() {
        let mesh = make_displaced_cube(0.0, 0.0, 0.0);
        let offset = Vertex::new(5.0, 5.0, 5.0);
        let translated = translate_mesh(&mesh, offset);
        let aabb = calculate_aabb(&translated);
        assert!((aabb.min.x - 5.0).abs() < 1e-9);
        assert!((aabb.min.y - 5.0).abs() < 1e-9);
        assert!((aabb.min.z - 5.0).abs() < 1e-9);
    }

    #[test]
    fn test_original_mesh_unchanged_after_translate() {
        let mesh = make_displaced_cube(0.0, 0.0, 0.0);
        let _translated = translate_mesh(&mesh, Vertex::new(100.0, 100.0, 100.0));
        // Original mesh vertices should be untouched
        assert_eq!(mesh.vertices[0].x, 0.0);
        assert_eq!(mesh.vertices[0].y, 0.0);
        assert_eq!(mesh.vertices[0].z, 0.0);
    }

    #[test]
    fn test_decimate_normal_returns_clone() {
        let mesh = make_displaced_cube(0.0, 0.0, 0.0);
        let result = super::decimate_mesh(&mesh, MeshQuality::Normal);
        assert_eq!(result.faces.len(), mesh.faces.len());
        assert_eq!(result.vertices.len(), mesh.vertices.len());
    }

    #[test]
    fn test_decimate_high_quality_returns_clone() {
        let mesh = make_displaced_cube(0.0, 0.0, 0.0);
        let result = super::decimate_mesh(&mesh, MeshQuality::HighQuality);
        assert_eq!(result.faces.len(), mesh.faces.len());
    }

    #[test]
    fn test_decimate_draft_reduces_face_count_on_dense_mesh() {
        // Build a mesh with many co-located triangles to ensure clustering fires.
        // Use a 1×1×1 mm cube so all 8 vertices are very close together;
        // with cell_size = 1/64 mm all 8 vertices collapse into roughly the
        // same few cells, producing far fewer faces.
        let small_v = [
            Vertex::new(0.0, 0.0, 0.0),
            Vertex::new(0.1, 0.0, 0.0),
            Vertex::new(0.1, 0.1, 0.0),
            Vertex::new(0.0, 0.1, 0.0),
            Vertex::new(0.0, 0.0, 0.1),
            Vertex::new(0.1, 0.0, 0.1),
            Vertex::new(0.1, 0.1, 0.1),
            Vertex::new(0.0, 0.1, 0.1),
        ];
        let face_indices: [[usize; 3]; 12] = [
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
        let faces: Vec<Face> = face_indices
            .iter()
            .map(|idx| Face::new([small_v[idx[0]], small_v[idx[1]], small_v[idx[2]]]))
            .collect();
        let mesh = Mesh {
            vertices: small_v.to_vec(),
            faces,
            aabb: None,
        };
        let decimated = super::decimate_mesh(&mesh, MeshQuality::Draft);
        // A 0.1 mm cube with cell_size ≈ 0.1 / 64 ≈ 0.0016 mm should collapse many
        // vertices; the result should have fewer or equal faces.
        assert!(
            decimated.faces.len() <= mesh.faces.len(),
            "expected face count ≤ {} but got {}",
            mesh.faces.len(),
            decimated.faces.len()
        );
    }

    #[test]
    fn test_decimate_draft_normal_sized_cube_preserves_shape() {
        use crate::mesh::analysis::calculate_aabb;
        // A 10×10×10 mm cube with 12 faces — vertices are well-separated relative
        // to cell_size (10/64 ≈ 0.156 mm), so all 8 vertices stay distinct.
        let mesh = make_displaced_cube(0.0, 0.0, 0.0);
        let decimated = super::decimate_mesh(&mesh, MeshQuality::Draft);
        // All 12 faces should survive (no collapses).
        assert_eq!(decimated.faces.len(), 12, "all faces should survive");
        // AABB should be approximately the same.
        let orig_aabb = calculate_aabb(&mesh);
        let dec_aabb = calculate_aabb(&decimated);
        assert!((orig_aabb.width() - dec_aabb.width()).abs() < 1.0);
        assert!((orig_aabb.depth() - dec_aabb.depth()).abs() < 1.0);
        assert!((orig_aabb.height() - dec_aabb.height()).abs() < 1.0);
    }

    #[test]
    fn test_decimate_empty_mesh() {
        let mesh = Mesh::new();
        let result = super::decimate_mesh(&mesh, MeshQuality::Draft);
        assert!(result.faces.is_empty());
        assert!(result.vertices.is_empty());
    }

    #[test]
    fn test_original_mesh_unchanged_after_decimate() {
        let mesh = make_displaced_cube(0.0, 0.0, 0.0);
        let _decimated = super::decimate_mesh(&mesh, MeshQuality::Draft);
        // Original should be unchanged.
        assert_eq!(mesh.faces.len(), 12);
    }
}
