import { Injectable, computed, inject } from '@angular/core';
import { resolveRuntimeMode } from '../../runtime/domain/runtime-mode.util';
import { Arrange } from '../arrange';
import { Logger } from '../logger';
import {
  ModelSourceRegistry,
  isSupportedModelFile,
  modelFormatOf,
  type ModelFormat,
} from '../model-source';
import { SceneCommand } from '../scene-command/scene-command';
import { SceneEngine } from '../scene-engine';
import { SlicerFile } from '../slicer-file';
import { WasmPerformanceNotice } from '../wasm-performance-notice';
import { clearOffsetX, footprintOf } from './placement';

export type { ModelFormat };

/** Outcome of adding one file to the plate. */
export interface AddObjectResult {
  file: File;
  /** Ids of every object the file produced — a 3MF can yield several. */
  objectIds?: bigint[];
  error?: string;
}

/**
 * Owns the set of objects on the current workplate.
 *
 * A workplate is a build plate, not a single file: it starts from one model
 * but must accept more. This service is the one place that turns "the user
 * wants this file on the plate" into an object in the scene engine, so every
 * entry point — the add-object button, drag-and-drop, restoring a saved plate
 * — produces identical state.
 *
 * Two rules it exists to enforce:
 *
 * 1. **Objects accumulate.** Adding a model never clears the ones already
 *    placed. Only an explicit clear does.
 * 2. **Every object remembers its source file.** Each file is recorded in the
 *    {@link ModelSourceRegistry} and its handle stamped on every object the
 *    file produces — in *every* runtime mode, not just cloud. That handle is
 *    how a slice resolves each object to its own bytes; without it a plate
 *    holding two different models slices the first one twice.
 *
 * The 3D viewer mirrors {@link objects}; it is not involved in loading.
 */
@Injectable({ providedIn: 'root' })
export class WorkplateObjects {
  private readonly log = inject(Logger).scope('WorkplateObjects');
  private readonly sceneEngine = inject(SceneEngine);
  private readonly sceneCommand = inject(SceneCommand);
  private readonly slicerFile = inject(SlicerFile);
  private readonly arrange = inject(Arrange);
  private readonly wasmPerfNotice = inject(WasmPerformanceNotice);
  private readonly modelSources = inject(ModelSourceRegistry);

  /** Live list of objects on the plate. */
  readonly objects = this.sceneEngine.objects;

  /** `true` when the plate has nothing on it. */
  readonly isEmpty = computed(() => this.objects().length === 0);

  /** Objects that cannot print where they currently sit. */
  readonly misplaced = this.sceneEngine.misplacedObjects;

  /**
   * Files dropped alongside the one that opened the plate.
   *
   * Opening a workplate navigates away before the scene exists, so extra
   * models are parked here and drained by the slice viewer once the plate is
   * up. Without this, dropping three files would silently keep only the first.
   */
  private readonly pending: File[] = [];

  /** Park extra files to be added once a plate is open. */
  queuePending(files: readonly File[]): void {
    this.pending.push(...files.filter((f) => WorkplateObjects.isSupported(f)));
  }

  /** Add and clear any parked files. Safe to call when there are none. */
  async flushPending(): Promise<AddObjectResult[]> {
    if (this.pending.length === 0) {
      return [];
    }
    const files = this.pending.splice(0, this.pending.length);
    return this.addFiles(files);
  }

  /**
   * Drop parked files without adding them.
   *
   * Called when a plate is reset so a queue left behind by an interrupted
   * navigation cannot surface on the *next* plate.
   */
  clearPending(): void {
    this.pending.length = 0;
  }

  /** How many files are waiting to be added. */
  pendingCount(): number {
    return this.pending.length;
  }

  /** Is this a file the scene engine can load? */
  static isSupported(file: File): boolean {
    return isSupportedModelFile(file.name);
  }

  /**
   * Add files to the plate, keeping everything already on it.
   *
   * Each file is uploaded (cloud only), parsed by the scene engine, oriented
   * flat and dropped to the bed, then nudged clear of the objects already
   * there. Failures are reported per file so one bad model does not abort the
   * rest of a multi-file drop.
   */
  async addFiles(files: readonly File[]): Promise<AddObjectResult[]> {
    await this.sceneEngine.ready();
    const results: AddObjectResult[] = [];

    for (const file of files) {
      if (!WorkplateObjects.isSupported(file)) {
        results.push({ file, error: `Unsupported file type: ${file.name}` });
        continue;
      }
      try {
        results.push({ file, objectIds: await this.addFile(file) });
      } catch (error) {
        const message = error instanceof Error ? error.message : 'Failed to add model';
        this.log.error(`addFiles '${file.name}' failed`, message);
        results.push({ file, error: message });
      }
    }

    return results;
  }

  /**
   * Record a file the plate already holds, without placing anything.
   *
   * The primary model of a restored plate is added by the viewer (through its
   * `model` input) rather than by this service, so its bytes would otherwise
   * never reach the registry and the invariant "every object's `source_id`
   * resolves to a file" would hold for every object but that one.
   */
  async registerExistingFile(file: File, sourceId: string): Promise<void> {
    this.modelSources.register({
      sourceId,
      fileName: file.name,
      format: modelFormatOf(file.name),
      bytes:
        resolveRuntimeMode() === 'cloud' ? undefined : new Uint8Array(await file.arrayBuffer()),
    });
  }

  /**
   * Add a model whose bytes are already on the server.
   *
   * Used when reopening a saved workplate: the upload id is known up front,
   * so it is stamped on the object without re-uploading the bytes.
   */
  async addUploadedFile(file: File, sourceId: string): Promise<bigint[]> {
    await this.sceneEngine.ready();
    const bytes = new Uint8Array(await file.arrayBuffer());
    this.modelSources.register({
      sourceId,
      fileName: file.name,
      format: modelFormatOf(file.name),
      // Restoring a plate is a cloud-only flow, and the server already holds
      // the model — see `addFile` for why a second copy is not kept.
      bytes: resolveRuntimeMode() === 'cloud' ? undefined : bytes,
    });
    return this.placeMesh(file.name, bytes, sourceId);
  }

  private async addFile(file: File): Promise<bigint[]> {
    const bytes = new Uint8Array(await file.arrayBuffer());

    // Upload first: the object must know its file id from the moment it
    // exists, or a slice fired before the upload settles cannot resolve it.
    // Outside cloud there is nothing to upload, but the object still needs a
    // handle — the registry mints a local one so every runtime can resolve
    // this object back to *these* bytes rather than to whichever file
    // happened to be first on the plate.
    const isCloud = resolveRuntimeMode() === 'cloud';
    let sourceId: string | undefined;
    if (isCloud) {
      const upload = await this.slicerFile.upload(file);
      sourceId = upload.ofids[0];
    }
    const source = this.modelSources.register({
      sourceId,
      fileName: file.name,
      format: modelFormatOf(file.name),
      // Only the local runtimes slice from bytes. The server already holds the
      // model, so keeping a second copy in the tab buys nothing and costs the
      // size of every model the user opens.
      bytes: isCloud ? undefined : bytes,
      filePath: nativePathOf(file),
    });

    return this.placeMesh(file.name, bytes, source.sourceId);
  }

  /**
   * Parse the bytes and place every object they contain.
   *
   * A 3MF can hold several parts, so this returns one id per part — each
   * placed with the same {@link Arrange} settings a "place all" would use, so
   * dropping a file in and pressing the place button agree on orientation and
   * on the gap between parts.
   */
  private placeMesh(name: string, bytes: Uint8Array, sourceId?: string): bigint[] {
    const ids = this.sceneEngine.addMesh(name, modelFormatOf(name), bytes, sourceId);
    // A model on the plate is the first moment the web build's headline
    // trade-off — everything runs here, so it is slower — is about to matter.
    this.wasmPerfNotice.maybeShow();
    const { autoOrient, preferredOrientationDeg } = this.arrange.settings();
    for (const id of ids) {
      if (autoOrient) {
        this.sceneEngine.apply({
          op: 'AutoOrient',
          args: { id, options: { preferred_z_rotation_deg: preferredOrientationDeg } },
        });
      }
      this.sceneEngine.apply({ op: 'DropToFloor', args: { id } });
      this.placeClear(id);
    }
    return ids;
  }

  /**
   * Clone an object, including its source file, and offset the copy so it
   * lands beside the original rather than inside it.
   */
  duplicate(id: bigint): void {
    const source = this.objects().find((o) => o.id === id);
    if (!source) {
      return;
    }
    const [width] = footprintOf(source.world_aabb);
    this.sceneCommand.apply({
      op: 'Duplicate',
      args: { id, offset: [width + this.arrange.spacingMm(), 0, 0] },
    });
  }

  /** Remove an object from the plate. */
  remove(id: bigint): void {
    const target = this.objects().find((o) => o.id === id);
    this.sceneCommand.apply({ op: 'Remove', args: { id } });
    // Drop the upload too, but only once no other object still points at it —
    // duplicates and a 3MF's sibling parts share one file and the survivors
    // still need it to slice.
    const sourceId = target?.source_id;
    if (sourceId && !this.objects().some((o) => o.source_id === sourceId)) {
      this.slicerFile.removeFile(sourceId);
      this.modelSources.forget(sourceId);
    }
  }

  /** Remove every object from the plate. */
  clear(): void {
    for (const object of [...this.objects()]) {
      this.sceneEngine.apply({ op: 'Remove', args: { id: object.id } });
    }
    this.modelSources.clear();
  }

  /**
   * Shift a freshly-added object clear of the ones already on the plate.
   *
   * Walks right along X, each time jumping past the far edge of whichever
   * object is still in the way. Stepping by the *new* object's own width would
   * not be enough — a neighbour wider than the new object would still overlap
   * after a step, and the walk would give up while sitting inside it.
   *
   * A plain "place everything" would be tidier, but it also moves models the
   * user already positioned; adding one object should not rearrange the plate.
   * If no free spot is found the object is left where it is and the placement
   * warning flags the overlap.
   */
  private placeClear(id: bigint): void {
    const target = this.objects().find((o) => o.id === id);
    if (!target) {
      return;
    }
    const others = this.objects()
      .filter((o) => o.id !== id)
      .map((o) => o.world_aabb);
    if (others.length === 0) {
      return;
    }

    const dx = clearOffsetX(target.world_aabb, others, this.arrange.spacingMm());
    if (dx !== null && dx > 0) {
      this.sceneEngine.apply({ op: 'Translate', args: { id, delta: [dx, 0, 0] } });
    }
  }
}

/**
 * The native path of a dropped file, when the host exposes one.
 *
 * Some webview hosts put the originating filesystem path on the `File`, which
 * lets the desktop slicer read the model directly instead of writing the bytes
 * back out to a cache file. Absent in a plain browser, and not guaranteed
 * anywhere — the desktop runtime caches the bytes itself when it is missing, so
 * this is an optimisation rather than something to depend on.
 */
function nativePathOf(file: File): string | undefined {
  const path = (file as File & { path?: unknown }).path;
  return typeof path === 'string' && path.length > 0 ? path : undefined;
}
