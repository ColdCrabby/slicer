//! Mesh-check command — validate (and optionally repair) a model without slicing.
//!
//! The same pass every model goes through on import, exposed on its own so a
//! defective STL can be diagnosed directly:
//!
//! ```text
//! slicer-engine mesh-check --input model.stl
//! slicer-engine mesh-check --input model.stl --output-format json
//! ```
//!
//! Exits non-zero when defects remain after the repair pass, so it can gate a
//! pipeline.

use clap::Parser;
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::cli::emit::Emitter;
use crate::cli::output::{EmitPayload, OutputFormat};
use crate::mesh::repair::{MeshReport, RepairOptions};

/// Validate a 3D model and report (or repair) its defects.
#[derive(Parser, Debug)]
pub struct MeshCheckCommand {
    /// Path to the model file (.stl, .obj, .3mf)
    #[arg(short, long)]
    pub input: PathBuf,

    /// Output format (json, human)
    #[arg(long, default_value = "human")]
    pub output_format: String,

    /// Report defects without attempting to repair them.
    #[arg(long)]
    pub no_mesh_repair: bool,

    /// Fail (exit non-zero) when the model has any defect, even one that was
    /// repaired. By default only *unrepairable* defects fail.
    #[arg(long)]
    pub strict: bool,
}

/// Result payload emitted by the `mesh-check` command.
struct MeshCheckResult {
    input_name: String,
    repair_enabled: bool,
    report: MeshReport,
}

impl EmitPayload for MeshCheckResult {
    fn schema(&self) -> &'static str {
        "slicer-engine/mesh-check-result-v1"
    }

    fn display_human(&self) -> String {
        let d = &self.report.before;
        let mut s = format!("Mesh check: {}", self.input_name);
        s.push_str(&format!(
            "\n  Triangles: {}\n  Vertices:  {}\n  Shells:    {}",
            d.triangles, d.vertices, d.shells
        ));

        match d.defect_summary() {
            None => s.push_str("\n  Status:    clean — watertight, manifold and outward-facing"),
            Some(found) => {
                s.push_str(&format!("\n  Defects:   {found}"));
                s.push_str("\n  Detail:");
                for (singular, plural, count) in [
                    (
                        "degenerate triangle",
                        "degenerate triangles",
                        d.degenerate_faces,
                    ),
                    (
                        "duplicate triangle",
                        "duplicate triangles",
                        d.duplicate_faces,
                    ),
                    ("hole", "holes", d.holes),
                    ("boundary edge", "boundary edges", d.boundary_edges),
                    (
                        "non-manifold edge",
                        "non-manifold edges",
                        d.non_manifold_edges,
                    ),
                    (
                        "inconsistently wound edge",
                        "inconsistently wound edges",
                        d.inconsistent_winding_edges,
                    ),
                    ("inverted shell", "inverted shells", d.inverted_shells),
                ] {
                    if count > 0 {
                        let label = if count == 1 { singular } else { plural };
                        s.push_str(&format!("\n    {count:>8}  {label}"));
                    }
                }
                if d.holes > 0 {
                    s.push_str(&format!(
                        "\n    {:>8}  edges in the largest hole",
                        d.largest_hole_edges
                    ));
                }
            }
        }

        if !self.repair_enabled {
            s.push_str("\n  Repair:    skipped (--no-mesh-repair)");
        } else if let Some(taken) = self.report.actions.summary() {
            s.push_str(&format!("\n  Repaired:  {taken}"));
            if self.report.actions.unfilled_holes > 0 {
                s.push_str(&format!(
                    "\n             {} hole(s) left open — larger than the cap limit",
                    self.report.actions.unfilled_holes
                ));
            }
        }

        match self.report.after.defect_summary() {
            Some(left) if self.repair_enabled => {
                s.push_str(&format!("\n  Remaining: {left}"));
            }
            _ => {}
        }

        // Informational, never a defect: a zero-area boundary loop (typically a
        // T-junction) encloses nothing, so the surface still bounds the same
        // solid. Worth showing because it explains stray open edges.
        let slits = self.report.after.slit_boundary_edges;
        if slits > 0 {
            s.push_str(&format!(
                "\n  Note:      {slits} zero-area slit {} (no enclosed volume — nothing to fill)",
                if slits == 1 { "edge" } else { "edges" }
            ));
        }

        s
    }

    fn to_json(&self) -> Value {
        json!({
            "input": self.input_name,
            "repair_enabled": self.repair_enabled,
            "clean": !self.report.is_noteworthy(),
            "remaining_defects": self.report.has_remaining_defects(),
            "mesh": self.report,
        })
    }
}

impl MeshCheckCommand {
    /// Execute the mesh-check command.
    pub fn execute(&self) -> Result<(), Box<dyn std::error::Error>> {
        let format = self
            .output_format
            .parse::<OutputFormat>()
            .map_err(|e| format!("Invalid output format: {}", e))?;

        if !self.input.exists() {
            return Err(format!("Input file not found: {}", self.input.display()).into());
        }

        let options = if self.no_mesh_repair {
            RepairOptions::analysis_only()
        } else {
            RepairOptions::default()
        };
        let (_, report) = crate::scene::load_path_reporting(&self.input, &options)
            .map_err(|e| format!("Failed to load mesh '{}': {}", self.input.display(), e))?;

        let failed = if self.strict {
            report.is_noteworthy()
        } else {
            report.has_remaining_defects()
        };

        let result = MeshCheckResult {
            input_name: self.input.display().to_string(),
            repair_enabled: !self.no_mesh_repair,
            report,
        };
        Emitter::new(format).emit(&result);

        if failed {
            return Err("mesh check failed: the model still has defects".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::repair::{MeshDiagnostics, RepairActions};

    fn report_with(before: MeshDiagnostics, actions: RepairActions) -> MeshReport {
        MeshReport {
            after: MeshDiagnostics {
                triangles: before.triangles,
                vertices: before.vertices,
                shells: before.shells,
                ..Default::default()
            },
            before,
            repaired: !actions.is_empty(),
            actions,
            summary: "test".to_string(),
        }
    }

    #[test]
    fn human_output_reports_a_clean_mesh() {
        let result = MeshCheckResult {
            input_name: "cube.stl".to_string(),
            repair_enabled: true,
            report: report_with(
                MeshDiagnostics {
                    triangles: 12,
                    vertices: 8,
                    shells: 1,
                    ..Default::default()
                },
                RepairActions::default(),
            ),
        };
        let s = result.display_human();
        assert!(s.contains("cube.stl"));
        assert!(s.contains("Triangles: 12"));
        assert!(s.contains("clean"));
        assert!(!s.contains("Defects"));
    }

    #[test]
    fn human_output_itemises_defects_and_repairs() {
        let result = MeshCheckResult {
            input_name: "broken.stl".to_string(),
            repair_enabled: true,
            report: report_with(
                MeshDiagnostics {
                    triangles: 10,
                    vertices: 8,
                    shells: 1,
                    holes: 1,
                    boundary_edges: 4,
                    largest_hole_edges: 4,
                    ..Default::default()
                },
                RepairActions {
                    filled_holes: 1,
                    added_fill_triangles: 4,
                    ..Default::default()
                },
            ),
        };
        let s = result.display_human();
        assert!(s.contains("Defects"));
        assert!(s.contains("boundary edges"));
        assert!(s.contains("edges in the largest hole"));
        assert!(s.contains("Repaired"));
    }

    #[test]
    fn json_output_carries_the_full_report() {
        let result = MeshCheckResult {
            input_name: "broken.stl".to_string(),
            repair_enabled: false,
            report: report_with(
                MeshDiagnostics {
                    triangles: 10,
                    non_manifold_edges: 2,
                    ..Default::default()
                },
                RepairActions::default(),
            ),
        };
        let v = result.to_json();
        assert_eq!(v["input"], "broken.stl");
        assert_eq!(v["repair_enabled"], false);
        assert_eq!(v["clean"], false);
        assert_eq!(v["mesh"]["before"]["non_manifold_edges"], 2);
    }

    #[test]
    fn missing_input_is_an_error() {
        let cmd = MeshCheckCommand {
            input: PathBuf::from("definitely-not-here.stl"),
            output_format: "human".to_string(),
            no_mesh_repair: false,
            strict: false,
        };
        assert!(cmd.execute().is_err());
    }
}
