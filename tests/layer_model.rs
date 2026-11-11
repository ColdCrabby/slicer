//! The non-planar layer model.
//!
//! `SliceLayer` gave every layer a single `z`, so a bead could not rise and
//! fall *within* a layer and a feature like a wavy overhang was inexpressible
//! at any hook point — not because the hooks were in the wrong place, but
//! because the type handed to them could not describe the idea. These tests
//! pin that it now can, and that an ordinary flat print is unaffected.

use slicer_engine::core::stages::ids;
use slicer_engine::core::{
    process_mesh, process_mesh_with_plugins, ExtrusionRole, PathPick, SliceLayer, VertexOrder,
};
use slicer_engine::gcode::{GcodeFlavor, GcodeGenerator};
use slicer_engine::logging::NullLogger;
use slicer_engine::mesh::types::Mesh;
use slicer_engine::plugin::{FnStage, Plugin, PluginManifest, SliceContext, StageRegistration};
use slicer_engine::settings::params::SlicingParams;

fn fixture() -> Mesh {
    let path = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/Voron_Design_Cube_v7.stl"
    ));
    slicer_engine::mesh::io::read_mesh(path).expect("fixture mesh")
}

fn to_gcode(layers: &[SliceLayer], p: &SlicingParams) -> String {
    GcodeGenerator::new(GcodeFlavor::Marlin).generate(layers, p)
}

/// The amplitude the ripple plugin below applies, in mm.
const RIPPLE_MM: f64 = 0.05;

/// A stand-in for a wavy-overhang plugin: it rides every outer wall up and
/// down within its own layer.
///
/// Deliberately the *whole* feature in miniature — if this can be expressed
/// through the hooks, so can the real thing.
struct Ripple;

impl Plugin for Ripple {
    fn manifest(&self) -> PluginManifest {
        PluginManifest::experiment("ripple", "Ripple", "Rides the outer wall up and down.")
    }
    fn stages(&self) -> Vec<StageRegistration> {
        vec![StageRegistration::after(
            ids::FLOW_COMPENSATION,
            FnStage::boxed("ripple:apply", |cx: &mut SliceContext<'_>| {
                for layer in cx.layers.iter_mut() {
                    if layer.path_vertex_z.is_empty() {
                        layer.path_vertex_z = vec![None; layer.paths.len()];
                    }
                    for i in 0..layer.paths.len() {
                        if layer.role_for_path(i) != ExtrusionRole::OuterWall {
                            continue;
                        }
                        let n = layer.paths.iter().nth(i).map(|p| p.len()).unwrap_or(0);
                        if n == 0 {
                            continue;
                        }
                        let profile = (0..n)
                            .map(|k| {
                                let phase = k as f64 / n as f64 * std::f64::consts::TAU;
                                RIPPLE_MM * phase.sin()
                            })
                            .collect();
                        layer.path_vertex_z[i] = Some(profile);
                    }
                }
            }),
        )]
    }
}

fn ripple_plugins() -> Vec<Box<dyn Plugin>> {
    vec![Box::new(Ripple)]
}

#[test]
fn a_flat_print_carries_no_per_vertex_z() {
    // The empty-vector sentinel is what keeps this free: an ordinary slice
    // must not allocate a Z profile per path, and must emit no Z-bearing
    // extrusion moves at all.
    let mesh = fixture();
    let p = SlicingParams::default();
    let layers = process_mesh(&mesh, &p, &NullLogger);

    assert!(
        layers.iter().all(|l| l.path_vertex_z.is_empty()),
        "no pipeline stage may populate per-vertex Z on its own"
    );
    assert!(
        layers.iter().all(|l| l.path_data.is_empty()),
        "no pipeline stage may populate per-path plugin data on its own"
    );

    let gcode = to_gcode(&layers, &p);
    assert!(
        !gcode
            .lines()
            .any(|l| l.starts_with("G1 ") && l.contains(" Z") && l.contains(" E")),
        "a flat print must emit no combined Z+E move"
    );
}

#[test]
fn a_plugin_can_make_a_wall_non_planar() {
    let mesh = fixture();
    let p = SlicingParams::default();
    let layers = process_mesh_with_plugins(&mesh, &p, &NullLogger, &ripple_plugins());
    let gcode = to_gcode(&layers, &p);

    let z_extrudes: Vec<&str> = gcode
        .lines()
        .filter(|l| l.starts_with("G1 ") && l.contains(" Z") && l.contains(" E"))
        .collect();
    assert!(
        !z_extrudes.is_empty(),
        "the ripple must reach the G-code as Z-bearing extrusion moves"
    );

    // The Z of those moves must actually vary — a constant Z would mean the
    // profile was flattened somewhere between the plugin and the emitter.
    let zs: Vec<f64> = z_extrudes
        .iter()
        .filter_map(|l| {
            l.split_whitespace()
                .find(|t| t.starts_with('Z'))
                .and_then(|t| t[1..].parse::<f64>().ok())
        })
        .collect();
    let lo = zs.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = zs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    assert!(
        hi - lo > RIPPLE_MM,
        "the emitted Z must span more than one layer's ripple ({lo} … {hi})"
    );
}

#[test]
fn a_non_planar_bead_is_charged_for_the_distance_it_climbs() {
    // The bead travels in 3D, so its length — and the filament it needs — is
    // more than its XY projection. Getting this wrong under-extrudes exactly
    // the feature that needed the Z in the first place.
    let mesh = fixture();
    let p = SlicingParams::default();

    let flat = to_gcode(&process_mesh(&mesh, &p, &NullLogger), &p);
    let rippled = to_gcode(
        &process_mesh_with_plugins(&mesh, &p, &NullLogger, &ripple_plugins()),
        &p,
    );

    let used = |g: &str| -> f64 {
        g.lines()
            .find_map(|l| l.strip_prefix("; filament used [mm] = "))
            .and_then(|v| v.trim().parse::<f64>().ok())
            .expect("filament used in header")
    };
    assert!(
        used(&rippled) > used(&flat),
        "a climbing bead must cost more filament than a flat one ({} vs {})",
        used(&rippled),
        used(&flat)
    );
}

#[test]
fn reordering_a_layer_keeps_every_per_vertex_array_aligned() {
    // The failure this guards is silent: a loop rotated to a new seam whose
    // widths or Z offsets were not rotated with it prints the wrong profile at
    // every vertex, and nothing errors.
    let mut layer = SliceLayer::new(0.2);
    let mut path = clipper2::Path::default();
    for (x, y) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        path.push(clipper2::Point::new(x, y));
    }
    layer.paths.push(path);
    layer.path_roles.push(ExtrusionRole::OuterWall);
    layer.path_widths.push(Some(0.4));
    layer
        .path_vertex_widths
        .push(Some(vec![0.1, 0.2, 0.3, 0.4]));
    layer.path_is_open.push(false);
    layer.path_vertex_z = vec![Some(vec![1.0, 2.0, 3.0, 4.0])];

    layer.rebuild_paths(&[PathPick {
        index: 0,
        order: VertexOrder::RotatedTo(2),
    }]);

    let xs: Vec<f64> = layer
        .paths
        .iter()
        .next()
        .unwrap()
        .iter()
        .map(|p| p.x())
        .collect();
    assert_eq!(
        xs,
        vec![1.0, 0.0, 0.0, 1.0],
        "vertices rotate to the new seam"
    );
    assert_eq!(
        layer.path_vertex_widths[0],
        Some(vec![0.3, 0.4, 0.1, 0.2]),
        "widths rotate with the vertices they describe"
    );
    assert_eq!(
        layer.path_vertex_z[0],
        Some(vec![3.0, 4.0, 1.0, 2.0]),
        "Z offsets rotate with them too"
    );
}

#[test]
fn per_path_plugin_data_survives_the_stages_after_it() {
    // Data attached early is only useful if reordering, prepending and the rest
    // carry it along — which is the whole reason it lives on the layer rather
    // than in a side map keyed by path index.
    let mut layer = SliceLayer::new(0.2);
    for k in 0..3 {
        let mut path = clipper2::Path::default();
        path.push(clipper2::Point::new(k as f64, 0.0));
        path.push(clipper2::Point::new(k as f64, 1.0));
        layer.paths.push(path);
        layer.path_roles.push(ExtrusionRole::OuterWall);
        layer.path_widths.push(None);
        layer.path_vertex_widths.push(None);
        layer.path_is_open.push(true);
    }
    layer.path_data = vec![Default::default(); 3];
    layer.path_data[2].set("ripple", 42u32);

    // Reverse the layer's path order.
    layer.rebuild_paths(&[PathPick::keep(2), PathPick::keep(1), PathPick::keep(0)]);

    assert_eq!(
        layer.data_for_path(0).and_then(|d| d.get::<u32>("ripple")),
        Some(&42),
        "the datum must follow its path, not its old index"
    );
    assert!(layer.data_for_path(2).unwrap().is_empty());
}

#[test]
fn two_plugins_can_annotate_the_same_path() {
    // A single slot would make whichever plugin ran second clobber the first.
    let mut data = slicer_engine::core::PathData::default();
    data.set("a", 1u32);
    data.set("b", "two");
    assert_eq!(data.get::<u32>("a"), Some(&1));
    assert_eq!(data.get::<&str>("b"), Some(&"two"));
    data.set("a", 3u32);
    assert_eq!(data.get::<u32>("a"), Some(&3), "an owner replaces its own");
    assert_eq!(data.get::<&str>("b"), Some(&"two"), "…and only its own");
}
