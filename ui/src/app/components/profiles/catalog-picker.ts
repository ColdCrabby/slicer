import { ChangeDetectionStrategy, Component, computed, input, output, signal } from '@angular/core';
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

/**
 * Cloud catalog browser. Renders a searchable list of vendor presets and emits
 * the chosen entry's id. Purely presentational — the parent owns loading, the
 * VM mapping, and what "pick" does (import, or seed a wizard draft).
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

  protected readonly query = signal('');

  protected readonly filtered = computed(() => {
    const q = this.query().trim().toLowerCase();
    const list = this.entries();
    if (!q) {
      return list;
    }
    return list.filter(
      (e) =>
        e.name.toLowerCase().includes(q) ||
        e.vendor.toLowerCase().includes(q) ||
        e.meta.toLowerCase().includes(q),
    );
  });

  protected onSearch(event: Event): void {
    this.query.set((event.target as HTMLInputElement).value);
  }
}
