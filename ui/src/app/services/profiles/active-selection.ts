import { Injectable, computed, inject } from '@angular/core';
import type { SlicingParams } from '../../../generated/slicer-engine-ws-client-message-v1';
import { DEFAULT_SETTINGS } from '../../models/slice-settings.model';
import { MATERIAL_WIRE_NAME } from '../../models/filament.model';
import { printerBedConfig, printerSceneBedConfig } from '../../models/printer.model';
import type { SceneBedSnapshot } from '../scene-engine';
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
  readonly printAreaConfig = computed(() => {
    const printer = this.printer();
    return printer ? printerBedConfig(printer) : null;
  });

  /** Full bed config for the scene engine's bed-aware operations. */
  readonly sceneBedConfig = computed<SceneBedSnapshot | null>(() => {
    const printer = this.printer();
    return printer ? printerSceneBedConfig(printer) : null;
  });

  /**
   * Resolved baseline slice params for the active profile stack — the same
   * plain merge the engine performs (`default → printer → filament → process`),
   * with **no field mapping**: every profile's `params` is already a partial
   * `SlicingParams`. User deviations on top are tracked separately and sent as
   * the override diff; the engine is the authority at slice time.
   *
   * Identity fields (`filament_type`/`filament_name`/`filament_color`/
   * `printer_vendor`/`printer_model`) are stamped from the *chosen* profiles
   * afterwards, same as `resolve.rs` — the desktop bridge slices from this
   * flattened object directly (no server-side re-resolve), so a filament
   * profile with no `filament_type` in its `params` blob must not leave
   * `{filament_type}` substituting to an empty string in custom start G-code
   * (Klippain / Klipper `MATERIAL=`).
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
      filament_type: MATERIAL_WIRE_NAME[filament.material],
      filament_name: filament.name,
      filament_color: filament.color,
      printer_vendor: printer.vendor,
      printer_model: printer.model,
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
