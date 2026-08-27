use super::types::{FanSample, InternalLayer, Role};

/// Oversize seam dots so they remain readable against overlapping extrusion
/// paths without having to hide other roles.
const SEAM_DOT_DIAMETER_FROM_LAYER_HEIGHT_SCALE: f32 = 4.0;
const MIN_SEAM_DOT_RADIUS_MM: f32 = 0.3;
/// Parameter keys used by common start-macro conventions to pass nozzle temp.
const NOZZLE_TEMP_KEYS: &[&str] = &[
    "EXTRUDER_TEMP=",
    "EXTRUDER=",
    "NOZZLE_TEMP=",
    "NOZZLE=",
    "HOTEND_TEMP=",
    "HOTEND=",
];

fn seam_dot_radius(layer_height_mm: f32) -> f32 {
    let diameter = layer_height_mm.max(0.0) * SEAM_DOT_DIAMETER_FROM_LAYER_HEIGHT_SCALE;
    (diameter * 0.5).max(MIN_SEAM_DOT_RADIUS_MM)
}

/// Parse the leading real number from a marker value, tolerating the unit suffix
/// the generator appends (e.g. `WIDTH:0.8mm`).  A bare `parse::<f32>()` rejects
/// the `mm`, which silently pinned every bead to the default width.
fn parse_leading_f32(value: &str) -> Option<f32> {
    let value = value.trim();
    let end = value
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(value.len());
    value[..end].parse::<f32>().ok()
}

/// Sticky machine state threaded through the parse so each layer can snapshot
/// the fan / temperature / tool values that were active while it printed.
///
/// Fan/temp/tool commands (`M104`, `M106`, `T0`, …) are sticky in G-code — a
/// value set on one layer persists until changed — so new layers inherit the
/// current state and per-layer view modes (fan speed, temperature, tool) read
/// it back without re-scanning.
#[derive(Clone, Default)]
struct Sticky {
    nozzle_temp: Option<f32>,
    tool: u32,
    /// Fan speeds keyed by a stable id (`"P0"`, `"P2"`, or a Klipper fan name),
    /// in first-seen order. Speed is a `0.0..=1.0` fraction.
    fans: Vec<(String, f32)>,
}

impl Sticky {
    /// Update (or insert) the speed for a fan key, preserving first-seen order.
    fn set_fan(&mut self, key: &str, speed: f32) {
        if let Some(entry) = self.fans.iter_mut().find(|(k, _)| k == key) {
            entry.1 = speed;
        } else {
            self.fans.push((key.to_string(), speed));
        }
    }

    /// Copy the current sticky state into a layer's metadata, leaving the
    /// layer's own per-layer fields (such as `layer_time_s`) untouched.
    fn seed(&self, layer: &mut InternalLayer) {
        layer.meta.nozzle_temp = self.nozzle_temp;
        layer.meta.tool = self.tool;
        layer.meta.fans = self
            .fans
            .iter()
            .map(|(k, v)| FanSample {
                key: k.clone(),
                speed: *v,
            })
            .collect();
    }
}

/// Parse `bytes` as UTF-8 GCode and return one [`InternalLayer`] per detected
/// layer change, plus any segments that appear before the first layer marker
/// in layer 0.
pub(super) fn parse_gcode_bytes(bytes: &[u8]) -> Vec<InternalLayer> {
    let text = String::from_utf8_lossy(bytes);

    let mut layers: Vec<InternalLayer> = Vec::new();
    let mut current = InternalLayer::new(0.0);

    let mut x: f32 = 0.0;
    let mut y: f32 = 0.0;
    let mut z: f32 = 0.0;
    let mut e: f32 = 0.0;
    // Feedrate (mm/min) is sticky across moves, exactly like the position
    // registers, so we track it as parser state and update it on any G0/G1 `F`.
    let mut feedrate: f32 = 0.0;
    let mut width: f32 = 0.4;
    let mut height: f32 = 0.2;
    let mut absolute_xyz = true;
    let mut absolute_e = true;
    let mut role = Role::Travel;

    // Sticky fan / temperature / tool state, snapshotted into each layer's meta
    // for the non-geometric view modes.
    let mut sticky = Sticky::default();

    // When true we prefer `;LAYER_CHANGE` comments for layer detection.
    // When false we fall back to Z-change detection (for slicers that don't
    // emit our markers).
    let mut seen_layer_change_comment = false;

    for raw_line in text.lines() {
        let prev_role = role;
        let line = match raw_line.find(';') {
            Some(pos) => {
                let comment = raw_line[pos + 1..].trim();
                process_comment(
                    comment,
                    &mut role,
                    &mut layers,
                    &mut current,
                    &mut seen_layer_change_comment,
                    &mut width,
                    &mut height,
                    z,
                    &sticky,
                );
                raw_line[..pos].trim()
            }
            None => raw_line.trim(),
        };

        // Detect `;TYPE:` transitions: when the role changes FROM OuterWall to
        // anything else, or TO OuterWall from anything else, inject a white
        // seam-point marker at the current nozzle position.  The seam is
        // traditionally placed at the loop start (the point where the outer
        // wall begins and closes back on itself).
        if role != prev_role {
            let entering_outer_wall = role == Role::OuterWall;
            let leaving_outer_wall = prev_role == Role::OuterWall;
            if entering_outer_wall || leaving_outer_wall {
                // Emit a degenerate (zero-length) segment at the current nozzle
                // position.  The viewer renders Seam blocks as white dot spheres.
                let seam_radius = seam_dot_radius(height);
                current.push_segment(
                    Role::Seam,
                    x,
                    y,
                    z,
                    x,
                    y,
                    z,
                    seam_radius,
                    seam_radius,
                    feedrate / 60.0,
                );
            }
        }

        if line.is_empty() {
            continue;
        }

        let mut parts = line.split_ascii_whitespace();
        let cmd = match parts.next() {
            Some(c) => c.to_ascii_uppercase(),
            None => continue,
        };

        match cmd.as_str() {
            "G90" => {
                absolute_xyz = true;
                absolute_e = true;
            }
            "G91" => {
                absolute_xyz = false;
                absolute_e = false;
            }
            "M82" => absolute_e = true,
            "M83" => absolute_e = false,
            "G92" => {
                for param in parts {
                    if param.starts_with('E') || param.starts_with('e') {
                        if let Ok(val) = param[1..].parse::<f32>() {
                            e = val;
                        }
                    }
                }
            }
            "G0" | "G1" => {
                let prev_x = x;
                let prev_y = y;
                let prev_z = z;
                let prev_e = e;

                let mut new_x = x;
                let mut new_y = y;
                let mut new_z = z;
                let mut new_e = e;
                let mut new_f = feedrate;
                let mut has_e = false;

                for param in parts {
                    if param.is_empty() {
                        continue;
                    }
                    let (letter, rest) = param.split_at(1);
                    let Ok(val) = rest.parse::<f32>() else {
                        continue;
                    };
                    match letter.to_ascii_uppercase().as_str() {
                        "X" => new_x = if absolute_xyz { val } else { x + val },
                        "Y" => new_y = if absolute_xyz { val } else { y + val },
                        "Z" => new_z = if absolute_xyz { val } else { z + val },
                        "E" => {
                            has_e = true;
                            new_e = if absolute_e { val } else { e + val };
                        }
                        "F" => new_f = val,
                        _ => {}
                    }
                }

                // Z-change layer boundary (fallback when no ;LAYER_CHANGE).
                if !seen_layer_change_comment && (new_z - prev_z).abs() > 1e-6 && new_z > prev_z {
                    let mut next = InternalLayer::new(new_z);
                    sticky.seed(&mut next);
                    let finished = std::mem::replace(&mut current, next);
                    layers.push(finished);
                }

                x = new_x;
                y = new_y;
                z = new_z;
                e = new_e;
                feedrate = new_f;

                let is_extruding = has_e && (new_e - prev_e) > 1e-7;
                let seg_role = if is_extruding { role } else { Role::Travel };

                // Convert the mm/min feedrate to mm/s so the viewer can label
                // the speed gradient in the units printers are configured in.
                let speed = feedrate / 60.0;

                let moved = (x - prev_x).abs() > 1e-6
                    || (y - prev_y).abs() > 1e-6
                    || (z - prev_z).abs() > 1e-6;
                if moved {
                    current.push_segment(
                        seg_role, prev_x, prev_y, prev_z, x, y, z, width, height, speed,
                    );
                }
            }
            // ── Non-geometric state for the fan / temperature / tool views ──
            "M104" | "M109" => {
                for param in parts {
                    if let Some(rest) = param.strip_prefix(['S', 's']) {
                        if let Ok(v) = rest.parse::<f32>() {
                            sticky.nozzle_temp = Some(v);
                            sticky.seed(&mut current);
                        }
                    }
                }
            }
            "SET_HEATER_TEMPERATURE" => {
                // Klipper direct heater command:
                // `SET_HEATER_TEMPERATURE HEATER=extruder TARGET=210`.
                let mut heater = None;
                let mut target = None;
                for param in parts {
                    if let Some(v) = strip_prefix_ci(param, "heater=") {
                        heater = Some(v);
                    } else if let Some(v) = strip_prefix_ci(param, "target=") {
                        target = parse_leading_f32(v);
                    }
                }
                if heater.is_some_and(|h| h.eq_ignore_ascii_case("extruder")) {
                    if let Some(v) = target {
                        sticky.nozzle_temp = Some(v);
                        sticky.seed(&mut current);
                    }
                }
            }
            "M106" => {
                // Marlin part-cooling: `M106 P<n> S<0-255>` (P defaults to 0).
                let mut fan_index: u32 = 0;
                let mut raw: f32 = 0.0;
                for param in parts {
                    match param.split_at(1) {
                        ("P" | "p", rest) => fan_index = rest.parse().unwrap_or(0),
                        ("S" | "s", rest) => raw = rest.parse().unwrap_or(0.0),
                        _ => {}
                    }
                }
                sticky.set_fan(&format!("P{fan_index}"), (raw / 255.0).clamp(0.0, 1.0));
                sticky.seed(&mut current);
            }
            "M107" => {
                // Fan off; honour an optional `P<n>` selector.
                let mut fan_index: u32 = 0;
                for param in parts {
                    if let Some(rest) = param.strip_prefix(['P', 'p']) {
                        fan_index = rest.parse().unwrap_or(0);
                    }
                }
                sticky.set_fan(&format!("P{fan_index}"), 0.0);
                sticky.seed(&mut current);
            }
            "SET_FAN_SPEED" => {
                // Klipper: `SET_FAN_SPEED FAN=<name> SPEED=<0-1>`.
                let mut name: Option<&str> = None;
                let mut speed: f32 = 0.0;
                for param in parts {
                    if let Some(v) = strip_prefix_ci(param, "fan=") {
                        name = Some(v);
                    } else if let Some(v) = strip_prefix_ci(param, "speed=") {
                        speed = v.parse().unwrap_or(0.0);
                    }
                }
                if let Some(name) = name {
                    sticky.set_fan(name, speed.clamp(0.0, 1.0));
                    sticky.seed(&mut current);
                }
            }
            tool_cmd
                if tool_cmd.len() > 1
                    && tool_cmd.starts_with('T')
                    && tool_cmd[1..].bytes().all(|b| b.is_ascii_digit()) =>
            {
                if let Ok(n) = tool_cmd[1..].parse::<u32>() {
                    sticky.tool = n;
                    sticky.seed(&mut current);
                }
            }
            _ => {
                // Custom start macros often pass temperatures as named args
                // (e.g. `START_PRINT EXTRUDER_TEMP=...`). Capture those so
                // temperature-based color modes work for Klipper-style starts.
                if let Some(v) = parse_nozzle_temp_from_params(parts) {
                    sticky.nozzle_temp = Some(v);
                    sticky.seed(&mut current);
                }
            } // G28, G4, M140, etc. — ignore
        }
    }

    layers.push(current);
    layers
}

/// Case-insensitive `strip_prefix` for ASCII G-code parameter keys.
fn strip_prefix_ci<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    if value.len() >= prefix.len() && value[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&value[prefix.len()..])
    } else {
        None
    }
}

/// Parse nozzle temperature from key/value-style macro args.
///
/// Supports common conventions such as:
/// - `START_PRINT EXTRUDER_TEMP=210`
/// - `START_PRINT EXTRUDER=210`
/// - `PRINT_START HOTEND=210`
fn parse_nozzle_temp_from_params<'a>(params: impl Iterator<Item = &'a str>) -> Option<f32> {
    for param in params {
        for key in NOZZLE_TEMP_KEYS {
            if let Some(raw) = strip_prefix_ci(param, key) {
                if let Some(temp) = parse_leading_f32(raw) {
                    return Some(temp);
                }
            }
        }
    }
    None
}

/// Handle a `;` comment line, mutating parser state as needed.
#[allow(clippy::too_many_arguments)]
fn process_comment(
    comment: &str,
    role: &mut Role,
    layers: &mut Vec<InternalLayer>,
    current: &mut InternalLayer,
    seen_layer_change_comment: &mut bool,
    width: &mut f32,
    height: &mut f32,
    current_z: f32,
    sticky: &Sticky,
) {
    let trimmed = comment.trim();

    if trimmed.eq_ignore_ascii_case("LAYER_CHANGE")
        || trimmed.eq_ignore_ascii_case("BEFORE_LAYER_CHANGE")
    {
        *seen_layer_change_comment = true;
        if !current.is_empty() {
            let mut next = InternalLayer::new(current_z);
            sticky.seed(&mut next);
            let finished = std::mem::replace(current, next);
            layers.push(finished);
        }
        // Do not reset current.z here if it is empty, because a preceding ;Z:
        // might have already set the correct future Z height for this empty layer.
        *role = Role::Travel;
    } else if let Some(type_val) = trimmed.strip_prefix("TYPE:") {
        *role = Role::from_type_comment(type_val);
    } else if let Some(z_val) = trimmed.strip_prefix("Z:") {
        if let Ok(z) = z_val.parse::<f32>() {
            if current.is_empty() {
                current.z = z;
            }
        }
    } else if let Some(width_val) = trimmed.strip_prefix("WIDTH:") {
        if let Some(w) = parse_leading_f32(width_val) {
            *width = w;
        }
    } else if let Some(height_val) = trimmed.strip_prefix("HEIGHT:") {
        if let Some(h) = parse_leading_f32(height_val) {
            *height = h;
        }
    } else if let Some(height_val) = trimmed.strip_prefix("LAYER_HEIGHT:") {
        if let Ok(h) = height_val.parse::<f32>() {
            *height = h;
        }
    } else if let Some(time_val) = trimmed.strip_prefix("LAYER_TIME:") {
        if let Some(t) = parse_leading_f32(time_val) {
            current.meta.layer_time_s = Some(t);
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_GCODE: &str = r#"
; Generated by test
G90
G92 E0
;LAYER_CHANGE
;Z:0.200
G0 Z0.200 F9000
;TYPE:Outer wall
G1 X10 Y10 Z0.2 E1.0 F1800
G1 X20 Y10 Z0.2 E2.0
G1 X20 Y20 Z0.2 E3.0
G1 X10 Y10 Z0.2 E4.0
;TYPE:Infill
G1 X15 Y15 Z0.2 E5.0
;LAYER_CHANGE
;Z:0.400
G0 Z0.400 F9000
;TYPE:Inner wall
G1 X10 Y10 Z0.4 E6.0 F1800
G1 X20 Y10 Z0.4 E7.0
"#;

    fn has_role(layers: &[InternalLayer], role: Role) -> bool {
        layers
            .iter()
            .any(|l| l.blocks.iter().any(|b| b.role == role))
    }

    #[test]
    fn test_layer_count() {
        let layers = parse_gcode_bytes(SAMPLE_GCODE.as_bytes());
        assert!(
            layers.len() >= 2,
            "expected at least 2 layers, got {}",
            layers.len()
        );
    }

    #[test]
    fn test_outer_wall_segments() {
        let layers = parse_gcode_bytes(SAMPLE_GCODE.as_bytes());
        assert!(
            has_role(&layers, Role::OuterWall),
            "expected outer wall segments"
        );
    }

    #[test]
    fn test_infill_segments() {
        let layers = parse_gcode_bytes(SAMPLE_GCODE.as_bytes());
        assert!(has_role(&layers, Role::Infill), "expected infill segments");
    }

    #[test]
    fn test_layer_z_values() {
        let layers = parse_gcode_bytes(SAMPLE_GCODE.as_bytes());
        let zs: Vec<f32> = layers.iter().map(|l| l.z).collect();
        assert!(
            zs.iter().any(|&z| (z - 0.2).abs() < 0.01),
            "expected z=0.2 layer, got {:?}",
            zs
        );
    }

    #[test]
    fn width_marker_with_mm_suffix_is_parsed() {
        // Regression: the generator emits `;WIDTH:0.8mm`; a bare `parse::<f32>()`
        // rejected the `mm` suffix and pinned every bead to the 0.4 default,
        // hiding the variable-width gap fill so wide gaps looked unfilled.
        assert!((parse_leading_f32("0.8mm").unwrap() - 0.8).abs() < 1e-6);
        assert!((parse_leading_f32("0.42mm").unwrap() - 0.42).abs() < 1e-6);
        assert!((parse_leading_f32("0.4").unwrap() - 0.4).abs() < 1e-6);
        assert!(parse_leading_f32("mm").is_none());

        let gcode = "\
;TYPE:Outer wall
;WIDTH:0.80mm
G1 X0 Y0 Z0.2 E0 F1800
G1 X10 Y0 Z0.2 E1.0
";
        let layers = parse_gcode_bytes(gcode.as_bytes());
        let w = layers
            .iter()
            .flat_map(|l| &l.blocks)
            .find(|b| b.role == Role::OuterWall)
            .map(|b| b.data[6])
            .expect("outer-wall segment");
        assert!(
            (w - 0.8).abs() < 1e-6,
            "segment width should come from the mm-suffixed marker, got {w}"
        );
    }

    #[test]
    fn feedrate_is_captured_as_mm_per_second() {
        // `F` is emitted in mm/min; the viewer wants mm/s, so a move at
        // F1800 must land as 30 mm/s in the segment's speed slot (index 8).
        let gcode = "\
;TYPE:Outer wall
G1 X0 Y0 Z0.2 E0 F1800
G1 X10 Y0 Z0.2 E1.0
";
        let layers = parse_gcode_bytes(gcode.as_bytes());
        let speed = layers
            .iter()
            .flat_map(|l| &l.blocks)
            .find(|b| b.role == Role::OuterWall)
            .map(|b| b.data[8])
            .expect("outer-wall segment");
        assert!(
            (speed - 30.0).abs() < 1e-4,
            "F1800 should be 30 mm/s, got {speed}"
        );
    }

    #[test]
    fn feedrate_persists_across_moves_without_f() {
        // A move that omits `F` inherits the last commanded feedrate, so both
        // extruding segments below must report the same 40 mm/s (F2400).
        let gcode = "\
;TYPE:Infill
G1 X0 Y0 Z0.2 E0 F2400
G1 X10 Y0 Z0.2 E1.0
G1 X10 Y10 Z0.2 E2.0
";
        let layers = parse_gcode_bytes(gcode.as_bytes());
        let speeds: Vec<f32> = layers
            .iter()
            .flat_map(|l| &l.blocks)
            .filter(|b| b.role == Role::Infill)
            .flat_map(|b| b.data.chunks_exact(9).map(|c| c[8]))
            .collect();
        assert!(!speeds.is_empty(), "expected infill segments");
        assert!(
            speeds.iter().all(|&s| (s - 40.0).abs() < 1e-4),
            "all infill segments should be 40 mm/s, got {speeds:?}"
        );
    }

    #[test]
    fn test_role_from_type_comment() {
        assert_eq!(Role::from_type_comment("Outer wall"), Role::OuterWall);
        assert_eq!(Role::from_type_comment("OuterWall"), Role::OuterWall);
        assert_eq!(Role::from_type_comment("Inner wall"), Role::InnerWall);
        assert_eq!(Role::from_type_comment("Infill"), Role::Infill);
        assert_eq!(Role::from_type_comment("Sparse infill"), Role::Infill);
        assert_eq!(
            Role::from_type_comment("Internal solid infill"),
            Role::SolidInfill
        );
        assert_eq!(Role::from_type_comment("Gap infill"), Role::GapFill);
        assert_eq!(Role::from_type_comment("Top surface"), Role::TopSurface);
        assert_eq!(
            Role::from_type_comment("Bottom surface"),
            Role::BottomSurface
        );
        assert_eq!(Role::from_type_comment("Bridge"), Role::Bridge);
        assert_eq!(Role::from_type_comment("Bridge infill"), Role::Bridge);
        assert_eq!(
            Role::from_type_comment("Internal bridge infill"),
            Role::InternalBridge
        );
        assert_eq!(Role::from_type_comment("bridge"), Role::Bridge);
        assert_eq!(Role::from_type_comment("Skirt"), Role::Skirt);
        assert_eq!(Role::from_type_comment("Brim"), Role::Brim);
        assert_eq!(
            Role::from_type_comment("Support interface"),
            Role::SupportInterface
        );
        assert_eq!(Role::from_type_comment("Support material"), Role::Support);
        assert_eq!(Role::from_type_comment("Prime tower"), Role::PrimeTower);
    }

    #[test]
    fn test_empty_input() {
        let layers = parse_gcode_bytes(b"");
        assert_eq!(
            layers.len(),
            1,
            "empty input should produce exactly 1 (empty) layer"
        );
    }

    /// When the parser sees `;TYPE:Outer wall` followed by another type, a
    /// `Seam` point block must appear in that layer.
    #[test]
    fn test_seam_points_emitted_for_outer_wall_transitions() {
        let layers = parse_gcode_bytes(SAMPLE_GCODE.as_bytes());
        assert!(
            has_role(&layers, Role::Seam),
            "expected Seam point blocks around the Outer wall section"
        );
    }

    /// Bridge role should be parsed from `;TYPE:Bridge` comments.
    #[test]
    fn test_bridge_role_parsed() {
        let gcode = b"
;LAYER_CHANGE
;Z:0.400
;TYPE:Bridge
G1 X10 Y10 Z0.4 E1.0 F1800
G1 X20 Y10 Z0.4 E2.0
";
        let layers = parse_gcode_bytes(gcode);
        assert!(has_role(&layers, Role::Bridge), "expected Bridge role");
    }

    /// Overhang perimeter role should be parsed from `;TYPE:Overhang wall` comments.
    #[test]
    fn test_overhang_perimeter_role_parsed() {
        let gcode = b"
;LAYER_CHANGE
;Z:0.400
;TYPE:Overhang wall
G1 X10 Y10 Z0.4 E1.0 F1800
G1 X20 Y10 Z0.4 E2.0
";
        let layers = parse_gcode_bytes(gcode);
        assert!(
            has_role(&layers, Role::OverhangPerimeter),
            "expected OverhangPerimeter role"
        );
    }

    /// Skirt role should be parsed from `;TYPE:Skirt` comments.
    #[test]
    fn test_skirt_role_parsed() {
        let gcode = b"
;LAYER_CHANGE
;Z:0.200
;TYPE:Skirt
G1 X5 Y5 Z0.2 E1.0 F1800
G1 X15 Y5 Z0.2 E2.0
";
        let layers = parse_gcode_bytes(gcode);
        assert!(has_role(&layers, Role::Skirt), "expected Skirt role");
    }

    /// Return the first layer that carries any extrusion geometry.
    fn first_printed_layer(layers: &[InternalLayer]) -> &InternalLayer {
        layers
            .iter()
            .find(|l| !l.is_empty())
            .expect("expected at least one printed layer")
    }

    fn fan_speed(layer: &InternalLayer, key: &str) -> Option<f32> {
        layer
            .meta
            .fans
            .iter()
            .find(|f| f.key == key)
            .map(|f| f.speed)
    }

    /// Marlin `M106 P<n> S<0-255>` is captured as a 0..1 fraction under key `P<n>`.
    #[test]
    fn marlin_fan_speed_is_captured_as_fraction() {
        let gcode = b"
;LAYER_CHANGE
;Z:0.200
M106 P0 S255
M106 P2 S127
;TYPE:Outer wall
G1 X0 Y0 Z0.2 E0 F1800
G1 X10 Y0 Z0.2 E1.0
";
        let layers = parse_gcode_bytes(gcode);
        let layer = first_printed_layer(&layers);
        assert!(
            (fan_speed(layer, "P0").expect("P0 fan") - 1.0).abs() < 1e-4,
            "M106 P0 S255 should be full speed"
        );
        assert!(
            (fan_speed(layer, "P2").expect("P2 fan") - 127.0 / 255.0).abs() < 1e-4,
            "M106 P2 S127 should be ~0.498"
        );
    }

    /// `M106 S<v>` without an explicit `P` targets the part-cooling fan `P0`.
    #[test]
    fn marlin_fan_without_p_defaults_to_p0() {
        let gcode = b"
;LAYER_CHANGE
;Z:0.200
M106 S204
;TYPE:Outer wall
G1 X0 Y0 Z0.2 E0 F1800
G1 X10 Y0 Z0.2 E1.0
";
        let layers = parse_gcode_bytes(gcode);
        let layer = first_printed_layer(&layers);
        assert!((fan_speed(layer, "P0").expect("P0 fan") - 204.0 / 255.0).abs() < 1e-4);
    }

    /// `M107` turns the (optionally P-selected) fan off.
    #[test]
    fn marlin_fan_off_sets_zero() {
        let gcode = b"
;LAYER_CHANGE
;Z:0.200
M106 P0 S255
M107
;TYPE:Outer wall
G1 X0 Y0 Z0.2 E0 F1800
G1 X10 Y0 Z0.2 E1.0
";
        let layers = parse_gcode_bytes(gcode);
        let layer = first_printed_layer(&layers);
        assert_eq!(fan_speed(layer, "P0"), Some(0.0));
    }

    /// Klipper `SET_FAN_SPEED FAN=<name> SPEED=<0-1>` is captured under the name.
    #[test]
    fn klipper_fan_speed_is_captured() {
        let gcode = b"
;LAYER_CHANGE
;Z:0.200
SET_FAN_SPEED FAN=fan SPEED=0.5000
SET_FAN_SPEED FAN=fan_chamber SPEED=0.2500
;TYPE:Outer wall
G1 X0 Y0 Z0.2 E0 F1800
G1 X10 Y0 Z0.2 E1.0
";
        let layers = parse_gcode_bytes(gcode);
        let layer = first_printed_layer(&layers);
        assert!((fan_speed(layer, "fan").expect("fan") - 0.5).abs() < 1e-4);
        assert!((fan_speed(layer, "fan_chamber").expect("chamber") - 0.25).abs() < 1e-4);
    }

    /// Nozzle temperature is captured from `M104`/`M109`.
    #[test]
    fn nozzle_temperature_is_captured() {
        let gcode = b"
M109 S210
;LAYER_CHANGE
;Z:0.200
;TYPE:Outer wall
G1 X0 Y0 Z0.2 E0 F1800
G1 X10 Y0 Z0.2 E1.0
";
        let layers = parse_gcode_bytes(gcode);
        let layer = first_printed_layer(&layers);
        assert_eq!(layer.meta.nozzle_temp, Some(210.0));
    }

    /// Klipper-style start macros pass nozzle temp as named arguments.
    #[test]
    fn nozzle_temperature_is_captured_from_start_print_macro() {
        let gcode = b"
START_PRINT BED_TEMP=60 EXTRUDER_TEMP=215 BED=60 EXTRUDER=215
;LAYER_CHANGE
;Z:0.200
;TYPE:Outer wall
G1 X0 Y0 Z0.2 E0 F1800
G1 X10 Y0 Z0.2 E1.0
";
        let layers = parse_gcode_bytes(gcode);
        let layer = first_printed_layer(&layers);
        assert_eq!(layer.meta.nozzle_temp, Some(215.0));
    }

    /// Generic macros with common aliases (`HOTEND`, `NOZZLE`) are supported too.
    #[test]
    fn nozzle_temperature_is_captured_from_generic_macro_aliases() {
        let gcode = b"
PRINT_START HOTEND=225
;LAYER_CHANGE
;Z:0.200
;TYPE:Outer wall
G1 X0 Y0 Z0.2 E0 F1800
G1 X10 Y0 Z0.2 E1.0
";
        let layers = parse_gcode_bytes(gcode);
        let layer = first_printed_layer(&layers);
        assert_eq!(layer.meta.nozzle_temp, Some(225.0));
    }

    /// Direct Klipper heater commands are also mapped to nozzle temperature.
    #[test]
    fn nozzle_temperature_is_captured_from_set_heater_temperature() {
        let gcode = b"
SET_HEATER_TEMPERATURE HEATER=extruder TARGET=235
;LAYER_CHANGE
;Z:0.200
;TYPE:Outer wall
G1 X0 Y0 Z0.2 E0 F1800
G1 X10 Y0 Z0.2 E1.0
";
        let layers = parse_gcode_bytes(gcode);
        let layer = first_printed_layer(&layers);
        assert_eq!(layer.meta.nozzle_temp, Some(235.0));
    }

    /// Active tool index is captured from `T<n>` commands.
    #[test]
    fn active_tool_is_captured() {
        let gcode = b"
;LAYER_CHANGE
;Z:0.200
T1
;TYPE:Outer wall
G1 X0 Y0 Z0.2 E0 F1800
G1 X10 Y0 Z0.2 E1.0
";
        let layers = parse_gcode_bytes(gcode);
        let layer = first_printed_layer(&layers);
        assert_eq!(layer.meta.tool, 1);
    }

    /// Sticky fan/temperature state persists to later layers without re-emission.
    #[test]
    fn sticky_state_persists_across_layers() {
        let gcode = b"
M104 S215
;LAYER_CHANGE
;Z:0.200
M106 P0 S255
;TYPE:Outer wall
G1 X0 Y0 Z0.2 E0 F1800
G1 X10 Y0 Z0.2 E1.0
;LAYER_CHANGE
;Z:0.400
;TYPE:Outer wall
G1 X0 Y0 Z0.4 E2.0 F1800
G1 X10 Y0 Z0.4 E3.0
";
        let layers = parse_gcode_bytes(gcode);
        // The second printed layer never re-emits temp or fan, but inherits both.
        let printed: Vec<&InternalLayer> = layers.iter().filter(|l| !l.is_empty()).collect();
        assert!(printed.len() >= 2, "expected two printed layers");
        let last = printed[printed.len() - 1];
        assert_eq!(last.meta.nozzle_temp, Some(215.0));
        assert_eq!(fan_speed(last, "P0"), Some(1.0));
    }

    /// `;LAYER_TIME:` markers populate the per-layer time.
    #[test]
    fn layer_time_marker_is_captured() {
        let gcode = b"
;LAYER_CHANGE
;Z:0.200
;LAYER_TIME:12.5
;TYPE:Outer wall
G1 X0 Y0 Z0.2 E0 F1800
G1 X10 Y0 Z0.2 E1.0
";
        let layers = parse_gcode_bytes(gcode);
        let layer = first_printed_layer(&layers);
        assert_eq!(layer.meta.layer_time_s, Some(12.5));
    }
}
