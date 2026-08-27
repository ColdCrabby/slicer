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

export type InfillPattern = 'Rectilinear' | 'Grid' | 'Honeycomb' | 'Gyroid' | 'TpmsD';
export const INFILL_PATTERNS: { value: InfillPattern; label: string }[] = [
  { value: 'Rectilinear', label: 'Rectilinear (fast)' },
  { value: 'Grid', label: 'Grid' },
  { value: 'Honeycomb', label: 'Honeycomb' },
  { value: 'Gyroid', label: 'Gyroid (strong)' },
  { value: 'TpmsD', label: 'TPMS-D (organic)' },
];

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
    infill_density: 0.2,
    infill_pattern: 'Gyroid',
    infill_base_angle: 45,
    print_speed: 120,
    perimeter_speed: 80,
    infill_speed: 150,
    top_surface_speed: 60,
    first_layer_speed: 30,
    support_threshold_angle: 55,
    adhesion_type: 'skirt',
    skirt_loops: 1,
    thumbnail_enabled: true,
    thumbnail_size_px: 320,
    thumbnail_view: 'isometric',
    thumbnail_theme: 'light',
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

/** The single offline default print profile. */
export const DEFAULT_PRINT_PROFILE: PrintProfile = makePrintProfile({
  id: 'builtin-standard-02',
  name: 'Standard — 0.20 mm',
  source: 'builtin',
  quality: 'standard',
});

export const DEFAULT_PRINT_PROFILES: PrintProfile[] = [DEFAULT_PRINT_PROFILE];
