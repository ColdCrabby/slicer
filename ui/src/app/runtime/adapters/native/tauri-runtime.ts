import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { appCacheDir, join } from '@tauri-apps/api/path';
import { open } from '@tauri-apps/plugin-dialog';
import { mkdir, readFile, writeFile } from '@tauri-apps/plugin-fs';

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

const NATIVE_CAPABILITIES: RuntimeCapabilities = {
  supportsLocalSlicing: true,
  supportsRemoteJobs: false,
  supportsStreamingProgress: false,
  supportsSceneSnapshotPull: true,
};

const MODEL_EXTENSIONS: string[] = ['stl', 'obj', '3mf'];

export class TauriRuntime implements RuntimePort {
  private readonly bus = new RuntimeEventBus();
  private readonly previewBySlice = new Map<string, RuntimePreviewSource>();
  private initialized = false;

  constructor(private readonly sceneEngine: SceneEngine) {}

  async init(): Promise<void> {
    // Ensure WASM scene engine is ready before any scene or slice operations.
    await this.sceneEngine.ready();
    await invoke('runtime_init');
    this.initialized = true;
    this.bus.emit({ type: 'connected', mode: 'native' });
  }

  getCapabilities(): RuntimeCapabilities {
    return NATIVE_CAPABILITIES;
  }

  /** Open a native OS file-picker dialog and return a populated mesh input.
   *  Only the file path is returned — bytes are NOT read eagerly. The WASM
   *  scene engine will read them on demand via `addMesh` (see below). */
  async openFilePicker(): Promise<RuntimeMeshInput | null> {
    const path = await open({
      multiple: false,
      filters: [{ name: '3D Model', extensions: MODEL_EXTENSIONS }],
    });

    if (!path || Array.isArray(path)) {
      return null;
    }

    const fileName = path.split(/[\\/]/).pop() ?? path;
    const ext = fileName.split('.').pop()?.toLowerCase() ?? '';
    const format = (MODEL_EXTENSIONS.includes(ext) ? ext : 'stl') as 'stl' | 'obj' | '3mf';

    // bytes intentionally absent — addMesh reads from filePath when needed.
    return { fileName, format, filePath: path };
  }

  async addMesh(input: RuntimeMeshInput): Promise<string> {
    this.requireReady();
    // Bytes may be absent when the file was opened via the native file picker.
    // Read them here — the single point where the WASM scene engine needs them
    // for 3D viewport rendering. The slicing path uses filePath and never
    // touches these bytes.
    const bytes = input.bytes ?? (input.filePath ? await readFile(input.filePath) : undefined);
    if (!bytes) {
      throw new Error(`Cannot add mesh '${input.fileName}': no bytes and no file path`);
    }
    const objectId = this.sceneEngine.addMesh(input.fileName, input.format, bytes);
    return objectId.toString();
  }

  async applySceneOps(ops: RuntimeSceneOp[]): Promise<void> {
    this.requireReady();
    const mappedOps: SceneOp[] = ops.map((op) => {
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
      })),
    };
  }

  async getHistory(): Promise<RuntimeHistorySession[]> {
    this.requireReady();
    const response = await invoke<{ sessions?: RuntimeHistorySession[] }>('history_list').catch(
      () => ({ sessions: [] }),
    );

    return response.sessions ?? [];
  }

  async clearHistory(): Promise<void> {
    this.requireReady();
    await invoke('history_clear');
  }

  async slice(request: RuntimeSliceRequest): Promise<RuntimeSliceResult> {
    this.requireReady();
    this.bus.emit({ type: 'phase-start', sliceId: request.sliceId, phase: 'total' });

    // Subscribe to pipeline events emitted by TauriAppLogger.
    // Unlisteners are called once the command settles.
    const unlisteners: Array<() => void> = [];
    unlisteners.push(
      await listen<{ level: string; message: string }>('slice-log', ({ payload }) => {
        this.bus.emit({
          type: 'log',
          level: payload.level as 'info' | 'debug' | 'warn',
          message: payload.message,
        });
      }),
      await listen<{ phase: string; event: string; elapsed_ms?: number }>(
        'slice-phase',
        ({ payload }) => {
          if (payload.event === 'start') {
            this.bus.emit({
              type: 'phase-start',
              sliceId: request.sliceId,
              phase: payload.phase,
            });
          } else {
            this.bus.emit({
              type: 'phase-end',
              sliceId: request.sliceId,
              phase: payload.phase,
              elapsedMs: payload.elapsed_ms ?? 0,
            });
          }
        },
      ),
    );

    try {
      // Resolve a native filesystem path for the model before invoking.
      // When the user opened a file via the native dialog, request.model.filePath
      // is already set. For drag-dropped files it is absent, so we cache the
      // bytes to the app cache dir here — async, off the main thread via the
      // fs plugin — and pass the resulting path. This avoids the catastrophic
      // Array.from(Uint8Array) → JSON.stringify(number[]) path that would
      // serialise ~300 MB of text synchronously on the main thread.
      const filePath = request.model
        ? (request.model.filePath ?? (await this.cacheModelFile(request.model)))
        : undefined;

      const response = await invoke<{
        layer_count?: number;
        gcode_path?: string;
        download_url?: string;
      }>('slice_start', {
        payload: {
          slice_id: request.sliceId,
          request_uuid: request.request_uuid,
          // Rust reads the model directly from disk — bytes never cross IPC.
          file_path: filePath,
          scene: request.scene,
          settings: request.settings,
        },
      });

      const layerCount = response.layer_count ?? 0;

      if (response.gcode_path) {
        // Convert the native path to an asset:// URL that the webview can
        // fetch directly, bypassing the IPC channel for the GCode bytes.
        const url = convertFileSrc(response.gcode_path);
        this.previewBySlice.set(request.sliceId, { kind: 'download-url', url });
      } else if (response.download_url) {
        this.previewBySlice.set(request.sliceId, {
          kind: 'download-url',
          url: response.download_url,
        });
      }

      this.bus.emit({
        type: 'phase-end',
        sliceId: request.sliceId,
        phase: 'total',
        elapsedMs: 0,
      });
      this.bus.emit({ type: 'slice-complete', sliceId: request.sliceId, layerCount });

      return {
        sliceId: request.sliceId,
        layerCount,
        downloadUrl: response.gcode_path
          ? convertFileSrc(response.gcode_path)
          : response.download_url,
      };
    } finally {
      for (const unlisten of unlisteners) {
        unlisten();
      }
    }
  }

  async cancel(sliceId: string): Promise<void> {
    this.requireReady();
    void sliceId;
    await invoke('slice_cancel');
  }

  async getPreviewSource(sliceId: string): Promise<RuntimePreviewSource> {
    this.requireReady();
    const cached = this.previewBySlice.get(sliceId);
    if (cached) {
      return cached;
    }

    const response = await invoke<{
      kind?: string;
      path?: string;
      url?: string;
      gcode?: string;
    }>('preview_get_source', { payload: { sliceId } });

    if (response.kind === 'gcode-path' && response.path) {
      // Convert native path to asset:// URL; served by the OS URI handler
      // with no data crossing the IPC channel.
      return { kind: 'download-url', url: convertFileSrc(response.path) };
    }

    if (response.kind === 'download-url' && response.url) {
      return { kind: 'download-url', url: response.url };
    }

    if (response.kind === 'gcode-inline' && response.gcode) {
      return { kind: 'gcode-inline', gcode: response.gcode };
    }

    return { kind: 'none' };
  }

  onEvent(listener: RuntimeEventListener): RuntimeSubscription {
    return this.bus.subscribe(listener);
  }

  async dispose(): Promise<void> {
    this.initialized = false;
    this.bus.clear();
  }

  /** Write model bytes to the app cache dir and return the absolute path.
   *
   * Called only for drag-and-dropped files (no native FS path, bytes present).
   * The write goes through the fs plugin's binary IPC — efficient and async —
   * so the main thread is never blocked by large byte serialisation. */
  private async cacheModelFile(model: RuntimeMeshInput): Promise<string> {
    if (!model.bytes) {
      throw new Error(`Cannot cache '${model.fileName}': no bytes available`);
    }
    const dir = await appCacheDir();
    // The app cache directory is not guaranteed to exist yet — the fs plugin's
    // writeFile does not create parent directories, so a missing cache dir
    // surfaces later as a confusing "No such file or directory" when Rust
    // tries to read the model path. Create it up front (idempotent).
    await mkdir(dir, { recursive: true });
    const path = await join(dir, model.fileName);
    await writeFile(path, model.bytes);
    return path;
  }

  private requireReady(): void {
    if (!this.initialized) {
      const error: RuntimeError = {
        code: 'not_ready',
        message: 'Tauri runtime has not been initialized.',
      };
      this.bus.emit({ type: 'error', error });
      throw new Error(error.message);
    }
  }
}
