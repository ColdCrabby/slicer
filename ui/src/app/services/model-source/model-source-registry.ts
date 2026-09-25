import { Injectable } from '@angular/core';
import { modelVault } from './model-vault';

/** Model file formats the scene engine can load. */
export const MODEL_EXTENSIONS = ['stl', 'obj', '3mf'] as const;

export type ModelFormat = (typeof MODEL_EXTENSIONS)[number];

/**
 * The `accept` list for every model file picker.
 *
 * Extensions alone are not enough: iPadOS and some desktop pickers know no type
 * for `.stl` or `.3mf` and grey the file out, so the MIME types — down to
 * `application/octet-stream` — are what let the user pick it at all. Every
 * picker shares this one list so opening a plate and adding to it can never
 * offer different files.
 */
export const MODEL_FILE_ACCEPT = [
  '.stl',
  '.obj',
  '.3mf',
  'model/stl',
  'model/obj',
  'model/3mf',
  'application/vnd.ms-pki.stl',
  'application/sla',
  'application/vnd.ms-package.3dmanufacturing-3dmodel+xml',
  'application/octet-stream',
].join(',');

/** Detect a model format from a filename, defaulting to STL. */
export function modelFormatOf(fileName: string): ModelFormat {
  const ext = fileName.toLowerCase().split('.').pop() ?? '';
  return (MODEL_EXTENSIONS as readonly string[]).includes(ext) ? (ext as ModelFormat) : 'stl';
}

/** Is this a file the scene engine can load? */
export function isSupportedModelFile(fileName: string): boolean {
  const ext = fileName.toLowerCase().split('.').pop() ?? '';
  return (MODEL_EXTENSIONS as readonly string[]).includes(ext);
}

/**
 * The native path of a file, when the host exposes one.
 *
 * Some webview hosts put the originating filesystem path on the `File`, which
 * lets the desktop slicer read the model directly instead of writing the bytes
 * back out to a cache file — and is how a model opened from Files or another
 * app keeps its identity on the way to the plate. Absent in a plain browser,
 * and not guaranteed anywhere: the desktop runtime caches the bytes itself when
 * it is missing, so this is an optimisation rather than something to depend on.
 */
export function nativePathOf(file: File): string | undefined {
  const path = (file as File & { path?: unknown }).path;
  return typeof path === 'string' && path.length > 0 ? path : undefined;
}

/**
 * One model file that objects on the plate were loaded from.
 *
 * `bytes` is what the browser slicer re-parses; `filePath` is what the native
 * slicer reads instead, so the desktop app never pushes a model through the
 * IPC channel. A source may carry either or both.
 */
export interface ModelSource {
  /** Stable handle stamped onto every scene object made from this file. */
  readonly sourceId: string;
  readonly fileName: string;
  readonly format: ModelFormat;
  /** Raw file bytes, when they are still held. */
  readonly bytes?: Uint8Array;
  /** Absolute path on the native filesystem, when one is known. */
  readonly filePath?: string;
}

/**
 * The one place a scene object's `source_id` resolves back to a real file.
 *
 * A workplate is a build plate, not a file: it can hold several *different*
 * models, and a 3MF contributes several objects that all share one file. So
 * "which bytes does this object slice from?" has to be answered per object, and
 * the only durable answer is the `source_id` the scene engine already stamps on
 * every object it creates.
 *
 * Before this existed each runtime guessed, and each guessed differently:
 *
 * - The browser slicer kept bytes per *object id*, populated only on the one
 *   add path that went through the runtime port. Objects added any other way
 *   had no entry, and a plate with more than one object failed outright with
 *   "Missing mesh bytes" because the single-object fallback no longer applied.
 * - The desktop app sent exactly one `file_path` for the whole plate, so every
 *   object resolved into the *first* file. A second model was silently sliced
 *   as a copy of the first.
 *
 * Both are the same bug: a per-plate answer to a per-object question. This
 * registry is that answer, and it is deliberately runtime-agnostic — the cloud
 * runtime's upload UUID, the desktop's file path and the browser's byte array
 * are all just a `ModelSource` under the id the object already carries.
 *
 * Storage is keyed by **file**, not by object, so a 3MF holding five parts and
 * a model duplicated ten times each cost exactly one copy of the bytes.
 *
 * The map is this tab's working set and nothing more; what survives a reload —
 * and, on iPadOS, the system reclaiming the webview — is the copy written
 * through to the {@link modelVault}. {@link hydrate} is how a plate reopened in
 * a later session gets its files back.
 */
@Injectable({ providedIn: 'root' })
export class ModelSourceRegistry {
  readonly #sources = new Map<string, ModelSource>();

  /**
   * Record a file and return the handle to stamp on objects made from it.
   *
   * Pass `sourceId` when the id is already decided elsewhere — the cloud
   * runtime uses the uploaded file's UUID so the server can resolve it too.
   * Otherwise a fresh local id is minted.
   *
   * Registering the same id twice merges: a later call that learns the native
   * `filePath` keeps the bytes already held, and vice versa.
   */
  register(input: {
    sourceId?: string;
    fileName: string;
    format?: ModelFormat;
    bytes?: Uint8Array;
    filePath?: string;
  }): ModelSource {
    const sourceId = input.sourceId ?? newSourceId();
    const existing = this.#sources.get(sourceId);
    const source: ModelSource = {
      sourceId,
      fileName: input.fileName || existing?.fileName || 'model',
      format: input.format ?? existing?.format ?? modelFormatOf(input.fileName),
      bytes: input.bytes ?? existing?.bytes,
      filePath: input.filePath ?? existing?.filePath,
    };
    this.#sources.set(sourceId, source);
    // Fire and forget: the plate is usable the moment the map holds the file,
    // and a user dragging a model must not wait on a database write.
    void modelVault.put(source);
    return source;
  }

  /**
   * Pull files back out of the vault and into this tab's working set.
   *
   * Resolves to the ids that are now resolvable — an id the vault never held,
   * or whose bytes it could not keep, is simply absent from the result, which
   * is what lets a restore report honestly on what it could not bring back.
   */
  async hydrate(sourceIds: readonly string[]): Promise<Set<string>> {
    const missing = sourceIds.filter((id) => id && !this.#sources.has(id));
    for (const source of await modelVault.read(missing)) {
      // A concurrent `register` for the same id wins: it holds bytes the user
      // just handed us, which are at least as fresh as the stored copy.
      if (!this.#sources.has(source.sourceId)) {
        this.#sources.set(source.sourceId, source);
      }
    }
    return new Set(sourceIds.filter((id) => this.#sources.has(id)));
  }

  /** Look up a file by the handle its objects carry. */
  get(sourceId: string | null | undefined): ModelSource | undefined {
    return sourceId ? this.#sources.get(sourceId) : undefined;
  }

  has(sourceId: string | null | undefined): boolean {
    return sourceId ? this.#sources.has(sourceId) : false;
  }

  /**
   * Attach a native path to an already-registered file.
   *
   * The desktop runtime writes dropped bytes to a cache file the first time it
   * slices; recording the path means later slices reuse it instead of writing
   * the same model out again.
   */
  attachFilePath(sourceId: string, filePath: string): void {
    const existing = this.#sources.get(sourceId);
    if (existing) {
      this.#sources.set(sourceId, { ...existing, filePath });
    }
  }

  /**
   * Forget a file once no object on the plate still points at it.
   *
   * Memory only, deliberately. Removing an object is undoable, and an undo that
   * restores the object but not the file it slices from would be worse than the
   * disk the copy costs. {@link trim} is what eventually reclaims it.
   */
  forget(sourceId: string | null | undefined): void {
    if (sourceId) {
      this.#sources.delete(sourceId);
    }
  }

  /**
   * Release this tab's working set.
   *
   * Memory only. The vault is keyed to the plates that are *open*, not to the
   * one on screen, so switching away from a plate must not forget the files the
   * plate behind it is still made of. The stored copy is bounded by age
   * instead — see {@link trim}.
   */
  clear(): void {
    this.#sources.clear();
  }

  /** Bound what is stored, dropping the files nobody has opened in longest. */
  async trim(): Promise<void> {
    await modelVault.trim();
  }

  /** Drop the working set *and* everything stored — wiping local data. */
  async clearPersisted(): Promise<void> {
    this.#sources.clear();
    await modelVault.clear();
  }
}

/** Mint a local handle for a file that has no server-side identity. */
function newSourceId(): string {
  if (typeof globalThis.crypto?.randomUUID === 'function') {
    return `local-${globalThis.crypto.randomUUID()}`;
  }
  return `local-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}
