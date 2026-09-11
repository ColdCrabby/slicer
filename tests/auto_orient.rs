//! Auto-orient against the real QA corpus.
//!
//! The unit tests in `src/orient` pin the scorer on synthetic shapes; this
//! pins the outcome on models people actually print.  Every one of them is
//! authored bed-down, so the property under test is simple: auto-orient must
//! leave a substantial flat face on the plate and must not stand the model up.
//!
//! Each of these was mis-oriented before bed contact was measured rather than
//! inferred from downward-facing normals — the caddy worst of all, balanced on
//! 16 mm² of edge and four times its authored height.

use glam::{Quat, Vec3};
use slicer_engine::mesh::types::Mesh;
use slicer_engine::orient::{auto_orient, AutoOrientOptions};
use std::path::{Path, PathBuf};

/// What the chosen orientation puts on the bed, and how tall it stands.
struct Pose {
    contact_area: f64,
    height: f64,
}

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn load(file: &str) -> Mesh {
    slicer_engine::scene::load_path(&crate_root().join(file))
        .unwrap_or_else(|e| panic!("load {file}: {e}"))
}

/// Re-measure the pose `q` selects.  Mirrors `orient::score::measure`, which is
/// private to the engine — the point here is to check the *outcome*, so an
/// independent measurement is the right thing to compare against.
fn pose(mesh: &Mesh, q: Quat) -> Pose {
    let (mut min_z, mut max_z) = (f32::INFINITY, f32::NEG_INFINITY);
    for v in &mesh.vertices {
        let z = (q * Vec3::new(v.x as f32, v.y as f32, v.z as f32)).z;
        min_z = min_z.min(z);
        max_z = max_z.max(z);
    }

    let mut contact_area = 0.0;
    for f in &mesh.faces {
        let a = Vec3::new(
            f.vertices[0].x as f32,
            f.vertices[0].y as f32,
            f.vertices[0].z as f32,
        );
        let b = Vec3::new(
            f.vertices[1].x as f32,
            f.vertices[1].y as f32,
            f.vertices[1].z as f32,
        );
        let c = Vec3::new(
            f.vertices[2].x as f32,
            f.vertices[2].y as f32,
            f.vertices[2].z as f32,
        );
        let n = (b - a).cross(c - a);
        if n.length_squared() < 1e-12 {
            continue;
        }
        if (q * (n / n.length())).z > -0.98 {
            continue;
        }
        let ceiling = [a, b, c]
            .iter()
            .map(|v| (q * *v).z)
            .fold(f32::NEG_INFINITY, f32::max);
        if ceiling - min_z < 0.2 {
            contact_area += f.area();
        }
    }

    Pose {
        contact_area,
        height: (max_z - min_z) as f64,
    }
}

/// `(file, minimum bed contact in mm², maximum height in mm)`.
///
/// The bounds are the authored pose with room to spare, not exact values — a
/// different-but-equally-flat face is a legitimate answer, standing the model
/// on end is not.
const CORPUS: &[(&str, f64, f64)] = &[
    // The cube has a plain 754 mm² face and a 613 mm² embossed one: the bound
    // sits between them, so settling for the lesser face is a failure too.
    ("Voron_Design_Cube_v7.stl", 700.0, 31.0),
    ("bottom_panel_hinge_x2.stl", 1000.0, 7.0),
    ("Filament_Card_Caddy_25.stl", 5000.0, 21.0),
    ("3DBenchy.stl", 400.0, 49.0),
];

#[test]
fn the_corpus_keeps_a_flat_face_on_the_bed() {
    for &(file, min_contact, max_height) in CORPUS {
        if !crate_root().join(file).exists() {
            continue; // heavy fixtures are not always checked out
        }
        let mesh = load(file);
        let p = pose(&mesh, auto_orient(&mesh, &AutoOrientOptions::default()));
        assert!(
            p.contact_area >= min_contact,
            "{file}: only {:.1} mm² on the bed, expected at least {min_contact}",
            p.contact_area
        );
        assert!(
            p.height <= max_height,
            "{file}: stands {:.1} mm tall, expected at most {max_height}",
            p.height
        );
    }
}

/// The support fixture is authored the wrong way up on purpose — an 8 x 8 mm
/// stem on the plate holding a wide cantilever in the air.  Auto-orient has to
/// *rotate* this one, which is the other half of the contract: staying put is
/// the tie-break, not the answer.
#[test]
fn a_deliberately_bad_pose_is_turned_over() {
    let mesh = load("tests/fixtures/support-overhang.stl");
    let authored = pose(&mesh, Quat::IDENTITY);
    assert!(
        authored.contact_area < 100.0,
        "fixture should arrive balanced on its stem, got {:.1} mm²",
        authored.contact_area
    );

    let p = pose(&mesh, auto_orient(&mesh, &AutoOrientOptions::default()));
    assert!(
        p.contact_area > 700.0,
        "the wide plate should end up on the bed, got {:.1} mm²",
        p.contact_area
    );
}

/// A multi-part 3MF is oriented part by part; none of them may be tipped up.
#[test]
fn every_part_of_a_multi_part_file_lands_flat() {
    let path = crate_root().join("tests/fixtures/TopAC.3mf");
    let parts = slicer_engine::scene::load_path_multi(Path::new(&path)).unwrap();
    assert!(parts.len() > 1, "fixture should have several parts");
    for part in &parts {
        let p = pose(
            &part.mesh,
            auto_orient(&part.mesh, &AutoOrientOptions::default()),
        );
        assert!(
            p.contact_area > 1000.0,
            "{:?}: only {:.1} mm² on the bed",
            part.name,
            p.contact_area
        );
    }
}
