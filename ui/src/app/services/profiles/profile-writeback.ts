import { Injectable, computed, inject, signal } from '@angular/core';
import globalSettingsSchema from '../../../schemas/slicer-engine-global-settings-v1.json';
import type {
  FilamentProfile,
  PrinterProfile,
  ProcessProfile,
} from '../../../generated/slicer-engine-ws-client-message-v1';
import { contractForGroup, type SettingContractId } from '../../models/setting-contract';
import { FILAMENT_MATERIAL_LABELS, type FilamentMaterial } from '../../models/filament.model';
import { fieldLabel } from '../../schema-form/models/field-labels';
import { parseSchema } from '../../schema-form/models/schema-parser';
import { SlicerFile } from '../slicer-file';
import { WorkplateSettingsStore } from '../workplate-settings';
import { ActiveSelection } from './active-selection';
import { FilamentsStore } from './filaments-store';
import { PrintProfilesStore } from './print-profiles-store';
import { PrintersStore } from './printers-store';

const SLICING_PARAMS_SCHEMA = {
  ...(globalSettingsSchema.$defs.SlicingParams as Record<string, unknown>),
  $defs: globalSettingsSchema.$defs as Record<string, unknown>,
};

/**
 * Which contract owns each slicer setting, derived once from the same schema
 * and grouping the settings panel uses to sort fields into its Printer /
 * Filament / Process tabs — so a write-back always lands where the panel
 * itself would have shown the field.
 */
const KEY_CONTRACTS: Record<string, SettingContractId> = Object.fromEntries(
  parseSchema(SLICING_PARAMS_SCHEMA).fields.map((field) => [
    field.key,
    contractForGroup(field.group ?? ''),
  ]),
);

/**
 * Settings that may be written as a correction on the *machine* instead of into
 * a profile every machine shares. Read out of the schema, so the engine's
 * `PER_MACHINE_MATERIAL_KEYS` stays the only list of them.
 */
const PER_MACHINE_MATERIAL_KEYS: ReadonlySet<string> = new Set(
  parseSchema(SLICING_PARAMS_SCHEMA)
    .fields.filter((field) => field.perMachineMaterial)
    .map((field) => field.key),
);

/**
 * Where an accepted row is written.
 *
 * `profile` edits the printer / filament / process preset that owns the
 * setting — the answer for anything that is true wherever it is printed.
 * `machine` records it as this printer's correction for the active material
 * family, which is the answer when one value cannot be right across machines.
 */
export type WritebackTarget = 'profile' | 'machine';

/** One overridden setting, ready to be reviewed and written back. */
export interface WritebackRow {
  key: string;
  label: string;
  contract: SettingContractId;
  /** What the setting resolves to from the active preset stack alone. */
  presetValue: unknown;
  /** What the plate currently overrides it to. */
  overrideValue: unknown;
  /** Whether this row may be written as a machine correction at all. */
  machineEligible: boolean;
}

/**
 * Reviews a workplate's override diff against its active presets and writes
 * the accepted deviations back into the printer / filament / process profile
 * that owns them — clearing those keys from the plate's own overrides, since
 * a value that now equals its profile is inherited again, not a deviation.
 *
 * The target profile for a row is always the plate's *currently active*
 * preset for that setting's contract (the same one {@link ActiveSelection}
 * resolves the plate against), matching the "auto-target, no per-row picker"
 * behaviour the write-back dialog presents.
 */
@Injectable({ providedIn: 'root' })
export class ProfileWriteback {
  private readonly slicerFile = inject(SlicerFile);
  private readonly workplateSettings = inject(WorkplateSettingsStore);
  private readonly activeSelection = inject(ActiveSelection);
  private readonly printers = inject(PrintersStore);
  private readonly filaments = inject(FilamentsStore);
  private readonly profiles = inject(PrintProfilesStore);

  /** Every current deviation, paired with its preset value and owning contract. */
  readonly rows = computed<WritebackRow[]>(() => {
    const uuid = this.slicerFile.requestUuid();
    const overrides = this.workplateSettings.settingsFor(uuid).overrides;
    const preset = (this.activeSelection.sliceParams() ?? {}) as Record<string, unknown>;
    return Object.keys(overrides)
      .filter((key) => key in KEY_CONTRACTS)
      .map((key) => ({
        key,
        label: fieldLabel(key),
        contract: KEY_CONTRACTS[key],
        presetValue: preset[key],
        overrideValue: overrides[key],
        machineEligible: PER_MACHINE_MATERIAL_KEYS.has(key),
      }));
  });

  /** Rows left checked in the dialog; reset to "everything accepted" by {@link open}. */
  private readonly _accepted = signal<ReadonlySet<string>>(new Set());
  readonly accepted = this._accepted.asReadonly();

  /**
   * Where each row is headed. Absent = the row's default.
   *
   * An eligible row defaults to `machine` because that is the write that cannot
   * damage anything: correcting this printer leaves every other printer, and
   * every other plate, exactly as it was. Editing the shared filament is the
   * broader claim, so it is the one the user makes deliberately.
   */
  private readonly _targets = signal<ReadonlyMap<string, WritebackTarget>>(new Map());

  /** Arm the dialog for a fresh review: every current row starts accepted. */
  open(): void {
    this._accepted.set(new Set(this.rows().map((row) => row.key)));
    this._targets.set(new Map());
  }

  /** Where this row will be written, falling back to its default. */
  targetFor(key: string): WritebackTarget {
    const chosen = this._targets().get(key);
    if (chosen) {
      return chosen;
    }
    return this.rows().find((row) => row.key === key)?.machineEligible ? 'machine' : 'profile';
  }

  setTarget(key: string, target: WritebackTarget): void {
    this._targets.update((current) => new Map(current).set(key, target));
  }

  /**
   * How a machine correction reads in the dialog — "Voron 2.4 · PLA".
   *
   * Naming both halves is the point: the scope of the write is the pair, and a
   * label that said only the printer would hide that PETG is untouched.
   */
  readonly machineTargetLabel = computed(() => {
    const printer = this.activeSelection.printer();
    const filament = this.activeSelection.filament();
    return printer && filament
      ? `${printer.name} · ${FILAMENT_MATERIAL_LABELS[filament.material]}`
      : '';
  });

  /** Names of the three active presets, for a dialog that says where a row lands. */
  readonly printerName = computed(() => this.activeSelection.printer()?.name ?? '');
  readonly filamentName = computed(() => this.activeSelection.filament()?.name ?? '');
  readonly processName = computed(() => this.activeSelection.profile()?.name ?? '');

  isAccepted(key: string): boolean {
    return this._accepted().has(key);
  }

  setAccepted(key: string, value: boolean): void {
    this._accepted.update((current) => {
      const next = new Set(current);
      if (value) {
        next.add(key);
      } else {
        next.delete(key);
      }
      return next;
    });
  }

  /**
   * Write every accepted row into its owning profile's own `params`, and drop
   * those keys from the workplate's overrides.
   */
  apply(): void {
    const accepted = this._accepted();
    const rows = this.rows().filter((row) => accepted.has(row.key));
    if (rows.length === 0) {
      return;
    }

    // Split first: a row headed for the machine must not also land in the
    // profile it would otherwise have edited, or the correction would be
    // written twice with different scopes.
    const toMachine = rows.filter((row) => this.targetFor(row.key) === 'machine');
    const toProfile = rows.filter((row) => this.targetFor(row.key) !== 'machine');

    const printer = this.activeSelection.printer();
    const filament = this.activeSelection.filament();

    const printerPatch: Partial<PrinterProfile> = {};
    const printerParams = this.buildPatch(toProfile, 'printer', printer.params);
    if (printerParams) {
      printerPatch.params = printerParams;
    }
    if (toMachine.length > 0) {
      printerPatch.material_overlays = this.buildOverlays(toMachine, printer, filament.material);
    }
    if (Object.keys(printerPatch).length > 0) {
      this.printers.update(printer.id, printerPatch);
    }

    const filamentPatch = this.buildPatch(toProfile, 'filament', filament.params);
    if (filamentPatch) {
      this.filaments.update(filament.id, { params: filamentPatch } as Partial<FilamentProfile>);
    }

    const profile = this.activeSelection.profile();
    const processPatch = this.buildPatch(toProfile, 'process', profile.params);
    if (processPatch) {
      this.profiles.update(profile.id, { params: processPatch } as Partial<ProcessProfile>);
    }

    const uuid = this.slicerFile.requestUuid();
    const remaining = { ...this.workplateSettings.settingsFor(uuid).overrides };
    for (const row of rows) {
      delete remaining[row.key];
    }
    this.workplateSettings.setOverrides(uuid, remaining);
    this._accepted.set(new Set());
    this._targets.set(new Map());
  }

  /**
   * This printer's corrections with `rows` folded into the active material's
   * entry — the other material families untouched.
   *
   * Merged rather than replaced: a machine usually has more than one material
   * corrected, and a user fixing PLA's flow must not lose what they already
   * recorded about ABS.
   */
  private buildOverlays(
    rows: WritebackRow[],
    printer: PrinterProfile,
    material: FilamentMaterial,
  ): Record<string, Record<string, unknown>> {
    const existing = (printer.material_overlays ?? {}) as Record<string, Record<string, unknown>>;
    const forMaterial = { ...(existing[material] ?? {}) };
    for (const row of rows) {
      forMaterial[row.key] = row.overrideValue;
    }
    return { ...existing, [material]: forMaterial };
  }

  /** Merge the accepted rows for one contract onto its profile's existing `params`. */
  private buildPatch(
    rows: WritebackRow[],
    contract: SettingContractId,
    currentParams: unknown,
  ): Record<string, unknown> | null {
    const relevant = rows.filter((row) => row.contract === contract);
    if (relevant.length === 0) {
      return null;
    }
    const patch: Record<string, unknown> = {
      ...((currentParams as Record<string, unknown>) ?? {}),
    };
    for (const row of relevant) {
      patch[row.key] = row.overrideValue;
    }
    return patch;
  }
}
