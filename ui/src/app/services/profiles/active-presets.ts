import { Injectable, computed, inject, signal } from '@angular/core';
import type { SettingContractId } from '../../models/setting-contract';
import type { SelectOption } from '../../ui/select/select';
import { BrowserStorage } from '../browser-storage';
import { FilamentsStore } from './filaments-store';
import { PrintProfilesStore } from './print-profiles-store';
import { PrintersStore } from './printers-store';

const STORAGE_KEY = 'profiles.active';

/** Minimal shape every profile model shares. */
interface NamedPreset {
  id: string;
  name: string;
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

  /** Dropdown options for the given contract. */
  options(contract: SettingContractId): SelectOption[] {
    return this.itemsFor(contract).map((item) => ({ value: item.id, label: item.name }));
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
