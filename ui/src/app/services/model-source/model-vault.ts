import type { ModelFormat, ModelSource } from './model-source-registry';

const DB_NAME = 'coldcrabby-models';
const DB_VERSION = 1;
const STORE = 'sources';

/** One model file as it is held between sessions. */
interface VaultRecord {
  sourceId: string;
  fileName: string;
  format: ModelFormat;
  /**
   * Raw bytes. Absent in cloud mode, where the server already holds the file
   * and the browser keeps no copy of it to begin with.
   */
  bytes?: ArrayBuffer;
  /** Epoch ms of the last store or read, so {@link ModelVault.trim} can rank. */
  lastUsed: number;
}

/**
 * How many bytes of model data the vault may hold.
 *
 * Generous, because the thing it buys — every open plate coming back after the
 * app is relaunched — is the whole point, and a slicer's users work in
 * hundreds of megabytes. Bounded, because an unbounded store of every model
 * ever opened ends in a quota error at the worst possible moment: the write
 * that would have saved the plate the user is working on right now.
 */
const BUDGET_BYTES = 512 * 1024 * 1024;

/**
 * Durable storage for the model files a plate's objects were loaded from.
 *
 * A workplate records a `file_id` per object, never mesh bytes — that is the
 * rule the whole document depends on. Cloud resolves those ids against the
 * server; the desktop, iPad and browser builds had nothing behind them at all,
 * so a plate could only be reopened while the page that made it was still
 * alive. Closing the app, or iOS reclaiming the webview while it sat in the
 * background, took every plate with it.
 *
 * This is the missing half: the id resolves here when there is no engine to ask.
 *
 * **IndexedDB rather than `localStorage`** because models are megabytes and
 * arrive as bytes, and because a write here must not block the frame the user
 * is dragging a model through.
 *
 * Deliberately *not* an Angular service. It has no injectable dependencies, and
 * keeping it a plain module singleton means {@link ModelSourceRegistry} — which
 * is constructed directly in its own tests — does not need an injection context
 * just to exist.
 *
 * Every method resolves rather than rejects. A browser with IndexedDB blocked,
 * a full quota, or a private window that hands back a store it then discards
 * all degrade to "nothing was remembered", which is exactly the behaviour that
 * shipped before this existed.
 */
class ModelVault {
  #db: Promise<IDBDatabase | null> | null = null;

  /** Remember one file, replacing any record already under its handle. */
  async put(source: ModelSource): Promise<void> {
    const record: VaultRecord = {
      sourceId: source.sourceId,
      fileName: source.fileName,
      format: source.format,
      // `slice()` detaches the bytes from whatever view the caller holds —
      // structured clone would otherwise copy the whole underlying buffer when
      // the Uint8Array is a window onto a larger one.
      bytes: source.bytes ? detach(source.bytes) : undefined,
      lastUsed: Date.now(),
    };
    await this.#write((store) => store.put(record));
  }

  /** Every remembered file among `sourceIds`, in no particular order. */
  async read(sourceIds: readonly string[]): Promise<ModelSource[]> {
    if (sourceIds.length === 0) {
      return [];
    }
    const db = await this.#open();
    if (!db) {
      return [];
    }
    try {
      return await new Promise<ModelSource[]>((resolve, reject) => {
        const tx = db.transaction(STORE, 'readwrite');
        const store = tx.objectStore(STORE);
        const found: ModelSource[] = [];
        for (const id of sourceIds) {
          const request = store.get(id);
          request.onsuccess = () => {
            const record = request.result as VaultRecord | undefined;
            if (record) {
              found.push(toSource(record));
              // Reading a file is what marks the plate holding it as one the
              // user still comes back to — which is what `trim` ranks on.
              store.put({ ...record, lastUsed: Date.now() });
            }
          };
        }
        tx.oncomplete = () => resolve(found);
        tx.onerror = () => reject(tx.error);
        tx.onabort = () => reject(tx.error);
      });
    } catch {
      return [];
    }
  }

  /**
   * Drop the least recently used files until the store fits its budget.
   *
   * Closing a tab deliberately does **not** forget its models: the plate is
   * still in the slice history, and a plate that reopens empty because the tab
   * strip happened to be tidied is worse than one that costs some disk. Age is
   * what decides instead, once at startup, so the cost is bounded without
   * anything the user just touched ever being the thing that goes.
   */
  async trim(): Promise<void> {
    const db = await this.#open();
    if (!db) {
      return;
    }
    try {
      await new Promise<void>((resolve, reject) => {
        const tx = db.transaction(STORE, 'readwrite');
        const store = tx.objectStore(STORE);
        const all = store.getAll();
        all.onsuccess = () => {
          const records = (all.result as VaultRecord[])
            .map((r) => ({ id: r.sourceId, size: r.bytes?.byteLength ?? 0, at: r.lastUsed ?? 0 }))
            .sort((a, b) => b.at - a.at);
          let kept = 0;
          for (const record of records) {
            kept += record.size;
            if (kept > BUDGET_BYTES) {
              store.delete(record.id);
            }
          }
        };
        tx.oncomplete = () => resolve();
        tx.onerror = () => reject(tx.error);
        tx.onabort = () => reject(tx.error);
      });
    } catch {
      // Housekeeping; failing it costs space, never correctness.
    }
  }

  /** Drop everything — the Danger Zone wiping local data. */
  async clear(): Promise<void> {
    await this.#write((store) => store.clear());
  }

  async #write(action: (store: IDBObjectStore) => IDBRequest): Promise<void> {
    const db = await this.#open();
    if (!db) {
      return;
    }
    try {
      await new Promise<void>((resolve, reject) => {
        const tx = db.transaction(STORE, 'readwrite');
        action(tx.objectStore(STORE));
        tx.oncomplete = () => resolve();
        tx.onerror = () => reject(tx.error);
        tx.onabort = () => reject(tx.error);
      });
    } catch (error) {
      // Almost always a full quota, and almost always a large model. Say so
      // once; the plate still works for this session either way.
      console.warn('[ModelVault] could not store model data', error);
    }
  }

  #open(): Promise<IDBDatabase | null> {
    this.#db ??= new Promise<IDBDatabase | null>((resolve) => {
      if (typeof indexedDB === 'undefined') {
        resolve(null);
        return;
      }
      let request: IDBOpenDBRequest;
      try {
        request = indexedDB.open(DB_NAME, DB_VERSION);
      } catch {
        resolve(null);
        return;
      }
      request.onupgradeneeded = () => {
        if (!request.result.objectStoreNames.contains(STORE)) {
          request.result.createObjectStore(STORE, { keyPath: 'sourceId' });
        }
      };
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => resolve(null);
      request.onblocked = () => resolve(null);
    });
    return this.#db;
  }
}

/**
 * A standalone `ArrayBuffer` holding exactly this view's bytes.
 *
 * Structured clone would otherwise copy the whole underlying buffer when the
 * `Uint8Array` is a window onto a larger one — and a wasm view is a window onto
 * the entire linear memory.
 */
function detach(bytes: Uint8Array): ArrayBuffer {
  const copy = new ArrayBuffer(bytes.byteLength);
  new Uint8Array(copy).set(bytes);
  return copy;
}

function toSource(record: VaultRecord): ModelSource {
  return {
    sourceId: record.sourceId,
    fileName: record.fileName,
    format: record.format,
    bytes: record.bytes ? new Uint8Array(record.bytes) : undefined,
    // Deliberately no `filePath`. A path is only true for as long as the
    // session that learned it: the desktop's own cache file may have been swept
    // and the user's original moved or deleted, and a source that names a path
    // that is gone fails the slice instead of falling back to the bytes right
    // beside it. The runtime re-caches from the bytes on the next slice.
  };
}

/** The one vault. See {@link ModelVault} for why this is not a service. */
export const modelVault = new ModelVault();
