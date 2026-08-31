import { ModelSourceRegistry } from '../../../services/model-source';
import { SceneEngine, SceneOp } from '../../../services/scene-engine';
import { RuntimeHistorySession } from '../../domain/history-models';
import { RuntimePreviewSource } from '../../domain/preview-models';
import {
  RuntimeMeshInput,
  RuntimeSceneOp,
  RuntimeSceneSnapshot,
} from '../../domain/scene-commands';
import { RuntimeSliceRequest, RuntimeSliceResult } from '../../domain/slice-commands';
import { RuntimeEventBus } from '../../infrastructure/event-bus';
import { RuntimeCapabilities } from '../../ports/runtime-capabilities';
import { RuntimeError } from '../../ports/runtime-errors';
import { RuntimeEventListener } from '../../ports/runtime-events';
import { RuntimePort, RuntimeSubscription } from '../../ports/runtime-port';
import type {
  SlicerWorkerRequest,
  SlicerWorkerResponse,
  WorkerSliceObject,
} from './slicer-worker-protocol';

const WEB_CAPABILITIES: RuntimeCapabilities = {
  supportsLocalSlicing: true,
  supportsRemoteJobs: false,
  supportsStreamingProgress: true,
  supportsSceneSnapshotPull: true,
};

interface CachedMesh {
  name: string;
  format: 'stl' | 'obj' | '3mf';
  bytes: Uint8Array;
  /** Which object inside those bytes this scene object is (0 for STL/OBJ). */
  partIndex: number;
}

export class WasmRuntime implements RuntimePort {
  private readonly bus = new RuntimeEventBus();
  private readonly previewBySlice = new Map<string, string>();
  private readonly meshByObjectId = new Map<string, CachedMesh>();
  private slicerWorker: Worker | null = null;
  private workerReady: Promise<void> | null = null;
  private pendingInit: { resolve: () => void; reject: (error: Error) => void } | null = null;
  private pendingSlice: {
    sliceId: string;
    resolve: (result: RuntimeSliceResult) => void;
    reject: (error: Error) => void;
  } | null = null;
  private initialized = false;

  constructor(
    private readonly sceneEngine: SceneEngine,
    private readonly modelSources: ModelSourceRegistry,
  ) {}

  async init(): Promise<void> {
    await this.sceneEngine.ready();
    await this.ensureWorkerReady();
    this.initialized = true;
    this.bus.emit({ type: 'connected', mode: 'web' });
  }

  getCapabilities(): RuntimeCapabilities {
    return WEB_CAPABILITIES;
  }

  async addMesh(input: RuntimeMeshInput): Promise<string[]> {
    this.requireReady();
    const bytes = input.bytes;
    if (!bytes) {
      throw new Error(`WASM runtime requires bytes for '${input.fileName}'`);
    }
    // Register the file once, then stamp its handle on every object it makes.
    // A multi-part 3MF and a duplicated model both resolve back to this single
    // entry, so the bytes are held once rather than once per object.
    const source = this.modelSources.register({
      sourceId: input.sourceId,
      fileName: input.fileName,
      format: input.format,
      bytes,
    });
    const objectIds = this.sceneEngine.addMesh(
      input.fileName,
      input.format,
      bytes,
      source.sourceId,
    );
    // The per-object cache additionally remembers *which part* of the file each
    // object is — the registry is keyed by file and cannot know that.
    objectIds.forEach((objectId, partIndex) => {
      this.meshByObjectId.set(objectId.toString(), {
        name: input.fileName,
        format: input.format,
        bytes: source.bytes ?? bytes,
        partIndex,
      });
    });
    return objectIds.map((id) => id.toString());
  }

  async applySceneOps(ops: RuntimeSceneOp[]): Promise<void> {
    this.requireReady();
    const mappedOps: SceneOp[] = ops.map((op) => {
      // Handled before the shared id lookup: this is the one op that
      // addresses many objects and therefore carries no single `id`.
      if (op.op === 'remove_many') {
        return { op: 'RemoveMany', args: { ids: op.ids.map(BigInt) } };
      }
      const id = BigInt(op.id);
      switch (op.op) {
        case 'remove':
          return { op: 'Remove', args: { id } };
        case 'duplicate':
          return { op: 'Duplicate', args: { id, offset: op.offset ?? [0, 0, 0] } };
        case 'translate':
          return { op: 'Translate', args: { id, delta: op.delta } };
        case 'set_transform':
          return {
            op: 'SetTransform',
            args: {
              id,
              translation: op.translation,
              euler_xyz_deg: op.euler_xyz_deg,
              scale: op.scale,
            },
          };
        case 'rotate':
          return { op: 'Rotate', args: { id, axis: op.axis, degrees: op.degrees } };
        case 'scale':
          return { op: 'Scale', args: { id, factors: op.factors } };
        case 'center_on_bed':
          return { op: 'CenterOnBed', args: { id } };
        case 'drop_to_floor':
          return { op: 'DropToFloor', args: { id } };
        case 'place_face_on_floor':
          return { op: 'PlaceFaceOnFloor', args: { id, face_index: op.face_index } };
      }
    });

    this.sceneEngine.applyBatch(mappedOps);
    for (const op of ops) {
      if (op.op === 'remove') {
        this.meshByObjectId.delete(op.id);
      }
    }
  }

  async getSceneSnapshot(): Promise<RuntimeSceneSnapshot> {
    this.requireReady();
    const snapshot = this.sceneEngine.snapshot();
    return {
      objects: snapshot.objects.map((object) => ({
        id: object.id.toString(),
        name: object.name,
        translation: object.translation,
        euler_xyz_deg: object.euler_xyz_deg,
        scale: object.scale,
        triangle_count: object.triangle_count,
        world_aabb: object.world_aabb,
        source_id: object.source_id,
        source_part: object.source_part,
      })),
    };
  }

  async getHistory(): Promise<RuntimeHistorySession[]> {
    this.requireReady();
    return [];
  }

  async clearHistory(): Promise<void> {
    // The web/wasm runtime keeps no persisted history — nothing to clear.
  }

  async slice(request: RuntimeSliceRequest): Promise<RuntimeSliceResult> {
    this.requireReady();
    await this.ensureWorkerReady();

    if (this.pendingSlice) {
      throw new Error('A WASM slice is already in progress.');
    }

    const { objects, transferables } = this.buildWorkerSlicePayload(request);
    const message: SlicerWorkerRequest = {
      type: 'slice',
      sliceId: request.sliceId,
      settings: request.settings,
      objects,
    };

    return new Promise<RuntimeSliceResult>((resolve, reject) => {
      this.pendingSlice = { sliceId: request.sliceId, resolve, reject };
      try {
        const worker = this.slicerWorker;
        if (!worker) {
          throw new Error('WASM slicer worker is not available.');
        }
        worker.postMessage(message, transferables);
      } catch (error) {
        this.rejectPendingSlice(errorOf(error));
      }
    });
  }

  async cancel(sliceId: string): Promise<void> {
    this.requireReady();
    if (this.pendingSlice?.sliceId === sliceId) {
      this.rejectPendingSlice(new Error('Slice canceled.'));
    }
    this.restartWorker();
  }

  async getPreviewSource(sliceId: string): Promise<RuntimePreviewSource> {
    this.requireReady();
    const gcode = this.previewBySlice.get(sliceId);
    if (!gcode) {
      return { kind: 'none' };
    }

    return {
      kind: 'gcode-inline',
      gcode,
    };
  }

  onEvent(listener: RuntimeEventListener): RuntimeSubscription {
    return this.bus.subscribe(listener);
  }

  async dispose(): Promise<void> {
    this.rejectPendingSlice(new Error('WASM runtime disposed.'));
    this.rejectPendingInit(new Error('WASM runtime disposed.'));
    this.slicerWorker?.terminate();
    this.slicerWorker = null;
    this.workerReady = null;
    this.meshByObjectId.clear();
    this.initialized = false;
    this.bus.clear();
  }

  private ensureWorkerReady(): Promise<void> {
    if (!this.slicerWorker) {
      this.createWorker();
    }

    if (!this.workerReady) {
      const worker = this.slicerWorker;
      if (!worker) {
        return Promise.reject(new Error('WASM slicer worker is not available.'));
      }

      const ready = new Promise<void>((resolve, reject) => {
        this.pendingInit = { resolve, reject };
      });
      try {
        const message: SlicerWorkerRequest = {
          type: 'init',
          wasmUrl: new URL('scene_engine_bg.wasm', document.baseURI).toString(),
        };
        worker.postMessage(message);
        this.workerReady = ready;
      } catch (error) {
        const initError = errorOf(error);
        this.rejectPendingInit(initError);
        return Promise.reject(initError);
      }
    }

    return this.workerReady;
  }

  private createWorker(): void {
    const worker = new Worker(new URL('./slicer.worker', import.meta.url), {
      type: 'module',
      name: 'slicer-worker',
    });
    worker.onmessage = (event: MessageEvent<SlicerWorkerResponse>) =>
      this.handleWorkerMessage(event.data);
    worker.onerror = (event) => {
      this.handleWorkerFailure(new Error(event.message || 'WASM slicer worker failed.'));
    };
    worker.onmessageerror = () => {
      this.handleWorkerFailure(new Error('WASM slicer worker sent an unreadable message.'));
    };
    this.slicerWorker = worker;
  }

  private restartWorker(): void {
    this.rejectPendingInit(new Error('WASM slicer worker restarted.'));
    this.slicerWorker?.terminate();
    this.slicerWorker = null;
    this.workerReady = null;
  }

  private handleWorkerMessage(message: SlicerWorkerResponse): void {
    switch (message.type) {
      case 'ready':
        this.pendingInit?.resolve();
        this.pendingInit = null;
        break;
      case 'log':
        this.bus.emit({
          type: 'log',
          level: message.level,
          message: message.message,
        });
        break;
      case 'phase-start':
        this.bus.emit({
          type: 'phase-start',
          sliceId: message.sliceId,
          phase: message.phase,
          object: message.object,
          objectCount: message.objectCount,
        });
        break;
      case 'phase-end':
        this.bus.emit({
          type: 'phase-end',
          sliceId: message.sliceId,
          phase: message.phase,
          elapsedMs: message.elapsedMs,
          object: message.object,
          objectCount: message.objectCount,
        });
        break;
      case 'progress':
        this.bus.emit({
          type: 'progress',
          sliceId: message.sliceId,
          currentLayer: message.currentLayer,
          totalLayers: message.totalLayers,
        });
        break;
      case 'slice-complete':
        this.previewBySlice.set(message.sliceId, message.gcode);
        this.bus.emit({
          type: 'slice-complete',
          sliceId: message.sliceId,
          layerCount: message.layerCount,
        });
        this.resolvePendingSlice({
          sliceId: message.sliceId,
          layerCount: message.layerCount,
          gcodeText: message.gcode,
        });
        break;
      case 'error':
        this.bus.emit({
          type: 'error',
          error: {
            code: 'internal_error',
            message: message.message,
          },
        });
        if (message.sliceId) {
          this.rejectPendingSlice(new Error(message.message));
        } else {
          this.rejectPendingInit(new Error(message.message));
          this.workerReady = null;
        }
        break;
    }
  }

  private handleWorkerFailure(error: Error): void {
    this.bus.emit({
      type: 'error',
      error: {
        code: 'internal_error',
        message: error.message,
        cause: error,
      },
    });
    this.rejectPendingSlice(error);
    this.rejectPendingInit(error);
    this.slicerWorker?.terminate();
    this.slicerWorker = null;
    this.workerReady = null;
  }

  private buildWorkerSlicePayload(request: RuntimeSliceRequest): {
    objects: WorkerSliceObject[];
    transferables: Transferable[];
  } {
    const sceneObjects = request.scene?.objects ?? this.sceneSnapshotObjects();
    if (sceneObjects.length === 0) {
      throw new Error('Cannot slice an empty scene.');
    }

    const transferables: Transferable[] = [];
    // One copy per *file*, not per object. A 300 MB 3MF with several parts, or
    // a model duplicated across the plate, would otherwise allocate the whole
    // file once per object and can exhaust memory before the message is even
    // posted. Structured clone preserves shared references within a single
    // message, so every object that shares a file also shares its buffer on the
    // worker side — and each buffer is transferred exactly once.
    const copies = new Map<Uint8Array, Uint8Array>();
    const objects = sceneObjects.map((object) => {
      const source = this.resolveSource(object, sceneObjects.length, request);

      // Copy rather than transfer the original: transferring detaches the
      // buffer on this side, and the registry has to keep its bytes for the
      // next slice.
      let bytes = copies.get(source.bytes);
      if (!bytes) {
        bytes = new Uint8Array(source.bytes);
        copies.set(source.bytes, bytes);
        transferables.push(bytes.buffer as ArrayBuffer);
      }

      return {
        name: source.name,
        format: source.format,
        bytes,
        partIndex: source.partIndex,
        transform: {
          translation: object.translation,
          euler_xyz_deg: object.euler_xyz_deg,
          scale: object.scale,
        },
      };
    });

    return { objects, transferables };
  }

  /**
   * Find the bytes one scene object slices from.
   *
   * Resolution is by the object's own `source_id`, so a plate holding several
   * different models slices each from its own file. The per-object-id cache is
   * consulted first only because it also remembers which *part* of a
   * multi-part file the object is; the registry is the authority on the bytes.
   */
  private resolveSource(
    object: { id: string; name: string; source_id?: string | null; source_part?: number },
    sceneObjectCount: number,
    request: RuntimeSliceRequest,
  ): { name: string; format: 'stl' | 'obj' | '3mf'; bytes: Uint8Array; partIndex: number } {
    const cached = this.meshByObjectId.get(object.id);
    if (cached) {
      return {
        name: cached.name,
        format: cached.format,
        bytes: cached.bytes,
        partIndex: cached.partIndex,
      };
    }

    const registered = this.modelSources.get(object.source_id);
    if (registered?.bytes) {
      return {
        name: registered.fileName,
        format: registered.format,
        bytes: registered.bytes,
        partIndex: object.source_part ?? 0,
      };
    }

    // Last resort, and only ever right for a single-object plate: the model
    // the slice request was built around. An object that reaches here on a
    // multi-object plate cannot be told apart from its neighbours, so guessing
    // would slice the wrong mesh silently.
    if (sceneObjectCount === 1 && request.model?.bytes) {
      return {
        name: request.model.fileName,
        format: request.model.format,
        bytes: request.model.bytes,
        partIndex: object.source_part ?? 0,
      };
    }

    throw new Error(
      `Missing mesh bytes for scene object '${object.name}'. Reload the model before slicing locally.`,
    );
  }

  private sceneSnapshotObjects(): RuntimeSceneSnapshot['objects'] {
    return this.sceneEngine.snapshot().objects.map((object) => ({
      id: object.id.toString(),
      name: object.name,
      translation: object.translation,
      euler_xyz_deg: object.euler_xyz_deg,
      scale: object.scale,
      triangle_count: object.triangle_count,
      world_aabb: object.world_aabb,
      source_id: object.source_id,
      source_part: object.source_part,
    }));
  }

  private resolvePendingSlice(result: RuntimeSliceResult): void {
    if (this.pendingSlice?.sliceId === result.sliceId) {
      this.pendingSlice.resolve(result);
      this.pendingSlice = null;
    }
  }

  private rejectPendingSlice(error: Error): void {
    this.pendingSlice?.reject(error);
    this.pendingSlice = null;
  }

  private rejectPendingInit(error: Error): void {
    this.pendingInit?.reject(error);
    this.pendingInit = null;
  }

  private requireReady(): void {
    if (!this.initialized) {
      const error: RuntimeError = {
        code: 'not_ready',
        message: 'Wasm runtime has not been initialized.',
      };
      this.bus.emit({ type: 'error', error });
      throw new Error(error.message);
    }
  }
}

function errorOf(error: unknown): Error {
  if (error instanceof Error) {
    return error;
  }
  return new Error(typeof error === 'string' ? error : 'Unknown WASM worker error');
}
