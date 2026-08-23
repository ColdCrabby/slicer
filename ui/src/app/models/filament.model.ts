import { uid } from './id';

export type FilamentMaterial = 'PLA' | 'PETG' | 'ABS' | 'ASA' | 'TPU' | 'PC' | 'Nylon';

export const FILAMENT_MATERIALS: FilamentMaterial[] = [
  'PLA',
  'PETG',
  'ABS',
  'ASA',
  'TPU',
  'PC',
  'Nylon',
];

export interface FilamentProfile {
  id: string;
  name: string;
  vendor: string;
  material: FilamentMaterial;
  color: string;
  diameterMm: number;
  nozzleTemp: number;
  bedTemp: number;
}

export function makeFilament(): FilamentProfile {
  return {
    id: uid(),
    name: 'New filament',
    vendor: 'Custom',
    material: 'PLA',
    color: '#e0730f',
    diameterMm: 1.75,
    nozzleTemp: 210,
    bedTemp: 60,
  };
}

export const DEFAULT_FILAMENTS: FilamentProfile[] = [
  {
    id: 'seed-pla',
    name: 'Prusament PLA Galaxy Black',
    vendor: 'Prusament',
    material: 'PLA',
    color: '#1c1c22',
    diameterMm: 1.75,
    nozzleTemp: 215,
    bedTemp: 60,
  },
  {
    id: 'seed-petg',
    name: 'Overture PETG White',
    vendor: 'Overture',
    material: 'PETG',
    color: '#f2f2f2',
    diameterMm: 1.75,
    nozzleTemp: 240,
    bedTemp: 80,
  },
];
