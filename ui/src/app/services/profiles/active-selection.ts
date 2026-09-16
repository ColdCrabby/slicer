import { Injectable, computed, inject } from '@angular/core';
import type { SlicingParams } from '../../../generated/slicer-engine-ws-client-message-v1';
import { ENGINE_DEFAULTS } from '../../models/slice-settings.model';
import {
  FILAMENT_MATERIAL_LABELS,
  MATERIAL_WIRE_NAME,
  type FilamentMaterial,
} from '../../models/filament.model';
import {
  materialOverlayOf,
  printerBedConfig,
  printerSceneBedConfig,
  type ParamOrigin,
} from '../../models/printer.model';
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
   * Resolved baseline slice params for the active profile stack — a local
   * mirror of `profiles::resolve`, in the same precedence
   * (`engine defaults → printer → filament → process`) and with **no field
   * mapping**: every profile's `params` is already a partial `SlicingParams`
   * and the defaults come from the engine's own generated schema.
   *
   * It is a mirror, not a second authority. Nothing is sliced from it: it is
   * what the form renders and what a user edit is measured against, so that
   * only genuine deviations become overrides. Every runtime re-resolves the
   * whole stack from the profiles plus that diff at slice time, and its answer
   * wins.
   *
   * Identity fields (`filament_type`/`filament_name`/`filament_color`/
   * `printer_vendor`/`printer_model`) are stamped from the *chosen* profiles
   * last, exactly as `resolve.rs` does. Mirroring that here is what keeps them
   * out of the override diff: a filament whose `params` blob carries no
   * `filament_type` would otherwise read as a deviation on every plate, and
   * the form would show it blank while the engine stamped it anyway.
   */
  readonly sliceParams = computed<Partial<SlicingParams> | null>(() => {
    const printer = this.printer();
    const filament = this.filament();
    const profile = this.profile();
    if (!printer || !filament || !profile) {
      return null;
    }
    return {
      ...ENGINE_DEFAULTS,
      ...((printer.params as Record<string, unknown>) ?? {}),
      ...((filament.params as Record<string, unknown>) ?? {}),
      ...((profile.params as Record<string, unknown>) ?? {}),
      ...materialOverlayOf(printer, filament.material),
      filament_type: MATERIAL_WIRE_NAME[filament.material],
      filament_name: filament.name,
      filament_color: filament.color,
      printer_vendor: printer.vendor,
      printer_model: printer.model,
    } as Partial<SlicingParams>;
  });

  /**
   * Which layer of the stack supplied each resolved setting — the same single
   * pass, in the same precedence, as {@link sliceParams}.
   *
   * Five layers are only comprehensible if the interface can say which one won.
   * Without this, a machine's correction for a material reads as a number that
   * disagrees with the filament the user picked, and looks like a defect.
   *
   * The user's own overrides are not folded in here: the panel already knows
   * them (they are what `modifiedKeys` is), and they are applied on top of this
   * baseline rather than being part of it.
   */
  readonly paramOrigins = computed<ReadonlyMap<string, ParamOrigin>>(() => {
    const printer = this.printer();
    const filament = this.filament();
    const profile = this.profile();
    const origins = new Map<string, ParamOrigin>();
    if (!printer || !filament || !profile) {
      return origins;
    }

    const layers: [Record<string, unknown>, ParamOrigin][] = [
      [ENGINE_DEFAULTS as Record<string, unknown>, 'default'],
      [(printer.params as Record<string, unknown>) ?? {}, 'printer'],
      [(filament.params as Record<string, unknown>) ?? {}, 'filament'],
      [(profile.params as Record<string, unknown>) ?? {}, 'process'],
      [materialOverlayOf(printer, filament.material), 'machine_material'],
    ];
    for (const [params, origin] of layers) {
      for (const key of Object.keys(params)) {
        origins.set(key, origin);
      }
    }

    // Identity fields are stamped from the chosen profiles last, exactly as
    // `resolve.rs` does, so they are attributed to the profile that names them
    // rather than to whichever layer happened to carry the key.
    for (const key of ['filament_type', 'filament_name', 'filament_color']) {
      origins.set(key, 'filament');
    }
    for (const key of ['printer_vendor', 'printer_model']) {
      origins.set(key, 'printer');
    }
    return origins;
  });

  /**
   * How a machine's correction for the active material is described where it
   * shows up — "Voron 2.4 · PLA". `null` when this machine has nothing to say
   * about this material, which is the ordinary case.
   */
  readonly materialOverlayLabel = computed<string | null>(() => {
    const printer = this.printer();
    const filament = this.filament();
    if (!printer || !filament) {
      return null;
    }
    const overlay = materialOverlayOf(printer, filament.material);
    return Object.keys(overlay).length
      ? `${printer.name} · ${FILAMENT_MATERIAL_LABELS[filament.material]}`
      : null;
  });

  /**
   * Ids of the active presets, for the plate to remember its baseline by.
   * A stored override diff is only meaningful against the stack it was
   * measured against.
   */
  readonly presetIds = computed(() => ({
    printer: this.printer()?.id,
    filament: this.filament()?.id,
    process: this.profile()?.id,
  }));

  /** Restore a plate's remembered preset stack; absent ids are left alone. */
  applyPresetIds(ids: { printer?: string; filament?: string; process?: string }): void {
    if (ids.printer) this.presets.select('printer', ids.printer);
    if (ids.filament) this.presets.select('filament', ids.filament);
    if (ids.process) this.presets.select('process', ids.process);
  }

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
