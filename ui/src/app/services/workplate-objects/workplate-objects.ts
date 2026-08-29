import { Injectable, computed, inject } from '@angular/core';
import { resolveRuntimeMode } from '../../runtime/domain/runtime-mode.util';
import { Logger } from '../logger';
import { SceneCommand } from '../scene-command/scene-command';
import { SceneEngine } from '../scene-engine';
import { SlicerFile } from '../slicer-file';
import { clearOffsetX, footprintOf } from './placement';

/** Model file extensions the scene engine can load. */
const MODEL_EXTENSIONS = ['stl', 'obj', '3mf'] as const;

export type ModelFormat = (typeof MODEL_EXTENSIONS)[number];

/** Gap left between objects when a new one is placed or duplicated (mm). */
const PLACEMENT_SPACING_MM = 4;

/** Outcome of adding one file to the plate. */
export interface AddObjectResult {
  file: File;
  objectId?: bigint;
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
 * 2. **Every object remembers its source file.** In cloud mode the bytes live
 *    on the server and a slice references them by upload id. Stamping that id
 *    on the object at add time is what lets a plate hold several *different*
 *    models — without it the slice can only guess which upload belongs to
 *    which object.
 *
 * The 3D viewer mirrors {@link objects}; it is not involved in loading.
 */
@Injectable({ providedIn: 'root' })
export class WorkplateObjects {
  private readonly log = inject(Logger).scope('WorkplateObjects');
  private readonly sceneEngine = inject(SceneEngine);
  private readonly sceneCommand = inject(SceneCommand);
  private readonly slicerFile = inject(SlicerFile);

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
    const ext = file.name.toLowerCase().split('.').pop() ?? '';
    return (MODEL_EXTENSIONS as readonly string[]).includes(ext);
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
        results.push({ file, objectId: await this.addFile(file) });
      } catch (error) {
        const message = error instanceof Error ? error.message : 'Failed to add model';
        this.log.error(`addFiles '${file.name}' failed`, message);
        results.push({ file, error: message });
      }
    }

    return results;
  }

  /**
   * Add a model whose bytes are already on the server.
   *
   * Used when reopening a saved workplate: the upload id is known up front,
   * so it is stamped on the object without re-uploading the bytes.
   */
  async addUploadedFile(file: File, sourceId: string): Promise<bigint> {
    await this.sceneEngine.ready();
    return this.placeMesh(file.name, new Uint8Array(await file.arrayBuffer()), sourceId);
  }

  private async addFile(file: File): Promise<bigint> {
    const bytes = new Uint8Array(await file.arrayBuffer());

    // Upload first: the object must know its file id from the moment it
    // exists, or a slice fired before the upload settles cannot resolve it.
    let sourceId: string | undefined;
    if (resolveRuntimeMode() === 'cloud') {
      const upload = await this.slicerFile.upload(file);
      sourceId = upload.ofids[0];
    }

    return this.placeMesh(file.name, bytes, sourceId);
  }

  private placeMesh(name: string, bytes: Uint8Array, sourceId?: string): bigint {
    const id = this.sceneEngine.addMesh(name, formatOf(name), bytes, sourceId);
    this.sceneEngine.apply({ op: 'AutoOrient', args: { id } });
    this.sceneEngine.apply({ op: 'DropToFloor', args: { id } });
    this.placeClear(id);
    return id;
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
      args: { id, offset: [width + PLACEMENT_SPACING_MM, 0, 0] },
    });
  }

  /** Remove an object from the plate. */
  remove(id: bigint): void {
    const target = this.objects().find((o) => o.id === id);
    this.sceneCommand.apply({ op: 'Remove', args: { id } });
    // Drop the upload too, but only once no other object still points at it —
    // duplicates share one file and the survivors still need it to slice.
    const sourceId = target?.source_id;
    if (sourceId && !this.objects().some((o) => o.source_id === sourceId)) {
      this.slicerFile.removeFile(sourceId);
    }
  }

  /** Remove every object from the plate. */
  clear(): void {
    for (const object of [...this.objects()]) {
      this.sceneEngine.apply({ op: 'Remove', args: { id: object.id } });
    }
  }

  /**
   * Repack every object onto the bed without overlap.
   *
   * Orientation is left alone — the user may have deliberately posed a model,
   * and silently re-rotating it on an arrange would throw that away.
   */
  arrange(): void {
    const ids = this.objects().map((o) => o.id);
    if (ids.length === 0) {
      return;
    }
    this.sceneCommand.apply({
      op: 'ArrangeOnBed',
      args: { ids, options: { spacing_mm: PLACEMENT_SPACING_MM, auto_orient: false } },
    });
  }

  /**
   * Shift a freshly-added object clear of the ones already on the plate.
   *
   * Walks right along X, each time jumping past the far edge of whichever
   * object is still in the way. Stepping by the *new* object's own width would
   * not be enough — a neighbour wider than the new object would still overlap
   * after a step, and the walk would give up while sitting inside it.
   *
   * A plain "arrange everything" would be tidier, but it also moves models the
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

    const dx = clearOffsetX(target.world_aabb, others, PLACEMENT_SPACING_MM);
    if (dx !== null && dx > 0) {
      this.sceneEngine.apply({ op: 'Translate', args: { id, delta: [dx, 0, 0] } });
    }
  }
}

/** Detect a model format from a filename, defaulting to STL. */
function formatOf(fileName: string): ModelFormat {
  const ext = fileName.toLowerCase().split('.').pop() ?? '';
  return (MODEL_EXTENSIONS as readonly string[]).includes(ext) ? (ext as ModelFormat) : 'stl';
}
