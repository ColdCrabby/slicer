/**
 * Human-friendly labels for the slicer settings schema.
 *
 * The schema is generated from Rust and carries no `title` for its fields, so
 * without help the form would show raw technical identifiers like
 * `wall_transition_filter_distance` or enum consts like `sharpest_corner`.
 * This module translates those ids into readable labels.
 *
 * Two curated dictionaries provide the high-quality wording; a generic
 * `humanize` fallback keeps any *unmapped* key/value (e.g. a newly added
 * parameter) legible until it earns a curated entry, so labels never regress
 * to a raw identifier.
 */

/** Curated field-key → label map. Keyed by the schema property name. */
const FIELD_LABELS: Record<string, string> = {
  // Layer
  layer_height: 'Layer Height',
  // Walls
  wall_generator: 'Wall Generator',
  wall_count: 'Wall Count',
  wall_line_width_min: 'Min Wall Line Width',
  wall_line_width_max: 'Max Wall Line Width',
  wall_transition_threshold: 'Wall Transition Threshold',
  wall_transition_length: 'Wall Transition Length',
  wall_distribution_count: 'Wall Distribution Count',
  wall_transition_angle: 'Wall Transition Angle',
  wall_transition_filter_distance: 'Wall Transition Filter Distance',
  seam_position: 'Seam Position',
  gap_fill_min_length_mm: 'Min Gap Fill Length',
  wall_overlap_compensation: 'Wall Overlap Compensation',
  // Infill
  infill_density: 'Infill Density',
  infill_pattern: 'Infill Pattern',
  infill_base_angle: 'Infill Angle',
  infill_overlap_percent: 'Infill Overlap',
  infill_perimeter_gap_mm: 'Infill–Perimeter Gap',
  // Speed
  print_speed: 'Print Speed',
  perimeter_speed: 'Perimeter Speed',
  infill_speed: 'Infill Speed',
  bridge_speed: 'Bridge Speed',
  bridge_flow_ratio: 'Bridge Flow Ratio',
  top_surface_speed: 'Top Surface Speed',
  gap_fill_speed: 'Gap Fill Speed',
  first_layer_speed: 'First Layer Speed',
  coasting_distance_mm: 'Coasting Distance',
  travel_speed_mm_min: 'Travel Speed',
  // Quality
  bridge_min_area_mm2: 'Min Bridge Area',
  bridge_noise_filter_mm: 'Bridge Noise Filter',
  bridge_anchor_mm: 'Bridge Anchor Length',
  // Cooling
  fan_speed: 'Fan Speed',
  bridge_fan_speed: 'Bridge Fan Speed',
  first_layer_fan_speed: 'First Layer Fan Speed',
  fan_configs: 'Fan Configurations',
  // Temperature
  nozzle_temp: 'Nozzle Temperature',
  bed_temp: 'Bed Temperature',
  // Surfaces
  top_layers: 'Top Layers',
  bottom_layers: 'Bottom Layers',
  surface_infill_angle: 'Surface Infill Angle',
  only_one_wall_top: 'Single Wall on Top Surfaces',
  only_one_wall_first_layer: 'Single Wall on First Layer',
  support_threshold_angle: 'Support Threshold Angle',
  min_infill_extrusion_mm: 'Min Infill Extrusion Length',
  // Hardware
  filament_diameter_mm: 'Filament Diameter',
  nozzle_diameter_mm: 'Nozzle Diameter',
  // Retraction
  z_hop_mm: 'Z Hop',
  retract_mm: 'Retraction Distance',
  // Output
  path_tolerance: 'Path Tolerance',
  gcode_flavor: 'G-code Flavor',
  // Mesh
  mesh_quality: 'Mesh Quality',
};

/** Curated enum-const → label map, shared across every enum in the schema. */
const ENUM_LABELS: Record<string, string> = {
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
  Grid: 'Grid',
  Honeycomb: 'Honeycomb',
  Gyroid: 'Gyroid',
  TpmsD: 'TPMS-D',
  // MeshQuality
  Normal: 'Normal',
  HighQuality: 'High Quality',
  Draft: 'Draft',
  // GcodeFlavor
  marlin: 'Marlin',
  klipper: 'Klipper',
};

/** Tokens that should keep a specific casing when the generic fallback runs. */
const TOKEN_OVERRIDES: Record<string, string> = {
  mm: 'mm',
  mm2: 'mm²',
  deg: '°',
  pct: '%',
  percent: '%',
  id: 'ID',
  gcode: 'G-code',
  tpms: 'TPMS',
  z: 'Z',
  min: 'Min',
  max: 'Max',
};

/**
 * Generic identifier → label fallback: splits snake_case / camelCase into words
 * and title-cases them, honouring the known-token casing overrides.
 */
export function humanize(id: string): string {
  return id
    .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
    .split(/[_\s]+/)
    .filter(Boolean)
    .map((word) => {
      const lower = word.toLowerCase();
      if (lower in TOKEN_OVERRIDES) return TOKEN_OVERRIDES[lower];
      return lower.charAt(0).toUpperCase() + lower.slice(1);
    })
    .join(' ');
}

/** Friendly label for a schema field key. */
export function fieldLabel(key: string): string {
  return FIELD_LABELS[key] ?? humanize(key);
}

/** Friendly label for an enum const value. */
export function enumLabel(value: string): string {
  return ENUM_LABELS[value] ?? humanize(value);
}
