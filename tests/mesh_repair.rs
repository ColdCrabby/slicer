//! Mesh repair regression gate.
//!
//! Two halves, both required by [issue #114](https://github.com/ColdCrabby/slicer/issues/114):
//!
//! 1. **A known-bad corpus** — `tests/fixtures/broken/` holds one 10 mm cube
//!    per defect class, so a failure names the repair step that broke.
//!    Regenerate with `python3 tests/fixtures/broken/generate.py`.
//! 2. **The no-op contract** — every mesh already in the repository must be
//!    reported clean and returned *borrowed*, which is what keeps the slicing
//!    quality baselines from drifting when repair is on by default.

use std::path::{Path, PathBuf};

use slicer_engine::core::slice_mesh;
use slicer_engine::mesh::repair::{analyze, repair, RepairOptions};
use slicer_engine::mesh::types::Mesh;
use slicer_engine::scene::{load_path, load_path_multi_reporting, load_path_reporting};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn broken(name: &str) -> PathBuf {
    crate_root().join("tests/fixtures/broken").join(name)
}

/// Load a fixture without letting the repair pass touch it.
fn load_raw(path: &Path) -> Mesh {
    load_path_reporting(path, &RepairOptions::analysis_only())
        .unwrap_or_else(|e| panic!("load {}: {e}", path.display()))
        .0
}

/// Load a fixture through the normal (repairing) path.
fn load_repaired(path: &Path) -> Mesh {
    load_path(path).unwrap_or_else(|e| panic!("load {}: {e}", path.display()))
}

/// Signed volume of a closed mesh; positive when the normals point outward.
fn signed_volume(mesh: &Mesh) -> f64 {
    mesh.faces
        .iter()
        .map(|f| {
            let [a, b, c] = f.vertices;
            (a.x * (b.y * c.z - b.z * c.y)
                + a.y * (b.z * c.x - b.x * c.z)
                + a.z * (b.x * c.y - b.y * c.x))
                / 6.0
        })
        .sum()
}

/// Every defective fixture must come out watertight, manifold, outward-facing,
/// and shaped like the 1000 mm³ cube it is meant to be.
fn assert_repaired_to_a_cube(name: &str) {
    let path = broken(name);
    let mesh = load_repaired(&path);
    let after = analyze(&mesh);

    assert!(
        after.is_clean(),
        "{name}: not clean after repair: {after:?}"
    );
    assert!(after.is_watertight(), "{name}: still has holes");
    assert!(after.is_manifold(), "{name}: still non-manifold");
    assert_eq!(after.shells, 1, "{name}: expected a single shell");
    assert!(
        signed_volume(&mesh) > 0.0,
        "{name}: normals still point inward"
    );
    // Hole capping adds a centroid vertex, so the volume is only exact for the
    // fixtures whose defect does not remove geometry.
    assert!(
        signed_volume(&mesh) > 900.0 && signed_volume(&mesh) < 1100.0,
        "{name}: volume {} is not cube-shaped",
        signed_volume(&mesh)
    );
}

/// A repaired fixture must still slice — the whole point of the pass.
fn assert_slices(name: &str) {
    let mesh = load_repaired(&broken(name));
    let layers = slice_mesh(&mesh, 0.2);
    assert!(!layers.is_empty(), "{name}: sliced to no layers");
    assert!(
        layers.iter().any(|l| !l.paths.is_empty()),
        "{name}: every layer came out empty"
    );
}

#[test]
fn a_zero_area_slit_is_reported_but_never_patched() {
    // Regression (found by importing a real Benchy export in the browser): a
    // T-junction leaves three collinear half-edges that read as a boundary
    // loop but enclose no area. Patching it could only add zero-area triangles
    // — which close nothing, since `diagnose` excludes degenerates from the
    // edge graph — so the model would be reported as permanently defective
    // *and* gain junk geometry.
    let path = broken("cube-tjunction.stl");
    let raw = load_raw(&path);
    let before = analyze(&raw);

    assert_eq!(before.slit_boundary_edges, 3, "the T-junction rim");
    assert_eq!(before.holes, 0, "a zero-area loop is not a hole");
    assert_eq!(before.boundary_edges, 0);
    assert!(before.is_watertight(), "a slit encloses nothing");
    assert!(before.is_clean());

    let (_, report) =
        load_path_reporting(&path, &RepairOptions::default()).expect("load cube-tjunction.stl");
    assert!(!report.repaired, "nothing to fix");
    assert_eq!(report.actions.added_fill_triangles, 0);
    assert!(
        !report.is_noteworthy(),
        "must not warn the user about a slit"
    );

    assert_slices("cube-tjunction.stl");
}

#[test]
fn hole_is_detected_and_capped() {
    let raw = load_raw(&broken("cube-hole.stl"));
    let before = analyze(&raw);
    assert_eq!(before.holes, 1);
    assert_eq!(before.boundary_edges, 4);
    assert_eq!(before.largest_hole_edges, 4);
    assert!(!before.is_watertight());

    assert_repaired_to_a_cube("cube-hole.stl");
    assert_slices("cube-hole.stl");
}

#[test]
fn flipped_face_is_detected_and_rewound() {
    let raw = load_raw(&broken("cube-flipped-face.stl"));
    let before = analyze(&raw);
    assert_eq!(before.inconsistent_winding_edges, 3);
    assert!(!before.is_manifold());

    let (_, report) =
        load_path_reporting(&broken("cube-flipped-face.stl"), &RepairOptions::default()).unwrap();
    assert_eq!(report.actions.flipped_faces, 1);

    assert_repaired_to_a_cube("cube-flipped-face.stl");
    assert_slices("cube-flipped-face.stl");
}

#[test]
fn inverted_shell_is_detected_and_turned_outward() {
    let raw = load_raw(&broken("cube-inverted.stl"));
    let before = analyze(&raw);
    assert_eq!(before.inverted_shells, 1);
    // An inside-out mesh is watertight and manifold — only the volume tells.
    assert!(before.is_watertight());
    assert!(before.is_manifold());
    assert!(!before.is_clean());

    assert_repaired_to_a_cube("cube-inverted.stl");
    assert_slices("cube-inverted.stl");
}

#[test]
fn degenerate_faces_are_detected_and_dropped() {
    let raw = load_raw(&broken("cube-degenerate.stl"));
    let before = analyze(&raw);
    assert_eq!(before.degenerate_faces, 2);

    let mesh = load_repaired(&broken("cube-degenerate.stl"));
    assert_eq!(mesh.faces.len(), 12);

    assert_repaired_to_a_cube("cube-degenerate.stl");
    assert_slices("cube-degenerate.stl");
}

#[test]
fn duplicate_faces_are_detected_and_dropped() {
    let raw = load_raw(&broken("cube-duplicate-faces.stl"));
    let before = analyze(&raw);
    assert_eq!(before.duplicate_faces, 1);
    // The repeat gives three edges a third incident triangle.
    assert_eq!(before.non_manifold_edges, 3);

    let mesh = load_repaired(&broken("cube-duplicate-faces.stl"));
    assert_eq!(mesh.faces.len(), 12);

    assert_repaired_to_a_cube("cube-duplicate-faces.stl");
    assert_slices("cube-duplicate-faces.stl");
}

#[test]
fn cracked_vertices_are_detected_and_welded() {
    let raw = load_raw(&broken("cube-unwelded.stl"));
    let before = analyze(&raw);
    assert_eq!(before.vertices, 9, "the crack should split one corner");
    assert_eq!(before.boundary_edges, 4);

    let (mesh, report) =
        load_path_reporting(&broken("cube-unwelded.stl"), &RepairOptions::default()).unwrap();
    assert_eq!(report.actions.welded_vertices, 1);
    assert_eq!(mesh.vertices.len(), 8);

    assert_repaired_to_a_cube("cube-unwelded.stl");
    assert_slices("cube-unwelded.stl");
}

#[test]
fn a_mesh_with_every_defect_is_fully_repaired() {
    let path = broken("cube-multi-defect.stl");
    let raw = load_raw(&path);
    let before = analyze(&raw);
    assert!(before.degenerate_faces > 0);
    assert!(before.duplicate_faces > 0);
    assert!(!before.is_watertight());
    assert!(!before.is_manifold());

    assert_repaired_to_a_cube("cube-multi-defect.stl");
    assert_slices("cube-multi-defect.stl");
}

#[test]
fn repair_can_be_turned_off() {
    let path = broken("cube-hole.stl");
    let (mesh, report) = load_path_reporting(&path, &RepairOptions::analysis_only()).unwrap();
    assert!(!report.repaired);
    assert_eq!(report.before, report.after);
    assert_eq!(mesh.faces.len(), 10, "the hole must still be there");
    assert!(report.is_noteworthy(), "but the user must still be told");
}

#[test]
fn every_report_carries_a_readable_summary() {
    for name in [
        "cube-hole.stl",
        "cube-flipped-face.stl",
        "cube-inverted.stl",
        "cube-degenerate.stl",
        "cube-duplicate-faces.stl",
        "cube-unwelded.stl",
        "cube-multi-defect.stl",
    ] {
        let (_, report) = load_path_reporting(&broken(name), &RepairOptions::default()).unwrap();
        assert!(report.is_noteworthy(), "{name}: should be worth reporting");
        assert!(report.repaired, "{name}: should have been repaired");
        assert!(
            report.summary.contains("found"),
            "{name}: unhelpful summary {:?}",
            report.summary
        );
        assert!(
            report.summary.contains("mesh is now clean"),
            "{name}: unhelpful summary {:?}",
            report.summary
        );
    }
}

/// The multi-part loader (a 3MF is a scene, not a model) must run the same
/// validation every other entry point does — otherwise a defective part of a
/// multi-part file would reach the slicer unrepaired.
#[test]
fn multi_part_loading_validates_every_part() {
    // Single-part formats resolve to exactly one entry, still reported.
    for name in ["cube-multi-defect.stl", "cube-hole.stl"] {
        let parts = load_path_multi_reporting(&broken(name), &RepairOptions::default())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(parts.len(), 1, "{name}: an STL is always one part");
        let part = &parts[0];
        assert!(part.report.repaired, "{name}: should have been repaired");
        assert!(
            analyze(&part.mesh).is_clean(),
            "{name}: the returned part must be the repaired mesh"
        );
    }

    // A 3MF takes the container path; its parts must be validated there too.
    let path = crate_root().join("tests/fixtures/simple-cube.3mf");
    let parts =
        load_path_multi_reporting(&path, &RepairOptions::default()).expect("load simple-cube.3mf");
    assert!(!parts.is_empty(), "a 3MF must yield at least one part");
    for part in &parts {
        assert!(analyze(&part.mesh).is_clean());
        assert!(!part.report.repaired, "the fixture is already clean");
    }
}

/// The contract that keeps the slicing-quality baselines stable: a clean mesh
/// is reported clean and handed back **borrowed**, never rebuilt.
#[test]
fn known_good_meshes_are_reported_clean_and_never_rewritten() {
    let candidates = [
        "3DBenchy.stl",
        "Voron_Design_Cube_v7.stl",
        "Filament_Card_Caddy_25.stl",
        "bottom_panel_hinge_x2.stl",
        "tests/fixtures/simple-cube.stl",
        "tests/fixtures/simple-cube-ascii.stl",
        "tests/fixtures/simple-cube.obj",
        "tests/fixtures/simple-cube.3mf",
    ];

    let mut checked = 0;
    for name in candidates {
        let path = crate_root().join(name);
        if !path.exists() {
            continue;
        }
        checked += 1;
        let raw = load_raw(&path);
        let diagnostics = analyze(&raw);
        assert!(
            diagnostics.is_clean(),
            "{name} is no longer clean: {diagnostics:?} — \
             the QA baselines assume repair is a no-op on this corpus"
        );

        let (mesh, report) = repair(&raw, &RepairOptions::default());
        assert!(
            matches!(mesh, std::borrow::Cow::Borrowed(_)),
            "{name}: a clean mesh must be borrowed, not rebuilt"
        );
        assert!(!report.repaired, "{name}: nothing should have been changed");
        assert!(!report.is_noteworthy(), "{name}: should not warn the user");
    }
    assert!(checked >= 5, "expected to find the known-good corpus");
}

/// Face indices travel over the wire (`SceneOp::PlaceFaceOnFloor`), so two
/// independent loads of the same bytes must agree exactly.
#[test]
fn repair_output_is_reproducible_across_loads() {
    for name in [
        "cube-multi-defect.stl",
        "cube-hole.stl",
        "cube-unwelded.stl",
    ] {
        let a = load_repaired(&broken(name));
        let b = load_repaired(&broken(name));
        assert_eq!(a.faces, b.faces, "{name}: face order drifted");
        assert_eq!(a.vertices, b.vertices, "{name}: vertex order drifted");
    }
}
