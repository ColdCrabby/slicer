import { Injectable, computed, inject, signal } from '@angular/core';
import type { SlicingParams } from '../../../generated/slicer-engine-ws-client-message-v1';
import { filamentSliceParams } from '../../models/filament.model';
import { printProfileSliceParams } from '../../models/print-profile.model';
import { printerBedConfig, printerSliceParams } from '../../models/printer.model';
import { BrowserStorage } from '../browser-storage';
import { FilamentsStore } from './filaments-store';
import { PrintProfilesStore } from './print-profiles-store';
import { PrintersStore } from './printers-store';

interface SelectionState {
  printerId: string | null;
  filamentId: string | null;
  profileId: string | null;
}

const STORAGE_KEY = 'profiles.selection.v2';

/**
 * Single source of truth for *which* printer / filament / print profile is
 * active, and the derived {@link SlicingParams} / bed config that choice
 * produces.
 *
 * Intentionally dependency-light: it injects only the three profile stores and
 * storage. It does **not** touch {@link Slicer} or {@link PrintArea} — applying
 * the selection to the live slice is done by the slice workspace (see
 * `NexusSlicingShell`) so opening Settings never boots the slicer runtime.
 */
@Injectable({ providedIn: 'root' })
export class ActiveSelection {
  private readonly storage = inject(BrowserStorage);
  private readonly printers = inject(PrintersStore);
  private readonly filaments = inject(FilamentsStore);
  private readonly profiles = inject(PrintProfilesStore);

  private readonly _selection = signal<SelectionState>(
    this.storage.getJson<SelectionState>(STORAGE_KEY, 'local') ?? {
      printerId: null,
      filamentId: null,
      profileId: null,
    },
  );

  /** Active printer — falls back to the first available entry. */
  readonly printer = computed(
    () => this.resolve(this.printers.items(), this._selection().printerId)!,
  );
  readonly filament = computed(
    () => this.resolve(this.filaments.items(), this._selection().filamentId)!,
  );
  readonly profile = computed(
    () => this.resolve(this.profiles.items(), this._selection().profileId)!,
  );

  /** Bed dimensions for the active printer, for {@link PrintArea}. */
  readonly bedConfig = computed(() => {
    const printer = this.printer();
    return printer ? printerBedConfig(printer) : null;
  });

  /** Composed slice-param patch from printer + filament + print profile. */
  readonly sliceParams = computed<Partial<SlicingParams> | null>(() => {
    const printer = this.printer();
    const filament = this.filament();
    const profile = this.profile();
    if (!printer || !filament || !profile) {
      return null;
    }
    return {
      ...printerSliceParams(printer),
      ...filamentSliceParams(filament),
      ...printProfileSliceParams(profile),
    };
  });

  selectPrinter(id: string): void {
    this.patch({ printerId: id });
  }

  selectFilament(id: string): void {
    this.patch({ filamentId: id });
  }

  selectProfile(id: string): void {
    this.patch({ profileId: id });
  }

  private patch(patch: Partial<SelectionState>): void {
    this._selection.update((s) => ({ ...s, ...patch }));
    this.storage.writeJson(STORAGE_KEY, this._selection(), 'local');
  }

  private resolve<T extends { id: string }>(items: readonly T[], id: string | null): T | null {
    if (items.length === 0) {
      return null;
    }
    return items.find((item) => item.id === id) ?? items[0];
  }
}
