import { Injectable, computed, inject, signal } from '@angular/core';
import { DEFAULT_LABELS, Label } from '../../models/label.model';
import { BrowserStorage } from '../browser-storage';

const STORAGE_KEY = 'profiles.labels';

/**
 * Persisted store for the flat, cross-area label vocabulary. Unlike the profile
 * stores there is no `builtin`/`catalog` provenance — labels are entirely
 * user-owned — so this is a small dedicated store rather than a
 * {@link LocalCollectionStore} subclass.
 */
@Injectable({ providedIn: 'root' })
export class LabelsStore {
  private readonly storage = inject(BrowserStorage);
  private readonly _items = signal<Label[]>(
    this.storage.getJson<Label[]>(STORAGE_KEY, 'local') ?? DEFAULT_LABELS,
  );

  readonly items = this._items.asReadonly();
  readonly count = computed(() => this._items().length);

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
  }
}
