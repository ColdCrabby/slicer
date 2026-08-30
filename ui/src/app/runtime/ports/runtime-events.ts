import { RuntimeError } from './runtime-errors';

export type RuntimeEvent =
  | { type: 'connected'; mode: 'native' | 'cloud' | 'web'; serverVersion?: string }
  | { type: 'phase-start'; sliceId: string; phase: string; object?: number; objectCount?: number }
  | {
      type: 'phase-end';
      sliceId: string;
      phase: string;
      elapsedMs?: number;
      object?: number;
      objectCount?: number;
    }
  | { type: 'progress'; sliceId: string; currentLayer: number; totalLayers: number }
  | { type: 'log'; level: 'debug' | 'info' | 'warn' | 'error'; message: string }
  | { type: 'slice-complete'; sliceId: string; layerCount: number; downloadUrl?: string }
  | { type: 'error'; error: RuntimeError };

export type RuntimeEventListener = (event: RuntimeEvent) => void;
