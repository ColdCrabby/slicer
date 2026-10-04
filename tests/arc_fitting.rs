//! Arc fitting, end to end: a real slice through the real generator, with the
//! experiment switched on through its settings the way a user would.

use serde_json::json;
use slicer_engine::core::process_mesh;
use slicer_engine::gcode::{GcodeFlavor, GcodeGenerator, SliceStatistics};
use slicer_engine::logging::NullLogger;
use slicer_engine::mesh::types::{Face, Mesh, Vertex};
use slicer_engine::settings::params::SlicingParams;

/// A tube — a round boss and a round hole in one, which is the shape the
/// experiment exists for.
fn tube(outer: f64, inner: f64, height: f64, facets: usize) -> Mesh {
    let at = |r: f64, k: usize, z: f64| {
        let a = std::f64::consts::TAU * (k % facets) as f64 / facets as f64;
        Vertex::new(60.0 + r * a.cos(), 60.0 + r * a.sin(), z)
    };
    let mut mesh = Mesh::new();
    for k in 0..facets {
        let (o0b, o1b, o0t, o1t) = (
            at(outer, k, 0.0),
            at(outer, k + 1, 0.0),
            at(outer, k, height),
            at(outer, k + 1, height),
        );
        let (i0b, i1b, i0t, i1t) = (
            at(inner, k, 0.0),
            at(inner, k + 1, 0.0),
            at(inner, k, height),
            at(inner, k + 1, height),
        );
        // Outer wall faces out, inner wall faces the axis, top up, bottom down.
        mesh.faces.push(Face::new([o0b, o1b, o1t]));
        mesh.faces.push(Face::new([o0b, o1t, o0t]));
        mesh.faces.push(Face::new([i0b, i1t, i1b]));
        mesh.faces.push(Face::new([i0b, i0t, i1t]));
        mesh.faces.push(Face::new([i0t, o0t, o1t]));
        mesh.faces.push(Face::new([i0t, o1t, i1t]));
        mesh.faces.push(Face::new([i0b, o1b, o0b]));
        mesh.faces.push(Face::new([i0b, i1b, o1b]));
    }
    // The slicer takes its Z range from the bounding box, which is computed
    // from `vertices` — leave them out and the mesh slices to nothing.
    mesh.vertices = mesh.faces.iter().flat_map(|f| f.vertices).collect();
    mesh.calculate_aabb();
    mesh
}

fn slice(arcs: bool) -> (String, SliceStatistics) {
    let mesh = tube(15.0, 5.0, 1.2, 180);
    let mut params = SlicingParams::default();
    if arcs {
        params.set_plugin_value("arc-fitting", "enabled", json!(true));
    }
    let layers = process_mesh(&mesh, &params, &NullLogger);
    GcodeGenerator::new(GcodeFlavor::Marlin).generate_with_stats(&layers, &params)
}

fn is_arc(line: &str) -> bool {
    line.starts_with("G2 ") || line.starts_with("G3 ")
}

/// The value after `letter` on a G-code line.
fn word(line: &str, letter: char) -> Option<f64> {
    line.split(';')
        .next()?
        .split_whitespace()
        .skip(1)
        .find(|w| w.starts_with(letter))
        .and_then(|w| w[1..].parse().ok())
}

/// The last E the program writes — with absolute E, the filament it uses.
fn last_e(gcode: &str) -> f64 {
    gcode
        .lines()
        .filter(|l| l.starts_with('G'))
        .filter_map(|l| word(l, 'E'))
        .last()
        .expect("the program extrudes")
}

#[test]
fn switched_off_it_writes_no_arcs() {
    let (gcode, _) = slice(false);
    assert!(!gcode.lines().any(is_arc));
}

#[test]
fn a_tube_prints_its_round_walls_as_arcs_and_nothing_else() {
    let (lines_only, _) = slice(false);
    let (with_arcs, _) = slice(true);

    let mut role = String::new();
    let mut arcs = 0;
    for line in with_arcs.lines() {
        if let Some(r) = line.strip_prefix(";TYPE:") {
            role = r.to_string();
        }
        if is_arc(line) {
            arcs += 1;
            assert!(role.contains("wall"), "an arc in {role}: {line}");
        }
    }
    assert!(arcs > 0, "no arcs on a tube");

    let moves = |g: &str| g.lines().filter(|l| l.starts_with('G')).count();
    assert!(
        moves(&with_arcs) * 2 < moves(&lines_only),
        "{} moves with arcs against {} without",
        moves(&with_arcs),
        moves(&lines_only)
    );
}

#[test]
fn arcs_keep_the_filament_and_the_estimate() {
    let (lines_only, plain) = slice(false);
    let (with_arcs, fitted) = slice(true);
    assert!((last_e(&lines_only) - last_e(&with_arcs)).abs() < 1e-4);
    assert_eq!(plain.filament_mm, fitted.filament_mm);
    let drift = (fitted.estimated_print_time_s / plain.estimated_print_time_s - 1.0).abs();
    assert!(drift < 0.02, "estimate moved {:.1} %", drift * 100.0);
}

#[test]
fn every_arc_starts_and_ends_on_its_own_circle() {
    let (gcode, _) = slice(true);
    let mut at = (0.0, 0.0);
    for line in gcode
        .lines()
        .filter(|l| l.starts_with("G0 ") || l.starts_with("G1 ") || is_arc(l))
    {
        let end = (
            word(line, 'X').unwrap_or(at.0),
            word(line, 'Y').unwrap_or(at.1),
        );
        if is_arc(line) {
            let center = (
                at.0 + word(line, 'I').unwrap(),
                at.1 + word(line, 'J').unwrap(),
            );
            let r_start = (at.0 - center.0).hypot(at.1 - center.1);
            let r_end = (end.0 - center.0).hypot(end.1 - center.1);
            assert!(
                (r_start - r_end).abs() < 2e-3,
                "{line}: {r_start} vs {r_end}"
            );
            assert!(
                (end.0 - at.0).hypot(end.1 - at.1) > 0.005,
                "an arc whose end is its start is a full circle: {line}"
            );
        }
        at = end;
    }
}
