//! Temporary diagnostic: dump one layer's geometry as JSON for plotting.
//!
//! `DUMP_LAYER=21 DUMP_ROT=45 DUMP_NOZZLE=0.4 cargo test --test dump_layer -- --ignored`

use std::sync::Arc;

use slicer_engine::core::{slice_mesh, ExtrusionRole};
use slicer_engine::scene::{apply_transform, load_path, BedConfig, SceneOp, SceneState};
use slicer_engine::settings::params::SlicingParams;
use slicer_engine::walls::generate_walls;

fn env_f(name: &str, default: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[test]
#[ignore = "diagnostic"]
fn dump_layer_geometry() {
    let rot = env_f("DUMP_ROT", 0.0);
    let nozzle = env_f("DUMP_NOZZLE", 0.4);
    let want = env_f("DUMP_LAYER", 21.0) as usize;

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(std::env::var("DUMP_MODEL").unwrap_or_else(|_| "Filament_Card_Caddy_25.stl".into()));
    let mesh = load_path(&path).expect("load");
    let mut scene = SceneState::new(BedConfig::default());
    let id = scene.add_mesh(String::from("dump"), Arc::new(mesh));
    if rot != 0.0 {
        scene
            .apply(SceneOp::Rotate {
                id,
                axis: [0.0, 0.0, 1.0],
                radians: rot.to_radians() as f32,
            })
            .expect("rotate");
    }
    scene.apply(SceneOp::CenterOnBed { id }).expect("center");
    scene.apply(SceneOp::DropToFloor { id }).expect("drop");
    let obj = scene.get(id).expect("obj");
    let mesh = apply_transform(obj.mesh.as_ref(), &obj.transform);

    let params = SlicingParams {
        nozzle_diameter_mm: nozzle,
        layer_height: 0.2,
        ..SlicingParams::default()
    };
    let mut layers = slice_mesh(&mesh, params.layer_height);

    // Island contours, before walls replace them.
    let island: Vec<Vec<(f64, f64)>> = layers[want]
        .paths
        .iter()
        .map(|p| p.iter().map(|q| (q.x(), q.y())).collect())
        .collect();

    generate_walls(&mut layers, &params);

    let layer = &layers[want];
    let mut beads = Vec::new();
    for (i, p) in layer.paths.iter().enumerate() {
        let role = layer.role_for_path(i);
        if !matches!(
            role,
            ExtrusionRole::OuterWall | ExtrusionRole::InnerWall | ExtrusionRole::GapFill
        ) {
            continue;
        }
        let pts: Vec<(f64, f64)> = p.iter().map(|q| (q.x(), q.y())).collect();
        let w = layer.vertex_widths_for_path(i).unwrap_or_else(|| {
            let scalar = layer.width_for_path(i).unwrap_or(nozzle);
            vec![scalar; pts.len()]
        });
        beads.push((
            format!("{role:?}"),
            layer.is_path_open(i),
            layer.is_medial_bead(i),
            pts,
            w,
        ));
    }

    let mut out = String::from("{\"island\":[");
    for (k, c) in island.iter().enumerate() {
        if k > 0 {
            out.push(',');
        }
        out.push('[');
        for (j, (x, y)) in c.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            out.push_str(&format!("[{x:.4},{y:.4}]"));
        }
        out.push(']');
    }
    out.push_str("],\"beads\":[");
    for (k, (role, open, medial, pts, w)) in beads.iter().enumerate() {
        if k > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"role\":\"{role}\",\"open\":{open},\"medial\":{medial},\"pts\":["
        ));
        for (j, (x, y)) in pts.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            out.push_str(&format!("[{x:.4},{y:.4}]"));
        }
        out.push_str("],\"w\":[");
        for (j, v) in w.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            out.push_str(&format!("{v:.4}"));
        }
        out.push_str("]}");
    }
    out.push_str("]}");
    let dest = std::env::var("DUMP_OUT").unwrap_or_else(|_| "/tmp/layer.json".into());
    std::fs::write(&dest, out).expect("write");
    eprintln!("wrote {dest}");
}
