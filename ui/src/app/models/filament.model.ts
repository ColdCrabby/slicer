import type { SlicingParams } from '../../generated/slicer-engine-ws-client-message-v1';
import { uid } from './id';
import type { ProfileMeta } from './profile-source';

export type FilamentMaterial = 'PLA' | 'PETG' | 'ABS' | 'ASA' | 'TPU' | 'PC' | 'Nylon' | 'PVA';

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

/**
 * A filament (material) profile.
 *
 * Owns everything that depends on the *material*: temperatures, cooling, flow,
 * and the physical constants used for weight / cost estimation. Deliberately
 * free of any machine or quality settings so one spool can be printed on any
 * printer with any quality profile.
 */
export interface FilamentProfile extends ProfileMeta {
  vendor: string;
  material: FilamentMaterial;
  color: string;
  diameterMm: number;

  // Temperatures ------------------------------------------------------------
  nozzleTemp: number;
  nozzleTempFirstLayer: number;
  bedTemp: number;
  bedTempFirstLayer: number;

  // Cooling -----------------------------------------------------------------
  fanSpeedMin: number;
  fanSpeedMax: number;
  fanAlwaysOn: boolean;
  disableFanFirstLayers: number;

  // Flow / advance ----------------------------------------------------------
  flowRatio: number;
  pressureAdvance: number;
  maxVolumetricSpeed: number;

  // Physical constants (estimation) ----------------------------------------
  densityGCm3: number;
  costPerKg: number;
}

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

/**
 * Typical starting temperatures/cooling per material. Used to pre-fill the
 * wizard when the user picks a material, so a from-scratch filament still
 * lands on sane values instead of PLA defaults.
 */
export const MATERIAL_PRESETS: Record<
  FilamentMaterial,
  Pick<
    FilamentProfile,
    | 'nozzleTemp'
    | 'nozzleTempFirstLayer'
    | 'bedTemp'
    | 'bedTempFirstLayer'
    | 'fanSpeedMin'
    | 'fanSpeedMax'
    | 'fanAlwaysOn'
    | 'maxVolumetricSpeed'
    | 'densityGCm3'
  >
> = {
  PLA: {
    nozzleTemp: 210,
    nozzleTempFirstLayer: 215,
    bedTemp: 60,
    bedTempFirstLayer: 60,
    fanSpeedMin: 100,
    fanSpeedMax: 100,
    fanAlwaysOn: true,
    maxVolumetricSpeed: 15,
    densityGCm3: 1.24,
  },
  PETG: {
    nozzleTemp: 240,
    nozzleTempFirstLayer: 245,
    bedTemp: 80,
    bedTempFirstLayer: 80,
    fanSpeedMin: 40,
    fanSpeedMax: 60,
    fanAlwaysOn: true,
    maxVolumetricSpeed: 12,
    densityGCm3: 1.27,
  },
  ABS: {
    nozzleTemp: 250,
    nozzleTempFirstLayer: 255,
    bedTemp: 100,
    bedTempFirstLayer: 105,
    fanSpeedMin: 0,
    fanSpeedMax: 30,
    fanAlwaysOn: false,
    maxVolumetricSpeed: 11,
    densityGCm3: 1.04,
  },
  ASA: {
    nozzleTemp: 250,
    nozzleTempFirstLayer: 255,
    bedTemp: 100,
    bedTempFirstLayer: 105,
    fanSpeedMin: 0,
    fanSpeedMax: 30,
    fanAlwaysOn: false,
    maxVolumetricSpeed: 11,
    densityGCm3: 1.07,
  },
  TPU: {
    nozzleTemp: 230,
    nozzleTempFirstLayer: 235,
    bedTemp: 40,
    bedTempFirstLayer: 45,
    fanSpeedMin: 50,
    fanSpeedMax: 80,
    fanAlwaysOn: true,
    maxVolumetricSpeed: 4,
    densityGCm3: 1.21,
  },
  PC: {
    nozzleTemp: 270,
    nozzleTempFirstLayer: 275,
    bedTemp: 110,
    bedTempFirstLayer: 110,
    fanSpeedMin: 0,
    fanSpeedMax: 20,
    fanAlwaysOn: false,
    maxVolumetricSpeed: 10,
    densityGCm3: 1.2,
  },
  Nylon: {
    nozzleTemp: 260,
    nozzleTempFirstLayer: 265,
    bedTemp: 90,
    bedTempFirstLayer: 90,
    fanSpeedMin: 0,
    fanSpeedMax: 20,
    fanAlwaysOn: false,
    maxVolumetricSpeed: 10,
    densityGCm3: 1.14,
  },
  PVA: {
    nozzleTemp: 215,
    nozzleTempFirstLayer: 220,
    bedTemp: 60,
    bedTempFirstLayer: 60,
    fanSpeedMin: 30,
    fanSpeedMax: 50,
    fanAlwaysOn: true,
    maxVolumetricSpeed: 6,
    densityGCm3: 1.23,
  },
};

export function makeFilament(overrides: Partial<FilamentProfile> = {}): FilamentProfile {
  const material = overrides.material ?? 'PLA';
  const preset = MATERIAL_PRESETS[material];
  return {
    id: uid(),
    name: 'New filament',
    source: 'user',
    vendor: 'Custom',
    material,
    color: '#e0730f',
    diameterMm: 1.75,
    ...preset,
    bedTempFirstLayer: preset.bedTempFirstLayer,
    disableFanFirstLayers: 1,
    flowRatio: 1.0,
    pressureAdvance: 0.04,
    costPerKg: 25,
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

/** Material-owned slice parameters contributed by the active filament. */
export function filamentSliceParams(filament: FilamentProfile): Partial<SlicingParams> {
  return {
    nozzle_temp: filament.nozzleTemp,
    bed_temp: filament.bedTemp,
    fan_speed: filament.fanSpeedMax,
    first_layer_fan_speed: filament.fanSpeedMin,
    filament_diameter_mm: filament.diameterMm,
  };
}
