import type { SlicingParams } from '../../generated/slicer-engine-ws-client-message-v1';
import { uid } from './id';
import type { ProfileMeta } from './profile-source';

/** Coarse quality tag used for badges / sorting. */
export type PrintQuality = 'draft' | 'standard' | 'fine';
export const PRINT_QUALITIES: PrintQuality[] = ['draft', 'standard', 'fine'];

/** Infill pattern — serialised PascalCase to match the engine's `InfillPattern`. */
export type InfillPattern = 'Rectilinear' | 'Grid' | 'Honeycomb' | 'Gyroid' | 'TpmsD';
export const INFILL_PATTERNS: { value: InfillPattern; label: string }[] = [
  { value: 'Rectilinear', label: 'Rectilinear (fast)' },
  { value: 'Grid', label: 'Grid' },
  { value: 'Honeycomb', label: 'Honeycomb' },
  { value: 'Gyroid', label: 'Gyroid (strong)' },
  { value: 'TpmsD', label: 'TPMS-D (organic)' },
];

/** Seam placement — serialised snake_case to match the engine's `SeamPosition`. */
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

/**
 * A print (process/quality) profile.
 *
 * Owns everything that trades speed against quality: layers, walls, infill,
 * speeds, supports, adhesion, and seam. Independent of the material and the
 * machine so the same quality preset can be reused everywhere.
 */
export interface PrintProfile extends ProfileMeta {
  quality: PrintQuality;

  // Layers ------------------------------------------------------------------
  layerHeight: number;
  firstLayerHeight: number;
  lineWidth: number;

  // Shell -------------------------------------------------------------------
  wallCount: number;
  topLayers: number;
  bottomLayers: number;
  seamPosition: SeamPosition;

  // Infill ------------------------------------------------------------------
  infillDensity: number;
  infillPattern: InfillPattern;
  infillAngle: number;

  // Speeds (mm/s) -----------------------------------------------------------
  speedPrint: number;
  speedWall: number;
  speedInfill: number;
  speedTopSurface: number;
  speedFirstLayer: number;

  // Supports ----------------------------------------------------------------
  supportEnabled: boolean;
  supportType: SupportType;
  supportThreshold: number;
  supportDensity: number;

  // Adhesion ----------------------------------------------------------------
  adhesionType: AdhesionType;
  brimWidth: number;
  skirtLoops: number;

  ironingEnabled: boolean;
}

export function makePrintProfile(overrides: Partial<PrintProfile> = {}): PrintProfile {
  return {
    id: uid(),
    name: 'New profile',
    source: 'user',
    quality: 'standard',
    layerHeight: 0.2,
    firstLayerHeight: 0.24,
    lineWidth: 0.44,
    wallCount: 3,
    topLayers: 4,
    bottomLayers: 3,
    seamPosition: 'aligned',
    infillDensity: 0.2,
    infillPattern: 'Gyroid',
    infillAngle: 45,
    speedPrint: 120,
    speedWall: 80,
    speedInfill: 150,
    speedTopSurface: 60,
    speedFirstLayer: 30,
    supportEnabled: false,
    supportType: 'normal',
    supportThreshold: 55,
    supportDensity: 0.15,
    adhesionType: 'skirt',
    brimWidth: 5,
    skirtLoops: 1,
    ironingEnabled: false,
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

/** Quality-owned slice parameters contributed by the active print profile. */
export function printProfileSliceParams(profile: PrintProfile): Partial<SlicingParams> {
  return {
    layer_height: profile.layerHeight,
    wall_count: profile.wallCount,
    top_layers: profile.topLayers,
    bottom_layers: profile.bottomLayers,
    seam_position: profile.seamPosition,
    infill_density: profile.infillDensity,
    infill_pattern: profile.infillPattern,
    infill_base_angle: profile.infillAngle,
    print_speed: profile.speedPrint,
    perimeter_speed: profile.speedWall,
    infill_speed: profile.speedInfill,
    top_surface_speed: profile.speedTopSurface,
    first_layer_speed: profile.speedFirstLayer,
    support_threshold_angle: profile.supportEnabled ? profile.supportThreshold : 0,
  };
}
