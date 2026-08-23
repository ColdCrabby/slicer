import { Injectable, computed, inject, signal } from '@angular/core';
import init, {
  SceneHandle,
  appInfo as wasmAppInfo,
  changelogEntries as wasmChangelogEntries,
  changelogMarkdown as wasmChangelogMarkdown,
  type RenderBuffer,
} from '../../generated/scene-wasm/scene_engine';
import type { SlicingParams } from '../../generated/slicer-engine-ws-client-message-v1';
import { Logger } from './logger';

/**
 * Build-time version snapshot exposed by the WASM bundle. Mirrors the Rust
 * `crate::version::AppInfo` struct.
 */
export interface AppInfo {
  version: string;
  git_describe: string;
  git_sha: string;
  build_date: string;
  cargo_version: string;
  is_release: boolean;
}

/**
 * One parsed changelog section. Mirrors the Rust
 * `crate::version::ChangelogEntry` struct.
 */
export interface ChangelogEntry {
  version: string;
  date: string | null;
  body: string;
}

/**
 * JS-side mirror of the Rust `SceneObjectJs` snapshot.
 *
 * IDs are exposed as `bigint` because that is the native return type of
 * `SceneHandle.addMesh`. They survive a JSON round-trip via `String(id)` if
 * needed at the protocol boundary.
 */
export interface SceneObjectSnapshot {
  id: bigint;
  name: string;
  translation: [number, number, number];
  euler_xyz_deg: [number, number, number];
  scale: [number, number, number];
  triangle_count: number;
  world_aabb: [[number, number, number], [number, number, number]];
}

export interface SceneBedSnapshot {
  width: number;
  depth: number;
  height: number;
  origin_offset_x: number;
  origin_offset_y: number;
}

export interface SceneSnapshot {
  objects: SceneObjectSnapshot[];
  bed: SceneBedSnapshot;
}

export interface LocalSliceResult {
  gcode: string;
  layer_count: number;
}

type SceneHandleWithWebSlicer = SceneHandle & {
  sliceGcode(params: SlicingParams): LocalSliceResult;
};

/**
 * Wire-format scene op accepted by the WASM `applyOp`.
 *
 * Mirrors `src/scene/wasm.rs::SceneOpJs` — the discriminant lives in `op` and
 * the variant payload in `args`. The `Add` variant is omitted because the
 * service exposes a dedicated {@link SceneEngine.addMesh} method that
 * keeps raw bytes off the JSON path.
 */
export type SceneOp =
  | { op: 'Remove'; args: { id: bigint } }
  | { op: 'Translate'; args: { id: bigint; delta: [number, number, number] } }
  | {
      op: 'SetTransform';
      args: {
        id: bigint;
        translation: [number, number, number];
        euler_xyz_deg: [number, number, number];
        scale: [number, number, number];
      };
    }
  | { op: 'Rotate'; args: { id: bigint; axis: [number, number, number]; degrees: number } }
  | { op: 'Scale'; args: { id: bigint; factors: [number, number, number] } }
  | { op: 'CenterOnBed'; args: { id: bigint } }
  | { op: 'DropToFloor'; args: { id: bigint } }
  | { op: 'PlaceFaceOnFloor'; args: { id: bigint; face_index: number } }
  | {
      op: 'AutoOrient';
      args: {
        id: bigint;
        options?: {
          allow_rotations?: boolean;
          preferred_z_rotation_deg?: number;
          overhang_threshold_deg?: number;
        };
      };
    }
  | {
      op: 'ArrangeOnBed';
      args: {
        ids: bigint[];
        options?: {
          /** Gap between objects in mm (default 2). */
          spacing_mm?: number;
          /** Auto-orient each object before packing (default true). */
          auto_orient?: boolean;
          orient_options?: {
            allow_rotations?: boolean;
            preferred_z_rotation_deg?: number;
            overhang_threshold_deg?: number;
          };
        };
      };
    };

const DEFAULT_BED: SceneBedSnapshot = {
  width: 220,
  depth: 220,
  height: 250,
  origin_offset_x: 0,
  origin_offset_y: 0,
};

/**
 * Single source of truth for object placement, orientation and transforms.
 *
 * Wraps the Rust `SceneState` (shipped as WASM) so the same op semantics —
 * `Translate`, `Rotate`, `CenterOnBed`, `DropToFloor`, `AlignFaceToFloor`,
 * etc. — are used by the CLI, the WS server and the Angular UI. Components
 * dispatch ops via {@link apply}; reactive consumers read the resulting
 * {@link snapshot} signal.
 *
 * The service is lazily initialised: callers must `await ready()` before
 * issuing ops.
 */
@Injectable({ providedIn: 'root' })
export class SceneEngine {
  private readonly log = inject(Logger).scope('SceneEngine');
  private handle: SceneHandle | null = null;
  private initPromise: Promise<void> | null = null;

  private readonly snapshotSignal = signal<SceneSnapshot>({ objects: [], bed: DEFAULT_BED });
  /**
   * Rolling-window stats for the most recent op label dispatched through
   * {@link apply}. Updated after every op so on-screen overlays can show
   * a live `last / avg over N` performance readout.
   */
  private readonly opStatsSignal = signal<{
    label: string;
    lastMs: number;
    avgMs: number;
    count: number;
  } | null>(null);

  /** Reactive snapshot of the entire scene. */
  readonly snapshot = computed(() => this.snapshotSignal());

  /** Reactive list of objects (convenience derivation). */
  readonly objects = computed(() => this.snapshotSignal().objects);

  /** Reactive bed configuration. */
  readonly bed = computed(() => this.snapshotSignal().bed);

  /** Last-op rolling stats (last/avg over up to 100 samples). */
  readonly opStats = computed(() => this.opStatsSignal());

  /**
   * Load the WASM module and instantiate a fresh scene with the given bed.
   * Idempotent: subsequent calls reuse the existing engine without touching
   * scene state. Throws on initialization failure.
   */
  ready(bed: SceneBedSnapshot = DEFAULT_BED): Promise<void> {
    if (!this.initPromise) {
      this.initPromise = this.bootstrap(bed).catch((err) => {
        this.log.error('WASM initialization failed', err);
        // Clear the cached promise so a retry is possible
        this.initPromise = null;
        throw err;
      });
    }
    return this.initPromise;
  }

  private async bootstrap(bed: SceneBedSnapshot): Promise<void> {
    this.log.info('bootstrap start', { bed });
    const stop = this.log.time('bootstrap');
    try {
      // Load the wasm binary from the deployed asset path (configured in
      // angular.json) instead of relying on `import.meta.url`, which after
      // bundling resolves to the chunk URL rather than the directory the
      // generated JS originally lived in.
      await init({ module_or_path: 'scene_engine_bg.wasm' });
      this.handle = new SceneHandle(bed as unknown as object);
      this.refreshSnapshot();
      stop();
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : 'Unknown WASM initialization error';
      this.log.error('WASM init error:', errorMsg);
      throw new Error(`Scene engine initialization failed: ${errorMsg}`);
    }
  }

  /**
   * Replace the entire scene with a new one bound to the given bed.
   * Use this when the active machine config changes.
   */
  async resetWithBed(bed: SceneBedSnapshot): Promise<void> {
    await this.ready();
    this.log.info('resetWithBed', { bed });
    this.disposeHandle();
    this.handle = new SceneHandle(bed as unknown as object);
    this.refreshSnapshot();
  }

  /** Remove every object from the scene, keeping the current bed. */
  async clear(): Promise<void> {
    await this.resetWithBed(this.snapshotSignal().bed);
  }

  /**
   * Build-time version metadata baked into the WASM bundle — the true running
   * version (a release semver or `"development"`), plus git/build details.
   */
  async appInfo(): Promise<AppInfo> {
    await this.ready();
    return wasmAppInfo() as AppInfo;
  }

  /** The embedded changelog as raw Keep a Changelog markdown. */
  async changelogMarkdown(): Promise<string> {
    await this.ready();
    return wasmChangelogMarkdown();
  }

  /** The embedded changelog parsed into ordered `{ version, date, body }` entries. */
  async changelogEntries(): Promise<ChangelogEntry[]> {
    await this.ready();
    return wasmChangelogEntries() as ChangelogEntry[];
  }

  /**
   * Add a mesh to the scene from raw bytes. Returns the assigned object id.
   */
  addMesh(name: string, format: 'stl' | 'obj' | '3mf', bytes: Uint8Array): bigint {
    const handle = this.requireHandle();
    const stop = this.log.time(`addMesh '${name}' (${format}, ${bytes.byteLength} B)`);
    const id = handle.addMesh(name, format, bytes);
    stop({ id: String(id) });
    this.refreshSnapshot();
    return id;
  }

  /** Apply a single scene op and refresh the snapshot signal. */
  apply(op: SceneOp): void {
    const handle = this.requireHandle();
    const label = `apply ${op.op}`;
    const stop = this.log.time(label);
    handle.applyOp(op);
    stop(op.args as unknown as Record<string, unknown>);
    this.publishOpStats(label);
    this.refreshSnapshot();
  }

  /**
   * Convenience method: auto-orient an object by id.
   *
   * Equivalent to `apply({ op: 'auto_orient', args: { id, options } })`.
   */
  autoOrientObject(
    id: bigint,
    options?: {
      allow_rotations?: boolean;
      preferred_z_rotation_deg?: number;
      overhang_threshold_deg?: number;
    },
  ): void {
    this.apply({ op: 'AutoOrient', args: { id, options } });
  }

  /**
   * Convenience method: auto-orient and arrange multiple objects on the bed.
   *
   * Each object is independently oriented to minimise overhangs (when
   * `options.auto_orient` is `true`), then the group is packed onto the bed
   * using a shelf-first-fit algorithm and centered.
   *
   * Equivalent to:
   * ```ts
   * apply({ op: 'arrange_on_bed', args: { ids, options } });
   * ```
   */
  arrangeOnBed(
    ids: bigint[],
    options?: {
      spacing_mm?: number;
      auto_orient?: boolean;
      orient_options?: {
        allow_rotations?: boolean;
        preferred_z_rotation_deg?: number;
        overhang_threshold_deg?: number;
      };
    },
  ): void {
    this.apply({ op: 'ArrangeOnBed', args: { ids, options } });
  }

  /** Apply a batch of ops as a single snapshot update. */
  applyBatch(ops: SceneOp[]): void {
    const handle = this.requireHandle();
    const label = `applyBatch x${ops.length}`;
    const stop = this.log.time(label);
    for (const op of ops) {
      handle.applyOp(op);
    }
    stop();
    this.publishOpStats(label);
    this.refreshSnapshot();
  }

  private publishOpStats(label: string): void {
    const stats = this.log.stats(label);
    if (stats) {
      this.opStatsSignal.set({ label, ...stats });
    }
  }

  /**
   * Render buffers for a given object. The returned arrays are owned by
   * the caller — typically copied into a `BufferGeometry` and disposed.
   */
  getRenderBuffer(id: bigint): {
    positions: Float32Array;
    normals: Float32Array;
    indices: Uint32Array;
  } {
    const handle = this.requireHandle();
    const stop = this.log.time(`getRenderBuffer id=${id}`);
    const buffer: RenderBuffer = handle.getRenderBuffer(id);
    // Copy out before the wasm RenderBuffer is freed.
    const result = {
      positions: new Float32Array(buffer.positions),
      normals: new Float32Array(buffer.normals),
      indices: new Uint32Array(buffer.indices),
    };
    buffer.free();
    stop({
      verts: result.positions.length / 3,
      tris: result.indices.length / 3,
    });
    return result;
  }

  /** 4×4 transform matrix as 16 column-major floats. */
  getMatrix(id: bigint): Float32Array {
    const handle = this.requireHandle();
    // The wasm side returns a copy; safe to hand out directly.
    return handle.getMatrix(id);
  }

  /**
   * Coplanar face groups for a mesh. Returns a `Uint32Array` of length
   * `face_count` where `result[i]` is the group id of face `i`.
   * Faces sharing a group id are coplanar and edge-adjacent.
   *
   * @param angleThresholdDeg  Merge tolerance in degrees (default 1°).
   */
  getFaceGroups(id: bigint, angleThresholdDeg = 1.0): Uint32Array {
    const handle = this.requireHandle();
    const stop = this.log.time(`getFaceGroups id=${id}`);
    const result = handle.getFaceGroups(id, angleThresholdDeg);
    stop({ groups: result.length });
    return result;
  }

  /** Slice the current scene locally through the opt-in `web-slicer` wasm build. */
  sliceToGcode(params: SlicingParams): LocalSliceResult {
    const handle = this.requireHandle() as unknown as Partial<SceneHandleWithWebSlicer>;
    if (typeof handle.sliceGcode !== 'function') {
      throw new Error(
        'This wasm bundle does not include local slicing. Rebuild with the web-slicer feature.',
      );
    }

    const stop = this.log.time('sliceGcode');
    const result = handle.sliceGcode(params);
    stop({ layers: result.layer_count, bytes: result.gcode.length });
    return result;
  }

  private refreshSnapshot(): void {
    const handle = this.requireHandle();
    const raw = handle.snapshot() as SceneSnapshot;
    // `serde-wasm-bindgen` serialises `u64` as a JS Number by default, but
    // every wasm method that takes an id (`getRenderBuffer`, `getMatrix`,
    // op payloads) expects a real `bigint`. Normalise here so consumers
    // never have to think about it.
    const snap: SceneSnapshot = {
      ...raw,
      objects: raw.objects.map((o) => ({ ...o, id: BigInt(o.id as unknown as string | number) })),
    };
    this.snapshotSignal.set(snap);
  }

  private requireHandle(): SceneHandle {
    if (!this.handle) {
      this.log.error('used before ready() resolved');
      throw new Error('SceneEngine used before ready() resolved');
    }
    return this.handle;
  }

  private disposeHandle(): void {
    if (this.handle) {
      this.log.debug('disposeHandle');
      this.handle.free();
      this.handle = null;
    }
  }
}
