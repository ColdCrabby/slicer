import type { FilamentProfile } from '../../generated/slicer-engine-ws-client-message-v1';
import { uid } from './id';

/**
 * Filament (material) profile — the engine's own type. Material/domain fields
 * live at the top level; temperatures, cooling, flow and filament diameter live
 * in {@link FilamentProfile.params} as a partial `SlicingParams` (engine field
 * names and units — fan speeds are fractions 0–1). No mapping layer.
 */
export type { FilamentProfile };

export type FilamentMaterial = FilamentProfile['material'];

export const FILAMENT_MATERIALS: FilamentMaterial[] = [
  'PLA',
  'PETG',
  'ABS',
  'ASA',
  'TPU',
  'PC',
  'Nylon',
  'PVA',
];

export const FILAMENT_MATERIAL_LABELS: Record<FilamentMaterial, string> = {
  PLA: 'PLA',
  PETG: 'PETG',
  ABS: 'ABS',
  ASA: 'ASA',
  TPU: 'TPU (flexible)',
  PC: 'Polycarbonate',
  Nylon: 'Nylon (PA)',
  PVA: 'PVA (support)',
};

/** Density (g/cm³) per material, for weight / cost estimation. */
export const MATERIAL_DENSITY: Record<FilamentMaterial, number> = {
  PLA: 1.24,
  PETG: 1.27,
  ABS: 1.04,
  ASA: 1.07,
  TPU: 1.21,
  PC: 1.2,
  Nylon: 1.14,
  PVA: 1.23,
};

/**
 * Typical starting slice params per material (engine-native units: fan speeds
 * are fractions 0–1). Used to pre-fill the wizard when the user picks a
 * material, mirroring the engine's `FilamentMaterial::default_params`.
 */
export const MATERIAL_PARAMS: Record<FilamentMaterial, Record<string, unknown>> = {
  PLA: mat(210, 215, 60, 60, 1.0, 1.0, 15),
  PETG: mat(240, 245, 80, 80, 0.4, 0.6, 12),
  ABS: mat(250, 255, 100, 105, 0.0, 0.3, 11),
  ASA: mat(250, 255, 100, 105, 0.0, 0.3, 11),
  TPU: mat(230, 235, 40, 45, 0.5, 0.8, 4),
  PC: mat(270, 275, 110, 110, 0.0, 0.2, 10),
  Nylon: mat(260, 265, 90, 90, 0.0, 0.2, 10),
  PVA: mat(215, 220, 60, 60, 0.3, 0.5, 6),
};

function mat(
  nozzle: number,
  nozzleFirst: number,
  bed: number,
  bedFirst: number,
  fanMin: number,
  fanMax: number,
  vmax: number,
): Record<string, unknown> {
  return {
    nozzle_temp: nozzle,
    nozzle_temp_first_layer: nozzleFirst,
    bed_temp: bed,
    bed_temp_first_layer: bedFirst,
    first_layer_fan_speed: fanMin,
    fan_speed: fanMax,
    max_volumetric_speed: vmax,
    disable_fan_first_layers: 1,
    flow_ratio: 1.0,
    pressure_advance: 0.04,
    filament_diameter_mm: 1.75,
  };
}

export function makeFilament(overrides: Partial<FilamentProfile> = {}): FilamentProfile {
  const material = overrides.material ?? 'PLA';
  return {
    id: uid(),
    name: 'New filament',
    source: 'user',
    vendor: 'Custom',
    material,
    color: '#e0730f',
    density_g_cm3: MATERIAL_DENSITY[material],
    cost_per_kg: 25,
    params: { ...MATERIAL_PARAMS[material] },
    ...overrides,
  };
}

/** The single offline default filament (a generic PLA). */
export const DEFAULT_FILAMENT: FilamentProfile = makeFilament({
  id: 'builtin-generic-pla',
  name: 'Generic PLA',
  source: 'builtin',
  vendor: 'Generic',
  material: 'PLA',
  color: '#d8d8dc',
});

export const DEFAULT_FILAMENTS: FilamentProfile[] = [DEFAULT_FILAMENT];
