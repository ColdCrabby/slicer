import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  inject,
  input,
  output,
  signal,
} from '@angular/core';
import { Icon } from '@coldcrabby/ui';

/**
 * View-model for one catalog entry, decoupled from the concrete profile type so
 * the picker can render printers, filaments, and profiles alike.
 */
export interface CatalogEntryVm {
  id: string;
  name: string;
  vendor: string;
  /** Short right-aligned spec line, e.g. "250×210 · 0.4 mm". */
  meta: string;
  /** Optional swatch color (filaments). */
  color?: string;
  /** Optional icon shown when no color swatch applies. */
  icon?: string;
  /** True when this catalog entry has already been imported locally. */
  imported?: boolean;
}

/** Debounce before a keystroke turns into a server search, in milliseconds. */
const SEARCH_DEBOUNCE_MS = 250;

/**
 * Cloud catalog browser. Renders a list of vendor presets and emits the chosen
 * entry's id. Purely presentational — the parent owns loading and the VM
 * mapping.
 *
 * Search is **server-side**: typing emits a debounced `search` query and the
 * parent re-fetches ranked results. The picker never filters {@link entries}
 * itself, so results reflect the whole catalog, not just the page already in
 * memory.
 */
@Component({
  selector: 'nexus-catalog-picker',
  standalone: true,
  imports: [Icon],
  templateUrl: './catalog-picker.html',
  styleUrl: './catalog-picker.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class CatalogPicker {
  readonly entries = input.required<readonly CatalogEntryVm[]>();
  readonly loading = input(false);
  readonly unavailable = input(false);
  /** Label for the pick action (e.g. "Import", "Use preset"). */
  readonly actionLabel = input('Import');

  readonly pick = output<string>();
  readonly retry = output<void>();
  /** Fuzzy query to re-fetch the catalog for; debounced from keystrokes. */
  readonly search = output<string>();

  protected readonly query = signal('');

  private debounce?: ReturnType<typeof setTimeout>;

  constructor() {
    inject(DestroyRef).onDestroy(() => clearTimeout(this.debounce));
  }

  protected onSearch(event: Event): void {
    const value = (event.target as HTMLInputElement).value;
    this.query.set(value);
    clearTimeout(this.debounce);
    this.debounce = setTimeout(() => this.search.emit(value.trim()), SEARCH_DEBOUNCE_MS);
  }
}
