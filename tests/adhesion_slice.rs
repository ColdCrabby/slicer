//! End-to-end verification for bed-adhesion generation (issue #93).
//!
//! Mirrors the server slice path (`read_stl` → `process_mesh` →
//! `GcodeGenerator::generate`) so skirt / brim / raft are exercised exactly as
//! a remote slice would produce them, then asserts on the emitted G-code.

use std::path::Path;

use slicer_engine::core::process_mesh;
use slicer_engine::gcode::{GcodeFlavor, GcodeGenerator};
use slicer_engine::logging::NullLogger;
use slicer_engine::mesh::io::read_stl;
use slicer_engine::settings::params::{AdhesionType, BrimType, SlicingParams};

fn cube_params() -> SlicingParams {
    SlicingParams {
        layer_height: 0.2,
        nozzle_diameter_mm: 0.4,
        top_layers: 3,
        bottom_layers: 3,
        infill_density: 0.15,
        adhesion_type: AdhesionType::None,
        ..Default::default()
    }
}

fn slice_to_gcode(params: &SlicingParams) -> String {
    let mesh = read_stl(Path::new("tests/fixtures/simple-cube.stl")).expect("load cube fixture");
    let layers = process_mesh(&mesh, params, &NullLogger);
    let generator = GcodeGenerator::new(GcodeFlavor::Marlin);
    generator.generate(&layers, params)
}

/// Count G-code lines emitted under a given `;TYPE:` role block.
fn count_role_extrusions(gcode: &str, type_name: &str) -> usize {
    let mut count = 0;
    let mut in_block = false;
    for line in gcode.lines() {
        if let Some(rest) = line.strip_prefix(";TYPE:") {
            in_block = rest.trim() == type_name;
            continue;
        }
        if line.starts_with(";TYPE:") {
            in_block = false;
        }
        if in_block && line.starts_with("G1 ") && line.contains('E') {
            count += 1;
        }
    }
    count
}

#[test]
fn skirt_emits_skirt_extrusions() {
    let mut p = cube_params();
    p.adhesion_type = AdhesionType::Skirt;
    p.skirt_loops = 3;
    p.skirt_distance = 3.0;
    p.skirt_height = 1;
    let gcode = slice_to_gcode(&p);
    assert!(gcode.contains(";TYPE:Skirt"), "skirt must emit ;TYPE:Skirt");
    assert!(
        count_role_extrusions(&gcode, "Skirt") >= 3,
        "expected at least 3 skirt extrusion moves"
    );
}

#[test]
fn brim_outer_emits_more_loops_than_skirt() {
    let mut p = cube_params();
    p.adhesion_type = AdhesionType::Brim;
    p.brim_type = BrimType::OuterOnly;
    p.brim_width = 4.0; // ~10 loops at 0.4 mm
    let gcode = slice_to_gcode(&p);
    assert!(
        gcode.contains(";TYPE:Skirt"),
        "brim emits ;TYPE:Skirt lines"
    );
    assert!(
        count_role_extrusions(&gcode, "Skirt") >= 8,
        "wide brim should emit many loop extrusions"
    );
}

#[test]
fn raft_emits_support_and_lifts_object() {
    let mut base = cube_params();
    base.adhesion_type = AdhesionType::None;
    let no_raft = slice_to_gcode(&base);

    let mut p = cube_params();
    p.adhesion_type = AdhesionType::Raft;
    p.raft_layers = 3;
    p.raft_air_gap = 0.2;
    let gcode = slice_to_gcode(&p);

    assert!(
        gcode.contains(";TYPE:Support material"),
        "raft must emit ;TYPE:Support material"
    );

    // The raft adds layers, so the total layer count must grow.
    let count_layers = |g: &str| g.matches(";LAYER_CHANGE").count();
    assert!(
        count_layers(&gcode) > count_layers(&no_raft),
        "raft must add layers below the object"
    );
}

#[test]
fn none_emits_no_adhesion_roles() {
    let gcode = slice_to_gcode(&cube_params());
    assert!(!gcode.contains(";TYPE:Skirt"));
}
