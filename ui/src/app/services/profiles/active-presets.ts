import { Injectable, computed, inject, signal } from '@angular/core';
import { labelDotColor } from '../../models/label.model';
import type { SettingContractId } from '../../models/setting-contract';
import type { SelectOption } from '../../ui/select/select';
import { BrowserStorage } from '../browser-storage';
import { FilamentsStore } from './filaments-store';
import { LabelFilterStore } from './label-filter-store';
import { matchesAllLabels } from './label-filtering';
import { LabelsStore } from './labels-store';
import { PrintProfilesStore } from './print-profiles-store';
import { PrintersStore } from './printers-store';

const STORAGE_KEY = 'profiles.active';

/** Minimal shape every profile model shares. */
interface NamedPreset {
  id: string;
  name: string;
  labelIds?: string[];
}

type ActiveIds = Record<SettingContractId, string | null>;

/**
 * Tracks which printer / filament / print-profile preset is *active* on the
 * slicing page, persisting the selection to localStorage. This is the seam that
 * merges the mocked profile stores into the main slice sidebar: selecting a
 * preset here records intent (and survives reloads) without yet applying preset
 * values into the live slicer parameters — that larger feature lands later.
 */
@Injectable({ providedIn: 'root' })
export class ActivePresets {
  private readonly printers = inject(PrintersStore);
  private readonly filaments = inject(FilamentsStore);
  private readonly profiles = inject(PrintProfilesStore);
  private readonly labels = inject(LabelsStore);
  private readonly labelFilter = inject(LabelFilterStore);
  private readonly storage = inject(BrowserStorage);

  private readonly ids = signal<ActiveIds>(
    this.storage.getJson<ActiveIds>(STORAGE_KEY, 'local') ?? {
      printer: null,
      filament: null,
      process: null,
    },
  );

  private itemsFor(contract: SettingContractId): readonly NamedPreset[] {
    switch (contract) {
      case 'printer':
        return this.printers.items();
      case 'filament':
        return this.filaments.items();
      case 'process':
        return this.profiles.items();
    }
  }

  /**
   * Dropdown options for the given contract, tagged with label colours and
   * narrowed by the global {@link LabelFilterStore}. The currently-active preset
   * is always kept in the list so the trigger label stays correct even when it
   * doesn't match the active filter.
   */
  options(contract: SettingContractId): SelectOption[] {
    const selected = this.labelFilter.selectedIds();
    const activeId = this.selectedId(contract);
    return this.itemsFor(contract)
      .filter((item) => item.id === activeId || matchesAllLabels(item, selected))
      .map((item) => {
        const swatches = this.labels.resolve(item.labelIds).map((l) => labelDotColor(l));
        return {
          value: item.id,
          label: item.name,
          ...(swatches.length ? { swatches } : {}),
        };
      });
  }

  /**
   * Currently-selected preset id for the contract. Falls back to the first
   * available preset when the stored id is missing (e.g. after a delete).
   */
  selectedId(contract: SettingContractId): string | null {
    const items = this.itemsFor(contract);
    const stored = this.ids()[contract];
    if (stored && items.some((item) => item.id === stored)) {
      return stored;
    }
    return items[0]?.id ?? null;
  }

  select(contract: SettingContractId, id: string): void {
    this.ids.update((current) => ({ ...current, [contract]: id }));
    this.storage.writeJson(STORAGE_KEY, this.ids(), 'local');
  }

  /** Reactive accessors for the currently-active preset objects. */
  readonly activePrinter = computed(() =>
    this.printers.items().find((p) => p.id === this.selectedId('printer')),
  );
  readonly activeFilament = computed(() =>
    this.filaments.items().find((f) => f.id === this.selectedId('filament')),
  );
  readonly activeProfile = computed(() =>
    this.profiles.items().find((p) => p.id === this.selectedId('process')),
  );
}
