use std::collections::BTreeMap;
use std::fmt::Write as FmtWrite;
use std::path::Path;

use super::types::{DebugGeometry, DebugPath};

const SVG_PADDING: f64 = 2.0;

/// Write per-layer SVG files to `dir`.
///
/// For each unique layer index a file `layer_NNNN.svg` is created.  All
/// records that belong to that layer are grouped by stage; each group becomes
/// a `<g id="<stage_id>">` element containing one `<path>` per polygon.
/// The view-box is computed from the union of all path bounding boxes in that
/// layer.
pub fn write_svgs(geometry: &DebugGeometry, dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;

    // Group records by layer_index (BTreeMap for deterministic ordering).
    let mut by_layer: BTreeMap<usize, Vec<&DebugPath>> = BTreeMap::new();
    for record in &geometry.records {
        by_layer.entry(record.layer_index).or_default().push(record);
    }

    for (layer_index, records) in &by_layer {
        let filename = dir.join(format!("layer_{:04}.svg", layer_index));
        let svg = build_layer_svg(records)?;
        std::fs::write(&filename, svg)?;
    }

    Ok(())
}

fn build_layer_svg(records: &[&DebugPath]) -> anyhow::Result<String> {
    let (min_x, min_y, max_x, max_y) = bounding_box(records);

    // SVG Y-axis increases downward; slicer Y-axis increases upward.
    // Negate Y throughout so geometry is right-side-up.
    let svg_min_y = -max_y;
    let svg_max_y = -min_y;

    let vb_x = min_x - SVG_PADDING;
    let vb_y = svg_min_y - SVG_PADDING;
    let vb_w = (max_x - min_x) + SVG_PADDING * 2.0;
    let vb_h = (svg_max_y - svg_min_y) + SVG_PADDING * 2.0;

    let z = records.first().map(|r| r.z).unwrap_or(0.0);

    let mut svg = String::new();
    writeln!(svg, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
    writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" viewBox="{:.4} {:.4} {:.4} {:.4}">"#,
        vb_x, vb_y, vb_w, vb_h
    )?;

    // Title
    writeln!(
        svg,
        r#"  <title>Layer {} — z={:.3} mm</title>"#,
        records.first().map(|r| r.layer_index).unwrap_or(0),
        z
    )?;

    // Collect stage ordering: use the order first encountered
    let mut stage_order: Vec<String> = Vec::new();
    for record in records {
        let id = record.stage.id();
        if !stage_order.contains(&id) {
            stage_order.push(id);
        }
    }

    // Group records by stage id
    let mut by_stage: BTreeMap<String, Vec<&DebugPath>> = BTreeMap::new();
    for record in records {
        by_stage.entry(record.stage.id()).or_default().push(record);
    }

    // Emit groups in encounter order
    for stage_id in &stage_order {
        let stage_records = match by_stage.get(stage_id) {
            Some(v) => v,
            None => continue,
        };

        let color = stage_records
            .first()
            .map(|r| r.stage.svg_color())
            .unwrap_or("#000000");
        let label = stage_records
            .first()
            .map(|r| r.stage.label())
            .unwrap_or_default();

        writeln!(
            svg,
            r#"  <g id="{}" inkscape:label="{}" fill="none" stroke="{}" stroke-width="0.15" stroke-linecap="round" stroke-linejoin="round">"#,
            stage_id, label, color
        )?;

        for record in stage_records {
            for path in record.paths.iter() {
                if path.len() < 2 {
                    continue;
                }
                let d = path_to_svg_d(path);
                writeln!(svg, r#"    <path d="{}" />"#, d)?;
            }
        }

        writeln!(svg, "  </g>")?;
    }

    // Legend — placed at top-left of the (Y-flipped) viewport
    emit_legend(&mut svg, records, min_x, svg_min_y, &stage_order, &by_stage)?;

    writeln!(svg, "</svg>")?;
    Ok(svg)
}

fn path_to_svg_d(path: &clipper2::Path) -> String {
    let mut d = String::new();
    for (i, pt) in path.iter().enumerate() {
        // Negate Y to convert slicer (Y-up) → SVG (Y-down) coordinates.
        if i == 0 {
            let _ = write!(d, "M {:.4} {:.4}", pt.x(), -pt.y());
        } else {
            let _ = write!(d, " L {:.4} {:.4}", pt.x(), -pt.y());
        }
    }
    d.push_str(" Z");
    d
}

fn bounding_box(records: &[&DebugPath]) -> (f64, f64, f64, f64) {
    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;

    for record in records {
        for path in record.paths.iter() {
            for pt in path.iter() {
                let x = pt.x();
                let y = pt.y();
                if x < min_x {
                    min_x = x;
                }
                if y < min_y {
                    min_y = y;
                }
                if x > max_x {
                    max_x = x;
                }
                if y > max_y {
                    max_y = y;
                }
            }
        }
    }

    if min_x == f64::MAX {
        (0.0, 0.0, 10.0, 10.0)
    } else {
        (min_x, min_y, max_x, max_y)
    }
}

fn emit_legend(
    svg: &mut String,
    _records: &[&DebugPath],
    min_x: f64,
    min_y: f64,
    stage_order: &[String],
    by_stage: &BTreeMap<String, Vec<&DebugPath>>,
) -> anyhow::Result<()> {
    let legend_x = min_x - SVG_PADDING;
    let mut legend_y = min_y - SVG_PADDING;
    let row_h = 2.5_f64;
    let swatch_w = 3.0_f64;

    writeln!(
        svg,
        r#"  <g id="legend" font-family="monospace" font-size="1.8">"#
    )?;

    for stage_id in stage_order {
        let stage_records = match by_stage.get(stage_id) {
            Some(v) => v,
            None => continue,
        };
        let color = stage_records
            .first()
            .map(|r| r.stage.svg_color())
            .unwrap_or("#000000");
        let label = stage_records
            .first()
            .map(|r| r.stage.label())
            .unwrap_or_default();

        writeln!(
            svg,
            r#"    <rect x="{:.4}" y="{:.4}" width="{:.4}" height="1.8" fill="{}" />"#,
            legend_x, legend_y, swatch_w, color
        )?;
        writeln!(
            svg,
            "    <text x=\"{:.4}\" y=\"{:.4}\" fill=\"#333333\">{}</text>",
            legend_x + swatch_w + 0.5,
            legend_y + 1.6,
            xml_escape(&label)
        )?;

        legend_y += row_h;
    }

    writeln!(svg, "  </g>")?;
    Ok(())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
