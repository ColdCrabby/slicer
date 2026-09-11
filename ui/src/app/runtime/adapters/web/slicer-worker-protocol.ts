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
  /** Encoded support paint (enforcers/blockers), or absent when unpainted. */
  supportPaint?: string;
}

/**
 * `{ printer, filament, process, overrides }` — the engine's own
 * `ProfileSelection`, passed through untouched. The worker hands it to the
 * wasm `resolveSliceParams` binding, so the browser composes profiles with the
 * same Rust code the server runs rather than a TypeScript lookalike.
 */
export type WorkerProfileSelection = Record<string, unknown>;

export type SlicerWorkerRequest =
  | { type: 'init'; wasmUrl: string }
  | {
      type: 'slice';
      sliceId: string;
      profiles: WorkerProfileSelection;
      /** Base64 PNG rendered by the viewer; folded in after resolution. */
      thumbnailPngBase64?: string;
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
