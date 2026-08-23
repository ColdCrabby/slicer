import { uid } from './id';

export type PrintQuality = 'draft' | 'standard' | 'fine';

export const PRINT_QUALITIES: PrintQuality[] = ['draft', 'standard', 'fine'];

export interface PrintProfile {
  id: string;
  name: string;
  quality: PrintQuality;
  layerHeight: number;
  wallCount: number;
  infillDensity: number;
}

export function makePrintProfile(): PrintProfile {
  return {
    id: uid(),
    name: 'New profile',
    quality: 'standard',
    layerHeight: 0.2,
    wallCount: 3,
    infillDensity: 0.2,
  };
}

export const DEFAULT_PRINT_PROFILES: PrintProfile[] = [
  {
    id: 'seed-draft',
    name: 'Draft — 0.28 mm',
    quality: 'draft',
    layerHeight: 0.28,
    wallCount: 2,
    infillDensity: 0.15,
  },
  {
    id: 'seed-standard',
    name: 'Standard — 0.20 mm',
    quality: 'standard',
    layerHeight: 0.2,
    wallCount: 3,
    infillDensity: 0.2,
  },
  {
    id: 'seed-fine',
    name: 'Fine — 0.12 mm',
    quality: 'fine',
    layerHeight: 0.12,
    wallCount: 4,
    infillDensity: 0.25,
  },
];
