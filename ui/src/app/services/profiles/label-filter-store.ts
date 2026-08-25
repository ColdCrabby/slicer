import { Injectable, computed, inject, signal } from '@angular/core';
import { BrowserStorage } from '../browser-storage';
import { toggledFilter } from './label-filtering';

const STORAGE_KEY = 'profiles.labelFilter';

/**
 * The single, app-wide set of labels the user is filtering by. Persisted so the
 * choice survives reloads and is shared by every surface that filters profiles
 * — the three settings lists and the slice-page preset dropdowns all read and
 * write this one store, so a filter set in the sidebar is reflected in Settings
 * and vice-versa.
 *
 * Filtering uses AND semantics (see {@link ./label-filtering}); this store owns
 * only the selected-id set, not the matching logic.
 */
@Injectable({ providedIn: 'root' })
export class LabelFilterStore {
  private readonly storage = inject(BrowserStorage);
  private readonly _selectedIds = signal<string[]>(
    this.storage.getJson<string[]>(STORAGE_KEY, 'local') ?? [],
  );

  readonly selectedIds = this._selectedIds.asReadonly();
  readonly hasSelection = computed(() => this._selectedIds().length > 0);

  toggle(id: string): void {
    this._selectedIds.update((ids) => toggledFilter(ids, id));
    this.persist();
  }

  clear(): void {
    this._selectedIds.set([]);
    this.persist();
  }

  private persist(): void {
    this.storage.writeJson(STORAGE_KEY, this._selectedIds(), 'local');
  }
}
