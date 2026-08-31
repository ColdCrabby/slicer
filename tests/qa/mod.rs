//! Shared support code for the slicing-quality regression gate.
//!
//! This module is deliberately *not* a cargo test target — it lives in a
//! subdirectory of `tests/` so it is only compiled when a sibling integration
//! test (`tests/slicing_quality.rs`) pulls it in with `mod qa;`.
//!
//! It gives three things the raw `cargo test` suite cannot:
//!   1. **G-code metrics** measured from the final artifact (not intermediate
//!      geometry), so generator bugs are caught too.
//!   2. **Invariants** — properties that must hold for *any* valid slice.
//!   3. **Baseline comparison** with tolerances, so a failure names the exact
//!      metric that drifted and by how much (the "quick diagnose" the plain
//!      assertion tests lack).
//!
//! The heavy `benchy` fixture is excluded from the fast local gate and only
//! runs under `QA_FULL=1` / `QA_REPORT=1` (set by CI).

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use slicer_engine::debug::{svg::write_svgs, DebugGeometry};
use slicer_engine::gcode::{GcodeFlavor, GcodeGenerator};
use slicer_engine::logging::NullLogger;
use slicer_engine::mesh::types::Mesh;
use slicer_engine::scene::{apply_transform, load_path, BedConfig, SceneOp, SceneState};
use slicer_engine::settings::params::{SlicingParams, WallGenerator};

/// Canonical role column order for stable, glanceable report tables.
pub const ROLE_ORDER: &[&str] = &[
    "outer_wall",
    "inner_wall",
    "overhang_wall",
    "gap_fill",
    "infill",
    "top_surface",
    "bottom_surface",
    "bridge",
    "skirt",
    "support",
    "other",
];

/// One entry in the QA corpus.
pub struct Fixture {
    /// Short stable key used in filenames and report sections.
    pub name: &'static str,
    /// Filename relative to the crate root.
    pub file: &'static str,
    /// Included in the fast local gate (plain `cargo test`). Heavy models are
    /// `false` and only run under `QA_FULL=1` / `QA_REPORT=1`.
    pub fast_gate: bool,
}

/// The prepared fixture corpus (Voron cube, hinge, caddy, Benchy).
pub fn corpus() -> Vec<Fixture> {
    vec![
        Fixture {
            name: "voron",
            file: "Voron_Design_Cube_v7.stl",
            fast_gate: true,
        },
        Fixture {
            name: "hinge",
            file: "bottom_panel_hinge_x2.stl",
            fast_gate: true,
        },
        Fixture {
            name: "caddy",
            file: "Filament_Card_Caddy_25.stl",
            fast_gate: true,
        },
        Fixture {
            name: "benchy",
            file: "3DBenchy.stl",
            fast_gate: false,
        },
    ]
}

/// The two wall generators every fixture is sliced with.
pub fn generators() -> [WallGenerator; 2] {
    [WallGenerator::Arachne, WallGenerator::Classic]
}

pub fn generator_key(g: WallGenerator) -> &'static str {
    g.name()
}

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn fixture_path(fx: &Fixture) -> PathBuf {
    crate_root().join(fx.file)
}

pub fn baseline_path(name: &str, g: WallGenerator) -> PathBuf {
    crate_root()
        .join("tests/qa/baselines")
        .join(format!("{}_{}.json", name, generator_key(g)))
}

pub fn out_dir() -> PathBuf {
    crate_root().join("target/qa")
}

// ── Mesh dimensions ─────────────────────────────────────────────────────────

/// Translation-invariant model dimensions, used by the invariants.
#[derive(Debug, Clone, Copy)]
pub struct MeshDims {
    pub dx: f64,
    pub dy: f64,
    pub dz: f64,
}

pub fn mesh_dims(mesh: &Mesh) -> MeshDims {
    let (mut min, mut max) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for f in &mesh.faces {
        for v in &f.vertices {
            for (i, c) in [v.x, v.y, v.z].into_iter().enumerate() {
                min[i] = min[i].min(c);
                max[i] = max[i].max(c);
            }
        }
    }
    MeshDims {
        dx: max[0] - min[0],
        dy: max[1] - min[1],
        dz: max[2] - min[2],
    }
}

/// Load a fixture and place it on the bed exactly as a real slice would —
/// centered and dropped to the floor through the scene engine (the single
/// source of truth), so `z_max` reflects printer space and overhang/bridge
/// classification matches production.
pub fn prepare(path: &Path) -> anyhow::Result<Mesh> {
    let mesh = load_path(path).map_err(|e| anyhow::anyhow!("load {}: {e}", path.display()))?;
    let mut scene = SceneState::new(BedConfig::default());
    let id = scene.add_mesh("qa".to_string(), Arc::new(mesh));
    scene
        .apply(SceneOp::CenterOnBed { id })
        .map_err(|e| anyhow::anyhow!("center: {e:?}"))?;
    scene
        .apply(SceneOp::DropToFloor { id })
        .map_err(|e| anyhow::anyhow!("drop: {e:?}"))?;
    let obj = scene
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("scene object vanished"))?;
    Ok(apply_transform(obj.mesh.as_ref(), &obj.transform))
}

// ── Slicing ─────────────────────────────────────────────────────────────────

const LAYER_HEIGHT: f64 = 0.2;

fn slice_params(g: WallGenerator) -> SlicingParams {
    SlicingParams {
        wall_generator: g,
        layer_height: LAYER_HEIGHT,
        ..Default::default()
    }
}

/// Slice a placed mesh to G-code via the production (parallel) pipeline. This
/// is the only path metrics are measured from.
pub fn slice_to_gcode(mesh: &Mesh, g: WallGenerator) -> String {
    let params = slice_params(g);
    let layers = slicer_engine::core::process_mesh(mesh, &params, &NullLogger);
    GcodeGenerator::new(GcodeFlavor::Marlin).generate(&layers, &params)
}

/// Capture per-stage geometry for the visual gallery only. This uses the debug
/// pipeline (sequential Arachne), which can emit slightly different medial
/// gap-fill than production, so it is deliberately **not** used for metrics.
pub fn debug_geometry(mesh: &Mesh, g: WallGenerator) -> DebugGeometry {
    let params = slice_params(g);
    let mut dg = DebugGeometry::new();
    let _ = slicer_engine::core::process_mesh_debug(mesh, &params, &NullLogger, &mut dg);
    dg
}

// ── G-code parsing ──────────────────────────────────────────────────────────

/// The measurements pulled straight out of a `.gcode` string.
#[derive(Debug, Default)]
pub struct Parsed {
    pub layer_count: usize,
    pub total_mm: f64,
    pub role_mm: BTreeMap<String, f64>,
    /// Extruded length per layer, index-aligned to `;LAYER_CHANGE` order.
    pub per_layer_mm: Vec<f64>,
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub max_z: f64,
    /// Count of malformed / non-finite X/Y/Z/E words. Must be zero.
    pub nonfinite: usize,
}

/// Map an OrcaSlicer-style `;TYPE:` label to a stable snake_case key.
pub fn canon_role(t: &str) -> String {
    let l = t.trim().to_ascii_lowercase();
    let key = if l.contains("bridge") {
        "bridge"
    } else if l.contains("ironing") {
        // Ahead of the "top" test: ironing sweeps a top surface but is measured
        // separately, and without this it would fall through to `other` and trip
        // the "new role appeared" check the moment anyone enables it.
        "ironing"
    } else if l.contains("overhang") {
        "overhang_wall"
    } else if l.contains("skirt") || l.contains("brim") {
        "skirt"
    } else if l.contains("support") {
        "support"
    } else if l.contains("gap") {
        "gap_fill"
    } else if l.contains("outer") {
        "outer_wall"
    } else if l.contains("inner") {
        "inner_wall"
    } else if l.contains("top") {
        "top_surface"
    } else if l.contains("bottom") {
        "bottom_surface"
    } else if l.contains("infill") || l.contains("sparse") || l.contains("solid") {
        "infill"
    } else {
        "other"
    };
    key.to_string()
}

fn parse_finite(s: &str, nonfinite: &mut usize) -> Option<f64> {
    match s.parse::<f64>() {
        Ok(v) if v.is_finite() => Some(v),
        _ => {
            *nonfinite += 1;
            None
        }
    }
}

/// Parse Marlin-style G-code (absolute XYZ, `M82`/`M83` E mode, `G92` resets)
/// into aggregate print metrics.
pub fn parse_gcode(text: &str) -> Parsed {
    let mut p = Parsed {
        min_x: f64::INFINITY,
        min_y: f64::INFINITY,
        max_x: f64::NEG_INFINITY,
        max_y: f64::NEG_INFINITY,
        max_z: f64::NEG_INFINITY,
        ..Default::default()
    };

    let (mut x, mut y, mut z, mut e) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    let mut abs_e = true;
    let mut role = "outer_wall".to_string();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix(';') {
            let rest = rest.trim();
            if rest == "LAYER_CHANGE" {
                p.layer_count += 1;
                p.per_layer_mm.push(0.0);
            } else if let Some(t) = rest.strip_prefix("TYPE:") {
                role = canon_role(t);
            }
            continue;
        }

        let code = match line.find(';') {
            Some(i) => line[..i].trim(),
            None => line,
        };
        if code.is_empty() {
            continue;
        }

        let mut it = code.split_whitespace();
        let cmd = it.next().unwrap_or("");
        match cmd {
            "G0" | "G1" => {
                let (mut nx, mut ny, mut nz) = (x, y, z);
                let mut e_word: Option<f64> = None;
                let mut moved_xy = false;
                for tok in it {
                    if tok.len() < 2 {
                        continue;
                    }
                    let letter = tok.as_bytes()[0].to_ascii_uppercase();
                    let val = &tok[1..];
                    match letter {
                        b'X' => {
                            if let Some(n) = parse_finite(val, &mut p.nonfinite) {
                                nx = n;
                                moved_xy = true;
                            }
                        }
                        b'Y' => {
                            if let Some(n) = parse_finite(val, &mut p.nonfinite) {
                                ny = n;
                                moved_xy = true;
                            }
                        }
                        b'Z' => {
                            if let Some(n) = parse_finite(val, &mut p.nonfinite) {
                                nz = n;
                            }
                        }
                        b'E' => {
                            e_word = parse_finite(val, &mut p.nonfinite);
                        }
                        _ => {}
                    }
                }

                let delta_e = match e_word {
                    Some(ew) if abs_e => {
                        let d = ew - e;
                        e = ew;
                        d
                    }
                    Some(ew) => {
                        e += ew;
                        ew
                    }
                    None => 0.0,
                };

                if moved_xy && delta_e > 1e-9 {
                    let len = ((nx - x).powi(2) + (ny - y).powi(2)).sqrt();
                    p.total_mm += len;
                    *p.role_mm.entry(role.clone()).or_default() += len;
                    if let Some(l) = p.per_layer_mm.last_mut() {
                        *l += len;
                    }
                    for (px, py) in [(x, y), (nx, ny)] {
                        p.min_x = p.min_x.min(px);
                        p.max_x = p.max_x.max(px);
                        p.min_y = p.min_y.min(py);
                        p.max_y = p.max_y.max(py);
                    }
                    p.max_z = p.max_z.max(nz);
                }

                x = nx;
                y = ny;
                z = nz;
            }
            "G92" => {
                for tok in code.split_whitespace().skip(1) {
                    if tok.len() >= 2 && tok.as_bytes()[0].eq_ignore_ascii_case(&b'E') {
                        if let Some(n) = parse_finite(&tok[1..], &mut p.nonfinite) {
                            e = n;
                        }
                    }
                }
            }
            "M82" => abs_e = true,
            "M83" => abs_e = false,
            _ => {}
        }
    }

    p
}

// ── Metrics ─────────────────────────────────────────────────────────────────

/// The committed, tolerance-compared regression signal per fixture×generator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureMetrics {
    pub fixture: String,
    pub generator: String,
    pub layer_count: usize,
    pub z_max: f64,
    pub bbox_x_mm: f64,
    pub bbox_y_mm: f64,
    pub total_extrusion_mm: f64,
    pub role_extrusion_mm: BTreeMap<String, f64>,
}

fn round_to(v: f64, decimals: i32) -> f64 {
    let f = 10f64.powi(decimals);
    (v * f).round() / f
}

pub fn metrics_from(fx: &str, g: WallGenerator, parsed: &Parsed) -> FixtureMetrics {
    let bbox_x = if parsed.max_x >= parsed.min_x {
        parsed.max_x - parsed.min_x
    } else {
        0.0
    };
    let bbox_y = if parsed.max_y >= parsed.min_y {
        parsed.max_y - parsed.min_y
    } else {
        0.0
    };
    let role = parsed
        .role_mm
        .iter()
        .map(|(k, v)| (k.clone(), round_to(*v, 1)))
        .collect();
    FixtureMetrics {
        fixture: fx.to_string(),
        generator: generator_key(g).to_string(),
        layer_count: parsed.layer_count,
        z_max: round_to(parsed.max_z, 3),
        bbox_x_mm: round_to(bbox_x, 2),
        bbox_y_mm: round_to(bbox_y, 2),
        total_extrusion_mm: round_to(parsed.total_mm, 1),
        role_extrusion_mm: role,
    }
}

// ── Invariants (baseline-independent) ───────────────────────────────────────

/// Check properties that must hold for any valid slice. Returns human-readable
/// violation messages; empty means the slice is structurally sound.
pub fn check_invariants(parsed: &Parsed, m: &FixtureMetrics, dims: MeshDims) -> Vec<String> {
    let mut v = Vec::new();
    let tag = format!("{}/{}", m.fixture, m.generator);

    if parsed.nonfinite > 0 {
        v.push(format!(
            "{tag}: {} non-finite/malformed coordinate word(s)",
            parsed.nonfinite
        ));
    }

    let expected = (dims.dz / LAYER_HEIGHT).round() as i64;
    let got = m.layer_count as i64;
    if (got - expected).abs() > 2 {
        v.push(format!(
            "{tag}: layer_count {got} not within ±2 of expected {expected} (height {:.2}mm)",
            dims.dz
        ));
    }

    let zero_layers = parsed.per_layer_mm.iter().filter(|l| **l <= 0.0).count();
    if zero_layers > 0 {
        v.push(format!("{tag}: {zero_layers} layer(s) extrude nothing"));
    }

    // Print footprint must be inside the model envelope (walls are inset, so it
    // is smaller) and must not collapse or balloon.
    for (axis, bbox, model) in [("x", m.bbox_x_mm, dims.dx), ("y", m.bbox_y_mm, dims.dy)] {
        if bbox > model + 1.0 {
            v.push(format!(
                "{tag}: bbox_{axis} {bbox:.2} exceeds model {model:.2} + 1.0mm"
            ));
        }
        if bbox < model - 3.0 {
            v.push(format!(
                "{tag}: bbox_{axis} {bbox:.2} collapsed below model {model:.2} - 3.0mm"
            ));
        }
    }

    let wall = m
        .role_extrusion_mm
        .get("outer_wall")
        .copied()
        .unwrap_or(0.0);
    if wall <= 0.0 {
        v.push(format!("{tag}: no outer_wall extrusion"));
    }
    let fill: f64 = ["infill", "top_surface", "bottom_surface"]
        .iter()
        .map(|k| m.role_extrusion_mm.get(*k).copied().unwrap_or(0.0))
        .sum();
    if fill <= 0.0 {
        v.push(format!("{tag}: no infill or solid-surface extrusion"));
    }

    v
}

// ── Baseline comparison (tolerance-based) ───────────────────────────────────

/// Absolute + relative tolerance. A value passes when it is within `abs` OR
/// within `rel` of the baseline — whichever is larger — so cross-platform float
/// noise does not flap while real regressions (missing infill, doubled walls,
/// collapsed dimensions) still trip.
struct Tol {
    abs: f64,
    rel: f64,
}

fn within(base: f64, cur: f64, t: &Tol) -> bool {
    (cur - base).abs() <= t.abs.max(t.rel * base.abs())
}

pub fn load_baseline(path: &Path) -> Option<FixtureMetrics> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn write_baseline(path: &Path, m: &FixtureMetrics) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(m)?;
    std::fs::write(path, json + "\n")?;
    Ok(())
}

/// Compare current metrics to a committed baseline, returning one message per
/// drifted metric.
pub fn compare_baseline(base: &FixtureMetrics, cur: &FixtureMetrics) -> Vec<String> {
    let mut d = Vec::new();
    let tag = format!("{}/{}", cur.fixture, cur.generator);

    if base.layer_count != cur.layer_count {
        d.push(format!(
            "{tag}: layer_count {} -> {} (Δ{:+})",
            base.layer_count,
            cur.layer_count,
            cur.layer_count as i64 - base.layer_count as i64
        ));
    }

    let len_tol = Tol {
        abs: 0.5,
        // 5% absorbs cross-platform/opt-level float variance in the
        // float-sensitive medial (gap-fill) beads while still tripping on real
        // regressions, which move lengths by far more (a dropped infill or a
        // doubled wall is a 100%+ swing).
        rel: 0.05,
    };
    let dim_tol = Tol {
        abs: 0.25,
        rel: 0.01,
    };

    let scalars: [(&str, f64, f64, &Tol); 4] = [
        ("z_max", base.z_max, cur.z_max, &dim_tol),
        ("bbox_x_mm", base.bbox_x_mm, cur.bbox_x_mm, &dim_tol),
        ("bbox_y_mm", base.bbox_y_mm, cur.bbox_y_mm, &dim_tol),
        (
            "total_extrusion_mm",
            base.total_extrusion_mm,
            cur.total_extrusion_mm,
            &len_tol,
        ),
    ];
    for (name, b, c, t) in scalars {
        if !within(b, c, t) {
            d.push(format!("{tag}: {name} {b:.2} -> {c:.2} (Δ{:+.2})", c - b));
        }
    }

    // Role coverage: flag new/missing roles and per-role drift.
    let mut keys: Vec<&String> = base
        .role_extrusion_mm
        .keys()
        .chain(cur.role_extrusion_mm.keys())
        .collect();
    keys.sort();
    keys.dedup();
    for k in keys {
        let b = base.role_extrusion_mm.get(k).copied();
        let c = cur.role_extrusion_mm.get(k).copied();
        match (b, c) {
            (Some(b), Some(c)) if !within(b, c, &len_tol) => {
                d.push(format!("{tag}: role {k} {b:.1} -> {c:.1} (Δ{:+.1})", c - b));
            }
            (Some(b), None) => d.push(format!("{tag}: role {k} disappeared (was {b:.1})")),
            (None, Some(c)) if c > len_tol.abs => {
                d.push(format!("{tag}: new role {k} appeared ({c:.1})"))
            }
            _ => {}
        }
    }

    d
}

// ── Per-case result + report ────────────────────────────────────────────────

/// Everything gathered for one fixture×generator, kept so the report can be
/// written in full before the gate asserts.
pub struct CaseResult {
    pub fixture: String,
    pub generator: WallGenerator,
    pub metrics: FixtureMetrics,
    pub baseline: Option<FixtureMetrics>,
    pub invariant_violations: Vec<String>,
    pub baseline_drift: Vec<String>,
    /// (layer_index, svg_markup) pairs for the visual gallery, if captured.
    pub svgs: Vec<(usize, String)>,
}

impl CaseResult {
    pub fn ok(&self) -> bool {
        self.invariant_violations.is_empty() && self.baseline_drift.is_empty()
    }
}

/// Choose a small, representative set of layer indices for the visual gallery.
pub fn gallery_layers(layer_count: usize) -> Vec<usize> {
    if layer_count == 0 {
        return vec![];
    }
    let last = layer_count - 1;
    let mut idx = vec![
        0,
        1.min(last),
        layer_count / 4,
        layer_count / 2,
        (layer_count * 3) / 4,
        last.saturating_sub(1),
    ];
    idx.retain(|i| *i <= last);
    idx.sort_unstable();
    idx.dedup();
    idx
}

/// Render selected layers of a captured debug run to inline SVG markup.
pub fn render_gallery_svgs(debug: &DebugGeometry, layers: &[usize]) -> Vec<(usize, String)> {
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let wanted: HashSet<usize> = layers.iter().copied().collect();

    let filtered = DebugGeometry {
        records: debug
            .records
            .iter()
            .filter(|r| wanted.contains(&r.layer_index))
            .map(clone_record)
            .collect(),
    };
    if filtered.records.is_empty() {
        return vec![];
    }

    let tmp = std::env::temp_dir().join(format!(
        "qa_svg_{}_{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    if write_svgs(&filtered, &tmp).is_err() {
        return vec![];
    }

    let mut out = Vec::new();
    for &i in layers {
        let f = tmp.join(format!("layer_{:04}.svg", i));
        if let Ok(s) = std::fs::read_to_string(&f) {
            out.push((i, strip_xml_prolog(&s)));
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    out
}

fn clone_record(r: &slicer_engine::debug::DebugPath) -> slicer_engine::debug::DebugPath {
    slicer_engine::debug::DebugPath {
        stage: r.stage.clone(),
        layer_index: r.layer_index,
        z: r.z,
        paths: r.paths.clone(),
        role: r.role,
    }
}

fn strip_xml_prolog(svg: &str) -> String {
    match svg.find("<svg") {
        Some(i) => svg[i..].to_string(),
        None => svg.to_string(),
    }
}

fn fmtn(v: f64, decimals: usize) -> String {
    format!("{:.*}", decimals, v)
}

/// A named scalar row in the report table: label, accessor, decimal places.
type ScalarRow = (&'static str, fn(&FixtureMetrics) -> f64, usize);

fn cell(base: Option<f64>, cur: f64, drifted: bool, decimals: usize) -> String {
    let cls = if drifted { "bad" } else { "ok" };
    match base {
        Some(b) => format!(
            "<td class=\"{cls}\">{}<span class=\"base\"> ({})</span></td>",
            fmtn(cur, decimals),
            fmtn(b, decimals)
        ),
        None => format!("<td class=\"{cls}\">{}</td>", fmtn(cur, decimals)),
    }
}

/// Write a single self-contained `report.html` plus per-case metric JSON dumps.
pub fn write_report(results: &[CaseResult], dir: &Path, with_gallery: bool) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir.join("metrics"))?;
    for r in results {
        let f =
            dir.join("metrics")
                .join(format!("{}_{}.json", r.fixture, generator_key(r.generator)));
        std::fs::write(f, serde_json::to_string_pretty(&r.metrics)? + "\n")?;
    }

    let mut h = String::new();
    h.push_str("<!doctype html><html><head><meta charset=\"utf-8\">");
    h.push_str("<title>Slicer quality report</title><style>");
    h.push_str(
        "body{font:14px/1.5 system-ui,sans-serif;margin:2rem;background:#111;color:#eee}\
         h1{font-size:1.4rem}h2{margin-top:2.5rem;border-bottom:1px solid #333;padding-bottom:.3rem}\
         table{border-collapse:collapse;margin:.5rem 0 1rem}td,th{border:1px solid #333;padding:.25rem .6rem;text-align:right}\
         th:first-child,td:first-child{text-align:left}\
         .ok{color:#9fd} .bad{color:#f88;font-weight:600;background:#3a1414}\
         .base{color:#777;font-size:.85em}\
         .pass{color:#7d7} .fail{color:#f77;font-weight:700}\
         .msg{color:#fb8;font-family:ui-monospace,monospace;font-size:.85em;white-space:pre-wrap}\
         .gallery{display:flex;flex-wrap:wrap;gap:1rem}\
         .cellsvg{background:#0a0a0a;border:1px solid #333;padding:.4rem;border-radius:6px}\
         .cellsvg svg{width:260px;height:260px;background:#000}\
         .lyr{font-size:.8em;color:#aaa;text-align:center}",
    );
    h.push_str("</style></head><body>");
    h.push_str(&format!(
        "<h1>Slicer quality report</h1><p>engine <code>{}</code></p>",
        html_escape(slicer_engine::version::VERSION)
    ));

    let fixtures: Vec<&str> = {
        let mut seen = Vec::new();
        for r in results {
            if !seen.contains(&r.fixture.as_str()) {
                seen.push(r.fixture.as_str());
            }
        }
        seen
    };

    for fx in fixtures {
        let cases: Vec<&CaseResult> = results.iter().filter(|r| r.fixture == fx).collect();
        let status_fail = cases.iter().any(|c| !c.ok());
        h.push_str(&format!(
            "<h2>{} {}</h2>",
            html_escape(fx),
            if status_fail {
                "<span class=\"fail\">✗ FAIL</span>"
            } else {
                "<span class=\"pass\">✓ pass</span>"
            }
        ));

        // Metric table: one column per generator.
        h.push_str("<table><tr><th>metric</th>");
        for c in &cases {
            h.push_str(&format!("<th>{}</th>", generator_key(c.generator)));
        }
        h.push_str("</tr>");

        let drift_has =
            |c: &CaseResult, needle: &str| c.baseline_drift.iter().any(|m| m.contains(needle));

        h.push_str("<tr><td>layer_count</td>");
        for c in &cases {
            let bad = drift_has(c, "layer_count");
            let base = c.baseline.as_ref().map(|b| b.layer_count as f64);
            h.push_str(&cell(base, c.metrics.layer_count as f64, bad, 0));
        }
        h.push_str("</tr>");

        let scalar_rows: [ScalarRow; 4] = [
            ("z_max", |m| m.z_max, 2),
            ("bbox_x_mm", |m| m.bbox_x_mm, 2),
            ("bbox_y_mm", |m| m.bbox_y_mm, 2),
            ("total_extrusion_mm", |m| m.total_extrusion_mm, 1),
        ];
        for (name, get, dec) in scalar_rows {
            h.push_str(&format!("<tr><td>{name}</td>"));
            for c in &cases {
                let bad = drift_has(c, name);
                let base = c.baseline.as_ref().map(get);
                h.push_str(&cell(base, get(&c.metrics), bad, dec));
            }
            h.push_str("</tr>");
        }

        for role in ROLE_ORDER {
            if !cases
                .iter()
                .any(|c| c.metrics.role_extrusion_mm.contains_key(*role))
            {
                continue;
            }
            h.push_str(&format!("<tr><td>role: {role}</td>"));
            for c in &cases {
                let bad = drift_has(c, &format!("role {role}"));
                let base = c
                    .baseline
                    .as_ref()
                    .and_then(|b| b.role_extrusion_mm.get(*role).copied());
                let cur = c
                    .metrics
                    .role_extrusion_mm
                    .get(*role)
                    .copied()
                    .unwrap_or(0.0);
                h.push_str(&cell(base, cur, bad, 1));
            }
            h.push_str("</tr>");
        }
        h.push_str("</table>");

        // Diagnostics: invariant + drift messages.
        for c in &cases {
            if c.ok() {
                continue;
            }
            h.push_str(&format!(
                "<div><strong>{}</strong><div class=\"msg\">{}</div></div>",
                generator_key(c.generator),
                html_escape(
                    &c.invariant_violations
                        .iter()
                        .chain(c.baseline_drift.iter())
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            ));
        }

        // Visual gallery (arachne vs classic per sampled layer).
        if with_gallery && cases.iter().any(|c| !c.svgs.is_empty()) {
            h.push_str("<div class=\"gallery\">");
            for c in &cases {
                for (layer, svg) in &c.svgs {
                    h.push_str(&format!(
                        "<div class=\"cellsvg\"><div class=\"lyr\">{} · layer {}</div>{}</div>",
                        generator_key(c.generator),
                        layer,
                        svg
                    ));
                }
            }
            h.push_str("</div>");
        }
    }

    h.push_str("</body></html>");
    std::fs::write(dir.join("report.html"), h)?;
    Ok(())
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
