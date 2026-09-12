/**
 * Human-friendly labels for the slicer settings schema.
 *
 * The schema is generated from Rust and carries no `title` for its fields, so
 * without help the form would show raw technical identifiers like
 * `wall_transition_filter_distance` or enum consts like `sharpest_corner`.
 * This module translates those ids into readable labels.
 *
 * Two curated dictionaries provide the wording. **Anything not in them shows
 * its raw schema key**, on purpose.
 *
 * There used to be a `humanize` fallback that split an identifier on
 * underscores and title-cased the pieces. It invented labels that looked
 * authored but were not: "Filament Density G Cm3", "Min Layer Time S", "Xy
 * Hole Compensation". A reader cannot tell a manufactured label from a written
 * one, so every such string quietly claimed an editorial care that was never
 * applied — and the raw key, which is at least searchable and unambiguous, was
 * hidden behind it. An unmapped parameter reads as `wall_transition_length`
 * until somebody gives it a real name — and `field-labels.spec.ts` fails while
 * one is unnamed, so the raw key is a build-time defect rather than something a
 * user finds.
 */

/** Curated field-key → label map. Keyed by the schema property name. */
export const FIELD_LABELS: Record<string, string> = {
  // Layer
  layer_height: 'Layer Height',
  first_layer_height: 'First Layer Height',
  // Walls
  wall_generator: 'Wall Generator',
  wall_count: 'Wall Count',
  line_width: 'Line Width',
  outer_wall_line_width: 'Outer Wall Line Width',
  inner_wall_line_width: 'Inner Wall Line Width',
  wall_line_width_min: 'Min Wall Line Width',
  wall_line_width_max: 'Max Wall Line Width',
  wall_transition_threshold: 'Wall Transition Threshold',
  wall_transition_length: 'Wall Transition Length',
  wall_distribution_count: 'Wall Distribution Count',
  wall_transition_angle: 'Wall Transition Angle',
  wall_transition_filter_distance: 'Wall Transition Filter Distance',
  seam_position: 'Seam Position',
  spiral_vase: 'Spiral Vase Mode',
  gap_fill_min_length_mm: 'Min Gap Fill Length',
  wall_overlap_compensation: 'Wall Overlap Compensation',
  thin_walls: 'Thin Wall Detection',
  external_perimeters_first: 'Outer Wall First',
  extra_perimeters: 'Extra Walls Where Needed',
  extra_perimeters_max_gap: 'Extra Wall Max Gap',
  ensure_vertical_shell_thickness: 'Vertical Shell Thickness',
  avoid_crossing_perimeters: 'Avoid Crossing Walls',
  fuzzy_skin: 'Fuzzy Skin',
  fuzzy_skin_thickness_mm: 'Fuzzy Skin Depth',
  fuzzy_skin_point_dist_mm: 'Fuzzy Skin Point Spacing',
  // Infill
  infill_density: 'Infill Density',
  infill_pattern: 'Infill Pattern',
  infill_base_angle: 'Infill Angle',
  infill_overlap_percent: 'Infill Overlap',
  infill_perimeter_gap_mm: 'Infill–Perimeter Gap',
  sparse_infill_line_width: 'Infill Line Width',
  infill_anchor_percent: 'Infill Anchor Length',
  infill_anchor_max_mm: 'Max Infill Anchor Length',
  infill_every_layers: 'Combine Infill Every',
  infill_combination_max_layer_height_mm: 'Max Combined Infill Height',
  solid_infill_every_layers: 'Solid Layer Every',
  // Speed
  print_speed: 'Print Speed',
  perimeter_speed: 'Perimeter Speed',
  infill_speed: 'Infill Speed',
  bridge_speed: 'Bridge Speed',
  enable_overhang_speed: 'Dynamic Overhang Speed',
  overhang_1_4_speed: 'Overhang Speed (0–25%)',
  overhang_2_4_speed: 'Overhang Speed (25–50%)',
  overhang_3_4_speed: 'Overhang Speed (50–75%)',
  overhang_4_4_speed: 'Overhang Speed (75–100%)',
  slowdown_for_curled_perimeters: 'Slow Down Curled Perimeters',
  bridge_flow_ratio: 'Bridge Flow Ratio',
  top_surface_speed: 'Top Surface Speed',
  gap_fill_speed: 'Gap Fill Speed',
  support_speed: 'Support Speed',
  first_layer_speed: 'First Layer Speed',
  coasting_distance_mm: 'Coasting Distance',
  travel_speed_mm_min: 'Travel Speed',
  acceleration: 'Acceleration',
  first_layer_acceleration: 'First Layer Acceleration',
  outer_wall_acceleration: 'Outer Wall Acceleration',
  inner_wall_acceleration: 'Inner Wall Acceleration',
  top_surface_acceleration: 'Top Surface Acceleration',
  solid_infill_acceleration: 'Solid Infill Acceleration',
  sparse_infill_acceleration: 'Infill Acceleration',
  gap_fill_acceleration: 'Gap Fill Acceleration',
  bridge_acceleration: 'Bridge Acceleration',
  support_acceleration: 'Support Acceleration',
  travel_acceleration: 'Travel Acceleration',
  square_corner_velocity: 'Square Corner Velocity',
  max_velocity: 'Max Velocity',
  // Extrusion
  flow_ratio: 'Flow Ratio',
  pressure_advance: 'Pressure Advance',
  max_volumetric_speed: 'Max Volumetric Speed',
  // Quality
  bridge_min_area_mm2: 'Min Bridge Area',
  bridge_noise_filter_mm: 'Bridge Noise Filter',
  bridge_anchor_mm: 'Bridge Anchor Length',
  bridge_angle: 'Bridge Angle',
  xy_size_compensation: 'XY Size Compensation',
  xy_hole_compensation: 'Hole Compensation',
  elephant_foot_compensation_mm: 'Elephant Foot Compensation',
  elephant_foot_layers: 'Elephant Foot Layers',
  elephant_foot_min_contour_width_mm: 'Elephant Foot Min Feature Width',
  // Cooling
  fan_speed: 'Fan Speed Limit',
  bridge_fan_speed: 'Bridge Fan Speed',
  overhang_fan_speed: 'Overhang Fan Speed',
  overhang_fan_threshold: 'Overhang Fan Threshold',
  first_layer_fan_speed: 'First Layer Fan Speed',
  disable_fan_first_layers: 'Fan Off For First Layers',
  fan_configs: 'Fan Configurations',
  min_layer_time_s: 'Min Layer Time',
  min_print_speed: 'Min Print Speed',
  // Temperature
  nozzle_temp: 'Nozzle Temperature',
  nozzle_temp_first_layer: 'First Layer Nozzle Temperature',
  bed_temp: 'Bed Temperature',
  bed_temp_first_layer: 'First Layer Bed Temperature',
  chamber_temp: 'Chamber Temperature',
  chamber_temp_first_layer: 'First Layer Chamber Temperature',
  // Surfaces
  top_layers: 'Top Layers',
  bottom_layers: 'Bottom Layers',
  surface_infill_angle: 'Surface Infill Angle',
  top_surface_pattern: 'Top Surface Pattern',
  bottom_surface_pattern: 'Bottom Surface Pattern',
  internal_solid_infill_pattern: 'Internal Solid Pattern',
  top_surface_line_width: 'Solid Surface Line Width',
  only_one_wall_top: 'Single Wall on Top Surfaces',
  only_one_wall_first_layer: 'Single Wall on First Layer',
  min_infill_extrusion_mm: 'Min Infill Extrusion Length',
  ironing_enabled: 'Ironing',
  ironing_type: 'Ironing Coverage',
  ironing_flow: 'Ironing Flow',
  ironing_spacing: 'Ironing Spacing',
  ironing_speed: 'Ironing Speed',
  ironing_angle: 'Ironing Angle',
  // Support
  support_enabled: 'Generate Supports',
  support_type: 'Support Style',
  support_auto: 'Detect Overhangs Automatically',
  support_density: 'Support Density',
  support_threshold_angle: 'Support Threshold Angle',
  support_interface_layers: 'Interface Layers',
  support_interface_density: 'Interface Density',
  support_xy_distance_mm: 'XY Clearance',
  support_z_gap_layers: 'Z Gap',
  support_on_build_plate_only: 'Only From Build Plate',
  support_line_width: 'Support Line Width',
  // Adhesion
  adhesion_type: 'Build Plate Adhesion',
  brim_width: 'Brim Width',
  brim_type: 'Brim Placement',
  brim_separation: 'Brim Separation',
  skirt_loops: 'Skirt Loops',
  skirt_distance: 'Skirt Distance',
  skirt_height: 'Skirt Height',
  raft_layers: 'Raft Layers',
  raft_air_gap: 'Raft Air Gap',
  // Hardware
  filament_diameter_mm: 'Filament Diameter',
  nozzle_diameter_mm: 'Nozzle Diameter',
  heated_chamber: 'Heated Chamber',
  z_offset_mm: 'Z Offset',
  bed_type: 'Build Plate Type',
  printer_vendor: 'Printer Vendor',
  printer_model: 'Printer Model',
  extruder_count: 'Extruder Count',
  exclude_object: 'Cancel Objects Individually',
  extruder_clearance_height_mm: 'Printhead Clearance Height',
  extruder_clearance_radius_mm: 'Printhead Clearance Radius',
  bed_mesh_mode: 'Bed Mesh Leveling',
  bed_mesh_profile_name: 'Mesh Profile Name',
  bed_mesh_adaptive: 'Adaptive Mesh Area',
  // Material
  filament_type: 'Material Type',
  filament_name: 'Filament Name',
  filament_color: 'Filament Color',
  filament_density_g_cm3: 'Filament Density',
  filament_cost_per_kg: 'Filament Cost',
  // Retraction
  z_hop_mm: 'Z Hop',
  retract_mm: 'Retraction Distance',
  retract_speed_mm_min: 'Retraction Speed',
  retract_before_travel_mm: 'Minimum Travel Before Retract',
  retract_restart_extra_mm: 'Restart Extra Prime',
  retract_on_layer_change: 'Retract on Layer Change',
  use_firmware_retraction: 'Firmware Retraction',
  use_relative_e_distances: 'Relative Extrusion Distances',
  wipe: 'Wipe While Retracting',
  wipe_distance_mm: 'Wipe Distance',
  retract_before_wipe_percent: 'Retract Before Wipe',
  // Objects
  print_sequence: 'Print Sequence',
  between_objects_gcode: 'Between Objects G-code',
  // Output
  path_tolerance: 'Path Tolerance',
  gcode_flavor: 'G-code Flavor',
  start_gcode: 'Start G-code',
  end_gcode: 'End G-code',
  layer_gcode: 'Layer Change G-code',
  triggers: 'Layer Triggers',
  thumbnail_enabled: 'Embed Thumbnail',
  thumbnail_size_px: 'Thumbnail Size',
  thumbnail_view: 'Thumbnail Angle',
  thumbnail_theme: 'Thumbnail Theme',
  thumbnail_color_mode: 'Model Color',
  thumbnail_custom_color: 'Custom Color',
  // Filament G-code
  start_filament_gcode: 'Filament Start G-code',
  end_filament_gcode: 'Filament End G-code',
  // Time estimate
  time_estimate_warmup_s: 'Warm-Up Allowance',
  time_estimate_cooldown_s: 'Cool-Down Allowance',
  time_estimate_scale: 'Estimate Calibration',
  // Mesh
  mesh_quality: 'Mesh Quality',
};

/**
 * Curated enum-const → label map, shared across every enum in the schema.
 *
 * One map for every enum means a const has one label wherever it appears —
 * `rear` reads as "Rear" for both a seam and a thumbnail angle. It also means a
 * const cannot carry two meanings: if a future enum needs `normal` to say
 * something other than "Normal", the two have to be told apart at the field
 * level rather than here.
 */
export const ENUM_LABELS: Record<string, string> = {
  // WallGenerator
  classic: 'Classic',
  arachne: 'Arachne',
  // SeamPosition
  nearest: 'Nearest',
  rear: 'Rear',
  aligned: 'Aligned',
  sharpest_corner: 'Sharpest Corner',
  random: 'Random',
  // InfillPattern
  Rectilinear: 'Rectilinear',
  AlignedRectilinear: 'Aligned Rectilinear',
  Grid: 'Grid',
  Triangles: 'Triangles',
  TriHexagon: 'Tri-Hexagon',
  Cubic: 'Cubic',
  Honeycomb: 'Honeycomb',
  Concentric: 'Concentric',
  Gyroid: 'Gyroid',
  TpmsD: 'TPMS-D',
  // SurfacePattern
  rectilinear: 'Rectilinear',
  'aligned-rectilinear': 'Aligned Rectilinear',
  monotonic: 'Monotonic',
  'monotonic-line': 'Monotonic Line',
  concentric: 'Concentric',
  // MeshQuality
  Normal: 'Normal',
  HighQuality: 'High Quality',
  Draft: 'Draft',
  // GcodeFlavor
  marlin: 'Marlin',
  klipper: 'Klipper',
  reprap: 'RepRap',
  // AdhesionType
  none: 'None',
  skirt: 'Skirt',
  brim: 'Brim',
  raft: 'Raft',
  // BrimType
  outer_only: 'Outside Only',
  inner_only: 'Inside Only',
  outer_and_inner: 'Inside and Outside',
  ears: 'Ears',
  // SupportType
  normal: 'Normal',
  tree: 'Tree',
  // IroningType
  top_surfaces: 'All Top Surfaces',
  topmost_only: 'Topmost Surface Only',
  all_solid: 'All Solid Surfaces',
  // PrintSequence
  by_layer: 'All at Once',
  by_object: 'One at a Time',
  // BedMeshMode
  off: 'Off',
  load_profile: 'Load Saved Mesh',
  calibrate: 'Calibrate Before Print',
  // ThumbnailColorMode — read under the "Model Color" label, so the noun is
  // already supplied and repeating it only makes the segments too wide to fit.
  generic: 'Generic',
  filament: 'Filament',
  custom: 'Custom',
  // ThumbnailTheme
  light: 'Light',
  dark: 'Dark',
  transparent: 'Transparent',
  // ThumbnailView
  isometric: 'Isometric',
  front: 'Front',
  left: 'Left',
  right: 'Right',
  top: 'Top',
};

/** Friendly label for a schema field key. */
export function fieldLabel(key: string): string {
  return FIELD_LABELS[key] ?? key;
}

/** Friendly label for an enum const value. */
export function enumLabel(value: string): string {
  return ENUM_LABELS[value] ?? value;
}

/**
 * The one-line form of a schema description, for a control that shows its
 * options' explanations rather than hiding them behind a tooltip.
 *
 * Variant docs in `params.rs` are written for the API reference and run to
 * several paragraphs — `wall_generator`'s two choices alone fill a sidebar.
 * The opening sentence is the part that separates the options ("Classic
 * fixed-width concentric perimeters with thin-wall gap fill"); the rest is
 * detail the reader can reach through the field's own ⓘ.
 *
 * The lightweight Markdown the engine emits (`**bold**`, `` `code` ``) is
 * stripped with it, so a card does not show a parameter name in backticks.
 */
export function optionSummary(description?: string): string {
  const plain = (description ?? '')
    .replace(/\*\*/g, '')
    .replace(/`/g, '')
    .split(/\n\s*\n/)[0]
    .replace(/\s+/g, ' ')
    .trim();
  const end = plain.search(/\.(\s|$)/);
  return end === -1 ? plain : plain.slice(0, end + 1);
}
