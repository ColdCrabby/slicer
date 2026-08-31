export type WorkerMeshFormat = 'stl' | 'obj' | '3mf';

export interface WorkerSliceTransform {
  translation: [number, number, number];
  euler_xyz_deg: [number, number, number];
  scale: [number, number, number];
}

export interface WorkerSliceObject {
  name: string;
  format: WorkerMeshFormat;
  bytes: Uint8Array;
  /** Which object inside `bytes` to slice — a 3MF can hold several. */
  partIndex?: number;
  transform: WorkerSliceTransform;
}

export type SlicerWorkerRequest =
  | { type: 'init'; wasmUrl: string }
  | {
      type: 'slice';
      sliceId: string;
      settings: Record<string, unknown>;
      objects: WorkerSliceObject[];
    };

export type WasmSliceEvent =
  | { type: 'log'; level: 'debug' | 'info' | 'warn' | 'error'; message: string }
  | {
      type: 'phase';
      phase: string;
      event: 'start' | 'end';
      elapsed_ms?: number;
      /** 1-based object index when slicing a plate object-by-object. */
      object?: number;
      /** Total objects sliced individually. */
      object_count?: number;
    }
  | { type: 'progress'; current_layer: number; total_layers: number };

export type SlicerWorkerResponse =
  | { type: 'ready' }
  | { type: 'log'; sliceId?: string; level: 'debug' | 'info' | 'warn' | 'error'; message: string }
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
  | { type: 'slice-complete'; sliceId: string; layerCount: number; gcode: string }
  | { type: 'error'; sliceId?: string; message: string };
