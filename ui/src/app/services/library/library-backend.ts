import { environment } from '../../../environments/environment';
import type {
  ImportOutcome,
  Library,
  LibraryEntry,
  LibrarySettings,
  ScanReport,
} from '../../../generated/slicer-engine-library-v1';
import { isTauriMobile } from '../../runtime/domain/runtime-mode.util';
import { nativePathOf } from '../model-source';

export type { ImportOutcome, Library, LibraryEntry, LibrarySettings, ScanReport };

/** How an import is kept: copied, referenced where it is, or both. */
export type StorageMode = NonNullable<LibrarySettings['mode']>;

/** What a runtime's library can do — which is what the settings may offer. */
export interface LibraryCapabilities {
  /**
   * Storage modes this runtime can honour. Only a native host can reopen a
   * path it was given once, so everywhere else this is `['copy']` and the
   * choice is not shown at all.
   */
  readonly modes: readonly StorageMode[];
  /** Whether the user can point the library at folders of their own. */
  readonly watchFolders: boolean;
  /**
   * Whether the library's own folder is somewhere the user can drop models
   * outside the app — the Files app on iPad, Finder or Explorer on desktop.
   */
  readonly visibleFolder: boolean;
}

/** One file that could not be recorded, alongside those that were. */
export type ImportResult = ImportOutcome | { error: string };

/**
 * Where the library lives, per runtime.
 *
 * Mirrors {@link WorkplatePersistence}: the engine owns the library wherever
 * there is one — the desktop and iPad apps through Tauri commands, the cloud
 * server over REST — and the browser build, where the browser *is* the
 * engine, keeps it in IndexedDB and asks the wasm engine to do the matching.
 */
export abstract class LibraryBackend {
  abstract readonly capabilities: LibraryCapabilities;

  abstract load(): Promise<Library>;

  abstract saveSettings(settings: LibrarySettings): Promise<Library>;

  /** Walk the library's folders for files added outside the app. */
  abstract scan(): Promise<ScanReport | null>;

  /**
   * Record a model that just reached a plate. Resolves `null` where the engine
   * records it on its own — the cloud server records every upload.
   */
  abstract remember(file: File): Promise<ImportOutcome | null>;

  /** Add models to the library without putting them on a plate. */
  abstract importFiles(files: readonly File[]): Promise<ImportResult[]>;

  /**
   * An entry's model as a `File` the plate flows already accept. On native
   * hosts it carries its path, so the slicer reads it off disk.
   */
  abstract open(entry: LibraryEntry): Promise<File | null>;

  abstract touch(id: string): Promise<void>;
  abstract rename(id: string, name: string): Promise<void>;
  abstract remove(id: string): Promise<void>;

  abstract thumbnail(id: string): Promise<Blob | null>;
  abstract setThumbnail(id: string, png: Blob): Promise<void>;
}

/** Attach a native path to a `File`, the way the native picker's files carry one. */
function withPath(file: File, path: string): File {
  Object.defineProperty(file, 'path', { value: path });
  return file;
}

function fileNameOf(entry: LibraryEntry): string {
  return `${entry.name}.${entry.format}`;
}

// ── Native (desktop and iPad) ────────────────────────────────────────────────

/** Tauri commands over the engine's own library directory. */
export class NativeLibraryBackend extends LibraryBackend {
  /**
   * iOS hands a picked file over as a throwaway copy and grants no lasting
   * access to anywhere else, so a reference could not be reopened. What it
   * *does* have is a folder the Files app shows — the library's own.
   */
  readonly capabilities: LibraryCapabilities = isTauriMobile()
    ? { modes: ['copy'], watchFolders: false, visibleFolder: true }
    : { modes: ['copy', 'reference', 'both'], watchFolders: true, visibleFolder: true };

  async #invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    const { invoke } = await import('@tauri-apps/api/core');
    return invoke<T>(command, args);
  }

  load(): Promise<Library> {
    return this.#invoke('library_load');
  }

  saveSettings(settings: LibrarySettings): Promise<Library> {
    return this.#invoke('library_save_settings', { settings });
  }

  scan(): Promise<ScanReport | null> {
    return this.#invoke('library_scan');
  }

  async remember(file: File): Promise<ImportOutcome | null> {
    const [result] = await this.importFiles([file]);
    return 'error' in result ? null : result;
  }

  async importFiles(files: readonly File[]): Promise<ImportResult[]> {
    const results: ImportResult[] = [];
    for (const file of files) {
      const path = nativePathOf(file);
      try {
        results.push(
          path
            ? (await this.#invoke<ImportResult[]>('library_import_paths', { paths: [path] }))[0]
            : await this.#importBytes(file),
        );
      } catch (error) {
        results.push({ error: String(error) });
      }
    }
    return results;
  }

  /** A file with no path — a drop into the webview — crosses as raw bytes. */
  async #importBytes(file: File): Promise<ImportOutcome> {
    const { invoke } = await import('@tauri-apps/api/core');
    return invoke<ImportOutcome>('library_import_bytes', new Uint8Array(await file.arrayBuffer()), {
      headers: { 'x-file-name': encodeURIComponent(file.name) },
    });
  }

  async open(entry: LibraryEntry): Promise<File | null> {
    const resolved = await this.#invoke<{ path: string } | null>('library_resolve', {
      id: entry.id,
    });
    if (!resolved) {
      return null;
    }
    const { readFile } = await import('@tauri-apps/plugin-fs');
    const bytes = await readFile(resolved.path);
    return withPath(new File([bytes as BlobPart], fileNameOf(entry)), resolved.path);
  }

  async touch(id: string): Promise<void> {
    await this.#invoke('library_touch', { id });
  }

  async rename(id: string, name: string): Promise<void> {
    await this.#invoke('library_rename', { id, name });
  }

  async remove(id: string): Promise<void> {
    await this.#invoke('library_remove', { id });
  }

  async thumbnail(id: string): Promise<Blob | null> {
    const bytes = await this.#invoke<ArrayBuffer>('library_thumbnail', { id });
    return bytes.byteLength > 0 ? new Blob([bytes], { type: 'image/png' }) : null;
  }

  async setThumbnail(id: string, png: Blob): Promise<void> {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('library_set_thumbnail', new Uint8Array(await png.arrayBuffer()), {
      headers: { 'x-entry-id': id },
    });
  }
}

// ── Cloud ────────────────────────────────────────────────────────────────────

/** REST to the slicer server's `/api/library`. */
export class RemoteLibraryBackend extends LibraryBackend {
  /**
   * The server cannot reach the browser's disk, so an upload is always a
   * copy. Its own watched folders are server paths — an administrator's
   * setting, made through the API rather than offered to every user here.
   */
  readonly capabilities: LibraryCapabilities = {
    modes: ['copy'],
    watchFolders: false,
    visibleFolder: false,
  };
  readonly #base = `${environment.apiUrl}/library`;

  async #json<T>(path: string, init?: RequestInit): Promise<T> {
    const response = await fetch(`${this.#base}${path}`, init);
    if (!response.ok) {
      throw new Error(`${init?.method ?? 'GET'} /library${path} failed (${response.status})`);
    }
    return (await response.json()) as T;
  }

  async #send(path: string, init: RequestInit): Promise<void> {
    const response = await fetch(`${this.#base}${path}`, init);
    if (!response.ok) {
      throw new Error(`${init.method} /library${path} failed (${response.status})`);
    }
  }

  load(): Promise<Library> {
    return this.#json('');
  }

  saveSettings(settings: LibrarySettings): Promise<Library> {
    return this.#json('/settings', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(settings),
    });
  }

  scan(): Promise<ScanReport | null> {
    return this.#json('/scan', { method: 'POST' });
  }

  /** The server records every upload itself. */
  async remember(): Promise<ImportOutcome | null> {
    return null;
  }

  async importFiles(files: readonly File[]): Promise<ImportResult[]> {
    const results: ImportResult[] = [];
    for (const file of files) {
      try {
        results.push(
          await this.#json<ImportOutcome>(`/import?name=${encodeURIComponent(file.name)}`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/octet-stream' },
            body: file,
          }),
        );
      } catch (error) {
        results.push({ error: error instanceof Error ? error.message : String(error) });
      }
    }
    return results;
  }

  async open(entry: LibraryEntry): Promise<File | null> {
    const response = await fetch(`${this.#base}/${entry.id}/file`);
    return response.ok ? new File([await response.blob()], fileNameOf(entry)) : null;
  }

  /**
   * Uses are counted by the server when it places an entry itself
   * (`POST /api/library/{id}/place`); a model re-added through an upload is
   * not counted twice.
   */
  async touch(): Promise<void> {}

  async rename(id: string, name: string): Promise<void> {
    await this.#send(`/${id}`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name }),
    });
  }

  async remove(id: string): Promise<void> {
    await this.#send(`/${id}`, { method: 'DELETE' });
  }

  async thumbnail(id: string): Promise<Blob | null> {
    const response = await fetch(`${this.#base}/${id}/thumbnail`);
    return response.ok ? response.blob() : null;
  }

  async setThumbnail(id: string, png: Blob): Promise<void> {
    await this.#send(`/${id}/thumbnail`, {
      method: 'PUT',
      headers: { 'Content-Type': 'image/png' },
      body: png,
    });
  }
}
