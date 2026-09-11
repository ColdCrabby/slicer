//! Slicing quality regression gate.
//!
//! Slices the prepared fixture corpus with both wall generators, measures the
//! resulting G-code, and fails when either a structural **invariant** breaks or
//! a **metric** drifts beyond tolerance from its committed baseline. Every run
//! also writes `target/qa/report.html` (with a visual layer gallery under
//! `QA_REPORT=1`) so a failure can be diagnosed at a glance instead of from a
//! bare assertion.
//!
//! Environment toggles:
//!   - `UPDATE_QA_BASELINES=1` — (re)write baselines instead of comparing.
//!   - `QA_FULL=1` — include the heavy `benchy` fixture.
//!   - `QA_REPORT=1` — capture per-stage geometry and embed SVGs (implies
//!     `QA_FULL`). Set by CI.

mod qa;

use qa::*;

fn flag(name: &str) -> bool {
    std::env::var(name)
        .map(|v| v != "0" && !v.is_empty())
        .unwrap_or(false)
}

#[test]
#[ignore = "golden gate: pinned to one platform+profile; run via the `quality` CI job or `cargo test --test slicing_quality -- --ignored`"]
fn slicing_quality_gate() -> anyhow::Result<()> {
    let update = flag("UPDATE_QA_BASELINES");
    let report = flag("QA_REPORT");
    let full = report || flag("QA_FULL");

    let mut results: Vec<CaseResult> = Vec::new();
    let mut fatal: Vec<String> = Vec::new();

    for fx in corpus() {
        if !fx.fast_gate && !full {
            continue;
        }
        let path = fixture_path(&fx);
        let mesh = match prepare(&path) {
            Ok(m) => m,
            Err(e) => {
                fatal.push(format!("{}: {e}", fx.name));
                continue;
            }
        };
        let dims = mesh_dims(&mesh);

        for g in generators() {
            let gcode = slice_to_gcode(&mesh, g, fx.supports);

            let parsed = parse_gcode(&gcode);
            let metrics = metrics_from(fx.name, g, &parsed);

            if update {
                write_baseline(&baseline_path(fx.name, g), &metrics)?;
            }

            let invariant_violations = check_invariants(&parsed, &metrics, dims);

            let baseline = load_baseline(&baseline_path(fx.name, g));
            let baseline_drift = if update {
                Vec::new()
            } else {
                match &baseline {
                    Some(b) => compare_baseline(b, &metrics),
                    None => {
                        fatal.push(format!(
                            "{}/{}: no baseline — run `UPDATE_QA_BASELINES=1 cargo test --test slicing_quality`",
                            fx.name,
                            generator_key(g)
                        ));
                        Vec::new()
                    }
                }
            };

            let svgs = if report {
                let dg = debug_geometry(&mesh, g, fx.supports);
                render_gallery_svgs(&dg, &gallery_layers(metrics.layer_count))
            } else {
                Vec::new()
            };

            results.push(CaseResult {
                fixture: fx.name.to_string(),
                generator: g,
                metrics,
                baseline,
                invariant_violations,
                baseline_drift,
                svgs,
            });
        }
    }

    // Write artifacts before any assertion so a failing run is still diagnosable.
    write_report(&results, &out_dir(), report)?;

    if update {
        eprintln!(
            "Updated {} baseline(s) under tests/qa/baselines/",
            results.len()
        );
        return Ok(());
    }

    let mut failures = fatal;
    for r in &results {
        failures.extend(r.invariant_violations.iter().cloned());
        failures.extend(r.baseline_drift.iter().cloned());
    }

    assert!(
        failures.is_empty(),
        "slicing quality gate failed ({} issue(s)):\n  {}\n\nOpen the report: {}",
        failures.len(),
        failures.join("\n  "),
        out_dir().join("report.html").display()
    );

    Ok(())
}
