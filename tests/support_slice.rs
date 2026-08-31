//! End-to-end verification for support generation.
//!
//! These go through `process_mesh` on purpose. The support unit tests build
//! `SliceLayer`s by hand, which skips `classify_overhang_perimeters` — and that
//! pass is exactly what broke sloped overhangs: it retags an overhanging wall
//! as `OverhangPerimeter` and splits its loop, so a steep slope keeps no
//! `OuterWall` path for the support stage to measure. A 60° cone reported 49 of
//! 50 footprints empty and produced no support at any threshold, while every
//! hand-built unit test passed. Only the pipeline path can catch that.

use slicer_engine::core::{process_mesh, ExtrusionRole, SliceLayer};
use slicer_engine::gcode::{GcodeFlavor, GcodeGenerator};
use slicer_engine::logging::NullLogger;
use slicer_engine::mesh::types::{Face, Mesh, Vertex};
use slicer_engine::settings::params::{AdhesionType, SlicingParams, SupportType};

fn v(x: f64, y: f64, z: f64) -> Vertex {
    Vertex { x, y, z }
}

fn quad(m: &mut Mesh, a: Vertex, b: Vertex, c: Vertex, d: Vertex) {
    m.faces.push(Face::new([a, b, c]));
    m.faces.push(Face::new([a, c, d]));
}

/// A square frustum whose side walls lean `slope_deg` from vertical: the
/// canonical "does this slicer support an overhang" shape.
fn frustum(slope_deg: f64) -> Mesh {
    let h = 10.0;
    let b = 2.0;
    let t = b + slope_deg.to_radians().tan() * h;
    let (cx, cy) = (25.0, 25.0);
    let mut m = Mesh::new();

    let bot = [
        v(cx - b, cy - b, 0.0),
        v(cx + b, cy - b, 0.0),
        v(cx + b, cy + b, 0.0),
        v(cx - b, cy + b, 0.0),
    ];
    let top = [
        v(cx - t, cy - t, h),
        v(cx + t, cy - t, h),
        v(cx + t, cy + t, h),
        v(cx - t, cy + t, h),
    ];

    quad(&mut m, bot[0], bot[3], bot[2], bot[1]);
    quad(&mut m, top[0], top[1], top[2], top[3]);
    for i in 0..4 {
        let j = (i + 1) % 4;
        quad(&mut m, bot[i], top[i], top[j], bot[j]);
    }
    // Loaders populate both; `calculate_aabb` reads `vertices`, and the slicer
    // derives its Z range from that AABB — leave either out and the mesh
    // silently produces zero layers.
    m.vertices = m.faces.iter().flat_map(|f| f.vertices).collect();
    m.calculate_aabb();
    m
}

fn support_params(threshold_deg: f64) -> SlicingParams {
    SlicingParams {
        layer_height: 0.2,
        nozzle_diameter_mm: 0.4,
        top_layers: 3,
        bottom_layers: 3,
        infill_density: 0.15,
        support_enabled: true,
        support_threshold_angle: threshold_deg,
        ..Default::default()
    }
}

/// Total length (mm) of every `Support` polyline across every layer.
fn support_len(layers: &[SliceLayer]) -> f64 {
    let mut total = 0.0;
    for layer in layers {
        for (i, path) in layer.paths.iter().enumerate() {
            if layer.role_for_path(i) != ExtrusionRole::Support {
                continue;
            }
            let pts: Vec<(f64, f64)> = path.iter().map(|p| (p.x(), p.y())).collect();
            for w in pts.windows(2) {
                total += ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt();
            }
        }
    }
    total
}

fn layers_with_support(layers: &[SliceLayer]) -> usize {
    layers
        .iter()
        .filter(|l| (0..l.paths.len()).any(|i| l.role_for_path(i) == ExtrusionRole::Support))
        .count()
}

#[test]
fn a_steep_slope_is_supported_through_the_whole_pipeline() {
    let mesh = frustum(60.0);
    let layers = process_mesh(&mesh, &support_params(45.0), &NullLogger);

    let len = support_len(&layers);
    let covered = layers_with_support(&layers);

    assert!(
        len > 100.0,
        "a 60° overhang must get real support, not hairline slivers (got {len:.1} mm)"
    );
    // The slope overhangs from the bed up, so support should span most of it —
    // the old behaviour reached a handful of layers at most.
    assert!(
        covered > layers.len() / 2,
        "support must span most of the slope ({covered} of {} layers)",
        layers.len()
    );
}

#[test]
fn a_self_supporting_slope_gets_no_support() {
    // 30° from vertical is well inside the 45° rule: adding support here would
    // be wasted plastic and a scarred surface.
    let mesh = frustum(30.0);
    let layers = process_mesh(&mesh, &support_params(45.0), &NullLogger);
    assert_eq!(
        support_len(&layers),
        0.0,
        "a self-supporting slope must not be supported"
    );
}

#[test]
fn the_threshold_angle_decides_whether_a_slope_is_supported() {
    // The same 60° cone: supported when the threshold is below its slope,
    // untouched when above it. Pins that the knob is actually wired to
    // geometry — it used to produce byte-identical G-code at 0° and 89°.
    let mesh = frustum(60.0);
    let supported = process_mesh(&mesh, &support_params(45.0), &NullLogger);
    let ignored = process_mesh(&mesh, &support_params(70.0), &NullLogger);

    assert!(
        support_len(&supported) > 100.0,
        "a 60° slope is steeper than a 45° threshold and must be supported"
    );
    assert_eq!(
        support_len(&ignored),
        0.0,
        "a 60° slope is shallower than a 70° threshold and must be left alone"
    );
}

#[test]
fn tree_supports_a_steep_slope_too() {
    let mesh = frustum(60.0);
    let params = SlicingParams {
        support_type: SupportType::Tree,
        ..support_params(45.0)
    };
    let layers = process_mesh(&mesh, &params, &NullLogger);
    assert!(
        support_len(&layers) > 100.0,
        "tree must reach a sloped overhang as well as normal does"
    );
}

#[test]
fn supports_stay_off_by_default() {
    let mesh = frustum(60.0);
    let params = SlicingParams {
        support_enabled: false,
        ..support_params(45.0)
    };
    let layers = process_mesh(&mesh, &params, &NullLogger);
    assert_eq!(support_len(&layers), 0.0, "supports must be opt-in");
}

/// An axis-aligned box as 12 triangles.
fn add_box(m: &mut Mesh, lo: (f64, f64, f64), hi: (f64, f64, f64)) {
    let (x0, y0, z0) = lo;
    let (x1, y1, z1) = hi;
    let p = [
        v(x0, y0, z0),
        v(x1, y0, z0),
        v(x1, y1, z0),
        v(x0, y1, z0),
        v(x0, y0, z1),
        v(x1, y0, z1),
        v(x1, y1, z1),
        v(x0, y1, z1),
    ];
    for (a, b, c, d) in [
        (0, 3, 2, 1),
        (4, 5, 6, 7),
        (0, 1, 5, 4),
        (1, 2, 6, 5),
        (2, 3, 7, 6),
        (3, 0, 4, 7),
    ] {
        quad(m, p[a], p[b], p[c], p[d]);
    }
}

/// A narrow post under a wide cap: the cap overhangs far past the post, so its
/// support columns stand on bare bed well outside the object's own footprint.
fn mushroom() -> Mesh {
    let mut m = Mesh::new();
    add_box(&mut m, (12.0, 12.0, 0.0), (18.0, 18.0, 10.0));
    add_box(&mut m, (0.0, 0.0, 10.0), (30.0, 30.0, 12.0));
    m.vertices = m.faces.iter().flat_map(|f| f.vertices).collect();
    m.calculate_aabb();
    m
}

/// XY bounds of every path with `role` at `layer`, or `None` if there are none.
fn role_bounds(layer: &SliceLayer, role: ExtrusionRole) -> Option<(f64, f64, f64, f64)> {
    let mut b = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    let mut seen = false;
    for (i, path) in layer.paths.iter().enumerate() {
        if layer.role_for_path(i) != role {
            continue;
        }
        for p in path.iter() {
            seen = true;
            b.0 = b.0.min(p.x());
            b.1 = b.1.min(p.y());
            b.2 = b.2.max(p.x());
            b.3 = b.3.max(p.y());
        }
    }
    seen.then_some(b)
}

#[test]
fn a_raft_covers_the_support_standing_beside_the_object() {
    // The raft is the surface everything else is printed onto. Built from the
    // object's walls alone it stopped short of the support columns, which then
    // extruded into thin air a layer above the plate.
    let params = SlicingParams {
        adhesion_type: AdhesionType::Raft,
        raft_layers: 3,
        ..support_params(45.0)
    };
    let layers = process_mesh(&mushroom(), &params, &NullLogger);

    // Raft layers come first and are the only ones with no object walls.
    let raft = layers
        .iter()
        .find(|l| role_bounds(l, ExtrusionRole::OuterWall).is_none())
        .and_then(|l| role_bounds(l, ExtrusionRole::Support))
        .expect("raft layer with support-role fill");

    // The widest support on any object layer must sit inside it.
    let widest = layers
        .iter()
        .filter(|l| role_bounds(l, ExtrusionRole::OuterWall).is_some())
        .filter_map(|l| role_bounds(l, ExtrusionRole::Support))
        .fold(None::<(f64, f64, f64, f64)>, |acc, b| {
            Some(acc.map_or(b, |a| {
                (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))
            }))
        })
        .expect("the cap overhang must produce support");

    assert!(
        raft.0 <= widest.0 && raft.1 <= widest.1 && raft.2 >= widest.2 && raft.3 >= widest.3,
        "raft {raft:?} must enclose the support it carries {widest:?}"
    );
}

#[test]
fn a_skirt_encloses_the_support_too() {
    let params = SlicingParams {
        adhesion_type: AdhesionType::Skirt,
        skirt_loops: 1,
        ..support_params(45.0)
    };
    let layers = process_mesh(&mushroom(), &params, &NullLogger);
    let first = &layers[0];

    let skirt = role_bounds(first, ExtrusionRole::Skirt).expect("skirt on the first layer");
    if let Some(sup) = role_bounds(first, ExtrusionRole::Support) {
        assert!(
            skirt.0 <= sup.0 && skirt.1 <= sup.1 && skirt.2 >= sup.2 && skirt.3 >= sup.3,
            "skirt {skirt:?} must enclose the support it draws around {sup:?}"
        );
    }
}

#[test]
fn support_is_charged_at_its_flow_spacing_not_the_nozzle_width() {
    // Support is a fill role: its strands are pitched `spacing / density`, so
    // each must deposit the volume of the strip it fills. Charging the full
    // nominal bead width instead over-extrudes by `width / spacing` (≈ 12 % at
    // nozzle width) and makes the requested density wrong on any other nozzle.
    let params = support_params(45.0);
    let layers = process_mesh(&frustum(60.0), &params, &NullLogger);
    let gcode = GcodeGenerator::new(GcodeFlavor::Marlin).generate(&layers, &params);

    // `Flow::spacing()` — width − h·(1 − π/4).
    let expected = 0.4 - 0.2 * (1.0 - std::f64::consts::FRAC_PI_4);

    let mut widths = Vec::new();
    let mut in_support = false;
    for line in gcode.lines() {
        if let Some(rest) = line.strip_prefix(";TYPE:") {
            in_support = rest.trim() == "Support material";
        } else if in_support {
            if let Some(w) = line.strip_prefix(";WIDTH:") {
                widths.push(w.trim_end_matches("mm").parse::<f64>().unwrap());
            }
        }
    }

    assert!(!widths.is_empty(), "the slope must emit support");
    for w in &widths {
        assert!(
            (w - expected).abs() < 0.01,
            "support width {w} should be the flow spacing {expected:.4}, not the nozzle diameter"
        );
    }
}

#[test]
fn support_density_tracks_the_nozzle_size() {
    // Same density on a wider nozzle must lay proportionally more material,
    // because both the pitch and the flow scale with the nominal width. When
    // the pitch was hardcoded to the nozzle and the flow to a fixed spacing,
    // the two disagreed and "15 %" meant something different per nozzle.
    let measure = |nozzle: f64| {
        let params = SlicingParams {
            nozzle_diameter_mm: nozzle,
            ..support_params(45.0)
        };
        support_len(&process_mesh(&frustum(60.0), &params, &NullLogger))
    };
    let narrow = measure(0.4);
    let wide = measure(0.6);
    assert!(
        narrow > 0.0 && wide > 0.0,
        "both nozzles must produce support"
    );
    // A wider bead at the same density covers the same area with fewer, longer
    // passes, so total path length falls rather than rises.
    assert!(
        wide < narrow,
        "a wider nozzle should need less support path for the same area \
         (0.6mm={wide:.0}mm, 0.4mm={narrow:.0}mm)"
    );
}

#[test]
fn the_xy_clearance_is_measured_from_the_model_surface() {
    // `support_xy_distance_mm` is the air gap a user expects between the print
    // and its support. The footprints it is applied to are outer-wall bead
    // *centrelines*, which sit half a bead inside the surface, so inflating by
    // the raw distance left only `xy - half_bead` of real air — 0.6 mm of a
    // requested 0.8 mm at defaults.
    let mut m = Mesh::new();
    add_box(&mut m, (20.0, 20.0, 0.0), (30.0, 30.0, 10.0)); // post
    add_box(&mut m, (0.0, 0.0, 10.0), (30.0, 30.0, 12.0)); // cap overhangs to x=0
    m.vertices = m.faces.iter().flat_map(|f| f.vertices).collect();
    m.calculate_aabb();

    let requested = 0.8;
    let params = SlicingParams {
        support_xy_distance_mm: requested,
        ..support_params(45.0)
    };
    let layers = process_mesh(&m, &params, &NullLogger);

    // Half the support bead, which extends from its centreline toward the model.
    let half_bead = 0.5 * (0.4 - 0.2 * (1.0 - std::f64::consts::FRAC_PI_4));

    // Closest approach to the post's x = 20 face, over the post's own height.
    let mut nearest_edge = f64::MIN;
    for layer in &layers {
        if layer.z <= 1.0 || layer.z >= 9.0 {
            continue;
        }
        for (i, path) in layer.paths.iter().enumerate() {
            if layer.role_for_path(i) != ExtrusionRole::Support {
                continue;
            }
            for p in path.iter() {
                if p.y() > 20.0 && p.y() < 30.0 {
                    nearest_edge = nearest_edge.max(p.x() + half_bead);
                }
            }
        }
    }

    assert!(
        nearest_edge > f64::MIN,
        "the cap overhang must produce support beside the post"
    );
    let gap = 20.0 - nearest_edge;
    assert!(
        (gap - requested).abs() < 0.12,
        "support should stand {requested} mm off the model surface, measured {gap:.3} mm"
    );
}
