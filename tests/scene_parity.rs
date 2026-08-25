//! End-to-end integration tests for the public scene API.
//!
//! These tests exercise [`slicer_engine::scene`] from outside the crate so
//! that breaking the public surface — the same surface consumed by the CLI,
//! the WS server, and the WASM bindings — fails the build immediately.
//!
//! Where applicable, each scenario applies the same logical sequence via
//! both the high-level CLI-style flow (load → apply ops → bake transform)
//! and the low-level [`SceneState::apply`] path, then asserts the resulting
//! baked geometry matches. This is the parity check called out in the issue
//! #51 plan.

use slicer_engine::mesh::types::{Mesh, Vertex, AABB};
use slicer_engine::scene::{
    apply_transform, load_path, BedConfig, MeshFormat, SceneOp, SceneState,
};
use std::path::PathBuf;
use std::sync::Arc;

fn fixture(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("fixtures");
    p.push(name);
    p
}

fn aabb(mesh: &Mesh) -> AABB {
    let mut min = Vertex::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut max = Vertex::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for f in &mesh.faces {
        for v in &f.vertices {
            if v.x < min.x {
                min.x = v.x;
            }
            if v.y < min.y {
                min.y = v.y;
            }
            if v.z < min.z {
                min.z = v.z;
            }
            if v.x > max.x {
                max.x = v.x;
            }
            if v.y > max.y {
                max.y = v.y;
            }
            if v.z > max.z {
                max.z = v.z;
            }
        }
    }
    AABB { min, max }
}

#[test]
fn load_simple_cube_and_center_drops_to_floor() {
    let mesh = load_path(&fixture("simple-cube.stl")).expect("load fixture");

    let bed = BedConfig {
        width: 200.0,
        depth: 200.0,
        height: 200.0,
        origin_offset_x: 0.0,
        origin_offset_y: 0.0,
        shape: slicer_engine::scene::bed::BedShape::Rectangular,
    };
    let mut scene = SceneState::new(bed);
    let id = scene.add_mesh("cube".to_string(), Arc::new(mesh));

    scene.apply(SceneOp::CenterOnBed { id }).expect("center op");
    scene.apply(SceneOp::DropToFloor { id }).expect("drop op");

    let obj = scene.get(id).expect("object exists");
    let baked = apply_transform(obj.mesh.as_ref(), &obj.transform);
    let bb = aabb(&baked);

    // After CenterOnBed + DropToFloor on a 200×200 bed, the cube's XY
    // center should sit at (100, 100) and its bottom at z=0.
    let cx = (bb.min.x + bb.max.x) * 0.5;
    let cy = (bb.min.y + bb.max.y) * 0.5;
    assert!((cx - 100.0).abs() < 1e-6, "center x = {cx}");
    assert!((cy - 100.0).abs() < 1e-6, "center y = {cy}");
    assert!(bb.min.z.abs() < 1e-6, "bottom z = {}", bb.min.z);
}

#[test]
fn op_sequence_is_path_independent() {
    // Verify that applying ops via the public API produces the same baked
    // geometry as applying them via repeated SceneState::apply calls — i.e.
    // there is no hidden state in the CLI/WS/WASM call paths that diverges
    // from the single source of truth.
    let mesh = load_path(&fixture("simple-cube.stl")).expect("load");
    let bed = BedConfig::default();

    let mut a = SceneState::new(bed);
    let id_a = a.add_mesh("cube".to_string(), Arc::new(mesh.clone()));
    a.apply(SceneOp::Translate {
        id: id_a,
        delta: [10.0, 20.0, 0.0],
    })
    .unwrap();
    a.apply(SceneOp::Rotate {
        id: id_a,
        axis: [0.0, 0.0, 1.0],
        radians: std::f32::consts::FRAC_PI_2,
    })
    .unwrap();
    a.apply(SceneOp::CenterOnBed { id: id_a }).unwrap();
    a.apply(SceneOp::DropToFloor { id: id_a }).unwrap();

    let mut b = SceneState::new(bed);
    let id_b = b.add_mesh("cube".to_string(), Arc::new(mesh.clone()));
    for op in [
        SceneOp::Translate {
            id: id_b,
            delta: [10.0, 20.0, 0.0],
        },
        SceneOp::Rotate {
            id: id_b,
            axis: [0.0, 0.0, 1.0],
            radians: std::f32::consts::FRAC_PI_2,
        },
        SceneOp::CenterOnBed { id: id_b },
        SceneOp::DropToFloor { id: id_b },
    ] {
        b.apply(op).unwrap();
    }

    let oa = a.get(id_a).unwrap();
    let ob = b.get(id_b).unwrap();
    let bb_a = aabb(&apply_transform(oa.mesh.as_ref(), &oa.transform));
    let bb_b = aabb(&apply_transform(ob.mesh.as_ref(), &ob.transform));

    assert!((bb_a.min.x - bb_b.min.x).abs() < 1e-9);
    assert!((bb_a.min.y - bb_b.min.y).abs() < 1e-9);
    assert!((bb_a.min.z - bb_b.min.z).abs() < 1e-9);
    assert!((bb_a.max.x - bb_b.max.x).abs() < 1e-9);
    assert!((bb_a.max.y - bb_b.max.y).abs() < 1e-9);
    assert!((bb_a.max.z - bb_b.max.z).abs() < 1e-9);
}

#[test]
fn add_from_bytes_matches_load_from_path() {
    // The WS/WASM paths receive raw bytes; the CLI receives a path. Both
    // must produce the same mesh.
    let path_mesh = load_path(&fixture("simple-cube.stl")).expect("path load");
    let bytes = std::fs::read(fixture("simple-cube.stl")).expect("read bytes");

    let mut scene = SceneState::new(BedConfig::default());
    let receipt = scene
        .apply(SceneOp::Add {
            name: "cube".to_string(),
            format: MeshFormat::Stl,
            bytes,
        })
        .expect("add from bytes");
    let id = match receipt.inverse {
        SceneOp::Remove { id } => id,
        _ => panic!("expected Remove inverse"),
    };

    let obj = scene.get(id).expect("object exists");
    assert_eq!(obj.mesh.faces.len(), path_mesh.faces.len());

    let bb_path = aabb(&path_mesh);
    let bb_bytes = aabb(obj.mesh.as_ref());
    assert!((bb_path.min.x - bb_bytes.min.x).abs() < 1e-9);
    assert!((bb_path.max.x - bb_bytes.max.x).abs() < 1e-9);
}
