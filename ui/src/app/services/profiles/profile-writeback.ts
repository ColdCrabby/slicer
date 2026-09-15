import { Injectable, computed, inject, signal } from '@angular/core';
import globalSettingsSchema from '../../../schemas/slicer-engine-global-settings-v1.json';
import type {
  FilamentProfile,
  PrinterProfile,
  ProcessProfile,
} from '../../../generated/slicer-engine-ws-client-message-v1';
import { contractForGroup, type SettingContractId } from '../../models/setting-contract';
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

/** One overridden setting, ready to be reviewed and written back. */
export interface WritebackRow {
  key: string;
  label: string;
  contract: SettingContractId;
  /** What the setting resolves to from the active preset stack alone. */
  presetValue: unknown;
  /** What the plate currently overrides it to. */
  overrideValue: unknown;
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
      }));
  });

  /** Rows left checked in the dialog; reset to "everything accepted" by {@link open}. */
  private readonly _accepted = signal<ReadonlySet<string>>(new Set());
  readonly accepted = this._accepted.asReadonly();

  /** Arm the dialog for a fresh review: every current row starts accepted. */
  open(): void {
    this._accepted.set(new Set(this.rows().map((row) => row.key)));
  }

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

    const printer = this.activeSelection.printer();
    const printerPatch = this.buildPatch(rows, 'printer', printer.params);
    if (printerPatch) {
      this.printers.update(printer.id, { params: printerPatch } as Partial<PrinterProfile>);
    }

    const filament = this.activeSelection.filament();
    const filamentPatch = this.buildPatch(rows, 'filament', filament.params);
    if (filamentPatch) {
      this.filaments.update(filament.id, { params: filamentPatch } as Partial<FilamentProfile>);
    }

    const profile = this.activeSelection.profile();
    const processPatch = this.buildPatch(rows, 'process', profile.params);
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
