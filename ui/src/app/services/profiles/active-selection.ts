import { Injectable, computed, inject } from '@angular/core';
import type { SlicingParams } from '../../../generated/slicer-engine-ws-client-message-v1';
import { DEFAULT_SETTINGS } from '../../models/slice-settings.model';
import { printerBedConfig } from '../../models/printer.model';
import { ActivePresets } from './active-presets';
import { FilamentsStore } from './filaments-store';
import { PrintProfilesStore } from './print-profiles-store';
import { PrintersStore } from './printers-store';

/**
 * Turns the *active* printer / filament / print profile into the live bed
 * config and composed {@link SlicingParams}, and exposes convenience accessors
 * used by the settings pages.
 *
 * Selection itself is owned by {@link ActivePresets} (the same store the slice
 * sidebar drives), so there is a single source of truth for "which preset is
 * active" — picking one in the sidebar and in Settings stays in sync. This
 * service is purely derived: it adds the non-null accessors plus the bed /
 * slice-param mapping that {@link ActivePresets} deliberately leaves out.
 *
 * It intentionally injects only the stores and {@link ActivePresets} — never
 * {@link Slicer} or {@link PrintArea} — so opening Settings never boots the
 * slicer runtime. Applying the derived values to the live slice is done by the
 * slice workspace shell.
 */
@Injectable({ providedIn: 'root' })
export class ActiveSelection {
  private readonly presets = inject(ActivePresets);
  private readonly printers = inject(PrintersStore);
  private readonly filaments = inject(FilamentsStore);
  private readonly profiles = inject(PrintProfilesStore);

  /**
   * Active preset objects. Non-null: a `builtin` default is always seeded, so
   * the fallback to the first stored entry can never be empty in practice.
   */
  readonly printer = computed(() => this.presets.activePrinter() ?? this.printers.items()[0]!);
  readonly filament = computed(() => this.presets.activeFilament() ?? this.filaments.items()[0]!);
  readonly profile = computed(() => this.presets.activeProfile() ?? this.profiles.items()[0]!);

  /** Bed dimensions for the active printer, for {@link PrintArea}. */
  readonly bedConfig = computed(() => {
    const printer = this.printer();
    return printer ? printerBedConfig(printer) : null;
  });

  /**
   * Resolved baseline slice params for the active profile stack — the same
   * plain merge the engine performs (`default → printer → filament → process`),
   * with **no field mapping**: every profile's `params` is already a partial
   * `SlicingParams`. User deviations on top are tracked separately and sent as
   * the override diff; the engine is the authority at slice time.
   */
  readonly sliceParams = computed<Partial<SlicingParams> | null>(() => {
    const printer = this.printer();
    const filament = this.filament();
    const profile = this.profile();
    if (!printer || !filament || !profile) {
      return null;
    }
    return {
      ...DEFAULT_SETTINGS,
      ...((printer.params as Record<string, unknown>) ?? {}),
      ...((filament.params as Record<string, unknown>) ?? {}),
      ...((profile.params as Record<string, unknown>) ?? {}),
    } as Partial<SlicingParams>;
  });

  selectPrinter(id: string): void {
    this.presets.select('printer', id);
  }

  selectFilament(id: string): void {
    this.presets.select('filament', id);
  }

  selectProfile(id: string): void {
    this.presets.select('process', id);
  }
}
