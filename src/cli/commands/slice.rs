//! Slice command - performs 3D model slicing

use crate::cli::emit::{CliLogger, Emitter};
use crate::cli::output::{EmitPayload, OutputFormat};
use crate::config::load_and_merge_config;
use crate::gcode::{resolve_gcode_source, GcodeFlavor, GcodeGenerator};
use crate::infill::InfillPattern;
use crate::logging::{phases, PhaseTimer, ProcessLogger};
use crate::mesh::analysis::{calculate_aabb, calculate_surface_area, calculate_volume};
use crate::scene::{apply_transform, BedConfig, SceneOp, SceneState};
use crate::settings::params::{LifecycleMarkerConfig, MeshQuality};
use clap::Parser;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

/// Parse `x,y,z` (three comma-separated floats) into `[f64; 3]`.
fn parse_vec3(s: &str) -> Result<[f64; 3], String> {
    let parts: Vec<&str> = s.split(',').map(str::trim).collect();
    if parts.len() != 3 {
        return Err(format!(
            "expected three comma-separated values (x,y,z), got '{}'",
            s
        ));
    }
    let mut out = [0.0; 3];
    for (i, p) in parts.iter().enumerate() {
        out[i] = p
            .parse::<f64>()
            .map_err(|e| format!("invalid number '{}': {}", p, e))?;
    }
    Ok(out)
}

/// Parse `axis:degrees` where axis is `x`, `y`, or `z` (case-insensitive),
/// optionally prefixed with `-` to negate.
fn parse_rotate(s: &str) -> Result<([f32; 3], f32), String> {
    let (axis_str, deg_str) = s
        .split_once(':')
        .ok_or_else(|| format!("expected 'axis:degrees', got '{}'", s))?;
    let deg: f32 = deg_str
        .trim()
        .parse()
        .map_err(|e| format!("invalid degrees '{}': {}", deg_str, e))?;
    let trimmed = axis_str.trim();
    let (sign, axis_char) = if let Some(rest) = trimmed.strip_prefix('-') {
        (-1.0_f32, rest)
    } else {
        (1.0_f32, trimmed)
    };
    let axis = match axis_char.to_ascii_lowercase().as_str() {
        "x" => [sign, 0.0, 0.0],
        "y" => [0.0, sign, 0.0],
        "z" => [0.0, 0.0, sign],
        other => return Err(format!("unknown axis '{}'; expected x, y, or z", other)),
    };
    Ok((axis, deg.to_radians()))
}

/// Parse `s` (uniform scale) or `x,y,z` (non-uniform).
fn parse_scale(s: &str) -> Result<[f32; 3], String> {
    if s.contains(',') {
        let v = parse_vec3(s)?;
        Ok([v[0] as f32, v[1] as f32, v[2] as f32])
    } else {
        let f: f32 = s
            .trim()
            .parse()
            .map_err(|e| format!("invalid scale '{}': {}", s, e))?;
        Ok([f, f, f])
    }
}

/// Slice a 3D model into layers
#[derive(Parser, Debug)]
pub struct SliceCommand {
    /// Input model file path (STL, OBJ, or 3MF)
    #[arg(short, long)]
    pub input: PathBuf,

    /// Layer height in millimeters
    #[arg(short = 'l', long)]
    pub layer_height: Option<f64>,

    /// Output file path (auto-generated if not specified)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Output format (json, human)
    #[arg(long, default_value = "human")]
    pub output_format: String,

    /// G-code firmware flavor (marlin, klipper).
    /// When omitted, falls back to the value stored in global settings (default: marlin).
    #[arg(long)]
    pub gcode_flavor: Option<String>,

    /// Custom start G-code (overrides dialect default and global settings).
    ///
    /// Accepts either a file path (auto-detected when the path exists) or a
    /// direct G-code string.  Multiple lines may be separated with `\n`.
    ///
    /// Examples:
    ///   --start-print-gcode ./my-start.gcode
    ///   --start-print-gcode "START_PRINT BED_TEMP=60 EXTRUDER_TEMP=210"
    #[arg(long)]
    pub start_print_gcode: Option<String>,

    /// Custom end G-code (overrides dialect default and global settings).
    ///
    /// Accepts either a file path (auto-detected when the path exists) or a
    /// direct G-code string.  Multiple lines may be separated with `\n`.
    ///
    /// Examples:
    ///   --end-print-gcode ./my-end.gcode
    ///   --end-print-gcode "END_PRINT"
    #[arg(long)]
    pub end_print_gcode: Option<String>,

    /// Enable verbose output (prints AABB, volume, surface area)
    #[arg(short, long)]
    pub verbose: bool,

    /// Center the mesh horizontally on the bed before slicing.
    #[arg(long)]
    pub center: bool,

    /// Drop the mesh so its lowest Z vertex sits on Z=0 before slicing.
    #[arg(long)]
    pub drop_to_floor: bool,

    /// Translate the mesh by `x,y,z` millimeters before slicing.
    #[arg(long, value_name = "X,Y,Z", value_parser = parse_vec3)]
    pub translate: Option<[f64; 3]>,

    /// Rotate around an axis by degrees: `x:90`, `-y:45`, `z:30`. Repeatable.
    #[arg(long, value_name = "AXIS:DEG", value_parser = parse_rotate, action = clap::ArgAction::Append)]
    pub rotate: Vec<([f32; 3], f32)>,

    /// Scale the mesh: uniform `--scale 2` or per-axis `--scale 1,1,2`.
    #[arg(long, value_name = "S|X,Y,Z", value_parser = parse_scale)]
    pub scale: Option<[f32; 3]>,

    /// Rotate the mesh so the chosen face's normal points down, then drop to floor.
    #[arg(long, value_name = "FACE_INDEX")]
    pub align_face: Option<usize>,

    /// Explicit path to a project config file (overrides auto-discovery of slicer.json).
    #[arg(long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Emit layer lifecycle markers (;LAYER_CHANGE, ;BEFORE/AFTER_LAYER_CHANGE, ;TYPE:, ;WIDTH:).
    /// When omitted, falls back to the per-flavor config in global settings (default: enabled).
    /// Use --lifecycle-markers to force-enable or --no-lifecycle-markers to force-disable.
    #[arg(long, conflicts_with = "no_lifecycle_markers")]
    pub lifecycle_markers: bool,

    /// Disable layer lifecycle markers.
    /// Overrides the global settings value and --lifecycle-markers.
    #[arg(long, conflicts_with = "lifecycle_markers")]
    pub no_lifecycle_markers: bool,

    /// Infill pattern (rectilinear, grid, honeycomb, gyroid).
    /// When omitted, falls back to the value in settings (default: rectilinear).
    #[arg(long)]
    pub infill_pattern: Option<String>,

    /// Infill density as a percentage (0-100).
    /// When omitted, uses the value from settings (default: 20%).
    #[arg(long)]
    pub infill_density: Option<f64>,

    /// Infill base angle in degrees (0-180).
    /// Alternating layers rotate +90° on top of this base angle.
    /// When omitted, uses the value from settings (default: 45°).
    #[arg(long)]
    pub infill_angle: Option<f64>,

    /// Mesh preprocessing quality (normal, high-quality, draft).
    ///
    /// - `normal` — no decimation, full mesh used (default).
    /// - `high-quality` — no decimation, maximum geometric fidelity.
    /// - `draft` — aggressive vertex-clustering decimation for faster slicing
    ///   of high-polygon-count models.
    ///
    /// When omitted, uses the value from settings (default: normal).
    #[arg(long, value_name = "QUALITY")]
    pub mesh_quality: Option<String>,

    /// Where to place the seam (start/end point) of each closed perimeter loop.
    ///
    /// - `nearest` — closest vertex to current nozzle position (fastest, scattered).
    /// - `rear` — single seam line at the back of the model (max Y).
    /// - `aligned` — vertices projected onto a fixed direction; consistent across loops.
    /// - `sharpest-corner` — hides the seam in the sharpest corner of each loop.
    /// - `random` — different vertex per loop (no visible seam line).
    ///
    /// When omitted, uses the value from settings (default: nearest).
    #[arg(long, value_name = "POLICY")]
    pub seam_position: Option<String>,

    /// Spiral (vase) mode: print a single continuous outer wall whose Z ramps
    /// smoothly over each layer, producing a seamless single-wall vase.
    ///
    /// Forces a single perimeter and disables sparse infill, top surfaces,
    /// retraction and Z-hop. The solid bottom layers are kept as the base (set
    /// `bottom_layers` to 0 in settings for an open tube). Best on solid,
    /// single-island models. When omitted, uses the value from settings.
    #[arg(long)]
    pub spiral_vase: bool,

    /// Dump internal geometry at every pipeline stage to this directory for
    /// visual debugging.  Produces per-layer `layer_NNNN.svg` files
    /// (Inkscape / browser) with each pipeline stage as a coloured group.
    /// Enables sequential (non-parallel) Arachne so intermediate inflate
    /// steps can be captured.  Has no effect on the G-code output.
    #[arg(long, value_name = "DIR")]
    pub debug_geometry: Option<PathBuf>,
}

/// Result payload emitted by the `slice` command.
struct SliceResult {
    input_name: String,
    layer_height: f64,
    layer_count: usize,
    output_path: Option<PathBuf>,
    gcode_flavor: String,
    /// Aggregate diagnostics for the slice (issue #11).
    stats: crate::gcode::SliceStatistics,
    /// Build-plate surface, when the profile specified one.
    bed_type: Option<String>,
}

impl EmitPayload for SliceResult {
    fn schema(&self) -> &'static str {
        "slicer-engine/slice-result-v1"
    }

    fn display_human(&self) -> String {
        let mut s = format!(
            "✓ Sliced {} into {} layers\n  Layer height: {} mm\n  G-code flavor: {}",
            self.input_name, self.layer_count, self.layer_height, self.gcode_flavor
        );
        s.push_str(&format!("\n  Model height: {:.2} mm", self.stats.max_z_mm));
        s.push_str(&format!(
            "\n  Filament: {:.2} mm ({:.2} g)",
            self.stats.filament_mm, self.stats.filament_g
        ));
        s.push_str(&format!(
            "\n  Estimated print time: {}",
            self.stats.estimated_time_human()
        ));
        if let Some(bed) = &self.bed_type {
            s.push_str(&format!("\n  Bed type: {}", bed));
        }
        if let Some(path) = &self.output_path {
            s.push_str(&format!("\n  Output: {}", path.display()));
        }
        s
    }

    fn to_json(&self) -> Value {
        json!({
            "status": "success",
            "input": self.input_name,
            "layer_height": self.layer_height,
            "layer_count": self.layer_count,
            "gcode_flavor": self.gcode_flavor,
            "output": self.output_path.as_ref().map(|p| p.display().to_string()),
            "bed_type": self.bed_type,
            "statistics": {
                "max_z_mm": self.stats.max_z_mm,
                "filament_mm": self.stats.filament_mm,
                "filament_cm3": self.stats.filament_cm3,
                "filament_g": self.stats.filament_g,
                "estimated_print_time_s": self.stats.estimated_print_time_s,
                "estimated_print_time_human": self.stats.estimated_time_human(),
                "bbox_min_mm": self.stats.bbox_min,
                "bbox_max_mm": self.stats.bbox_max,
            },
        })
    }
}

impl SliceCommand {
    /// Execute the slice command
    pub fn execute(&self) -> Result<(), Box<dyn std::error::Error>> {
        let format = self
            .output_format
            .parse::<OutputFormat>()
            .map_err(|e| format!("Invalid output format: {}", e))?;

        let emitter = Emitter::new(format);

        // Load and merge config following the priority hierarchy:
        // global defaults → user slicer.toml → project slicer.toml → CLI args
        let config = match load_and_merge_config(self.config.as_deref()) {
            Ok(c) => c,
            Err(e) => {
                emitter.log_warn(&format!("Failed to load config, using defaults: {}", e));
                crate::config::AppConfig::default()
            }
        };

        let settings = config.slicing.unwrap_or_default();

        // Resolve gcode flavor: CLI arg → params in settings → built-in default (Marlin)
        let flavor = if let Some(ref flavor_str) = self.gcode_flavor {
            flavor_str
                .parse::<GcodeFlavor>()
                .map_err(|e| format!("Invalid G-code flavor: {}", e))?
        } else {
            settings.gcode_flavor
        };
        let default_layer_height = settings.layer_height;
        let layer_height = self.layer_height.unwrap_or(default_layer_height);

        // Build slicing params (layer height may be overridden by CLI flag)
        let mut slice_params = settings.clone();
        slice_params.layer_height = layer_height;

        // Apply CLI overrides for infill settings
        if let Some(density) = self.infill_density {
            slice_params.infill_density = density / 100.0; // Convert percentage to fraction
        }
        if let Some(ref pattern) = self.infill_pattern {
            slice_params.infill_pattern = InfillPattern::parse(pattern)
                .ok_or_else(|| format!("Unknown infill pattern: '{}'. Supported: rectilinear, grid, honeycomb, gyroid, tpms-d", pattern))?;
        }
        if let Some(angle) = self.infill_angle {
            slice_params.infill_base_angle = angle;
        }
        if let Some(ref quality_str) = self.mesh_quality {
            slice_params.mesh_quality = match quality_str.to_lowercase().as_str() {
                "normal" => MeshQuality::Normal,
                "high-quality" => MeshQuality::HighQuality,
                "draft" => MeshQuality::Draft,
                other => {
                    return Err(format!(
                        "Unknown mesh quality: '{}'. Supported: normal, high-quality, draft",
                        other
                    )
                    .into())
                }
            };
        }
        if let Some(ref policy_str) = self.seam_position {
            slice_params.seam_position = crate::settings::params::SeamPosition::parse(policy_str)
                .ok_or_else(|| format!(
                    "Unknown seam position: '{}'. Supported: nearest, rear, aligned, sharpest-corner, random",
                    policy_str
                ))?;
        }

        // Spiral (vase) mode is a plain on/off flag; enabling it here defers the
        // actual single-wall normalization to the pipeline/generator so every
        // runtime shares one code path.
        if self.spiral_vase {
            slice_params.spiral_vase = true;
        }

        // Validate input file exists
        if !self.input.exists() {
            return Err(format!("Input file not found: {}", self.input.display()).into());
        }

        // Build the request-specific logger for this CLI invocation.
        // All pipeline messages are routed through this logger; debug output
        // is only emitted when --verbose is active.
        let logger = CliLogger::new(emitter.clone(), self.verbose);

        // Start overall timing for the entire process
        let t_total = PhaseTimer::start(phases::TOTAL, &logger);

        logger.log_debug(&format!("loading mesh: {:?}", self.input));
        logger.log_debug(&format!("G-code flavor: {}", flavor));

        // Load mesh — format is auto-detected from file extension
        let t_load = PhaseTimer::start(phases::MESH_LOAD, &logger);
        let raw_mesh = crate::scene::load_path(&self.input)
            .map_err(|e| format!("Failed to load mesh '{}': {}", self.input.display(), e))?;
        t_load.finish();

        // Build a single-object scene rooted on the configured bed; every
        // transform flag is translated into a SceneOp so the CLI shares the
        // exact code path used by the WS server and (later) WASM/UI.
        let bed = BedConfig::from(&config.machine);
        let mut scene = SceneState::new(bed);
        let object_id = scene.add_mesh(
            self.input
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "mesh".to_string()),
            Arc::new(raw_mesh),
        );

        // Order: explicit translate → rotate → scale → align-face → center → drop-to-floor.
        // Center and drop-to-floor are placement helpers and intentionally run
        // last so other ops compose into them naturally.
        if let Some(delta) = self.translate {
            scene.apply(SceneOp::Translate {
                id: object_id,
                delta,
            })?;
            logger.log_debug(&format!("applied translate: {:?}", delta));
        }
        for (axis, radians) in &self.rotate {
            scene.apply(SceneOp::Rotate {
                id: object_id,
                axis: *axis,
                radians: *radians,
            })?;
            logger.log_debug(&format!(
                "applied rotate: axis={:?} deg={:.3}",
                axis,
                radians.to_degrees()
            ));
        }
        if let Some(factors) = self.scale {
            scene.apply(SceneOp::Scale {
                id: object_id,
                factors,
            })?;
            logger.log_debug(&format!("applied scale: {:?}", factors));
        }
        if let Some(face_index) = self.align_face {
            scene.apply(SceneOp::PlaceFaceOnFloor {
                id: object_id,
                face_index,
            })?;
            logger.log_debug(&format!("applied align-face: {}", face_index));
        }
        if self.center {
            logger.log_warn(
                "--center is deprecated; prefer the scene op CenterOnBed (kept as an alias for one release)",
            );
            scene.apply(SceneOp::CenterOnBed { id: object_id })?;
            logger.log_debug("applied center transform");
        }
        if self.drop_to_floor {
            logger.log_warn(
                "--drop-to-floor is deprecated; prefer the scene op DropToFloor (kept as an alias for one release)",
            );
            scene.apply(SceneOp::DropToFloor { id: object_id })?;
            logger.log_debug("applied drop-to-floor transform");
        }

        // Bake the scene transform into the mesh that the slicer pipeline sees.
        let scene_object = scene.get(object_id).expect("object just added");
        let baked_mesh = apply_transform(scene_object.mesh.as_ref(), &scene_object.transform);

        // Apply optional mesh decimation. The original (baked) mesh is kept
        // in `baked_mesh` for reference; only the pipeline receives the
        // potentially-decimated copy.
        let mesh = if slice_params.mesh_quality == MeshQuality::Draft {
            let before = baked_mesh.faces.len();
            let decimated =
                crate::mesh::transforms::decimate_mesh(&baked_mesh, slice_params.mesh_quality);
            logger.log_debug(&format!(
                "mesh decimation (draft): {} → {} faces",
                before,
                decimated.faces.len()
            ));
            decimated
        } else {
            baked_mesh
        };

        // Compute and log mesh geometry (verbose detail available to this CLI request).
        {
            let t_analysis = PhaseTimer::start(phases::MESH_ANALYSIS, &logger);
            let aabb = calculate_aabb(&mesh);
            logger.log_debug(&format!(
                "AABB: ({:.3}, {:.3}, {:.3}) → ({:.3}, {:.3}, {:.3})",
                aabb.min.x, aabb.min.y, aabb.min.z, aabb.max.x, aabb.max.y, aabb.max.z
            ));
            logger.log_debug(&format!(
                "dimensions: {:.3} × {:.3} × {:.3} mm",
                aabb.width(),
                aabb.depth(),
                aabb.height()
            ));

            match calculate_volume(&mesh) {
                Ok(vol) => logger.log_debug(&format!("volume: {:.3} mm³", vol)),
                Err(e) => logger.log_debug(&format!("volume: {}", e)),
            }

            let area = calculate_surface_area(&mesh);
            logger.log_debug(&format!("surface area: {:.3} mm²", area));

            logger.log_debug(&format!(
                "faces: {}, vertices: {}",
                mesh.faces.len(),
                mesh.vertices.len()
            ));
            logger.log_debug(&format!("layer height: {:.3} mm", layer_height));
            t_analysis.finish();
        }

        // Run the unified slicing pipeline. All step-level logging is handled
        // inside process_mesh and routed through `logger`.
        let layers = if let Some(ref debug_dir) = self.debug_geometry {
            std::fs::create_dir_all(debug_dir).map_err(|e| {
                format!(
                    "Failed to create debug directory '{}': {}",
                    debug_dir.display(),
                    e
                )
            })?;
            logger.log_info(&format!(
                "debug geometry enabled — writing to {}",
                debug_dir.display()
            ));
            let mut debug_geometry = crate::debug::DebugGeometry::new();
            let layers =
                crate::core::process_mesh_debug(&mesh, &slice_params, &logger, &mut debug_geometry);
            crate::debug::svg::write_svgs(&debug_geometry, debug_dir)
                .map_err(|e| format!("Failed to write debug SVGs: {}", e))?;
            let svg_count = debug_geometry
                .records
                .iter()
                .map(|r| r.layer_index)
                .collect::<std::collections::HashSet<_>>()
                .len();
            logger.log_info(&format!(
                "debug geometry written: {} SVG files ({} records)",
                svg_count,
                debug_geometry.len()
            ));
            layers
        } else {
            crate::core::process_mesh(&mesh, &slice_params, &logger)
        };

        // Resolve per-flavor lifecycle marker config from config.
        // CLI flags override the enabled field.
        let marker_config = config
            .lifecycle_markers
            .get(&flavor.to_string())
            .cloned()
            .unwrap_or_default();
        let marker_config = if self.no_lifecycle_markers {
            LifecycleMarkerConfig {
                enabled: false,
                ..marker_config
            }
        } else if self.lifecycle_markers {
            LifecycleMarkerConfig {
                enabled: true,
                ..marker_config
            }
        } else {
            marker_config
        };

        // Generate G-code using the selected firmware flavor; route dialect
        // warnings through the logger's warn channel.
        // Script precedence: CLI arg → global settings → dialect default.
        let warn_logger = logger.clone();
        let mut generator = GcodeGenerator::new(flavor)
            .with_marker_config(marker_config)
            .with_warn_fn(move |msg| warn_logger.log_warn(msg));

        // Embed the source model name (file stem) in the metadata header.
        if let Some(stem) = self
            .input
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
        {
            generator = generator.with_model_name(stem);
        }

        // Resolve custom start script (CLI arg takes priority over config)
        let start_source = self
            .start_print_gcode
            .as_deref()
            .or(config.start_print_gcode.as_deref());
        if let Some(src) = start_source {
            let lines = resolve_gcode_source(src)
                .map_err(|e| format!("Failed to read start G-code: {}", e))?;
            generator = generator.with_start_script(lines);
        }

        // Resolve custom end script (CLI arg takes priority over config)
        let end_source = self
            .end_print_gcode
            .as_deref()
            .or(config.end_print_gcode.as_deref());
        if let Some(src) = end_source {
            let lines = resolve_gcode_source(src)
                .map_err(|e| format!("Failed to read end G-code: {}", e))?;
            generator = generator.with_end_script(lines);
        }

        let t_gcode = PhaseTimer::start(phases::GCODE_GENERATION, &logger);
        let (gcode, stats) = generator.generate_with_stats(&layers, &slice_params);
        t_gcode.finish();

        // Determine output path
        let output_path = self.output.clone().or_else(|| {
            // Auto-generate from input filename: model.stl → model.gcode
            // Guard against empty stems (e.g. hidden files like ".stl")
            let stem = self.input.file_stem()?;
            if stem.is_empty() {
                return None;
            }
            Some(self.input.with_file_name(stem).with_extension("gcode"))
        });

        // Write G-code to file
        if let Some(ref path) = output_path {
            let t_write = PhaseTimer::start(phases::FILE_WRITE, &logger);
            std::fs::write(path, &gcode)
                .map_err(|e| format!("Failed to write G-code to '{}': {}", path.display(), e))?;
            t_write.finish();
            logger.log_debug(&format!("wrote G-code to {}", path.display()));
        }

        let input_name = self
            .input
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        let result = SliceResult {
            input_name,
            layer_height,
            layer_count: layers.len(),
            output_path,
            gcode_flavor: flavor.to_string(),
            bed_type: Some(slice_params.bed_type.clone()).filter(|b| !b.trim().is_empty()),
            stats,
        };

        emitter.emit(&result);

        // Finish overall timing
        t_total.finish();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A non-zero sample statistics bundle for `SliceResult` tests.
    fn sample_stats() -> crate::gcode::SliceStatistics {
        crate::gcode::SliceStatistics {
            layer_count: 5,
            max_z_mm: 1.0,
            filament_mm: 123.45,
            filament_cm3: 0.3,
            filament_g: 0.37,
            estimated_print_time_s: 65.0,
            bbox_min: [0.0, 0.0, 0.1],
            bbox_max: [10.0, 10.0, 1.0],
            model_name: Some("model".to_string()),
        }
    }

    #[test]
    fn test_slice_command_creation() {
        let cmd = SliceCommand {
            input: PathBuf::from("test.stl"),
            layer_height: Some(0.2),
            output: None,
            output_format: "human".to_string(),
            gcode_flavor: Some("marlin".to_string()),
            start_print_gcode: None,
            end_print_gcode: None,
            verbose: false,
            center: false,
            drop_to_floor: false,
            config: None,
            lifecycle_markers: false,
            no_lifecycle_markers: false,
            infill_pattern: None,
            infill_density: None,
            infill_angle: None,
            translate: None,
            rotate: Vec::new(),
            scale: None,
            align_face: None,
            mesh_quality: None,
            seam_position: None,
            spiral_vase: false,
            debug_geometry: None,
        };
        assert_eq!(cmd.layer_height, Some(0.2));
        assert_eq!(cmd.gcode_flavor.as_deref(), Some("marlin"));
    }

    #[test]
    fn test_slice_command_no_flavor_uses_none() {
        let cmd = SliceCommand {
            input: PathBuf::from("test.stl"),
            layer_height: Some(0.2),
            output: None,
            output_format: "human".to_string(),
            gcode_flavor: None, // will fall back to settings / marlin
            start_print_gcode: None,
            end_print_gcode: None,
            verbose: false,
            center: false,
            drop_to_floor: false,
            config: None,
            lifecycle_markers: false,
            no_lifecycle_markers: false,
            infill_pattern: None,
            infill_density: None,
            infill_angle: None,
            translate: None,
            rotate: Vec::new(),
            scale: None,
            align_face: None,
            mesh_quality: None,
            seam_position: None,
            spiral_vase: false,
            debug_geometry: None,
        };
        assert!(cmd.gcode_flavor.is_none());
    }

    #[test]
    fn test_slice_command_klipper_flavor() {
        let cmd = SliceCommand {
            input: PathBuf::from("test.stl"),
            layer_height: Some(0.2),
            output: None,
            output_format: "human".to_string(),
            gcode_flavor: Some("klipper".to_string()),
            start_print_gcode: None,
            end_print_gcode: None,
            verbose: false,
            center: false,
            drop_to_floor: false,
            config: None,
            lifecycle_markers: false,
            no_lifecycle_markers: false,
            infill_pattern: None,
            infill_density: None,
            infill_angle: None,
            translate: None,
            rotate: Vec::new(),
            scale: None,
            align_face: None,
            mesh_quality: None,
            seam_position: None,
            spiral_vase: false,
            debug_geometry: None,
        };
        assert_eq!(cmd.gcode_flavor.as_deref(), Some("klipper"));
    }

    #[test]
    fn test_slice_command_start_end_gcode_args() {
        let cmd = SliceCommand {
            input: PathBuf::from("test.stl"),
            layer_height: Some(0.2),
            output: None,
            output_format: "human".to_string(),
            gcode_flavor: Some("klipper".to_string()),
            start_print_gcode: Some("START_PRINT BED_TEMP=65".to_string()),
            end_print_gcode: Some("END_PRINT".to_string()),
            verbose: false,
            center: false,
            drop_to_floor: false,
            config: None,
            lifecycle_markers: false,
            no_lifecycle_markers: false,
            infill_pattern: None,
            infill_density: None,
            infill_angle: None,
            translate: None,
            rotate: Vec::new(),
            scale: None,
            align_face: None,
            mesh_quality: None,
            seam_position: None,
            spiral_vase: false,
            debug_geometry: None,
        };
        assert_eq!(
            cmd.start_print_gcode.as_deref(),
            Some("START_PRINT BED_TEMP=65")
        );
        assert_eq!(cmd.end_print_gcode.as_deref(), Some("END_PRINT"));
    }

    #[test]
    fn test_slice_command_lifecycle_markers_flags() {
        let cmd_on = SliceCommand {
            input: PathBuf::from("test.stl"),
            layer_height: Some(0.2),
            output: None,
            output_format: "human".to_string(),
            gcode_flavor: None,
            start_print_gcode: None,
            end_print_gcode: None,
            verbose: false,
            center: false,
            drop_to_floor: false,
            config: None,
            lifecycle_markers: true,
            no_lifecycle_markers: false,
            infill_pattern: None,
            infill_density: None,
            infill_angle: None,
            translate: None,
            rotate: Vec::new(),
            scale: None,
            align_face: None,
            mesh_quality: None,
            seam_position: None,
            spiral_vase: false,
            debug_geometry: None,
        };
        assert!(cmd_on.lifecycle_markers);
        assert!(!cmd_on.no_lifecycle_markers);

        let cmd_off = SliceCommand {
            input: PathBuf::from("test.stl"),
            layer_height: Some(0.2),
            output: None,
            output_format: "human".to_string(),
            gcode_flavor: None,
            start_print_gcode: None,
            end_print_gcode: None,
            verbose: false,
            center: false,
            drop_to_floor: false,
            config: None,
            lifecycle_markers: false,
            no_lifecycle_markers: true,
            infill_pattern: None,
            infill_density: None,
            infill_angle: None,
            translate: None,
            rotate: Vec::new(),
            scale: None,
            align_face: None,
            mesh_quality: None,
            seam_position: None,
            spiral_vase: false,
            debug_geometry: None,
        };
        assert!(!cmd_off.lifecycle_markers);
        assert!(cmd_off.no_lifecycle_markers);
    }

    #[test]
    fn test_slice_result_schema() {
        let r = SliceResult {
            input_name: "model.stl".to_string(),
            layer_height: 0.2,
            layer_count: 5,
            output_path: None,
            gcode_flavor: "marlin".to_string(),
            bed_type: None,
            stats: sample_stats(),
        };
        assert_eq!(r.schema(), "slicer-engine/slice-result-v1");
    }

    #[test]
    fn test_slice_result_human() {
        let r = SliceResult {
            input_name: "model.stl".to_string(),
            layer_height: 0.2,
            layer_count: 5,
            output_path: None,
            gcode_flavor: "marlin".to_string(),
            bed_type: Some("Textured PEI Plate".to_string()),
            stats: sample_stats(),
        };
        let s = r.display_human();
        assert!(s.contains("model.stl"));
        assert!(s.contains("0.2"));
        assert!(s.contains('5'));
        assert!(s.contains("marlin"));
        // Diagnostics (issue #11) surface in the human output.
        assert!(
            s.contains("Estimated print time: 1m 5s"),
            "missing time: {s}"
        );
        assert!(s.contains("Filament:"), "missing filament: {s}");
        assert!(
            s.contains("Bed type: Textured PEI Plate"),
            "missing bed type: {s}"
        );
    }

    #[test]
    fn test_slice_result_human_klipper() {
        let r = SliceResult {
            input_name: "model.stl".to_string(),
            layer_height: 0.2,
            layer_count: 5,
            output_path: None,
            gcode_flavor: "klipper".to_string(),
            bed_type: None,
            stats: sample_stats(),
        };
        let s = r.display_human();
        assert!(s.contains("klipper"));
        // No bed type selected → no bed-type line.
        assert!(!s.contains("Bed type:"), "unexpected bed type line: {s}");
    }

    #[test]
    fn test_slice_result_human_with_output() {
        let r = SliceResult {
            input_name: "model.stl".to_string(),
            layer_height: 0.2,
            layer_count: 5,
            output_path: Some(PathBuf::from("/some/path/model.gcode")),
            gcode_flavor: "marlin".to_string(),
            bed_type: None,
            stats: sample_stats(),
        };
        let s = r.display_human();
        assert!(s.contains("model.gcode"));
    }

    #[test]
    fn test_slice_result_json_fields() {
        let r = SliceResult {
            input_name: "model.stl".to_string(),
            layer_height: 0.2,
            layer_count: 5,
            output_path: None,
            gcode_flavor: "marlin".to_string(),
            bed_type: Some("Cool Plate".to_string()),
            stats: sample_stats(),
        };
        let v = r.to_json();
        assert_eq!(v["status"], "success");
        assert_eq!(v["input"], "model.stl");
        assert_eq!(v["layer_height"], 0.2);
        assert_eq!(v["layer_count"], 5);
        assert_eq!(v["gcode_flavor"], "marlin");
        // Diagnostics (issue #11).
        assert_eq!(v["bed_type"], "Cool Plate");
        assert_eq!(v["statistics"]["filament_mm"], 123.45);
        assert_eq!(v["statistics"]["filament_g"], 0.37);
        assert_eq!(v["statistics"]["estimated_print_time_s"], 65.0);
        assert_eq!(v["statistics"]["estimated_print_time_human"], "1m 5s");
        assert_eq!(v["statistics"]["max_z_mm"], 1.0);
    }

    #[test]
    fn test_success_result_still_compiles() {
        use crate::cli::emit::SuccessResult;
        // Verify the built-in SuccessResult is still usable
        let r = SuccessResult {
            message: "ok".to_string(),
            details: None,
        };
        assert_eq!(r.schema(), "slicer-engine/result-v1");
    }
}
