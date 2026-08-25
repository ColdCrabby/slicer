import { DestroyRef, Injectable, computed, inject, signal } from '@angular/core';
import { DEFAULT_LABELS } from '../../models/label.model';
import type { Label } from '../../models/label.model';
import { BrowserStorage } from '../browser-storage';
import { EngineWriteThrough } from './engine-write-through';
import type { LoadStatus } from './local-collection-store';
import { ProfilePersistence } from './profile-persistence';

const STORAGE_KEY = 'profiles.labels';

/**
 * Persisted store for the flat, cross-area label vocabulary. Unlike the profile
 * stores there is no `builtin`/`catalog` provenance — labels are entirely
 * user-owned — so this is a small dedicated store rather than a
 * {@link LocalCollectionStore} subclass.
 *
 * Like the profile stores, localStorage is a fast cache; on an engine-backed
 * runtime (native/cloud) labels also hydrate from and write through (debounced)
 * to the engine's `profiles.toml` so they live with the slicer.
 */
@Injectable({ providedIn: 'root' })
export class LabelsStore {
  private readonly storage = inject(BrowserStorage);
  private readonly persistence = inject(ProfilePersistence);
  private readonly destroyRef = inject(DestroyRef);
  private readonly _items = signal<Label[]>(
    this.storage.getJson<Label[]>(STORAGE_KEY, 'local') ?? DEFAULT_LABELS,
  );

  readonly items = this._items.asReadonly();
  readonly count = computed(() => this._items().length);

  /** Debounced write-through to the engine store. */
  private readonly writer = new EngineWriteThrough(
    this.persistence,
    'labels',
    () => this._items(),
    this.destroyRef,
  );
  /** Save lifecycle: `pending` while debouncing, `saving`, or `error`. */
  readonly saveStatus = this.writer.status;
  /** Last save failure message, or `null`. */
  readonly saveError = this.writer.error;

  /** Load lifecycle for the initial hydrate / a reload from the engine. */
  private readonly _loadStatus = signal<LoadStatus>('idle');
  readonly loadStatus = this._loadStatus.asReadonly();
  /** True while a hydrate/reload from the engine is in flight. */
  readonly loading = computed(() => this._loadStatus() === 'loading');

  constructor() {
    void this.hydrate();
  }

  /**
   * Adopt the engine's labels on startup (native/cloud). On the first run
   * against an empty engine store, push the local labels up rather than losing
   * them. No-op on the browser-local (wasm) backend.
   */
  private async hydrate(): Promise<void> {
    if (!this.persistence.isEngineBacked) {
      return;
    }
    this._loadStatus.set('loading');
    try {
      const library = await this.persistence.loadLibrary();
      const remote = (library.labels as Label[] | undefined) ?? [];
      if (remote.length === 0) {
        await this.persistence.saveCategory('labels', this._items());
      } else {
        this._items.set(remote);
        this.storage.writeJson(STORAGE_KEY, remote, 'local');
      }
      this._loadStatus.set('loaded');
    } catch (error) {
      this._loadStatus.set('error');
      console.warn('[profiles] could not sync labels from the engine; using local cache', error);
    }
  }

  /**
   * Adopt the engine's labels after they changed elsewhere (another
   * client/tab). No-op on the browser-local backend. Skipped while a local
   * save is pending or in flight so it never clobbers an in-progress edit.
   */
  async reload(): Promise<void> {
    if (!this.persistence.isEngineBacked || this.saveStatus() !== 'idle') {
      return;
    }
    this._loadStatus.set('loading');
    try {
      const library = await this.persistence.reloadLibrary();
      if (this.saveStatus() !== 'idle') {
        this._loadStatus.set('loaded');
        return;
      }
      const remote = (library.labels as Label[] | undefined) ?? [];
      if (remote.length) {
        this._items.set(remote);
        this.storage.writeJson(STORAGE_KEY, remote, 'local');
      }
      this._loadStatus.set('loaded');
    } catch (error) {
      this._loadStatus.set('error');
      console.warn('[profiles] could not reload labels from the engine', error);
    }
  }

  /** Fast id → label lookup, recomputed only when the list changes. */
  private readonly byId = computed(() => {
    const map = new Map<string, Label>();
    for (const label of this._items()) {
      map.set(label.id, label);
    }
    return map;
  });

  getById(id: string): Label | undefined {
    return this.byId().get(id);
  }

  /** Resolve a list of label ids to label objects, dropping unknown ids. */
  resolve(ids: readonly string[] | undefined): Label[] {
    if (!ids?.length) {
      return [];
    }
    const map = this.byId();
    return ids.map((id) => map.get(id)).filter((l): l is Label => l !== undefined);
  }

  add(label: Label): Label {
    this._items.update((list) => [...list, label]);
    this.persist();
    return label;
  }

  update(id: string, patch: Partial<Label>): void {
    this._items.update((list) =>
      list.map((label) => (label.id === id ? { ...label, ...patch } : label)),
    );
    this.persist();
  }

  remove(id: string): void {
    this._items.update((list) => list.filter((label) => label.id !== id));
    this.persist();
  }

  private persist(): void {
    this.storage.writeJson(STORAGE_KEY, this._items(), 'local');
    this.writer.queue();
  }
}
