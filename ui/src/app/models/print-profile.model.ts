import type { ProcessProfile } from '../../generated/slicer-engine-ws-client-message-v1';
import { uid } from './id';

/**
 * Process (print / quality) profile — the engine's own type. Only the coarse
 * `quality` tag lives at the top level; every slice parameter lives in
 * {@link ProcessProfile.params} as a partial `SlicingParams` (engine field
 * names and units). No separate camelCase model, no mapping.
 */
export type { ProcessProfile };
/** Back-compat alias — the process profile *is* the print profile. */
export type PrintProfile = ProcessProfile;

export type PrintQuality = NonNullable<ProcessProfile['quality']>;
export const PRINT_QUALITIES: PrintQuality[] = ['draft', 'standard', 'fine'];

/**
 * Infill pattern, taken straight from the engine's generated schema type rather
 * than re-declared here — a hand-written union silently omitted every pattern
 * added after it was written.
 */
export type InfillPattern = NonNullable<NonNullable<ProcessProfile['params']>['infill_pattern']>;

/**
 * Every infill pattern the engine offers, in rough order of how often it is
 * wanted. The list must stay exhaustive: `Record<InfillPattern, string>` makes
 * the compiler fail the build if the engine gains a pattern and this is not
 * updated.
 */
const INFILL_PATTERN_LABELS: Record<InfillPattern, string> = {
  Rectilinear: 'Rectilinear (fast)',
  AlignedRectilinear: 'Aligned rectilinear',
  Grid: 'Grid',
  Triangles: 'Triangles',
  TriHexagon: 'Tri-hexagon',
  Cubic: 'Cubic (3D)',
  Honeycomb: 'Honeycomb',
  Concentric: 'Concentric',
  Gyroid: 'Gyroid (strong)',
  TpmsD: 'TPMS-D (organic)',
};

export const INFILL_PATTERNS: { value: InfillPattern; label: string }[] = (
  Object.keys(INFILL_PATTERN_LABELS) as InfillPattern[]
).map((value) => ({ value, label: INFILL_PATTERN_LABELS[value] }));

export type SeamPosition = 'nearest' | 'rear' | 'aligned' | 'sharpest_corner' | 'random';
export const SEAM_POSITIONS: { value: SeamPosition; label: string }[] = [
  { value: 'nearest', label: 'Nearest (fastest)' },
  { value: 'aligned', label: 'Aligned' },
  { value: 'rear', label: 'Rear' },
  { value: 'sharpest_corner', label: 'Sharpest corner (hidden)' },
  { value: 'random', label: 'Random' },
];

export type AdhesionType = 'none' | 'skirt' | 'brim' | 'raft';
export const ADHESION_TYPES: { value: AdhesionType; label: string }[] = [
  { value: 'none', label: 'None' },
  { value: 'skirt', label: 'Skirt' },
  { value: 'brim', label: 'Brim' },
  { value: 'raft', label: 'Raft' },
];

export type SupportType = 'normal' | 'tree';

/** Default slice params contributed by a from-scratch standard profile. */
export function defaultProcessParams(): Record<string, unknown> {
  return {
    layer_height: 0.2,
    first_layer_height: 0.24,
    line_width: 0.44,
    wall_generator: 'arachne',
    wall_count: 3,
    top_layers: 4,
    bottom_layers: 3,
    seam_position: 'aligned',
    infill_density: 0.15,
    infill_pattern: 'Rectilinear',
    infill_base_angle: 45,
    print_speed: 120,
    perimeter_speed: 80,
    infill_speed: 150,
    top_surface_speed: 60,
    first_layer_speed: 30,
    support_threshold_angle: 45,
    adhesion_type: 'skirt',
    skirt_loops: 1,
    thumbnail_enabled: true,
    thumbnail_size_px: 320,
    thumbnail_view: 'isometric',
    thumbnail_theme: 'transparent',
    thumbnail_color_mode: 'filament',
    thumbnail_custom_color: '#e0912f',
  };
}

export function makePrintProfile(overrides: Partial<PrintProfile> = {}): PrintProfile {
  return {
    id: uid(),
    name: 'New profile',
    source: 'user',
    quality: 'standard',
    params: defaultProcessParams(),
    ...overrides,
  };
}

/**
 * Speeds and accelerations for a well-built CoreXY, at `x` times the standard
 * preset's pace.
 *
 * Everything the fast presets change lives in one place because they differ
 * from Standard in exactly this and nothing else: a "go faster" preset that
 * quietly reshaped the print — a wall count, an infill pattern — would not be
 * what the user picked it for.
 *
 * The outer wall and the top surface deliberately lag the rest. They are what
 * the print is judged by and neither is where the time goes; going fast on the
 * inside is what pays for going slowly on the outside.
 */
function fastParams(speeds: Record<string, number>): Record<string, unknown> {
  return {
    ...defaultProcessParams(),
    // Derived from the nozzle rather than pinned at 0.44: these are the
    // machines least likely to be running a 0.4.
    line_width: 0,
    ...speeds,
  };
}

/** The offline default print profile — what every fallback resolves to. */
export const DEFAULT_PRINT_PROFILE: PrintProfile = makePrintProfile({
  id: 'builtin-standard-02',
  name: 'Standard — 0.20 mm',
  source: 'builtin',
  quality: 'standard',
});

/** 0.20 mm for a well-built CoreXY with a high-flow hotend. */
export const HIGH_SPEED_PRINT_PROFILE: PrintProfile = makePrintProfile({
  id: 'builtin-high-speed-02',
  name: 'High Speed — 0.20 mm',
  source: 'builtin',
  quality: 'standard',
  params: fastParams({
    print_speed: 200,
    perimeter_speed: 120,
    infill_speed: 250,
    top_surface_speed: 100,
    first_layer_speed: 40,
    travel_speed_mm_min: 24000,
    acceleration: 15000,
    first_layer_acceleration: 3000,
    outer_wall_acceleration: 6000,
    inner_wall_acceleration: 12000,
    sparse_infill_acceleration: 18000,
    solid_infill_acceleration: 12000,
    top_surface_acceleration: 8000,
    travel_acceleration: 25000,
    // Below this a fast machine rounds every corner it is allowed to; above it,
    // it rings.
    square_corner_velocity: 5,
  }),
});

/** 0.20 mm at the limits a tuned machine can actually hold. */
export const MAXIMUM_PRINT_PROFILE: PrintProfile = makePrintProfile({
  id: 'builtin-maximum-02',
  name: 'Maximum — 0.20 mm',
  source: 'builtin',
  quality: 'standard',
  params: fastParams({
    print_speed: 300,
    perimeter_speed: 200,
    infill_speed: 300,
    top_surface_speed: 150,
    first_layer_speed: 50,
    travel_speed_mm_min: 36000,
    acceleration: 25000,
    first_layer_acceleration: 5000,
    outer_wall_acceleration: 10000,
    inner_wall_acceleration: 20000,
    sparse_infill_acceleration: 30000,
    solid_infill_acceleration: 20000,
    top_surface_acceleration: 10000,
    gap_fill_acceleration: 5000,
    support_acceleration: 20000,
    travel_acceleration: 30000,
    square_corner_velocity: 5,
  }),
});

/** The built-in print profiles, slowest first — they read as one scale. */
export const DEFAULT_PRINT_PROFILES: PrintProfile[] = [
  DEFAULT_PRINT_PROFILE,
  HIGH_SPEED_PRINT_PROFILE,
  MAXIMUM_PRINT_PROFILE,
];
