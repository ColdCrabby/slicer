import { DestroyRef, type Signal, computed, inject, signal } from '@angular/core';
import type { ProfileMeta } from '../../models/profile-source';
import { toUserCopy } from '../catalog/cloud-catalog';
import { BrowserStorage } from '../browser-storage';
import { EngineWriteThrough } from './engine-write-through';
import type { SaveStatus } from './engine-write-through';
import { ProfilePersistence } from './profile-persistence';
import type { ProfileCategory } from './profile-persistence';

/** Lifecycle of a hydrate/reload from the engine store. */
export type LoadStatus = 'idle' | 'loading' | 'loaded' | 'error';

/**
 * Signal-backed collection persisted to localStorage. Backs every profile
 * category (printers, filaments, print profiles).
 *
 * Invariants:
 * - Always seeded with the single offline `builtin` default so an offline
 *   install has exactly one working entry per category.
 * - `builtin` entries cannot be removed (mirrors "system presets" in other
 *   slicers). Everything else is fully user-owned.
 * - Importing a catalog entry always lands as a `user` copy via
 *   {@link toUserCopy} — the catalog itself is never written to disk.
 *
 * Storage: localStorage is always a fast local cache. When the runtime is
 * engine-backed (native/cloud, see {@link ProfilePersistence}), the store also
 * hydrates from and writes through to the engine's on-disk library, so the
 * user's profiles live with the slicer instead of only in this browser. The
 * write-through is debounced (see {@link EngineWriteThrough}) so a burst of
 * edits collapses into a single save.
 */
export class LocalCollectionStore<T extends ProfileMeta> {
  private readonly storage = inject(BrowserStorage);
  private readonly persistence = inject(ProfilePersistence);
  private readonly destroyRef = inject(DestroyRef);
  private readonly _items = signal<T[]>([]);

  readonly items = this._items.asReadonly();
  readonly count = computed(() => this._items().length);

  /** Debounced write-through to the engine store. */
  private readonly writer: EngineWriteThrough;

  /** Save lifecycle: `pending` while debouncing, `saving`, or `error`. */
  readonly saveStatus: Signal<SaveStatus>;
  /** Last save failure message, or `null`. */
  readonly saveError: Signal<string | null>;

  /** Load lifecycle for the initial hydrate / a reload from the engine. */
  private readonly _loadStatus = signal<LoadStatus>('idle');
  readonly loadStatus = this._loadStatus.asReadonly();
  /** True while a hydrate/reload from the engine is in flight. */
  readonly loading = computed(() => this._loadStatus() === 'loading');

  constructor(
    private readonly storageKey: string,
    private readonly seed: T[],
    private readonly category: ProfileCategory,
  ) {
    this.writer = new EngineWriteThrough(
      this.persistence,
      this.category,
      () => this._items(),
      this.destroyRef,
    );
    this.saveStatus = this.writer.status;
    this.saveError = this.writer.error;

    const stored = this.storage.getJson<T[]>(storageKey, 'local');
    // Guarantee the builtin defaults are always present even if a stored
    // payload predates them or dropped them.
    this._items.set(this.mergeSeed(stored ?? []));
    void this.hydrate();
  }

  /**
   * Pull the authoritative copy from the engine store on startup. No-op on the
   * browser-local (wasm) backend. On the first run against an empty engine
   * store, migrate any existing local user data up instead of clobbering it.
   */
  private async hydrate(): Promise<void> {
    if (!this.persistence.isEngineBacked) {
      return;
    }
    this._loadStatus.set('loading');
    try {
      const library = await this.persistence.loadLibrary();
      const remote = (library[this.category] as T[] | undefined) ?? [];
      const hasLocalUserData = this._items().some((item) => item.source !== 'builtin');
      if (remote.length === 0 && hasLocalUserData) {
        // First run: the engine has nothing yet but this browser does — push
        // the local library up so the user keeps what they already made.
        await this.persistence.saveCategory(this.category, this._items());
      } else {
        const merged = this.mergeSeed(remote);
        this._items.set(merged);
        this.storage.writeJson(this.storageKey, merged, 'local');
      }
      this._loadStatus.set('loaded');
    } catch (error) {
      this._loadStatus.set('error');
      console.warn(
        `[profiles] could not sync '${this.category}' from the engine; using local cache`,
        error,
      );
    }
  }

  getById(id: string): T | undefined {
    return this._items().find((item) => item.id === id);
  }

  /**
   * Adopt the engine's current copy of this category after it changed
   * elsewhere (another client/tab). No-op on the browser-local backend.
   *
   * Skipped while a local save is pending or in flight so an incoming
   * notification (including the echo of our own write) never clobbers an edit
   * the user is still making.
   */
  async reload(): Promise<void> {
    if (!this.persistence.isEngineBacked || this.saveStatus() !== 'idle') {
      return;
    }
    this._loadStatus.set('loading');
    try {
      const library = await this.persistence.reloadLibrary();
      if (this.saveStatus() !== 'idle') {
        // A fresh local edit started mid-fetch — keep it, drop the remote copy.
        this._loadStatus.set('loaded');
        return;
      }
      const merged = this.mergeSeed((library[this.category] as T[] | undefined) ?? []);
      this._items.set(merged);
      this.storage.writeJson(this.storageKey, merged, 'local');
      this._loadStatus.set('loaded');
    } catch (error) {
      this._loadStatus.set('error');
      console.warn(`[profiles] could not reload '${this.category}' from the engine`, error);
    }
  }

  add(item: T): T {
    this._items.update((list) => [...list, item]);
    this.persist();
    return item;
  }

  update(id: string, patch: Partial<T>): void {
    this._items.update((list) =>
      list.map((item) => (item.id === id ? { ...item, ...patch } : item)),
    );
    this.persist();
  }

  /** Remove an entry. Builtin defaults are protected and silently ignored. */
  remove(id: string): void {
    const target = this.getById(id);
    if (!target || target.source === 'builtin') {
      return;
    }
    this._items.update((list) => list.filter((item) => item.id !== id));
    this.persist();
  }

  /** Deep-copy an existing entry into a new `user` entry named "… (copy)". */
  duplicate(id: string): T | undefined {
    const source = this.getById(id);
    if (!source) {
      return undefined;
    }
    return this.add(toUserCopy(source, `${source.name} (copy)`));
  }

  /**
   * Import a catalog entry as a local `user` copy. If an entry already based on
   * the same catalog id exists it is returned instead of duplicating.
   */
  importFromCatalog(entry: T): T {
    const existing = this._items().find((item) => item.based_on === entry.id);
    if (existing) {
      return existing;
    }
    return this.add(toUserCopy(entry));
  }

  private mergeSeed(stored: T[]): T[] {
    const byId = new Map(stored.map((item) => [item.id, item]));
    for (const seed of this.seed) {
      if (!byId.has(seed.id)) {
        byId.set(seed.id, seed);
      }
    }
    return [...byId.values()];
  }

  private persist(): void {
    // localStorage is written immediately as a fast local cache / offline
    // fallback; the engine write-through is debounced so a burst of edits
    // (e.g. dragging a color) collapses into one save.
    this.storage.writeJson(this.storageKey, this._items(), 'local');
    this.writer.queue();
  }
}
