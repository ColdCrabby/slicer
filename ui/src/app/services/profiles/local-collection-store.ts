import { inject, signal } from '@angular/core';
import { BrowserStorage } from '../browser-storage';

/**
 * Minimal signal-backed collection persisted to localStorage. Mocked profile
 * stores (printers, filaments, print profiles) extend this until the real
 * backend-backed profile system lands.
 */
export class LocalCollectionStore<T extends { id: string }> {
  private readonly storage = inject(BrowserStorage);
  private readonly _items = signal<T[]>([]);

  readonly items = this._items.asReadonly();

  constructor(
    private readonly storageKey: string,
    seed: T[],
  ) {
    this._items.set(this.storage.getJson<T[]>(storageKey, 'local') ?? seed);
  }

  add(item: T): void {
    this._items.update((list) => [...list, item]);
    this.persist();
  }

  update(id: string, patch: Partial<T>): void {
    this._items.update((list) =>
      list.map((item) => (item.id === id ? { ...item, ...patch } : item)),
    );
    this.persist();
  }

  remove(id: string): void {
    this._items.update((list) => list.filter((item) => item.id !== id));
    this.persist();
  }

  private persist(): void {
    this.storage.writeJson(this.storageKey, this._items(), 'local');
  }
}
