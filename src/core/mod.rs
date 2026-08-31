//! Core slicing operations and data structures

mod compensation;
mod infill;
mod objects;
mod pipeline;
mod slicer;
mod surfaces;
mod types;
mod walls;

pub use infill::{add_infill_to_layers, InfillConfig};
pub use objects::{
    merge_meshes, sequential_order, sequential_warnings, slice_plate, ObjectIdentity, ObjectInput,
    PlateSlice,
};
pub use pipeline::process_mesh;
#[cfg(not(target_arch = "wasm32"))]
pub use pipeline::process_mesh_debug;
pub use slicer::slice_mesh;
pub use surfaces::{
    generate_top_bottom_surfaces, generate_top_bottom_surfaces_with_interior, SurfaceConfig,
    SurfaceSubTimings,
};
// Solid-surface line pitch + its nominal-width basis, shared with the G-code
// generator so the flow it charges for each top/bottom fill line matches the
// pitch the lines are laid at (deposited volume = spacing × layer_height, no
// over-extrusion) and both honour `top_surface_line_width` / `line_width`.
// `sparse_infill_nominal_width_mm` is the sparse-infill twin, and
// `extrusion_flow_spacing_mm` the shared `Flow::spacing()` relation both rest on.
pub(crate) use surfaces::{
    extrusion_flow_spacing_mm, outer_wall_nominal_width_mm, solid_surface_nominal_width_mm,
    sparse_infill_nominal_width_mm,
};
pub use types::{ExtrusionRole, OverhangClass, SliceLayer};

#[cfg(test)]
mod tests {
    use super::compensation::{apply_dimensional_compensation, CompensationConfig};
    use super::surfaces::{add_solid_infill_for_region, generate_rectilinear_infill};
    use super::*;
    use crate::infill::SurfacePattern;
    use crate::mesh::types::{Face, Mesh, Vertex};
    use crate::settings::params::SlicingParams;
    use clipper2::{Path, Paths};

    /// Build a simple 10×10×10 mm axis-aligned box mesh (12 triangles).
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

        let faces = face_indices
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
    fn test_slice_layer_creation() {
        let layer = SliceLayer::new(1.0);
        assert_eq!(layer.z, 1.0);
        assert!(layer.paths.is_empty());
        assert!(layer.path_roles.is_empty());
    }

    /// Regression test: with `only_one_wall_top = true`, the topmost layer
    /// of each top-surface run must be reduced to a single (outer) wall, and
    /// its TopSurface must extend out to the outer-wall edge — mirroring the
    /// behaviour of `only_one_wall_first_layer` on layer 0.
    ///
    /// The previous role-based detection in `apply_single_wall_restrictions`
    /// was a no-op (TopSurface roles are assigned later, after this runs),
    /// so the topmost layer kept all walls and the top surface was confined
    /// to a tiny disk inside the innermost wall — leaving a visible
    /// inter-wall gap that users perceived as the "between walls" bug
    /// persisting on top surfaces.
    #[test]
    fn test_only_one_wall_top_reduces_topmost_layer() {
        use crate::logging::NullLogger;
        let mesh = make_cube_mesh();
        let params = crate::settings::params::SlicingParams {
            layer_height: 2.0,
            top_layers: 2,
            bottom_layers: 2,
            surface_infill_angle: 0.0,
            only_one_wall_first_layer: true,
            only_one_wall_top: true,
            wall_count: 3,
            nozzle_diameter_mm: 0.4,
            infill_overlap_percent: 0.25,
            ensure_vertical_shell_thickness: false,
            ..crate::settings::params::SlicingParams::default()
        };

        let layers = process_mesh(&mesh, &params, &NullLogger);
        assert!(!layers.is_empty(), "expected sliced layers");

        let last = layers.len() - 1;
        let n_outer_top = layers[last]
            .path_roles
            .iter()
            .filter(|r| **r == ExtrusionRole::OuterWall)
            .count();
        let n_inner_top = layers[last]
            .path_roles
            .iter()
            .filter(|r| **r == ExtrusionRole::InnerWall)
            .count();
        assert!(n_outer_top >= 1, "topmost layer must keep its outer wall");
        assert_eq!(
            n_inner_top, 0,
            "only_one_wall_top should strip all InnerWall paths from the topmost \
             layer of a top-surface run, but {n_inner_top} remain"
        );

        // The layer below the topmost is also part of the top-surface run
        // (top_layers = 2) but is NOT the last layer of the run — it must
        // keep its inner walls.
        let n_inner_below_top = layers[last - 1]
            .path_roles
            .iter()
            .filter(|r| **r == ExtrusionRole::InnerWall)
            .count();
        assert!(
            n_inner_below_top > 0,
            "only_one_wall_top must NOT strip inner walls from layers in the \
             middle of a top-surface run (only the very topmost)"
        );

        // With only the outer wall remaining on the topmost layer, the
        // TopSurface should now extend to the outer-wall edge (within the
        // configured overlap), exactly mirroring the BottomSurface AABB on
        // layer 0 where only_one_wall_first_layer has the same effect.
        let top_pts: Vec<(f64, f64)> = layers[last]
            .paths
            .iter()
            .enumerate()
            .filter(|(i, _)| layers[last].role_for_path(*i) == ExtrusionRole::TopSurface)
            .flat_map(|(_, p)| p.iter().map(|pt| (pt.x(), pt.y())).collect::<Vec<_>>())
            .collect();
        assert!(
            !top_pts.is_empty(),
            "topmost layer should have TopSurface paths"
        );
        let xmax = top_pts
            .iter()
            .map(|(x, _)| *x)
            .fold(f64::NEG_INFINITY, f64::max);
        let xmin = top_pts
            .iter()
            .map(|(x, _)| *x)
            .fold(f64::INFINITY, f64::min);
        // Cube spans [0, 10]. With 1 wall (centerline ~0.2mm in) plus 25%
        // overlap, the surface should reach within ~0.5mm of each edge.
        // The buggy 3-wall behaviour confined it to ~[1.1, 8.9] (≥1.1mm from
        // each edge), so this threshold reliably separates fixed vs broken.
        assert!(
            xmax >= 9.5 && xmin <= 0.5,
            "top surface should extend close to the outer wall edge \
             (got xmin={xmin:.2}, xmax={xmax:.2}); the buggy multi-wall behaviour \
             would confine it to ~[1.1, 8.9]"
        );
    }

    #[test]
    fn test_slice_layer_role_for_path_default() {
        let layer = SliceLayer::new(1.0);
        // No roles set → should fall back to OuterWall
        assert_eq!(layer.role_for_path(0), ExtrusionRole::OuterWall);
        assert_eq!(layer.role_for_path(99), ExtrusionRole::OuterWall);
    }

    #[test]
    fn test_slice_layer_role_for_path_explicit() {
        let mut layer = SliceLayer::new(1.0);
        layer.path_roles.push(ExtrusionRole::Skirt);
        layer.path_roles.push(ExtrusionRole::Infill);
        assert_eq!(layer.role_for_path(0), ExtrusionRole::Skirt);
        assert_eq!(layer.role_for_path(1), ExtrusionRole::Infill);
        // Out of bounds → OuterWall default
        assert_eq!(layer.role_for_path(2), ExtrusionRole::OuterWall);
    }

    #[test]
    fn test_extrusion_role_type_names() {
        assert_eq!(ExtrusionRole::OuterWall.type_name(), "Outer wall");
        assert_eq!(ExtrusionRole::InnerWall.type_name(), "Inner wall");
        assert_eq!(ExtrusionRole::Infill.type_name(), "Sparse infill");
        assert_eq!(ExtrusionRole::Bridge.type_name(), "Bridge");
        assert_eq!(ExtrusionRole::TopSurface.type_name(), "Top surface");
        assert_eq!(ExtrusionRole::BottomSurface.type_name(), "Bottom surface");
        assert_eq!(ExtrusionRole::GapFill.type_name(), "Gap infill");
        assert_eq!(ExtrusionRole::Support.type_name(), "Support material");
        assert_eq!(ExtrusionRole::Skirt.type_name(), "Skirt");
    }

    #[test]
    fn test_extrusion_role_widths_positive() {
        for role in [
            ExtrusionRole::OuterWall,
            ExtrusionRole::InnerWall,
            ExtrusionRole::Infill,
            ExtrusionRole::Bridge,
            ExtrusionRole::TopSurface,
            ExtrusionRole::Support,
            ExtrusionRole::Skirt,
        ] {
            assert!(
                role.default_width_mm() > 0.0,
                "{:?} width must be positive",
                role
            );
        }
    }

    #[test]
    fn test_slice_mesh_path_roles_match_paths() {
        let mesh = make_cube_mesh();
        let layers = slice_mesh(&mesh, 2.0);
        for layer in &layers {
            assert_eq!(
                layer.paths.len(),
                layer.path_roles.len(),
                "path_roles length must match paths length at z={}",
                layer.z
            );
            for role in &layer.path_roles {
                assert_eq!(
                    *role,
                    ExtrusionRole::OuterWall,
                    "slice_mesh assigns OuterWall"
                );
            }
        }
    }

    #[test]
    fn test_slice_mesh_empty_mesh() {
        let mesh = Mesh::new();
        let layers = slice_mesh(&mesh, 0.2);
        assert!(layers.is_empty());
    }

    #[test]
    fn test_slice_mesh_zero_layer_height() {
        let mesh = make_cube_mesh();
        let layers = slice_mesh(&mesh, 0.0);
        assert!(layers.is_empty());
    }

    #[test]
    fn test_slice_mesh_negative_layer_height() {
        let mesh = make_cube_mesh();
        let layers = slice_mesh(&mesh, -0.2);
        assert!(layers.is_empty());
    }

    #[test]
    fn test_slice_mesh_cube_layer_count() {
        let mesh = make_cube_mesh();
        // 10 mm cube sliced at 2 mm → 5 layers at z=1,3,5,7,9
        let layers = slice_mesh(&mesh, 2.0);
        assert_eq!(layers.len(), 5, "Expected 5 layers, got {}", layers.len());
    }

    #[test]
    fn test_slice_mesh_cube_z_values() {
        let mesh = make_cube_mesh();
        let layers = slice_mesh(&mesh, 2.0);
        let zs: Vec<f64> = layers.iter().map(|l| l.z).collect();
        // First layer at z_min + layer_height/2 = 0 + 1 = 1.0
        assert!((zs[0] - 1.0).abs() < 1e-10, "First layer z={}", zs[0]);
        assert!((zs[1] - 3.0).abs() < 1e-10, "Second layer z={}", zs[1]);
    }

    #[test]
    fn test_slice_mesh_cube_has_contours() {
        let mesh = make_cube_mesh();
        let layers = slice_mesh(&mesh, 2.0);
        // Every layer through the cube should have at least one contour
        for layer in &layers {
            assert!(
                !layer.paths.is_empty(),
                "Layer at z={} has no contours",
                layer.z
            );
        }
    }

    #[test]
    fn test_add_infill_to_layers_basic() {
        use crate::infill::InfillPattern;

        let mesh = make_cube_mesh();
        let mut layers = slice_mesh(&mesh, 2.0);

        // Before infill: only wall paths
        for layer in &layers {
            for role in &layer.path_roles {
                assert!(
                    *role == ExtrusionRole::OuterWall || *role == ExtrusionRole::InnerWall,
                    "Expected wall role, got {:?}",
                    role
                );
            }
        }

        // Add infill
        add_infill_to_layers(
            &mut layers,
            &InfillConfig {
                density: 0.2,
                pattern: InfillPattern::Rectilinear,
                base_angle_deg: 45.0,
                spacing_mm: 0.4,
                nozzle_diameter_mm: 0.4,
                perimeter_gap_mm: 0.0,
                min_extrusion_mm: 0.0,
                anchor_length_mm: 0.0,
                anchor_length_max_mm: 0.0,
                every_layers: 1,
                combination_max_layer_height_mm: 0.0,
                layer_height_mm: 0.2,
                solid_every_layers: 0,
                solid_spacing_mm: 0.357,
                solid_pattern: SurfacePattern::Monotonic,
            },
            None,
        );

        // After infill: should have both wall and infill paths
        for layer in &layers {
            let has_walls = layer
                .path_roles
                .iter()
                .any(|r| *r == ExtrusionRole::OuterWall || *r == ExtrusionRole::InnerWall);
            let has_infill = layer.path_roles.contains(&ExtrusionRole::Infill);
            assert!(has_walls, "Layer at z={} missing walls", layer.z);
            assert!(has_infill, "Layer at z={} missing infill", layer.z);
        }
    }

    #[test]
    fn test_add_infill_to_layers_zero_density() {
        use crate::infill::InfillPattern;

        let mesh = make_cube_mesh();
        let mut layers = slice_mesh(&mesh, 2.0);
        let initial_path_count: usize = layers.iter().map(|l| l.paths.len()).sum();

        // Add zero-density infill (should do nothing)
        add_infill_to_layers(
            &mut layers,
            &InfillConfig {
                density: 0.0,
                pattern: InfillPattern::Rectilinear,
                base_angle_deg: 45.0,
                spacing_mm: 0.4,
                nozzle_diameter_mm: 0.4,
                perimeter_gap_mm: 0.0,
                min_extrusion_mm: 0.0,
                anchor_length_mm: 0.0,
                anchor_length_max_mm: 0.0,
                every_layers: 1,
                combination_max_layer_height_mm: 0.0,
                layer_height_mm: 0.2,
                solid_every_layers: 0,
                solid_spacing_mm: 0.357,
                solid_pattern: SurfacePattern::Monotonic,
            },
            None,
        );

        let final_path_count: usize = layers.iter().map(|l| l.paths.len()).sum();
        assert_eq!(
            initial_path_count, final_path_count,
            "Zero density should not add paths"
        );
    }

    #[test]
    fn test_add_infill_to_layers_grid_pattern() {
        use crate::infill::InfillPattern;

        let mesh = make_cube_mesh();
        let mut layers = slice_mesh(&mesh, 2.0);

        // Add grid infill
        add_infill_to_layers(
            &mut layers,
            &InfillConfig {
                density: 0.3,
                pattern: InfillPattern::Grid,
                base_angle_deg: 45.0,
                spacing_mm: 0.4,
                nozzle_diameter_mm: 0.4,
                perimeter_gap_mm: 0.0,
                min_extrusion_mm: 0.0,
                anchor_length_mm: 0.0,
                anchor_length_max_mm: 0.0,
                every_layers: 1,
                combination_max_layer_height_mm: 0.0,
                layer_height_mm: 0.2,
                solid_every_layers: 0,
                solid_spacing_mm: 0.357,
                solid_pattern: SurfacePattern::Monotonic,
            },
            None,
        );

        // Grid pattern should produce more infill paths than rectilinear
        for layer in &layers {
            let infill_count = layer
                .path_roles
                .iter()
                .filter(|r| **r == ExtrusionRole::Infill)
                .count();
            assert!(
                infill_count > 0,
                "Layer at z={} has no infill paths",
                layer.z
            );
        }
    }

    #[test]
    fn test_infill_not_placed_on_fully_solid_surface_layers() {
        use crate::infill::InfillPattern;

        // A 2-layer cube: with 2 top + 2 bottom layers, every layer is a
        // surface layer — sparse infill should not be added on top of solid surfaces.
        let mesh = make_cube_mesh();
        // 5 layers at 2mm.  Use top=2/bottom=2 so the first two and last two
        // layers are fully solid surfaces; the middle layer is interior.
        let mut layers = slice_mesh(&mesh, 2.0);
        generate_top_bottom_surfaces(&mut layers, 2, 2, 2.0, 45.0);

        // Confirm solid_regions are populated for the top/bottom layers.
        let n = layers.len();
        assert!(
            !layers[0].solid_regions.is_empty(),
            "Layer 0 should have solid_regions"
        );
        assert!(
            !layers[n - 1].solid_regions.is_empty(),
            "Last layer should have solid_regions"
        );

        // Count surface-only infill paths before adding sparse infill.
        let surface_counts: Vec<usize> = layers
            .iter()
            .map(|l| {
                l.path_roles
                    .iter()
                    .filter(|r| {
                        **r == ExtrusionRole::TopSurface || **r == ExtrusionRole::BottomSurface
                    })
                    .count()
            })
            .collect();

        // Now add sparse infill.
        add_infill_to_layers(
            &mut layers,
            &InfillConfig {
                density: 0.3,
                pattern: InfillPattern::Rectilinear,
                base_angle_deg: 45.0,
                spacing_mm: 0.4,
                nozzle_diameter_mm: 0.4,
                perimeter_gap_mm: 0.0,
                min_extrusion_mm: 0.0,
                anchor_length_mm: 0.0,
                anchor_length_max_mm: 0.0,
                every_layers: 1,
                combination_max_layer_height_mm: 0.0,
                layer_height_mm: 0.2,
                solid_every_layers: 0,
                solid_spacing_mm: 0.357,
                solid_pattern: SurfacePattern::Monotonic,
            },
            None,
        );

        // For a layer that is entirely solid (solid_regions == perimeter area),
        // no new Infill paths should have been added.
        // Layers 0 and n-1 are entirely solid surfaces on a simple cube.
        for i in [0, n - 1] {
            let infill_added = layers[i]
                .path_roles
                .iter()
                .filter(|r| **r == ExtrusionRole::Infill)
                .count();
            assert_eq!(
                infill_added, 0,
                "Layer {} (fully solid surface) should not have sparse infill (got {})",
                i, infill_added
            );
            // Surface paths must remain unchanged.
            let surface_now = layers[i]
                .path_roles
                .iter()
                .filter(|r| **r == ExtrusionRole::TopSurface || **r == ExtrusionRole::BottomSurface)
                .count();
            assert_eq!(
                surface_now, surface_counts[i],
                "Surface path count on layer {} must not change",
                i
            );
        }
    }

    #[test]
    fn test_solid_regions_populated_by_surface_generation() {
        let mesh = make_cube_mesh();
        let mut layers = slice_mesh(&mesh, 2.0);

        // Initially no solid regions.
        for layer in &layers {
            assert!(layer.solid_regions.is_empty());
        }

        generate_top_bottom_surfaces(&mut layers, 3, 3, 2.0, 45.0);

        // After surface generation the topmost and bottommost layers must have
        // non-empty solid_regions.
        let n = layers.len();
        assert!(
            !layers[0].solid_regions.is_empty(),
            "Bottom layer should have solid_regions after surface generation"
        );
        assert!(
            !layers[n - 1].solid_regions.is_empty(),
            "Top layer should have solid_regions after surface generation"
        );
    }

    #[test]
    fn test_generate_top_bottom_surfaces_empty_layers() {
        let mut layers: Vec<SliceLayer> = vec![];
        generate_top_bottom_surfaces(&mut layers, 3, 3, 0.2, 45.0);
        // Should handle empty input gracefully
        assert!(layers.is_empty());
    }

    #[test]
    fn test_generate_top_bottom_surfaces_zero_count() {
        let mesh = make_cube_mesh();
        let mut layers = slice_mesh(&mesh, 2.0);
        let original_count = layers.len();

        generate_top_bottom_surfaces(&mut layers, 0, 0, 2.0, 45.0);

        // Layers should remain unchanged when both counts are 0
        assert_eq!(layers.len(), original_count);
    }

    #[test]
    fn test_generate_top_bottom_surfaces_adds_infill() {
        let mesh = make_cube_mesh();
        let mut layers = slice_mesh(&mesh, 2.0);
        let original_paths_first = layers[0].paths.len();

        // Generate bottom surfaces for first 2 layers, top for last 2
        generate_top_bottom_surfaces(&mut layers, 2, 2, 2.0, 45.0);

        // First layer should have more paths (original perimeters + infill)
        assert!(
            layers[0].paths.len() > original_paths_first,
            "Expected infill to be added to bottom layer"
        );
    }

    #[test]
    fn test_generate_top_bottom_surfaces_roles_assigned() {
        let mesh = make_cube_mesh();
        let mut layers = slice_mesh(&mesh, 2.0);
        let total = layers.len();

        generate_top_bottom_surfaces(&mut layers, 2, 2, 2.0, 45.0);

        // Check that bottom layers have BottomSurface role
        for (i, layer) in layers.iter().take(2).enumerate() {
            let has_bottom_role = layer.path_roles.contains(&ExtrusionRole::BottomSurface);
            assert!(
                has_bottom_role,
                "Layer {} should have BottomSurface role",
                i
            );
        }

        // Check that top layers have TopSurface role
        for (i, layer) in layers.iter().enumerate().skip(total - 2).take(2) {
            let has_top_role = layer.path_roles.contains(&ExtrusionRole::TopSurface);
            assert!(has_top_role, "Layer {} should have TopSurface role", i);
        }
    }

    #[test]
    fn test_generate_top_bottom_surfaces_mid_model_detection() {
        // Build a stacked two-cube mesh: a 10×10×4 base with a 6×6×4 column on top.
        // When sliced at layer_height=2 we get:
        //   layer 0  z=1 – base (10×10)
        //   layer 1  z=3 – base (10×10)
        //   layer 2  z=5 – column (6×6)
        //   layer 3  z=7 – column (6×6)
        //
        // With top_layers=1, bottom_layers=1 (intersection-based algorithm):
        //
        //   TopSurface on layer 1 (z=3):
        //     covered = intersect(10×10, layer_above=6×6) = 6×6
        //     top_region = diff(10×10, 6×6) = annular region → non-empty
        //     → layer 1 must have TopSurface infill
        //
        //   No TopSurface on layer 2 (z=5):
        //     covered = intersect(6×6, layer_above=6×6) = 6×6
        //     top_region = diff(6×6, 6×6) = empty
        //     → layer 2 is fully covered by layer 3 and must NOT have TopSurface infill
        //
        //   BottomSurface on layer 0 (z=1):
        //     i < j → covered = empty → bottom_region = perimeters[0] (first layer)
        //
        //   No BottomSurface on layer 2 (z=5):
        //     covered = intersect(6×6, layer_below=10×10) = 6×6 (column inside base)
        //     bottom_region = diff(6×6, 6×6) = empty
        //     → the column is fully supported; it must NOT get spurious BottomSurface infill

        let v: Vec<Vertex> = vec![
            // Base cube 10×10×4 (z 0..4)
            Vertex::new(0.0, 0.0, 0.0),
            Vertex::new(10.0, 0.0, 0.0),
            Vertex::new(10.0, 10.0, 0.0),
            Vertex::new(0.0, 10.0, 0.0),
            Vertex::new(0.0, 0.0, 4.0),
            Vertex::new(10.0, 0.0, 4.0),
            Vertex::new(10.0, 10.0, 4.0),
            Vertex::new(0.0, 10.0, 4.0),
            // Upper column 6×6×4 (z 4..8), centred at (2,2)..(8,8)
            Vertex::new(2.0, 2.0, 4.0),
            Vertex::new(8.0, 2.0, 4.0),
            Vertex::new(8.0, 8.0, 4.0),
            Vertex::new(2.0, 8.0, 4.0),
            Vertex::new(2.0, 2.0, 8.0),
            Vertex::new(8.0, 2.0, 8.0),
            Vertex::new(8.0, 8.0, 8.0),
            Vertex::new(2.0, 8.0, 8.0),
        ];
        let face_indices: &[[usize; 3]] = &[
            // Base cube faces
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
            // Column cube faces
            [8, 10, 9],
            [8, 11, 10],
            [12, 13, 14],
            [12, 14, 15],
            [8, 9, 13],
            [8, 13, 12],
            [10, 11, 15],
            [10, 15, 14],
            [8, 12, 15],
            [8, 15, 11],
            [9, 10, 14],
            [9, 14, 13],
        ];

        let faces = face_indices
            .iter()
            .map(|idx| Face::new([v[idx[0]], v[idx[1]], v[idx[2]]]))
            .collect();

        let mesh = Mesh {
            vertices: v,
            faces,
            aabb: None,
        };

        let mut layers = slice_mesh(&mesh, 2.0);
        assert_eq!(layers.len(), 4, "Expected 4 layers for the step mesh");

        generate_top_bottom_surfaces(&mut layers, 1, 1, 2.0, 45.0);

        // Layer 0 (z=1) is the absolute bottom → BottomSurface
        assert!(
            layers[0].path_roles.contains(&ExtrusionRole::BottomSurface),
            "Layer 0 (z=1) should be a bottom surface (first layer)"
        );

        // Layer 1 (z=3) is below the column; the annular 10×10 minus 6×6 region
        // is exposed above → TopSurface infill must be added.
        assert!(
            layers[1].path_roles.contains(&ExtrusionRole::TopSurface),
            "Layer 1 (z=3) should detect the step-down as a top surface"
        );

        // Layer 2 (z=5) is fully covered by layer 3 above → no TopSurface here.
        assert!(
            !layers[2].path_roles.contains(&ExtrusionRole::TopSurface),
            "Layer 2 (z=5) is fully covered above and must NOT have TopSurface infill"
        );

        // Layer 2 (z=5) is fully supported by layer 1 below → no BottomSurface.
        assert!(
            !layers[2].path_roles.contains(&ExtrusionRole::BottomSurface),
            "Layer 2 (z=5) is fully supported and must NOT have spurious BottomSurface infill"
        );

        // Layer 3 (z=7) is the absolute top → TopSurface
        assert!(
            layers[3].path_roles.contains(&ExtrusionRole::TopSurface),
            "Layer 3 (z=7) should be a top surface (last layer)"
        );
    }

    #[test]
    fn test_infill_clipped_to_contour() {
        // Verify that infill lines are clipped to the contour and don't extend
        // beyond the bounding box of the given paths.
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        let mut paths = Paths::new(vec![]);
        paths.push(square);

        let infill = generate_rectilinear_infill(&paths, 1.0, 0.0, 0.0);

        assert!(!infill.is_empty(), "Expected infill lines to be generated");

        // All clipped endpoints should lie within the contour bounding box
        // (with a small epsilon for floating-point rounding by Clipper2).
        let eps = 0.01;
        for path in infill.iter() {
            for pt in path.iter() {
                let x = pt.x();
                let y = pt.y();
                assert!(
                    x >= -eps && x <= 10.0 + eps,
                    "Infill x={x} is outside contour bounds [0, 10]"
                );
                assert!(
                    y >= -eps && y <= 10.0 + eps,
                    "Infill y={y} is outside contour bounds [0, 10]"
                );
            }
        }
    }

    #[test]
    fn test_add_solid_infill_for_region_empty_region() {
        let mut layer = SliceLayer::new(1.0);
        let empty: Paths = Paths::new(vec![]);
        add_solid_infill_for_region(
            &mut layer,
            &empty,
            ExtrusionRole::TopSurface,
            0.4,
            45.0,
            0.0,
            crate::infill::SurfacePattern::Rectilinear,
        );
        // Should handle empty region gracefully – no paths added
        assert!(layer.paths.is_empty());
    }

    #[test]
    fn test_extrusion_role_bottom_surface() {
        assert_eq!(ExtrusionRole::BottomSurface.type_name(), "Bottom surface");
        assert!(ExtrusionRole::BottomSurface.default_width_mm() > 0.0);
    }

    /// A 40×40×40 mm box: tall enough that the middle layers are plain walls
    /// plus sparse infill, which is what the combining and forced-solid options
    /// act on.
    fn make_tall_box_mesh() -> Mesh {
        let s = 40.0;
        let v = [
            Vertex::new(0.0, 0.0, 0.0),
            Vertex::new(s, 0.0, 0.0),
            Vertex::new(s, s, 0.0),
            Vertex::new(0.0, s, 0.0),
            Vertex::new(0.0, 0.0, s),
            Vertex::new(s, 0.0, s),
            Vertex::new(s, s, s),
            Vertex::new(0.0, s, s),
        ];
        let faces = [
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ];
        Mesh {
            vertices: v.to_vec(),
            faces: faces
                .iter()
                .map(|idx| Face::new([v[idx[0]], v[idx[1]], v[idx[2]]]))
                .collect(),
            aabb: None,
        }
    }

    #[test]
    fn solid_infill_every_layers_inserts_internal_solid_floors() {
        use crate::logging::NullLogger;
        let mesh = make_tall_box_mesh();
        let base = crate::settings::params::SlicingParams {
            layer_height: 0.4,
            ..Default::default()
        };

        let plain = process_mesh(&mesh, &base, &NullLogger);
        assert!(
            !plain
                .iter()
                .any(|l| l.path_roles.contains(&ExtrusionRole::InternalSolid)),
            "internal solid floors must not appear unless asked for"
        );

        let every = 10;
        let forced = process_mesh(
            &mesh,
            &crate::settings::params::SlicingParams {
                solid_infill_every_layers: every,
                ..base
            },
            &NullLogger,
        );

        let solid_layers: Vec<usize> = forced
            .iter()
            .enumerate()
            .filter(|(_, l)| l.path_roles.contains(&ExtrusionRole::InternalSolid))
            .map(|(i, _)| i)
            .collect();
        assert!(
            !solid_layers.is_empty(),
            "expected forced solid layers every {every}"
        );
        for i in &solid_layers {
            assert_eq!(
                i % every as usize,
                0,
                "layer {i} is solid but is not a multiple of {every}"
            );
        }
    }

    #[test]
    fn infill_every_layers_prints_taller_beads_on_fewer_layers() {
        use crate::logging::NullLogger;
        let mesh = make_tall_box_mesh();
        // A 0.6 mm nozzle at 0.2 mm layers leaves room for three combined layers.
        let base = crate::settings::params::SlicingParams {
            layer_height: 0.2,
            nozzle_diameter_mm: 0.6,
            ..Default::default()
        };

        let plain = process_mesh(&mesh, &base, &NullLogger);
        assert!(
            plain.iter().all(|l| l.path_heights.is_empty()),
            "an ordinary print must carry no height overrides at all"
        );

        let combined = process_mesh(
            &mesh,
            &crate::settings::params::SlicingParams {
                infill_every_layers: 3,
                ..base
            },
            &NullLogger,
        );

        let infill_length = |layers: &[SliceLayer]| -> f64 {
            layers
                .iter()
                .flat_map(|l| {
                    l.paths
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| l.role_for_path(*i) == ExtrusionRole::Infill)
                        .map(|(_, p)| {
                            let pts: Vec<(f64, f64)> = p.iter().map(|v| (v.x(), v.y())).collect();
                            pts.windows(2)
                                .map(|w| {
                                    ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt()
                                })
                                .sum::<f64>()
                        })
                })
                .sum()
        };

        let tall: Vec<f64> = combined
            .iter()
            .flat_map(|l| l.path_heights.iter().filter_map(|h| *h))
            .collect();
        assert!(
            !tall.is_empty(),
            "combining should tag some infill with a stacked height"
        );
        assert!(
            tall.iter().all(|h| (*h - 0.6).abs() < 1e-9),
            "three 0.2 mm layers should stack to 0.6 mm, got {tall:?}"
        );

        // Far less infill is drawn, because most of it is printed once, taller.
        let before = infill_length(&plain);
        let after = infill_length(&combined);
        assert!(
            after < before * 0.6,
            "combined infill ({after:.0} mm) should be well under the per-layer total ({before:.0} mm)"
        );
    }

    #[test]
    fn test_process_mesh() {
        use crate::logging::NullLogger;
        let mesh = make_cube_mesh();
        let params = crate::settings::params::SlicingParams {
            layer_height: 2.0,
            top_layers: 2,
            bottom_layers: 2,
            surface_infill_angle: 45.0,
            // Use old defaults for this test to verify basic functionality
            only_one_wall_first_layer: false,
            only_one_wall_top: false,
            ..crate::settings::params::SlicingParams::default()
        };

        let layers = process_mesh(&mesh, &params, &NullLogger);

        // Should have layers
        assert!(!layers.is_empty());

        // First layer should have BottomSurface paths
        assert!(layers[0].path_roles.contains(&ExtrusionRole::BottomSurface));

        // Last layer should have TopSurface paths
        let last_idx = layers.len() - 1;
        assert!(layers[last_idx]
            .path_roles
            .contains(&ExtrusionRole::TopSurface));
    }

    /// Regression test for the "surfaces between walls" bug.
    ///
    /// Slices a 10×10×10 mm cube with the default 3 walls and verifies that
    /// every BottomSurface coordinate on layer 0 is well inside the innermost
    /// wall, i.e. no surface line is drawn in the band between concentric
    /// walls.  The previous EvenOdd-based interior calculation produced
    /// surfaces in that band on multi-wall layers; this test guards against
    /// that regression.
    #[test]
    fn test_smart_surface_skipping_no_between_walls_artifacts() {
        use crate::logging::NullLogger;
        let mesh = make_cube_mesh();
        let params = crate::settings::params::SlicingParams {
            layer_height: 2.0,
            top_layers: 2,
            bottom_layers: 2,
            surface_infill_angle: 0.0,
            // Disable single-wall restrictions so all layers carry the full
            // multi-wall stack – this is the configuration that triggered the
            // original bug.
            only_one_wall_first_layer: false,
            only_one_wall_top: false,
            // Explicit defaults to make the geometry expectations precise.
            wall_count: 3,
            nozzle_diameter_mm: 0.4,
            infill_overlap_percent: 0.25,
            ensure_vertical_shell_thickness: false,
            ..crate::settings::params::SlicingParams::default()
        };

        let layers = process_mesh(&mesh, &params, &NullLogger);
        assert!(!layers.is_empty(), "expected sliced layers");

        // Cube is at [0, 10]² in XY.  With 3 × 0.4 mm walls the innermost
        // wall centerline sits ~1.0 mm from each edge, so its inner bound is
        // ~1.2 mm.  The 25 % overlap (= 0.1 mm) lets surfaces extend back
        // out to ~1.1 mm.  Any surface point closer than 0.5 mm to an edge
        // would lie in the inter-wall band and is the bug we are guarding.
        const SAFE_MARGIN_MM: f64 = 0.5;

        let mut total_surface_points = 0;
        for layer in &layers {
            for (i, path) in layer.paths.iter().enumerate() {
                let role = layer.role_for_path(i);
                if role != ExtrusionRole::BottomSurface && role != ExtrusionRole::TopSurface {
                    continue;
                }
                for pt in path.iter() {
                    total_surface_points += 1;
                    let (x, y) = (pt.x(), pt.y());
                    assert!(
                        (SAFE_MARGIN_MM..=10.0 - SAFE_MARGIN_MM).contains(&x)
                            && (SAFE_MARGIN_MM..=10.0 - SAFE_MARGIN_MM).contains(&y),
                        "surface point ({x}, {y}) lies in the inter-wall band on \
                         layer z={} (role={:?}) – smart surface skipping regressed",
                        layer.z,
                        role,
                    );
                }
            }
        }

        assert!(
            total_surface_points > 0,
            "expected some surface paths; smart-skip should not skip cube top/bottom"
        );
    }

    #[test]
    fn test_generate_rectilinear_infill_basic() {
        // Create a simple square path
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        let mut paths = Paths::new(vec![]);
        paths.push(square);

        let infill = generate_rectilinear_infill(&paths, 1.0, 45.0, 0.0);

        // Should generate some infill lines
        assert!(!infill.is_empty(), "Expected infill lines to be generated");
    }

    #[test]
    fn test_generate_rectilinear_infill_empty_contours() {
        let paths = Paths::new(vec![]);
        let infill = generate_rectilinear_infill(&paths, 1.0, 45.0, 0.0);
        assert!(infill.is_empty());
    }

    #[test]
    fn test_generate_rectilinear_infill_zero_spacing() {
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        let mut paths = Paths::new(vec![]);
        paths.push(square);

        let infill = generate_rectilinear_infill(&paths, 0.0, 45.0, 0.0);
        assert!(infill.is_empty());
    }

    /// Regression test: the surface detection algorithm must use progressive
    /// intersection, not a single comparison against the N-th neighbour.
    ///
    /// The "hourglass" scenario has a wide layer, a narrow intermediate layer,
    /// and then wide layers again.  The old `difference(layer[i], layer[i+N])`
    /// approach compared a wide layer against a later wide layer and returned
    /// empty → no top surface, silently missing the narrow gap in between.
    /// With the intersection-based approach the coverage is narrowed by the
    /// intermediate narrow layer, so the annular region is correctly flagged.
    #[test]
    fn test_surface_detection_non_monotonic_shape() {
        // Layers (manual construction, not mesh-derived):
        //   layer 0: 10×10 wide
        //   layer 1: 10×10 wide
        //   layer 2:  4×4  narrow  ← the "waist"
        //   layer 3: 10×10 wide
        //   layer 4: 10×10 wide
        //   layer 5: 10×10 wide
        //
        // With top_layers=3:
        //
        //   layer 2 (narrow, 4×4): covered by layers 3,4,5 (all 10×10 ⊇ 4×4)
        //     → NOT a top surface ✓
        //
        //   layer 0 (wide, 10×10):
        //     NEW: j=1 → intersect(10×10, 10×10) = 10×10
        //          j=2 → intersect(10×10, 4×4)   = 4×4   ← narrows
        //          j=3 → intersect(4×4,  10×10)  = 4×4
        //          top_region = diff(10×10, 4×4) = annular  ← TOP SURFACE ✓
        //     OLD: diff(layer[0], layer[3]) = diff(10×10, 10×10) = empty  ✗
        let make_rect_layer = |z: f64, w: f64, h: f64| -> SliceLayer {
            let mut layer = SliceLayer::new(z);
            let path: Path = vec![(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)].into();
            layer.paths.push(path);
            layer.path_roles.push(ExtrusionRole::OuterWall);
            layer
        };

        let mut layers = vec![
            make_rect_layer(1.0, 10.0, 10.0), // 0 – wide
            make_rect_layer(2.0, 10.0, 10.0), // 1 – wide
            make_rect_layer(3.0, 4.0, 4.0),   // 2 – narrow
            make_rect_layer(4.0, 10.0, 10.0), // 3 – wide
            make_rect_layer(5.0, 10.0, 10.0), // 4 – wide
            make_rect_layer(6.0, 10.0, 10.0), // 5 – wide
        ];

        generate_top_bottom_surfaces(&mut layers, 3, 0, 1.0, 45.0);

        // Layer 2 (narrow): fully covered by the three wide layers above it.
        assert!(
            !layers[2].path_roles.contains(&ExtrusionRole::TopSurface),
            "Narrow layer 2 is fully covered above and must NOT have TopSurface infill"
        );

        // Layer 0: the 10×10 annular area is NOT covered at layer 2 (only 4×4)
        // → must be flagged as a top surface even though layer[0+3]=layer3 is
        //   also 10×10 (the gap at layer 2 is in between).
        assert!(
            layers[0].path_roles.contains(&ExtrusionRole::TopSurface),
            "Layer 0 should have TopSurface infill: the annular region is exposed at layer 2"
        );

        // Layers 3, 4, 5 are the top-3 wide layers → must all be top surfaces.
        for idx in [3, 4, 5] {
            assert!(
                layers[idx].path_roles.contains(&ExtrusionRole::TopSurface),
                "Layer {idx} is within top_layers=3 of the model top and must have TopSurface"
            );
        }
    }

    /// Test that top and bottom surfaces don't overlap on the same layer.
    /// This was a bug where regions could be marked as both top AND bottom,
    /// causing incorrect G-code output.
    #[test]
    fn test_no_overlapping_top_bottom_surfaces() {
        // Create a simple layer stack where the first layer could potentially
        // be marked as both top and bottom if the algorithm is broken.
        let make_rect_layer = |z: f64, w: f64, h: f64| -> SliceLayer {
            let mut layer = SliceLayer::new(z);
            let path: Path = vec![(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)].into();
            layer.paths.push(path);
            layer.path_roles.push(ExtrusionRole::OuterWall);
            layer
        };

        let mut layers = vec![
            make_rect_layer(1.0, 10.0, 10.0), // 0 – base layer
            make_rect_layer(2.0, 10.0, 10.0), // 1
            make_rect_layer(3.0, 10.0, 10.0), // 2
            make_rect_layer(4.0, 10.0, 10.0), // 3 – top layer
        ];

        // Generate with both top_layers and bottom_layers enabled
        generate_top_bottom_surfaces(&mut layers, 2, 2, 1.0, 45.0);

        // Check each layer to ensure no path is in BOTH top and bottom regions
        for (layer_idx, layer) in layers.iter().enumerate() {
            let has_top = layer.path_roles.contains(&ExtrusionRole::TopSurface);
            let has_bottom = layer.path_roles.contains(&ExtrusionRole::BottomSurface);

            // Count the actual number of each type
            let top_count = layer
                .path_roles
                .iter()
                .filter(|&&r| r == ExtrusionRole::TopSurface)
                .count();
            let bottom_count = layer
                .path_roles
                .iter()
                .filter(|&&r| r == ExtrusionRole::BottomSurface)
                .count();

            if has_top && has_bottom {
                panic!(
                    "Layer {} has BOTH top ({}) and bottom ({}) surface paths - they should not overlap!",
                    layer_idx, top_count, bottom_count
                );
            }
        }

        // Layer 0 should be bottom only (first two layers)
        assert!(
            layers[0].path_roles.contains(&ExtrusionRole::BottomSurface),
            "Layer 0 should have bottom surface"
        );
        assert!(
            !layers[0].path_roles.contains(&ExtrusionRole::TopSurface),
            "Layer 0 should NOT have top surface"
        );

        // Layer 3 (top) should be top only
        assert!(
            layers[3].path_roles.contains(&ExtrusionRole::TopSurface),
            "Layer 3 should have top surface"
        );
        assert!(
            !layers[3].path_roles.contains(&ExtrusionRole::BottomSurface),
            "Layer 3 should NOT have bottom surface"
        );
    }

    /// Test that surface generation correctly handles holes (inner contours).
    /// When a layer has a hole, the surface infill should not fill the hole.
    ///
    /// Critically, this test uses the **same winding order** for both outer
    /// and hole contours — the exact case that `FillRule::NonZero` gets wrong
    /// (it treats the hole as doubly-wound solid material).  `EvenOdd` handles
    /// it correctly regardless of winding direction.
    #[test]
    fn test_surface_generation_with_holes() {
        use clipper2::Path;

        // Create a layer with an outer square and an inner square (hole).
        // Both contours use the same winding order (right → up → left → down),
        // which is the problematic case for NonZero but handled correctly by EvenOdd.
        let mut layer = SliceLayer::new(1.0);

        // Outer square 10x10 (same winding as inner = the hard case)
        let outer: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();

        // Inner square 4x4 (same winding as outer — NonZero would treat this as solid)
        let hole: Path = vec![(3.0, 3.0), (7.0, 3.0), (7.0, 7.0), (3.0, 7.0)].into();

        layer.paths.push(outer);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.paths.push(hole);
        layer.path_roles.push(ExtrusionRole::OuterWall);

        // Create a simple 1-layer setup
        let mut layers = vec![layer];

        // Generate bottom surfaces (first layer, no layers below)
        generate_top_bottom_surfaces(&mut layers, 0, 1, 1.0, 45.0);

        // Count the surface infill paths
        let surface_path_count = layers[0]
            .path_roles
            .iter()
            .filter(|&&r| r == ExtrusionRole::BottomSurface)
            .count();

        // There should be surface paths
        assert!(
            surface_path_count > 0,
            "Should have generated bottom surface infill"
        );

        // Collect all bottom surface path segments
        let surface_paths: Vec<&Path> = layers[0]
            .paths
            .iter()
            .enumerate()
            .filter(|(i, _)| layers[0].role_for_path(*i) == ExtrusionRole::BottomSurface)
            .map(|(_, p)| p)
            .collect();

        println!("Generated {} bottom surface paths", surface_paths.len());

        // Check if any surface path segments pass through the hole region
        // The hole is at (3,3) to (7,7).
        // With EvenOdd fill rule, holes are correctly excluded regardless of
        // the winding order of the contours, so no infill should be inside.
        let mut paths_in_hole = 0;
        for path in &surface_paths {
            for pt in path.iter() {
                let x = pt.x();
                let y = pt.y();
                // Check if point is inside the hole region (with small margin)
                if x > 3.5 && x < 6.5 && y > 3.5 && y < 6.5 {
                    paths_in_hole += 1;
                    break; // Count each path only once
                }
            }
        }

        assert_eq!(
            paths_in_hole, 0,
            "Surface infill must not penetrate the hole region (found {} paths inside hole). \
             EvenOdd fill rule should handle this regardless of contour winding order.",
            paths_in_hole
        );
    }

    // ── Bridge detection ──────────────────────────────────────────────────────

    /// Test that unsupported bottom areas are classified as Bridge role.
    ///
    /// Layout (two layers):
    /// - Layer 0: thin 1×10 strip on the left side
    /// - Layer 1: full 10×10 square
    ///
    /// The 9mm region of layer 1 not covered by layer 0 has no direct support
    /// from the previous layer and should be labelled Bridge.
    #[test]
    fn test_bridge_detection_assigns_bridge_role_to_unsupported_area() {
        use clipper2::Path;

        // Layer 0: thin 1×10 strip on the left side
        let mut layer0 = SliceLayer::new(0.2);
        let strip: Path = vec![(0.0, 0.0), (1.0, 0.0), (1.0, 10.0), (0.0, 10.0)].into();
        layer0.paths.push(strip);
        layer0.path_roles.push(ExtrusionRole::OuterWall);

        // Layer 1: full 10×10 square
        let mut layer1 = SliceLayer::new(0.4);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer1.paths.push(square);
        layer1.path_roles.push(ExtrusionRole::OuterWall);

        let mut layers = vec![layer0, layer1];
        // Duplicate the support layer so the unsupported span is ≥2 layers deep
        // (a genuine bridge, not a 1-layer recess the min-depth gate suppresses).
        let dup = layers[0].clone();
        layers.insert(0, dup);
        // Use 1 bottom layer to trigger surface detection
        generate_top_bottom_surfaces(&mut layers, 0, 1, 0.2, 45.0);

        let layer1 = &layers[2];
        let has_bridge = layer1.path_roles.contains(&ExtrusionRole::Bridge);

        assert!(
            has_bridge,
            "unsupported area of layer 1 should be labelled Bridge: roles={:?}",
            layer1.path_roles
        );
        // No BottomSurface on layer 1: the 1mm overlap is fully covered (not a surface)
        // and the rest is Bridge.
        let has_bottom_surface = layer1.path_roles.contains(&ExtrusionRole::BottomSurface);
        assert!(
            !has_bottom_surface,
            "no BottomSurface expected when all unsupported area is Bridge: roles={:?}",
            layer1.path_roles
        );
    }

    /// Test that when the entire bottom surface is fully covered by the previous
    /// layer, no Bridge role is generated (only BottomSurface for the step-down
    /// portion from multi-layer detection, or no surface at all).
    ///
    /// Layout: two identical 10×10 layers.  With bottom_layers=1, the intersection
    /// covers the entire area so region = empty → no surface infill at all.
    #[test]
    fn test_bridge_detection_no_bridge_when_fully_covered() {
        use clipper2::Path;

        // Both layers are identical 10×10 squares.
        let square_path = |z: f64| {
            let mut layer = SliceLayer::new(z);
            let sq: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
            layer.paths.push(sq);
            layer.path_roles.push(ExtrusionRole::OuterWall);
            layer
        };

        let mut layers = vec![square_path(0.2), square_path(0.4)];
        generate_top_bottom_surfaces(&mut layers, 0, 1, 0.2, 45.0);

        // Layer 1 is fully covered by layer 0 → no surface infill whatsoever.
        let has_bridge = layers[1].path_roles.contains(&ExtrusionRole::Bridge);
        let has_bottom = layers[1].path_roles.contains(&ExtrusionRole::BottomSurface);

        assert!(
            !has_bridge,
            "identical layers should produce no Bridge role: roles={:?}",
            layers[1].path_roles
        );
        assert!(
            !has_bottom,
            "identical layers should produce no BottomSurface (fully covered): roles={:?}",
            layers[1].path_roles
        );
    }

    /// Layer 0 (no layer below it) must produce only BottomSurface, never Bridge.
    #[test]
    fn test_bridge_detection_first_layer_is_not_bridge() {
        use clipper2::Path;

        let mut layer0 = SliceLayer::new(0.2);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer0.paths.push(square);
        layer0.path_roles.push(ExtrusionRole::OuterWall);

        let mut layers = vec![layer0];
        generate_top_bottom_surfaces(&mut layers, 0, 1, 0.2, 45.0);

        let has_bridge = layers[0].path_roles.contains(&ExtrusionRole::Bridge);

        assert!(
            !has_bridge,
            "layer 0 (model bottom) must not be classified as Bridge"
        );

        let has_bottom = layers[0].path_roles.contains(&ExtrusionRole::BottomSurface);
        assert!(
            has_bottom,
            "layer 0 (model bottom) must have BottomSurface infill"
        );
    }

    /// Bridge infill paths must store a reduced path width in `path_widths`
    /// equal to `nozzle_diameter_mm × bridge_flow_ratio`.
    #[test]
    fn test_bridge_flow_ratio_stored_in_path_widths() {
        use crate::core::surfaces::{generate_top_bottom_surfaces_with_interior, SurfaceConfig};
        use clipper2::Path;

        // Layer 0: thin 1×10 strip (support)
        let mut layer0 = SliceLayer::new(0.2);
        let strip: Path = vec![(0.0, 0.0), (1.0, 0.0), (1.0, 10.0), (0.0, 10.0)].into();
        layer0.paths.push(strip);
        layer0.path_roles.push(ExtrusionRole::OuterWall);

        // Layer 1: full 10×10 square — 9mm unsupported → Bridge
        let mut layer1 = SliceLayer::new(0.4);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer1.paths.push(square);
        layer1.path_roles.push(ExtrusionRole::OuterWall);

        let nozzle_diameter_mm = 0.4_f64;
        let bridge_flow_ratio = 0.8_f64;
        let expected_width = nozzle_diameter_mm * bridge_flow_ratio; // 0.32 mm

        let mut layers = vec![layer0, layer1];
        // Duplicate the support layer so the bridge spans ≥2 layers (genuine).
        let dup = layers[0].clone();
        layers.insert(0, dup);
        generate_top_bottom_surfaces_with_interior(
            &mut layers,
            &SurfaceConfig {
                top_layers: 0,
                bottom_layers: 1,
                layer_height: 0.2,
                infill_angle: 0.0,
                nozzle_diameter_mm,
                solid_surface_line_width_mm: 0.0,
                bridge_flow_ratio,
                bridge_min_area_mm2: 0.0,
                bridge_noise_filter_mm: 0.0,
                bridge_anchor_mm: 0.0,
                infill_overlap_percent: 0.25,
                ensure_vertical_shell_thickness: false,
                bridge_angle_deg: 0.0,
                top_pattern: SurfacePattern::Rectilinear,
                bottom_pattern: SurfacePattern::Rectilinear,
                internal_solid_pattern: SurfacePattern::Rectilinear,
                min_infill_extrusion_mm: 0.0,
            },
            None,
        );

        // Find all bridge paths and check their stored widths
        let bridge_widths: Vec<Option<f64>> = layers[2]
            .path_roles
            .iter()
            .enumerate()
            .filter(|(_, r)| **r == ExtrusionRole::Bridge)
            .map(|(i, _)| layers[2].path_widths.get(i).copied().flatten())
            .collect();

        assert!(
            !bridge_widths.is_empty(),
            "expected at least one Bridge path in layer 1"
        );
        for w in bridge_widths {
            let w = w.expect("Bridge path must have a stored path_width");
            assert!(
                (w - expected_width).abs() < 1e-9,
                "expected bridge width {expected_width:.4} mm, got {w:.4} mm"
            );
        }
    }

    /// Serpentine infill chaining: for a rectangular region, consecutive scan
    /// lines should be chained into a single continuous path rather than being
    /// returned as individual 2-point segments.  This eliminates travel moves
    /// between infill lines.
    #[test]
    fn test_generate_rectilinear_infill_serpentine_chains() {
        // 10×10 square should produce many scan lines at 0.5mm spacing.
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        let mut contours = Paths::new(vec![]);
        contours.push(square);

        let line_spacing = 0.5_f64;
        let infill = generate_rectilinear_infill(&contours, line_spacing, 0.0, 0.0);

        // The number of scan lines for a 10mm region at 0.5mm spacing is ~20.
        // + 2 accounts for the grid-alignment floor/ceil and the half-spacing
        // end-guard used inside generate_rectilinear_infill.
        let scan_lines_expected = ((10.0 / line_spacing) as usize) + 2;
        assert!(
            !infill.is_empty(),
            "Expected non-empty infill for a 10×10 square"
        );
        // For a convex rectangle, all scan lines should chain into a single
        // serpentine path (or at most 2 if the first/last line is marginal).
        // Assert we're nowhere near N separate line segments.
        assert!(
            infill.len() <= 2,
            "Serpentine chaining should reduce {} scan lines to ≤2 paths, got {}",
            scan_lines_expected,
            infill.len()
        );
        // Every path in the result must have more than 2 points (otherwise
        // it's an unchained 2-point segment, not a serpentine chain).
        for (i, path) in infill.iter().enumerate() {
            assert!(
                path.len() > 2,
                "Path {} has only {} points; expected a multi-point serpentine chain",
                i,
                path.len()
            );
        }
    }

    /// For a region with two disjoint rectangles, the serpentine chaining must
    /// NEVER emit a connector that crosses the void between them.
    ///
    /// The old buggy algorithm used global nearest-X matching: a chain from the
    /// left rectangle could "reach across" to grab a segment from the right
    /// rectangle, creating an extrusion line through the empty gap.
    ///
    /// The fixed algorithm uses sorted-index matching: j-th chain → j-th
    /// segment (both sorted left-to-right), so each chain stays in one island.
    #[test]
    fn test_generate_rectilinear_infill_no_cross_island_connector() {
        use clipper2::Path;

        // Two 3×5 rectangles separated by a 4mm gap (x=3 to x=7 is void).
        let left: Path = vec![(0.0, 0.0), (3.0, 0.0), (3.0, 5.0), (0.0, 5.0)].into();
        let right: Path = vec![(7.0, 0.0), (10.0, 0.0), (10.0, 5.0), (7.0, 5.0)].into();
        let mut contours = Paths::new(vec![]);
        contours.push(left);
        contours.push(right);

        let infill = generate_rectilinear_infill(&contours, 0.5, 0.0, 0.0);
        assert!(!infill.is_empty(), "expected infill to be generated");

        // Every point in the infill must be inside one of the two rectangles.
        // Any point in the gap (x ∈ (3, 7)) is a cross-island connector — a bug.
        let gap_x_lo = 3.0 + 1e-6;
        let gap_x_hi = 7.0 - 1e-6;
        for (pi, path) in infill.iter().enumerate() {
            for pt in path.iter() {
                let x = pt.x();
                assert!(
                    !(x > gap_x_lo && x < gap_x_hi),
                    "infill path {} has a point at x={:.4} which is in the void gap \
                     between the two rectangles — cross-island connector detected",
                    pi,
                    x
                );
            }
        }
    }

    /// Stale-chain regression: a chain that has no matching segment on scan
    /// line N must be closed immediately.  The old algorithm kept it open, and
    /// it could reconnect on scan line N+K, producing a long diagonal extrusion
    /// that "plows through" all the material printed between the two rows.
    #[test]
    fn test_generate_rectilinear_infill_no_stale_chain_jump() {
        use clipper2::Path;

        // A C-shape: full rectangle with a rectangular notch cut from the right
        // side at y=[2, 3], creating a scan line with a single shorter segment
        // in that band, while adjacent scan lines are full-width.
        //
        //  ┌──────────┐
        //  │          │  y=0..2  (full width 0..10)
        //  │   ┌──────┘  y=2..3  (only left half: 0..5)
        //  │   │
        //  │   └──────┐  y=3..5  (full width 0..10)
        //  └──────────┘

        // Full-width rectangle (the outer frame)
        let outer: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 5.0), (0.0, 5.0)].into();
        // Notch cut out of the right side (CW winding = hole)
        let notch: Path = vec![(5.0, 2.0), (5.0, 3.0), (10.0, 3.0), (10.0, 2.0)].into();
        let mut contours = Paths::new(vec![]);
        contours.push(outer);
        contours.push(notch);

        let infill = generate_rectilinear_infill(&contours, 0.5, 0.0, 0.0);
        assert!(!infill.is_empty(), "expected infill to be generated");

        // No infill point should be in the notched-out region (x>5, y∈[2,3]).
        for (pi, path) in infill.iter().enumerate() {
            for pt in path.iter() {
                let x = pt.x();
                let y = pt.y();
                assert!(
                    !(x > 5.0 + 1e-6 && y > 2.0 + 1e-6 && y < 3.0 - 1e-6),
                    "infill path {} has a point ({:.4}, {:.4}) inside the notch void",
                    pi,
                    x,
                    y
                );
            }
        }

        // Additionally, no single infill path should have a Y span larger than
        // the shape height (~5mm) — a stale chain reattaching after many scan
        // lines would create a path spanning the entire Y range.
        for (pi, path) in infill.iter().enumerate() {
            let ys: Vec<f64> = path.iter().map(|pt| pt.y()).collect();
            let y_span = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
                - ys.iter().cloned().fold(f64::INFINITY, f64::min);
            assert!(
                y_span <= 5.5,
                "infill path {} has a Y span of {:.4}mm which suggests a stale-chain jump",
                pi,
                y_span
            );
        }
    }

    /// Infill perimeter gap: with a positive gap, the infill area should be
    /// smaller (fewer infill paths) than with gap=0.
    #[test]
    fn test_infill_perimeter_gap_reduces_infill_area() {
        use crate::infill::InfillPattern;

        let mesh = make_cube_mesh();

        let mut layers_no_gap = slice_mesh(&mesh, 2.0);
        add_infill_to_layers(
            &mut layers_no_gap,
            &InfillConfig {
                density: 0.5,
                pattern: InfillPattern::Rectilinear,
                base_angle_deg: 45.0,
                spacing_mm: 0.4,
                nozzle_diameter_mm: 0.4,
                perimeter_gap_mm: 0.0,
                min_extrusion_mm: 0.0,
                anchor_length_mm: 0.0,
                anchor_length_max_mm: 0.0,
                every_layers: 1,
                combination_max_layer_height_mm: 0.0,
                layer_height_mm: 0.2,
                solid_every_layers: 0,
                solid_spacing_mm: 0.357,
                solid_pattern: SurfacePattern::Monotonic,
            },
            None,
        );

        let mut layers_with_gap = slice_mesh(&mesh, 2.0);
        add_infill_to_layers(
            &mut layers_with_gap,
            &InfillConfig {
                density: 0.5,
                pattern: InfillPattern::Rectilinear,
                base_angle_deg: 45.0,
                spacing_mm: 0.4,
                nozzle_diameter_mm: 0.4,
                perimeter_gap_mm: 0.2, // 0.2 mm gap from walls
                min_extrusion_mm: 0.0,
                anchor_length_mm: 0.0,
                anchor_length_max_mm: 0.0,
                every_layers: 1,
                combination_max_layer_height_mm: 0.0,
                layer_height_mm: 0.2,
                solid_every_layers: 0,
                solid_spacing_mm: 0.357,
                solid_pattern: SurfacePattern::Monotonic,
            },
            None,
        );

        // Count total infill paths across all layers
        let count_infill = |layers: &[SliceLayer]| {
            layers
                .iter()
                .flat_map(|l| l.path_roles.iter())
                .filter(|&&r| r == ExtrusionRole::Infill)
                .count()
        };

        let no_gap_count = count_infill(&layers_no_gap);
        let gap_count = count_infill(&layers_with_gap);

        assert!(
            gap_count <= no_gap_count,
            "Infill with gap ({} paths) should not have more paths than no-gap ({} paths)",
            gap_count,
            no_gap_count
        );
    }

    /// Isolated sparse-infill dashes shorter than `min_infill_extrusion_mm` are
    /// dropped as splats, while the same scene with the filter disabled keeps
    /// them — the "tiny inner-body splat" cleanup.
    #[test]
    fn test_min_infill_extrusion_drops_sparse_splats() {
        use crate::infill::InfillPattern;

        // Build a layer with a large interior so rectilinear infill generates a
        // mix of long lines and — at the rounded corners of the region — short
        // dashes.  A disc gives the diagonal boundary that yields sub-mm crossings.
        let make_disc_layer = || {
            let mut layer = SliceLayer::new(0.2);
            let mut ring: Vec<(f64, f64)> = Vec::new();
            let r = 8.0;
            for k in 0..64 {
                let a = std::f64::consts::TAU * (k as f64) / 64.0;
                ring.push((r * a.cos(), r * a.sin()));
            }
            let disc: clipper2::Path = ring.into();
            layer.paths.push(disc);
            layer.path_roles.push(ExtrusionRole::OuterWall);
            layer.path_widths.push(Some(0.4));
            layer
        };

        let count_short = |layers: &[SliceLayer], max_len: f64| -> usize {
            let mut n = 0;
            for l in layers {
                for (i, p) in l.paths.iter().enumerate() {
                    if l.role_for_path(i) != ExtrusionRole::Infill {
                        continue;
                    }
                    let pts: Vec<(f64, f64)> = p.iter().map(|q| (q.x(), q.y())).collect();
                    let len: f64 = pts
                        .windows(2)
                        .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
                        .sum();
                    if len < max_len {
                        n += 1;
                    }
                }
            }
            n
        };

        let mut unfiltered = vec![make_disc_layer()];
        add_infill_to_layers(
            &mut unfiltered,
            &InfillConfig {
                density: 0.15,
                pattern: InfillPattern::Rectilinear,
                base_angle_deg: 45.0,
                spacing_mm: 0.4,
                nozzle_diameter_mm: 0.4,
                perimeter_gap_mm: 0.0,
                min_extrusion_mm: 0.0, // filter disabled
                anchor_length_mm: 0.0,
                anchor_length_max_mm: 0.0,
                every_layers: 1,
                combination_max_layer_height_mm: 0.0,
                layer_height_mm: 0.2,
                solid_every_layers: 0,
                solid_spacing_mm: 0.357,
                solid_pattern: SurfacePattern::Monotonic,
            },
            None,
        );

        let mut filtered = vec![make_disc_layer()];
        add_infill_to_layers(
            &mut filtered,
            &InfillConfig {
                density: 0.15,
                pattern: InfillPattern::Rectilinear,
                base_angle_deg: 45.0,
                spacing_mm: 0.4,
                nozzle_diameter_mm: 0.4,
                perimeter_gap_mm: 0.0,
                min_extrusion_mm: 0.4, // drop sub-0.4 mm dashes
                anchor_length_mm: 0.0,
                anchor_length_max_mm: 0.0,
                every_layers: 1,
                combination_max_layer_height_mm: 0.0,
                layer_height_mm: 0.2,
                solid_every_layers: 0,
                solid_spacing_mm: 0.357,
                solid_pattern: SurfacePattern::Monotonic,
            },
            None,
        );

        // The filter must leave no sub-0.4 mm infill dash …
        assert_eq!(
            count_short(&filtered, 0.4),
            0,
            "no sub-threshold sparse dash should survive the filter"
        );
        // … and must not add paths; it only ever removes.
        let n_unfiltered: usize = unfiltered.iter().map(|l| l.paths.len()).sum();
        let n_filtered: usize = filtered.iter().map(|l| l.paths.len()).sum();
        assert!(
            n_filtered <= n_unfiltered,
            "filter must not add paths ({n_filtered} > {n_unfiltered})"
        );
    }

    /// A thin wall-to-wall cavity with **no solid surface** on the layer must
    /// keep its full sparse lattice: the solid-region margin is keyed to
    /// `solid_regions`, so it must be an exact no-op here.
    ///
    /// This is the hollow-box case (filament-caddy mid-height layers). An
    /// earlier implementation morphologically *opened* the whole infill area and
    /// erased this lattice outright — its wall-zone void more than doubled —
    /// which the slicing quality gate caught. This test pins that behaviour so
    /// the mistake cannot come back.
    #[test]
    fn test_thin_cavity_without_solid_surface_keeps_its_infill() {
        use crate::infill::InfillPattern;
        use clipper2::Path;

        let count_infill = |layers: &[SliceLayer]| -> usize {
            layers
                .iter()
                .flat_map(|l| l.path_roles.iter())
                .filter(|&&r| r == ExtrusionRole::Infill)
                .count()
        };

        // A tall, narrow cavity between two walls — thin, but genuine: nothing
        // else on the layer covers it, so it must still be filled.
        let mut layer = SliceLayer::new(0.2);
        let outer: Path = vec![(-20.0, -1.2), (20.0, -1.2), (20.0, 1.2), (-20.0, 1.2)].into();
        layer.paths.push(outer);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.path_widths.push(Some(0.4));
        // Deliberately no solid_regions on this layer.
        assert!(layer.solid_regions.is_empty());

        let mut layers = vec![layer];
        add_infill_to_layers(
            &mut layers,
            &InfillConfig {
                density: 0.4,
                pattern: InfillPattern::Rectilinear,
                base_angle_deg: 45.0,
                spacing_mm: 0.4,
                nozzle_diameter_mm: 0.4,
                perimeter_gap_mm: 0.0,
                min_extrusion_mm: 0.0,
                anchor_length_mm: 0.0,
                anchor_length_max_mm: 0.0,
                every_layers: 1,
                combination_max_layer_height_mm: 0.0,
                layer_height_mm: 0.2,
                solid_every_layers: 0,
                solid_spacing_mm: 0.357,
                solid_pattern: SurfacePattern::Monotonic,
            },
            None,
        );
        assert!(
            count_infill(&layers) > 0,
            "a thin cavity with no solid surface must keep its sparse lattice"
        );
    }

    /// A wedge too small to hold more than one isolated dash must get **no**
    /// sparse infill: reaching that dash costs a full retract → travel →
    /// un-retract to deposit a disconnected speck (the Benchy bow tip).
    #[test]
    fn test_splat_sized_region_gets_no_infill() {
        use crate::infill::InfillPattern;
        use clipper2::Path;

        // ~1.6 mm² triangle — the measured size of the Benchy bow-tip sliver,
        // and below the 2.0 mm² floor at a 0.4 mm nozzle.
        let mut layer = SliceLayer::new(0.2);
        let wedge: Path = vec![(0.0, 0.0), (4.0, 0.0), (0.0, 0.8)].into();
        layer.paths.push(wedge);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.path_widths.push(Some(0.4));

        let mut layers = vec![layer];
        add_infill_to_layers(
            &mut layers,
            &InfillConfig {
                density: 0.2,
                pattern: InfillPattern::Rectilinear,
                base_angle_deg: 45.0,
                spacing_mm: 0.4,
                nozzle_diameter_mm: 0.4,
                perimeter_gap_mm: 0.0,
                min_extrusion_mm: 0.0,
                anchor_length_mm: 0.0,
                anchor_length_max_mm: 0.0,
                every_layers: 1,
                combination_max_layer_height_mm: 0.0,
                layer_height_mm: 0.2,
                solid_every_layers: 0,
                solid_spacing_mm: 0.357,
                solid_pattern: SurfacePattern::Monotonic,
            },
            None,
        );
        let n = layers
            .iter()
            .flat_map(|l| l.path_roles.iter())
            .filter(|&&r| r == ExtrusionRole::Infill)
            .count();
        assert_eq!(n, 0, "a splat-sized region must get no sparse infill");
    }

    /// The splat filter is an **area** rule on whole connected regions, not a
    /// width rule: a *large* region that merely happens to be narrow (the
    /// caddy's hollow-box lattice) must keep every line.
    ///
    /// Pairs with [`test_thin_cavity_without_solid_surface_keeps_its_infill`] —
    /// together they pin that "thin" and "tiny" are different things.
    #[test]
    fn test_splat_filter_spares_a_large_but_narrow_region() {
        use crate::infill::InfillPattern;
        use clipper2::Path;

        let count = |layers: &[SliceLayer]| -> usize {
            layers
                .iter()
                .flat_map(|l| l.path_roles.iter())
                .filter(|&&r| r == ExtrusionRole::Infill)
                .count()
        };

        // 40 × 2.4 mm cavity: narrower than many features but ~90 mm² of area,
        // far above the splat floor.
        let mut layer = SliceLayer::new(0.2);
        let outer: Path = vec![(-20.0, -1.2), (20.0, -1.2), (20.0, 1.2), (-20.0, 1.2)].into();
        layer.paths.push(outer);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.path_widths.push(Some(0.4));

        let mut layers = vec![layer];
        add_infill_to_layers(
            &mut layers,
            &InfillConfig {
                density: 0.4,
                pattern: InfillPattern::Rectilinear,
                base_angle_deg: 45.0,
                spacing_mm: 0.4,
                nozzle_diameter_mm: 0.4,
                perimeter_gap_mm: 0.0,
                min_extrusion_mm: 0.0,
                anchor_length_mm: 0.0,
                anchor_length_max_mm: 0.0,
                every_layers: 1,
                combination_max_layer_height_mm: 0.0,
                layer_height_mm: 0.2,
                solid_every_layers: 0,
                solid_spacing_mm: 0.357,
                solid_pattern: SurfacePattern::Monotonic,
            },
            None,
        );
        assert!(
            count(&layers) > 0,
            "a large-but-narrow cavity must keep its lattice — the splat filter \
             is an area rule, not a width rule"
        );
    }

    /// Sparse infill must not be laid in the sub-bead sliver between a solid
    /// surface and the wall: `solid_regions` is grown by
    /// `SOLID_MARGIN_NOZZLE_MULT × nozzle` before being subtracted, so infill
    /// generated against a solid region keeps clear of it.
    #[test]
    fn test_solid_margin_keeps_infill_off_the_surface_sliver() {
        use crate::infill::InfillPattern;
        use clipper2::Path;

        let infill_area_mm = |layers: &[SliceLayer]| -> f64 {
            let mut total = 0.0;
            for l in layers {
                for (i, p) in l.paths.iter().enumerate() {
                    if l.role_for_path(i) != ExtrusionRole::Infill {
                        continue;
                    }
                    let pts: Vec<(f64, f64)> = p.iter().map(|q| (q.x(), q.y())).collect();
                    total += pts
                        .windows(2)
                        .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
                        .sum::<f64>();
                }
            }
            total
        };

        // A 30 mm square whose interior is almost entirely solid surface, save a
        // thin ring left over against the wall — the sliver.
        let build = |with_solid: bool| {
            let mut layer = SliceLayer::new(0.2);
            let sq: Path = vec![(-15.0, -15.0), (15.0, -15.0), (15.0, 15.0), (-15.0, 15.0)].into();
            layer.paths.push(sq);
            layer.path_roles.push(ExtrusionRole::OuterWall);
            layer.path_widths.push(Some(0.4));
            if with_solid {
                let solid: Path =
                    vec![(-13.0, -13.0), (13.0, -13.0), (13.0, 13.0), (-13.0, 13.0)].into();
                layer.solid_regions = Paths::new(vec![solid]);
            }
            let mut layers = vec![layer];
            add_infill_to_layers(
                &mut layers,
                &InfillConfig {
                    density: 0.4,
                    pattern: InfillPattern::Rectilinear,
                    base_angle_deg: 45.0,
                    spacing_mm: 0.4,
                    nozzle_diameter_mm: 0.4,
                    perimeter_gap_mm: 0.0,
                    min_extrusion_mm: 0.0,
                    anchor_length_mm: 0.0,
                    anchor_length_max_mm: 0.0,
                    every_layers: 1,
                    combination_max_layer_height_mm: 0.0,
                    layer_height_mm: 0.2,
                    solid_every_layers: 0,
                    solid_spacing_mm: 0.357,
                    solid_pattern: SurfacePattern::Monotonic,
                },
                None,
            );
            layers
        };

        // With a solid region covering the middle, only the ring outside it can
        // host infill — and the margin must shrink that ring further, so the
        // total is strictly less than the un-margined solid outline would allow.
        let with_solid = infill_area_mm(&build(true));
        let without_solid = infill_area_mm(&build(false));
        assert!(
            with_solid < without_solid,
            "solid region must reduce infill ({with_solid:.1} !< {without_solid:.1})"
        );
    }

    // ── Bridge filter pipeline (issue: noisy bridges & overhang walls) ──────

    /// A tiny sliver of unsupported area (sub-mm) must be reclassified as
    /// `BottomSurface`, not `Bridge`, when `bridge_min_area_mm2` is set.
    #[test]
    fn test_bridge_min_area_filter_reclassifies_tiny_sliver() {
        use crate::core::surfaces::{generate_top_bottom_surfaces_with_interior, SurfaceConfig};
        use clipper2::Path;

        // Layer 0: 10×10 square minus a 0.5×0.5 mm cut-out at the corner (so
        // layer 1 has a 0.25 mm² unsupported sliver). Easiest: make layer 0
        // slightly smaller than layer 1.
        let mut layer0 = SliceLayer::new(0.2);
        // 10×10 minus a 0.5mm corner = covers everything except a 0.5×0.5 corner
        let l_shape: Path = vec![
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 9.5),
            (9.5, 9.5),
            (9.5, 10.0),
            (0.0, 10.0),
        ]
        .into();
        layer0.paths.push(l_shape);
        layer0.path_roles.push(ExtrusionRole::OuterWall);

        // Layer 1: full 10×10 square
        let mut layer1 = SliceLayer::new(0.4);
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer1.paths.push(square);
        layer1.path_roles.push(ExtrusionRole::OuterWall);

        let mut layers = vec![layer0, layer1];

        // Min-area filter at 1.0 mm² is well above the 0.25 mm² sliver.
        // Disable the noise filter / anchor so we isolate the area test.
        generate_top_bottom_surfaces_with_interior(
            &mut layers,
            &SurfaceConfig {
                top_layers: 0,
                bottom_layers: 1,
                layer_height: 0.2,
                infill_angle: 45.0,
                nozzle_diameter_mm: 0.4,
                solid_surface_line_width_mm: 0.0,
                bridge_flow_ratio: 0.8,
                bridge_min_area_mm2: 1.0,
                bridge_noise_filter_mm: 0.0,
                bridge_anchor_mm: 0.0,
                infill_overlap_percent: 0.25,
                ensure_vertical_shell_thickness: false,
                bridge_angle_deg: 0.0,
                top_pattern: SurfacePattern::Rectilinear,
                bottom_pattern: SurfacePattern::Rectilinear,
                internal_solid_pattern: SurfacePattern::Rectilinear,
                min_infill_extrusion_mm: 0.0,
            },
            None,
        );

        let layer1 = &layers[1];
        let has_bridge = layer1.path_roles.contains(&ExtrusionRole::Bridge);
        assert!(
            !has_bridge,
            "0.25mm² sliver must NOT be classified as Bridge with min_area=1.0: roles={:?}",
            layer1.path_roles
        );
    }

    /// The morphological-opening filter must wipe out a thread-thin spur
    /// connecting two larger unsupported regions, while preserving the
    /// larger regions themselves.
    #[test]
    fn test_bridge_noise_filter_removes_thin_spurs() {
        use crate::core::surfaces::{generate_top_bottom_surfaces_with_interior, SurfaceConfig};
        use clipper2::Path;

        // Layer 0: support strip in the middle. Layer 1 covers more →
        // unsupported region is the area outside the strip.
        let mut layer0 = SliceLayer::new(0.2);
        let support: Path = vec![(0.0, 4.95), (10.0, 4.95), (10.0, 5.05), (0.0, 5.05)].into();
        layer0.paths.push(support);
        layer0.path_roles.push(ExtrusionRole::OuterWall);

        // Layer 1: full 10×10 square. Without the support's tiny 0.1mm
        // height, the entire layer 1 would be a big bridge. Verify the
        // opening filter hammers down the thinness and the result is small.
        let mut layer1 = SliceLayer::new(0.4);
        let sq: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer1.paths.push(sq);
        layer1.path_roles.push(ExtrusionRole::OuterWall);

        let mut layers = vec![layer0, layer1];
        // Duplicate the support layer so the two genuine bridge halves span ≥2
        // layers (not a 1-layer recess the min-depth gate suppresses).
        let dup = layers[0].clone();
        layers.insert(0, dup);

        // A 0.5 mm opening filter is easily larger than the 0.1mm-wide
        // sub-pixel artefacts but smaller than the genuine 4.95mm-wide
        // unsupported zones, so the genuine bridges survive.
        generate_top_bottom_surfaces_with_interior(
            &mut layers,
            &SurfaceConfig {
                top_layers: 0,
                bottom_layers: 1,
                layer_height: 0.2,
                infill_angle: 0.0,
                nozzle_diameter_mm: 0.4,
                solid_surface_line_width_mm: 0.0,
                bridge_flow_ratio: 0.8,
                bridge_min_area_mm2: 0.0,
                bridge_noise_filter_mm: 0.5,
                bridge_anchor_mm: 0.0,
                infill_overlap_percent: 0.25,
                ensure_vertical_shell_thickness: false,
                bridge_angle_deg: 0.0,
                top_pattern: SurfacePattern::Rectilinear,
                bottom_pattern: SurfacePattern::Rectilinear,
                internal_solid_pattern: SurfacePattern::Rectilinear,
                min_infill_extrusion_mm: 0.0,
            },
            None,
        );

        // Both genuine bridge halves (≈4.95×10 each = 49.5mm² each) must
        // survive the 0.5mm opening filter.
        let bridge_count = layers[2]
            .path_roles
            .iter()
            .filter(|r| **r == ExtrusionRole::Bridge)
            .count();
        assert!(
            bridge_count > 0,
            "Genuine wide unsupported regions must still be classified as Bridge"
        );
    }

    /// `bridge_anchor_mm` must dilate the unsupported region into the
    /// supported area, increasing the area filled with bridge infill.
    #[test]
    fn test_bridge_anchor_expands_into_supported_area() {
        use crate::core::surfaces::{generate_top_bottom_surfaces_with_interior, SurfaceConfig};
        use clipper2::Path;

        // Layer 0: 1×10 support strip on the left
        let strip: Path = vec![(0.0, 0.0), (1.0, 0.0), (1.0, 10.0), (0.0, 10.0)].into();
        // Layer 1: full 10×10 square (9mm unsupported gap)
        let square: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();

        let mut layers_no_anchor = vec![SliceLayer::new(0.2), SliceLayer::new(0.4)];
        layers_no_anchor[0].paths.push(strip.clone());
        layers_no_anchor[0]
            .path_roles
            .push(ExtrusionRole::OuterWall);
        layers_no_anchor[1].paths.push(square.clone());
        layers_no_anchor[1]
            .path_roles
            .push(ExtrusionRole::OuterWall);

        let mut layers_anchor = vec![SliceLayer::new(0.2), SliceLayer::new(0.4)];
        layers_anchor[0].paths.push(strip);
        layers_anchor[0].path_roles.push(ExtrusionRole::OuterWall);
        layers_anchor[1].paths.push(square);
        layers_anchor[1].path_roles.push(ExtrusionRole::OuterWall);

        let cfg = |anchor| SurfaceConfig {
            top_layers: 0,
            bottom_layers: 1,
            layer_height: 0.2,
            infill_angle: 0.0,
            nozzle_diameter_mm: 0.4,
            solid_surface_line_width_mm: 0.0,
            bridge_flow_ratio: 0.8,
            bridge_min_area_mm2: 0.0,
            bridge_noise_filter_mm: 0.0,
            bridge_anchor_mm: anchor,
            infill_overlap_percent: 0.25,
            ensure_vertical_shell_thickness: false,
            bridge_angle_deg: 0.0,
            top_pattern: SurfacePattern::Rectilinear,
            bottom_pattern: SurfacePattern::Rectilinear,
            internal_solid_pattern: SurfacePattern::Rectilinear,
            min_infill_extrusion_mm: 0.0,
        };

        let dup = layers_no_anchor[0].clone();
        layers_no_anchor.insert(0, dup);
        let dup = layers_anchor[0].clone();
        layers_anchor.insert(0, dup);
        generate_top_bottom_surfaces_with_interior(&mut layers_no_anchor, &cfg(0.0), None);
        generate_top_bottom_surfaces_with_interior(&mut layers_anchor, &cfg(2.0), None);

        // The anchored bridge region must enclose more area than the un-anchored
        // one (it bites 2mm into the supported strip on the left).
        let bridge_area = |layers: &[SliceLayer]| -> f64 {
            // solid_regions includes Bridge by design.
            layers[2].solid_regions.signed_area().abs()
        };
        let no_anchor = bridge_area(&layers_no_anchor);
        let with_anchor = bridge_area(&layers_anchor);
        assert!(
            with_anchor > no_anchor + 1.0,
            "anchor expansion must add at least ~1mm² of bridge area (no_anchor={} with_anchor={})",
            no_anchor,
            with_anchor
        );
    }

    /// **Regression — bridge-in-wall-zone (Benchy porthole / window frame)**
    ///
    /// This is the exact scenario where the original code produced NO bridge
    /// infill even though there is a genuine unsupported horizontal span:
    ///
    /// - `interior_regions[i]` is the cabin interior — a small 2×2 area
    ///   centred in a large 10×10 hull cross-section.
    /// - The bridge area (the 2×2 hole closure at the top of the porthole)
    ///   is in the hull **wall zone** — it does NOT overlap the cabin interior.
    ///
    /// The previous code did:
    ///   `region = intersect(region, interior_regions[i])` → EMPTY (no overlap)
    ///   then returned (empty, empty) → zero infill generated.
    ///
    /// The fix: interior_regions clipping is applied ONLY to `supported`
    /// (solid bottom infill), NOT to `region` before the bridge/bottom split.
    /// Also, `anchor_bounds = perimeters[i]` (not interior_regions[i]) so
    /// `expand_to_anchor` can see the surrounding wall material.
    #[test]
    fn test_bridge_detected_when_bridge_area_is_in_wall_zone() {
        use crate::core::surfaces::{generate_top_bottom_surfaces_with_interior, SurfaceConfig};
        use clipper2::Path;

        // Layer 0: 10×10 hull ring with a 2×2 porthole in the centre.
        //   Outer hull boundary (CCW): (0,0)→(10,0)→(10,10)→(0,10)
        //   Porthole hole (CW): (4,4)→(4,6)→(6,6)→(6,4)  (hole in hull).
        //   The porthole is centred: 4mm offset on each side of the 10mm hull.
        let mut layer0 = SliceLayer::new(0.2);
        let hull0: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        let hole0: Path = vec![(4.0, 4.0), (4.0, 6.0), (6.0, 6.0), (6.0, 4.0)].into();
        layer0.paths.push(hull0);
        layer0.paths.push(hole0);
        layer0.path_roles.push(ExtrusionRole::OuterWall);
        layer0.path_roles.push(ExtrusionRole::OuterWall);

        // Layer 1: solid 10×10 hull (porthole closed at this layer).
        let mut layer1 = SliceLayer::new(0.4);
        let hull1: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer1.paths.push(hull1);
        layer1.path_roles.push(ExtrusionRole::OuterWall);

        let mut layers = vec![layer0, layer1];

        // interior_regions[1] = the cabin interior = 2×2 square at (4,4)→(6,6).
        // This is the same area as the porthole hole but represents the
        // imaginary cabin inside — it deliberately does NOT overlap the bridge
        // area (which is the 2×2 porthole closure sitting in the hull-wall zone).
        let interior_layer0 = clipper2::Paths::new(vec![]);
        let interior_layer1: clipper2::Paths =
            clipper2::Paths::new(vec![clipper2::Path::from(vec![
                (4.0, 4.0),
                (4.0, 6.0),
                (6.0, 6.0),
                (6.0, 4.0_f64),
            ])]);
        let mut interior_regions = vec![interior_layer0, interior_layer1];
        // Deepen the porthole to ≥2 layers (duplicate the holed layer and its
        // interior) so the closure is a genuine bridge, not a 1-layer recess.
        let dup = layers[0].clone();
        layers.insert(0, dup);
        let idup = interior_regions[0].clone();
        interior_regions.insert(0, idup);

        generate_top_bottom_surfaces_with_interior(
            &mut layers,
            &SurfaceConfig {
                top_layers: 0,
                bottom_layers: 1,
                layer_height: 0.2,
                infill_angle: 45.0,
                nozzle_diameter_mm: 0.4,
                solid_surface_line_width_mm: 0.0,
                bridge_flow_ratio: 0.8,
                bridge_min_area_mm2: 0.0, // disable area filter so small porthole still passes
                bridge_noise_filter_mm: 0.0, // disable noise filter
                bridge_anchor_mm: 0.0,    // disable anchor so result area is minimal / predictable
                infill_overlap_percent: 0.25,
                ensure_vertical_shell_thickness: false,
                bridge_angle_deg: 0.0,
                top_pattern: SurfacePattern::Rectilinear,
                bottom_pattern: SurfacePattern::Rectilinear,
                internal_solid_pattern: SurfacePattern::Rectilinear,
                min_infill_extrusion_mm: 0.0,
            },
            Some(&interior_regions),
        );

        let has_bridge = layers[2].path_roles.contains(&ExtrusionRole::Bridge);
        assert!(
            has_bridge,
            "Porthole closure (bridge in wall zone) must be detected as Bridge even when \
             interior_regions does not overlap the bridge area. roles={:?}",
            layers[2].path_roles
        );
    }

    /// **Regression** — a hull that grows slightly outward (step < d/2) must
    /// NOT produce bridge infill in the wall zone.  Before the `+d/2` inflate
    /// fix, `difference(region, prev_perimeter)` returned the thin new-area
    /// strip as "unsupported" even though it is within the previous bead's
    /// physical extent.
    #[test]
    fn test_no_false_bridge_for_thin_outward_lean() {
        use crate::core::surfaces::{generate_top_bottom_surfaces_with_interior, SurfaceConfig};
        use clipper2::Path;

        // Layer 0: solid 10×10 hull.
        let mut layer0 = SliceLayer::new(0.2);
        let hull0: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer0.paths.push(hull0);
        layer0.path_roles.push(ExtrusionRole::OuterWall);

        // Layer 1: hull grown by 0.05 mm on every side (step = 0.05 mm < d/2 = 0.2 mm).
        // The new thin annular strip IS physically supported by the previous bead.
        let mut layer1 = SliceLayer::new(0.2);
        let hull1: Path = vec![
            (-0.05, -0.05),
            (10.05, -0.05),
            (10.05, 10.05),
            (-0.05, 10.05),
        ]
        .into();
        layer1.paths.push(hull1);
        layer1.path_roles.push(ExtrusionRole::OuterWall);

        let mut layers = vec![layer0, layer1];

        // Disable area and noise filters so only the d/2 inflate guard can
        // suppress the false bridge.
        generate_top_bottom_surfaces_with_interior(
            &mut layers,
            &SurfaceConfig {
                top_layers: 0,
                bottom_layers: 1,
                layer_height: 0.2,
                infill_angle: 45.0,
                nozzle_diameter_mm: 0.4,
                solid_surface_line_width_mm: 0.0,
                bridge_flow_ratio: 0.8,
                bridge_min_area_mm2: 0.0,
                bridge_noise_filter_mm: 0.0,
                bridge_anchor_mm: 0.0,
                infill_overlap_percent: 0.25,
                ensure_vertical_shell_thickness: false,
                bridge_angle_deg: 0.0,
                top_pattern: SurfacePattern::Rectilinear,
                bottom_pattern: SurfacePattern::Rectilinear,
                internal_solid_pattern: SurfacePattern::Rectilinear,
                min_infill_extrusion_mm: 0.0,
            },
            None,
        );

        let has_bridge = layers[1].path_roles.contains(&ExtrusionRole::Bridge);
        assert!(
            !has_bridge,
            "Thin outward lean (step < d/2) must NOT produce bridge infill in the wall zone. \
             roles={:?}",
            layers[1].path_roles
        );
    }

    /// **Regression** — a closed wall loop that crosses over an air gap on
    /// only one side must be **split**: the in-air sub-segment gets
    /// `OverhangPerimeter`; the supported sub-segment keeps the original
    /// `OuterWall` role.  The whole-path 50%-threshold would never flag this.
    #[test]
    fn test_classify_overhang_perimeters_splits_mixed_path() {
        use super::walls::classify_overhang_perimeters;
        use clipper2::Path;

        // 10×10 outer wall square.  Air region: a 10×3 strip covering the top
        // side (y ∈ [8, 11]).  Two of the four vertices sit in this strip.
        let wall: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        let mut layer = SliceLayer::new(0.4);
        layer.paths.push(wall);
        layer.path_roles.push(ExtrusionRole::OuterWall);

        let air: Path = vec![(0.0, 8.0), (10.0, 8.0), (10.0, 11.0), (0.0, 11.0)].into();
        layer.unsupported_regions = Paths::new(vec![air]);

        let mut layers = vec![layer];
        classify_overhang_perimeters(&mut layers, 0.4, None);

        // After splitting there must be at least two separate paths.
        let path_count = layers[0].paths.iter().count();
        assert!(
            path_count >= 2,
            "Mixed path must be split into at least 2 sub-segments; got {path_count}"
        );
        // One segment must carry OverhangPerimeter (the top-side crossing).
        assert!(
            layers[0]
                .path_roles
                .contains(&ExtrusionRole::OverhangPerimeter),
            "Split must produce at least one OverhangPerimeter segment. \
             roles={:?}",
            layers[0].path_roles
        );
        // The remaining segment(s) must keep OuterWall.
        assert!(
            layers[0].path_roles.contains(&ExtrusionRole::OuterWall),
            "Split must keep at least one OuterWall segment. roles={:?}",
            layers[0].path_roles
        );
    }

    /// **Regression** — on the bridge *closing* layer the outer hull path must
    /// not survive as an `OverhangPerimeter` arc inside the bridge zone.
    ///
    /// Geometry:
    /// * Layer 0: 10×10 hull ring with a 2×2 porthole hole (the porthole is
    ///   *open* on this layer, so the next layer must bridge across it).
    /// * Layer 1: solid 10×10 hull (the porthole *closes* here — this is the
    ///   bridge layer).
    ///
    /// On layer 1 the bridge region covers the porthole void + anchor_mm strip.
    /// Any outer hull path segments that run along the bridge zone outer
    /// boundary would — before this fix — survive `clip_walls_against_bridge_region`
    /// (IsOn = outside in the old strict test), then be classified as
    /// `OverhangPerimeter`, and finally be extruded again when bridge infill
    /// printed on top: double-extrusion.
    ///
    /// After the fix (IsOn = inside), those boundary segments are removed and
    /// no `OverhangPerimeter` paths appear at the bridge layer.
    #[test]
    fn test_bridge_closing_layer_has_no_overhang_in_bridge_zone() {
        use crate::core::surfaces::{generate_top_bottom_surfaces_with_interior, SurfaceConfig};
        use crate::core::walls::classify_overhang_perimeters;
        use clipper2::Path;

        // Layer 0: 10×10 hull (CCW outer) with a 2×2 porthole hole (CW inner).
        // Porthole at (4,4)-(6,6).
        let mut layer0 = SliceLayer::new(0.2);
        let hull0: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        // CW winding for the hole
        let hole0: Path = vec![(4.0, 4.0), (4.0, 6.0), (6.0, 6.0), (6.0, 4.0)].into();
        layer0.paths.push(hull0);
        layer0.paths.push(hole0);
        layer0.path_roles.push(ExtrusionRole::OuterWall);
        layer0.path_roles.push(ExtrusionRole::OuterWall);

        // Layer 1: solid 10×10 hull — this is the bridge (closing) layer.
        let mut layer1 = SliceLayer::new(0.4);
        let hull1: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        layer1.paths.push(hull1);
        layer1.path_roles.push(ExtrusionRole::OuterWall);

        let mut layers = vec![layer0, layer1];
        // Deepen the porthole to ≥2 layers so the closure is a genuine bridge.
        let dup = layers[0].clone();
        layers.insert(0, dup);

        generate_top_bottom_surfaces_with_interior(
            &mut layers,
            &SurfaceConfig {
                top_layers: 0,
                bottom_layers: 1,
                layer_height: 0.2,
                infill_angle: 45.0,
                nozzle_diameter_mm: 0.4,
                solid_surface_line_width_mm: 0.0,
                bridge_flow_ratio: 0.8,
                bridge_min_area_mm2: 0.0,
                bridge_noise_filter_mm: 0.0,
                bridge_anchor_mm: 0.4,
                infill_overlap_percent: 0.25,
                ensure_vertical_shell_thickness: false,
                bridge_angle_deg: 0.0,
                top_pattern: SurfacePattern::Rectilinear,
                bottom_pattern: SurfacePattern::Rectilinear,
                internal_solid_pattern: SurfacePattern::Rectilinear,
                min_infill_extrusion_mm: 0.0,
            },
            None,
        );

        // Bridge infill must have been generated on the closing layer.
        assert!(
            layers[2].path_roles.contains(&ExtrusionRole::Bridge),
            "Bridge must be detected at the porthole closing layer; \
             roles={:?}",
            layers[2].path_roles
        );

        // Now classify overhang perimeters (uses unsupported_regions set above).
        classify_overhang_perimeters(&mut layers, 0.4, None);

        // After clipping and overhang classification, no OuterWall or InnerWall
        // paths that were *inside the bridge zone* should carry OverhangPerimeter.
        // The fix ensures that wall segments running along the bridge zone outer
        // boundary are removed by clip_walls_against_bridge_region so they cannot
        // be reclassified as OverhangPerimeter and double-extruded by the bridge.
        //
        // For this simple geometry the outer hull vertices are far from the
        // porthole bridge zone, so no OverhangPerimeter should appear on the
        // closing layer at all.
        assert!(
            !layers[2]
                .path_roles
                .contains(&ExtrusionRole::OverhangPerimeter),
            "No OverhangPerimeter must exist on the bridge closing layer; \
             double-extrusion with bridge infill would result. \
             roles={:?}",
            layers[2].path_roles
        );
    }

    /// **Regression** — wall paths whose vertices fall inside the bridge zone
    /// must never end up as `OverhangPerimeter` arcs that geometrically
    /// overlap the bridge infill.
    ///
    /// This is the failure mode that produced double extrusion on Benchy
    /// layer 172 (overhang perimeter + bridge printed in the same space).
    /// The wall vertices land *inside* the bridge zone (which extends an
    /// `bridge_anchor_mm` strip into the supported wall material), so
    /// `clip_walls_against_bridge_region` either drops the path entirely
    /// or keeps an open arc whose seam vertex still sits inside the bridge
    /// zone.  If `unsupported_regions` were the *raw* unsupported area
    /// (without subtracting `bridge_region`), that seam vertex would test
    /// `IsInside` and `classify_overhang_perimeters` would emit a tiny
    /// `OverhangPerimeter` arc inside the bridge — overlapping the bridge
    /// extrusion.
    ///
    /// The fix subtracts `bridge_region` from `unsupported_regions` before
    /// stashing it on the layer, so areas already handled by bridge cannot
    /// be re-flagged as overhang.
    #[test]
    fn test_no_overhang_in_bridge_zone_with_walls_inside_bridge() {
        use crate::core::surfaces::{generate_top_bottom_surfaces_with_interior, SurfaceConfig};
        use crate::core::walls::classify_overhang_perimeters;
        use clipper2::Path;

        // Layer 0: 10×10 hull with a large 6×6 hole — the next layer must
        // bridge across most of the cross-section.
        let mut layer0 = SliceLayer::new(0.2);
        let hull0: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        let hole0: Path = vec![(2.0, 2.0), (2.0, 8.0), (8.0, 8.0), (8.0, 2.0)].into();
        layer0.paths.push(hull0);
        layer0.paths.push(hole0);
        layer0.path_roles.push(ExtrusionRole::OuterWall);
        layer0.path_roles.push(ExtrusionRole::OuterWall);

        // Layer 1: same outer hull plus an inner wall ring that sits
        // partially inside the bridge zone (the 6×6 hole + anchor strip).
        // The 4×4 inner ring's vertices land inside the anchor-expanded
        // bridge_region, so they must not be re-flagged as OverhangPerimeter.
        let mut layer1 = SliceLayer::new(0.4);
        let hull1: Path = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into();
        let inner1: Path = vec![(3.0, 3.0), (3.0, 7.0), (7.0, 7.0), (7.0, 3.0)].into();
        layer1.paths.push(hull1);
        layer1.paths.push(inner1);
        layer1.path_roles.push(ExtrusionRole::OuterWall);
        layer1.path_roles.push(ExtrusionRole::InnerWall);

        let mut layers = vec![layer0, layer1];
        // Deepen the hole to ≥2 layers so the closure is a genuine bridge.
        let dup = layers[0].clone();
        layers.insert(0, dup);

        generate_top_bottom_surfaces_with_interior(
            &mut layers,
            &SurfaceConfig {
                top_layers: 0,
                bottom_layers: 1,
                layer_height: 0.2,
                infill_angle: 45.0,
                nozzle_diameter_mm: 0.4,
                solid_surface_line_width_mm: 0.0,
                bridge_flow_ratio: 0.8,
                bridge_min_area_mm2: 0.0,
                bridge_noise_filter_mm: 0.0,
                bridge_anchor_mm: 0.5,
                infill_overlap_percent: 0.25,
                ensure_vertical_shell_thickness: false,
                bridge_angle_deg: 0.0,
                top_pattern: SurfacePattern::Rectilinear,
                bottom_pattern: SurfacePattern::Rectilinear,
                internal_solid_pattern: SurfacePattern::Rectilinear,
                min_infill_extrusion_mm: 0.0,
            },
            None,
        );

        assert!(
            layers[2].path_roles.contains(&ExtrusionRole::Bridge),
            "Bridge must be detected at the hole closing layer; roles={:?}",
            layers[2].path_roles
        );

        classify_overhang_perimeters(&mut layers, 0.4, None);

        // CRITICAL: no OverhangPerimeter on the bridge layer.  Any such arc
        // would overlap the bridge infill and produce double extrusion.
        assert!(
            !layers[2]
                .path_roles
                .contains(&ExtrusionRole::OverhangPerimeter),
            "OverhangPerimeter must not coexist with Bridge in the same zone; \
             this is the Benchy layer-172 double-extrusion regression. \
             roles={:?}",
            layers[2].path_roles
        );
    }

    /// **Regression** — Benchy layer 172 (rear deck overhang).
    ///
    /// Geometry: an overhanging strip whose width is fully consumed by wall
    /// material (multiple Arachne walls + outer perimeters).  The unsupported
    /// area exists, but every square millimetre of it is *physically covered*
    /// by perimeter beads — there is no genuine void to bridge.
    ///
    /// On the user-reported Benchy slice this produced the failure mode:
    ///   • Bridge filled the entire strip (X∈[-4.4,6], Y∈[-12.31,-10.4]).
    ///   • Outer + inner perimeters were *also* printed in the same strip.
    ///   • Bridge infill ran on top of perimeters → double extrusion.
    ///
    /// Fix: when `interior_regions[i]` is empty for the strip (= perimeters
    /// fully cover the cross-section, no infill area), the bridge candidate
    /// is clipped to nothing → no Bridge role emitted.  The perimeters in the
    /// strip are then classified as `OverhangPerimeter` by
    /// `classify_overhang_perimeters` and printed with bridge speed/flow.
    ///
    /// We provide `interior_regions` explicitly here (mirroring what
    /// `pipeline.rs` would compute via `calculate_interior_region`) so the
    /// test exercises the actual `clip_to_void` step.
    #[test]
    fn test_thin_overhang_strip_no_bridge_when_perimeters_cover_it() {
        use crate::core::surfaces::{generate_top_bottom_surfaces_with_interior, SurfaceConfig};
        use clipper2::Path;

        // Layer 0: small support stub at the bottom-back of the strip — a
        // 10×0.4 mm anchor at Y∈[-12.31,-11.91].  This means that on layer 1
        // most of the strip Y∈[-11.91,-10.4] is unsupported (overhang).
        let mut layer0 = SliceLayer::new(0.2);
        let support: Path =
            vec![(-5.0, -12.31), (5.0, -12.31), (5.0, -11.91), (-5.0, -11.91)].into();
        layer0.paths.push(support);
        layer0.path_roles.push(ExtrusionRole::OuterWall);

        // Layer 1: the deck strip — 10×1.91 mm, fully covered by perimeters.
        // We model this with a single OuterWall outline; pipeline-equivalent
        // wisdom is that 1.91 mm is too thin for any infill area, so
        // interior_regions[1] is empty.
        let mut layer1 = SliceLayer::new(0.2);
        let deck: Path = vec![(-5.0, -12.31), (5.0, -12.31), (5.0, -10.4), (-5.0, -10.4)].into();
        layer1.paths.push(deck);
        layer1.path_roles.push(ExtrusionRole::OuterWall);

        let mut layers = vec![layer0, layer1];

        // Empty interior_regions[1] models "no infill area on this layer"
        // (perimeters consume the entire cross-section).  This is what
        // `calculate_interior_region` produces when the wall band is wider
        // than the cross-section.
        let interior_regions = vec![Paths::new(vec![]), Paths::new(vec![])];

        generate_top_bottom_surfaces_with_interior(
            &mut layers,
            &SurfaceConfig {
                top_layers: 0,
                bottom_layers: 1,
                layer_height: 0.2,
                infill_angle: 45.0,
                nozzle_diameter_mm: 0.4,
                solid_surface_line_width_mm: 0.0,
                bridge_flow_ratio: 0.8,
                bridge_min_area_mm2: 0.0,
                bridge_noise_filter_mm: 0.0,
                bridge_anchor_mm: 0.5,
                infill_overlap_percent: 0.25,
                ensure_vertical_shell_thickness: false,
                bridge_angle_deg: 0.0,
                top_pattern: SurfacePattern::Rectilinear,
                bottom_pattern: SurfacePattern::Rectilinear,
                internal_solid_pattern: SurfacePattern::Rectilinear,
                min_infill_extrusion_mm: 0.0,
            },
            Some(&interior_regions),
        );

        // The whole point: NO Bridge must be generated when the strip has no
        // infill area.  Perimeters alone cover it; bridge infill would
        // double-extrude on top.
        assert!(
            !layers[1].path_roles.contains(&ExtrusionRole::Bridge),
            "Bridge must NOT be generated for an overhang strip whose entire \
             cross-section is consumed by perimeters (interior_regions empty). \
             This is the Benchy layer-172 deck-strip regression. roles={:?}",
            layers[1].path_roles
        );
    }

    /// Regression for the "bridges extrude into walls" defect on the Benchy
    /// hull-side deck edge and sloped cabin front (Arachne layers ≈ 159–172).
    ///
    /// Unlike `test_thin_overhang_strip_no_bridge_when_perimeters_cover_it`
    /// (where `interior_regions` is *empty*), here the interior estimate is
    /// **non-empty but a thin (< 1 mm) channel** — exactly what Arachne's
    /// per-island **average** `walls_per_island` leaves in a locally-thin
    /// cross-section.  The old code fired a bridge there (interior non-empty),
    /// laying sparse bridge lines straight over the wall + gap-fill beads that
    /// already fill the channel (measured "Gap infill × Bridge" and "Inner
    /// wall × Bridge" double-extrusion, absent from the `classic` reference).
    ///
    /// The fix clips the bridge candidate to the **morphologically-opened**
    /// interior (`open_interior_for_surface`), so a candidate that lands
    /// entirely inside a sub-`SURFACE_MIN_INTERIOR_WIDTH_NOZZLE_MULT × nozzle`
    /// channel disappears — while a genuine bridge over a *wide* void (cabin
    /// roof, porthole / window closure) keeps its full extent.  This test
    /// asserts both halves of that discriminator on identical geometry that
    /// differs only in interior width.
    #[test]
    fn test_no_bridge_in_thin_interior_channel_but_yes_in_wide_void() {
        use crate::core::surfaces::{generate_top_bottom_surfaces_with_interior, SurfaceConfig};
        use clipper2::Path;

        // Runs the surface pass on a 3-layer stack whose top layer (i == 2)
        // overhangs a support that is identical on the two layers below (so the
        // overhang survives the min-depth `deepen` gate), with the supplied
        // interior region for that top layer.  Returns whether a Bridge role
        // was emitted on the top layer.
        let run_with_interior = |interior_top: Paths| -> bool {
            // Support footprint present on layers 0 and 1: 10 × 1 mm at Y∈[0,1].
            let support: Path = vec![(-5.0, 0.0), (5.0, 0.0), (5.0, 1.0), (-5.0, 1.0)].into();
            let mut layer0 = SliceLayer::new(0.2);
            layer0.paths.push(support.clone());
            layer0.path_roles.push(ExtrusionRole::OuterWall);
            let mut layer1 = SliceLayer::new(0.4);
            layer1.paths.push(support);
            layer1.path_roles.push(ExtrusionRole::OuterWall);

            // Top layer 2: strip extends to Y = 3, so Y∈[1,3] overhangs the
            // support below and is still unsupported two layers down → a bridge
            // candidate forms and passes the `deepen` gate.
            let mut layer2 = SliceLayer::new(0.6);
            let strip: Path = vec![(-5.0, 0.0), (5.0, 0.0), (5.0, 3.0), (-5.0, 3.0)].into();
            layer2.paths.push(strip);
            layer2.path_roles.push(ExtrusionRole::OuterWall);

            let mut layers = vec![layer0, layer1, layer2];
            let interior_regions = vec![Paths::new(vec![]), Paths::new(vec![]), interior_top];

            generate_top_bottom_surfaces_with_interior(
                &mut layers,
                &SurfaceConfig {
                    top_layers: 0,
                    bottom_layers: 1,
                    layer_height: 0.2,
                    infill_angle: 45.0,
                    nozzle_diameter_mm: 0.4,
                    solid_surface_line_width_mm: 0.0,
                    bridge_flow_ratio: 0.8,
                    // Keep the noise/area filters off so the ONLY thing that can
                    // suppress the bridge is the opened-interior clip under test.
                    bridge_min_area_mm2: 0.0,
                    bridge_noise_filter_mm: 0.0,
                    bridge_anchor_mm: 0.5,
                    infill_overlap_percent: 0.25,
                    ensure_vertical_shell_thickness: false,
                    bridge_angle_deg: 0.0,
                    top_pattern: SurfacePattern::Rectilinear,
                    bottom_pattern: SurfacePattern::Rectilinear,
                    internal_solid_pattern: SurfacePattern::Rectilinear,
                    min_infill_extrusion_mm: 0.0,
                },
                Some(&interior_regions),
            );

            layers[2].path_roles.contains(&ExtrusionRole::Bridge)
        };

        // Thin channel: 8 mm long × 0.6 mm wide (< 1.0 mm = 2.5 × 0.4 nozzle).
        // Morphological opening erodes it to nothing → NO bridge.
        let thin_path: clipper2::Path =
            vec![(-4.0, 1.7), (4.0, 1.7), (4.0, 2.3), (-4.0, 2.3)].into();
        assert!(
            !run_with_interior(Paths::new(vec![thin_path])),
            "Bridge must NOT be generated when the interior estimate is a thin \
             (< 1 mm) wall-band channel — the wall + gap-fill beads already fill \
             it, and a bridge would double-extrude over them (Benchy hull-side / \
             sloped-front regression)."
        );

        // Wide void: 8 mm long × 2 mm wide (well above the opening threshold).
        // The opening keeps it → a genuine bridge IS generated.
        let wide_path: clipper2::Path =
            vec![(-4.0, 0.5), (4.0, 0.5), (4.0, 2.5), (-4.0, 2.5)].into();
        assert!(
            run_with_interior(Paths::new(vec![wide_path])),
            "Bridge MUST still be generated over a genuine wide void (cabin roof \
             / porthole / window closure); the opened-interior clip must not \
             suppress real bridges."
        );
    }
    /// The pass is off by default, so a default slice must be untouched by it —
    /// this is what keeps the whole QA baseline corpus stable.
    #[test]
    fn default_params_slice_identically_with_and_without_the_compensation_pass() {
        let mesh = make_cube_mesh();
        let params = SlicingParams::default();

        assert!(
            CompensationConfig::resolve(&params).is_none(),
            "default params must resolve to no compensation at all"
        );

        // Running the pass explicitly over raw contours must change nothing,
        // so every stage downstream sees exactly what it saw before.
        let untouched = slice_mesh(&mesh, params.layer_height);
        let mut passed_through = untouched.clone();
        apply_dimensional_compensation(&mut passed_through, &params);

        assert_eq!(passed_through.len(), untouched.len());
        for (i, (after, before)) in passed_through.iter().zip(&untouched).enumerate() {
            assert_eq!(after.paths, before.paths, "layer {i} contours moved");
            assert_eq!(after.path_roles, before.path_roles, "layer {i} roles moved");
        }
    }

    /// Elephant-foot compensation must reach the finished walls, not just the
    /// contours — the first layer's outer wall has to move in with them.
    #[test]
    fn elephant_foot_moves_the_first_layers_outer_wall_inward() {
        let mesh = make_cube_mesh();
        let mut params = SlicingParams {
            nozzle_diameter_mm: 0.4,
            ..SlicingParams::default()
        };

        let plain = process_mesh(&mesh, &params, &crate::logging::NullLogger);
        params.elephant_foot_compensation_mm = 0.2;
        let corrected = process_mesh(&mesh, &params, &crate::logging::NullLogger);

        let extent = |layers: &[SliceLayer], i: usize| {
            let bounds = layers[i].paths.bounds();
            bounds.max.x() - bounds.min.x()
        };

        let shrunk = extent(&plain, 0) - extent(&corrected, 0);
        assert!(
            (shrunk - 0.4).abs() < 0.05,
            "the first layer should lose 0.2 mm per side, lost {shrunk} mm across"
        );

        let above = extent(&plain, 3) - extent(&corrected, 3);
        assert!(
            above.abs() < 1e-6,
            "layers off the bed must be identical, differ by {above} mm"
        );
    }
}
