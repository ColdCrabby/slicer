import { libraryImport } from '../../../generated/scene-wasm/scene_engine';
import {
  LibraryBackend,
  type ImportOutcome,
  type ImportResult,
  type Library,
  type LibraryCapabilities,
  type LibraryEntry,
  type LibrarySettings,
  type ScanReport,
} from './library-backend';

const DB_NAME = 'coldcrabby-library';
const DB_VERSION = 1;
/** One record, under {@link DOC_KEY}: the whole {@link Library} document. */
const DOC = 'doc';
const DOC_KEY = 'library';
/** Model bytes, keyed by entry id. */
const FILES = 'files';
/** Thumbnail PNGs, keyed by entry id. */
const THUMBS = 'thumbs';

/**
 * The browser build's library: IndexedDB, because here the browser *is* the
 * engine and there is nothing behind it to ask.
 *
 * The document has the engine's exact shape, and whether a dropped file is new,
 * the same file again or a re-export of one already held is decided by the
 * engine's own `Library::import`, compiled to wasm — never by a TypeScript
 * re-telling of it. Only the storage is local.
 *
 * Separate from {@link modelVault}: the vault is a bounded cache of what open
 * plates are made of and forgets by age; the library is what the user chose
 * to keep and forgets only when told to.
 */
export class BrowserLibraryBackend extends LibraryBackend {
  readonly capabilities: LibraryCapabilities = {
    modes: ['copy'],
    watchFolders: false,
    visibleFolder: false,
    reveal: false,
  };

  #db: Promise<IDBDatabase> | null = null;
  /** Serialises read–modify–write of the document across overlapping imports. */
  #queue: Promise<unknown> = Promise.resolve();

  async load(): Promise<Library> {
    const library = await this.#readDoc();
    const thumbs = new Set(await this.#keys(THUMBS));
    return {
      ...library,
      entries: (library.entries ?? []).map((entry) => ({
        ...entry,
        has_thumbnail: thumbs.has(entry.id),
      })),
    };
  }

  async saveSettings(settings: LibrarySettings): Promise<Library> {
    await this.#update((library) => ({ ...library, settings: { ...settings, mode: 'copy' } }));
    return this.load();
  }

  async scan(): Promise<ScanReport | null> {
    return null;
  }

  async remember(file: File): Promise<ImportOutcome | null> {
    const [result] = await this.importFiles([file]);
    if ('error' in result) {
      return null;
    }
    await this.#touch(result.entry_id);
    return result;
  }

  async importFiles(files: readonly File[]): Promise<ImportResult[]> {
    const results: ImportResult[] = [];
    for (const file of files) {
      try {
        results.push(await this.#import(file));
      } catch (error) {
        results.push({ error: error instanceof Error ? error.message : String(error) });
      }
    }
    return results;
  }

  #import(file: File): Promise<ImportOutcome> {
    return this.#serial(async () => {
      const bytes = new Uint8Array(await file.arrayBuffer());
      const current = await this.#readDoc();
      const { library, outcome } = libraryImport(
        current,
        file.name,
        bytes,
        null,
        new Date().toISOString(),
      ) as { library: Library; outcome: ImportOutcome };
      if (outcome.needs_copy) {
        await this.#put(FILES, outcome.entry_id, bytes.buffer);
        const entry = library.entries?.find((e) => e.id === outcome.entry_id);
        entry?.locations?.unshift({ kind: 'copy', path: `idb:${outcome.entry_id}` });
      }
      await this.#put(DOC, DOC_KEY, library);
      return outcome;
    });
  }

  async open(entry: LibraryEntry): Promise<File | null> {
    const bytes = await this.#get<ArrayBuffer>(FILES, entry.id);
    return bytes ? new File([bytes], `${entry.name}.${entry.format}`) : null;
  }

  async #touch(id: string): Promise<void> {
    await this.#update((library) => ({
      ...library,
      entries: library.entries?.map((e) =>
        e.id === id
          ? { ...e, use_count: (e.use_count ?? 0) + 1, last_used_at: new Date().toISOString() }
          : e,
      ),
    }));
  }

  async rename(id: string, name: string): Promise<void> {
    const trimmed = name.trim();
    if (!trimmed) {
      return;
    }
    await this.#update((library) => ({
      ...library,
      entries: library.entries?.map((e) => (e.id === id ? { ...e, name: trimmed } : e)),
    }));
  }

  async remove(id: string): Promise<void> {
    await this.#update((library) => ({
      ...library,
      entries: library.entries?.filter((e) => e.id !== id),
    }));
    await this.#delete(FILES, id);
    await this.#delete(THUMBS, id);
  }

  async thumbnail(id: string): Promise<Blob | null> {
    return (await this.#get<Blob>(THUMBS, id)) ?? null;
  }

  async setThumbnail(id: string, png: Blob): Promise<void> {
    await this.#put(THUMBS, id, png);
  }

  // ── IndexedDB plumbing ────────────────────────────────────────────────────

  #serial<T>(work: () => Promise<T>): Promise<T> {
    const next = this.#queue.then(work, work);
    this.#queue = next.catch(() => undefined);
    return next;
  }

  #update(change: (library: Library) => Library): Promise<void> {
    return this.#serial(async () => {
      await this.#put(DOC, DOC_KEY, change(await this.#readDoc()));
    });
  }

  async #readDoc(): Promise<Library> {
    return (await this.#get<Library>(DOC, DOC_KEY)) ?? { settings: { mode: 'copy' }, entries: [] };
  }

  #open(): Promise<IDBDatabase> {
    this.#db ??= new Promise<IDBDatabase>((resolve, reject) => {
      const request = indexedDB.open(DB_NAME, DB_VERSION);
      request.onupgradeneeded = () => {
        for (const store of [DOC, FILES, THUMBS]) {
          if (!request.result.objectStoreNames.contains(store)) {
            request.result.createObjectStore(store);
          }
        }
      };
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
      request.onblocked = () => reject(new Error('library storage is blocked'));
    }).catch((error) => {
      // Let a later call try again rather than caching the failure.
      this.#db = null;
      throw error;
    });
    return this.#db;
  }

  async #request<T>(
    store: string,
    mode: IDBTransactionMode,
    action: (store: IDBObjectStore) => IDBRequest,
  ): Promise<T> {
    const db = await this.#open();
    return new Promise<T>((resolve, reject) => {
      const tx = db.transaction(store, mode);
      const request = action(tx.objectStore(store));
      tx.oncomplete = () => resolve(request.result as T);
      tx.onerror = () => reject(tx.error);
      tx.onabort = () => reject(tx.error);
    });
  }

  #get<T>(store: string, key: string): Promise<T | undefined> {
    return this.#request<T | undefined>(store, 'readonly', (s) => s.get(key));
  }

  async #put(store: string, key: string, value: unknown): Promise<void> {
    await this.#request(store, 'readwrite', (s) => s.put(value, key));
  }

  async #delete(store: string, key: string): Promise<void> {
    await this.#request(store, 'readwrite', (s) => s.delete(key));
  }

  #keys(store: string): Promise<string[]> {
    return this.#request<string[]>(store, 'readonly', (s) => s.getAllKeys());
  }
}
