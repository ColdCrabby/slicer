import { Component, computed, inject, signal } from '@angular/core';
import { RouterLink } from '@angular/router';
import globalSettingsSchema from '../../../schemas/slicer-engine-global-settings-v1.json';
import {
  bucketGroupsByContract,
  GROUP_ICONS,
  SETTING_CONTRACTS,
  type SettingContractId,
} from '../../models/setting-contract';
import { parseSchema } from '../../schema-form/models/schema-parser';
import { FieldChangeEvent, SchemaForm } from '../../schema-form/schema-form';
import { BrowserStorage } from '../../services/browser-storage';
import { ActivePresets } from '../../services/profiles/active-presets';
import { LabelFilterStore } from '../../services/profiles/label-filter-store';
import { LabelFilterBar } from '../labels/label-filter-bar';
import { Slicer } from '../../services/slicer';
import { Icon } from '../../shared/icon/icon';
import { IconButton } from '../../ui/icon-button/icon-button';
import { Segmented, type SegmentOption } from '../../ui/segmented/segmented';
import { Select } from '../../ui/select/select';

// Extract the SlicingParams sub-schema so the form renders all slicer settings.
// (`SlicingParams` is now the wire-format type — the legacy `WsSlicingParams`
// has been collapsed into a Rust type alias for it, so every form field
// reaches the slicer pipeline as-is.)
const SLICING_PARAMS_SCHEMA = {
  ...(globalSettingsSchema.$defs.SlicingParams as Record<string, unknown>),
  $defs: globalSettingsSchema.$defs as Record<string, unknown>,
};

const CONTRACT_STORAGE_KEY = 'settings-panel.contract';

/**
 * Slice-page sidebar. Categorises the flat slicer parameters by *contract*
 * (Printer / Filament / Process) the way established slicers do: a tab switches
 * contract, a preset dropdown selects the active printer / filament / print
 * profile for that contract, and the schema form below shows only that
 * contract's parameter groups. Global settings search still spans everything.
 */
@Component({
  selector: 'nexus-settings-panel',
  standalone: true,
  imports: [SchemaForm, Segmented, Select, Icon, IconButton, RouterLink, LabelFilterBar],
  templateUrl: './settings-panel.component.html',
  styleUrl: './settings-panel.component.scss',
})
export class SettingsPanel {
  private readonly slicer = inject(Slicer);
  private readonly storage = inject(BrowserStorage);
  protected readonly presets = inject(ActivePresets);
  protected readonly labelFilter = inject(LabelFilterStore);

  readonly settings = this.slicer.settings;
  readonly schema = SLICING_PARAMS_SCHEMA;
  protected readonly groupIcons = GROUP_ICONS;

  protected readonly contractTabs: SegmentOption[] = SETTING_CONTRACTS.map((contract) => ({
    value: contract.id,
    label: contract.label,
    icon: contract.icon,
    description: `${contract.label} settings`,
  }));

  protected readonly activeContract = signal<SettingContractId>(
    this.storage.getJson<SettingContractId>(CONTRACT_STORAGE_KEY, 'local') ?? 'process',
  );

  protected readonly activeContractMeta = computed(
    () => SETTING_CONTRACTS.find((c) => c.id === this.activeContract())!,
  );

  private readonly groupsByContract = computed(() =>
    bucketGroupsByContract(parseSchema(this.schema).groups.map((g) => g.name)),
  );

  /** Group names shown for the active contract. */
  protected readonly activeGroups = computed(() => this.groupsByContract()[this.activeContract()]);

  /** Preset dropdown options + current selection for the active contract. */
  protected readonly presetOptions = computed(() => this.presets.options(this.activeContract()));
  protected readonly activePresetId = computed(() =>
    this.presets.selectedId(this.activeContract()),
  );

  setContract(id: string): void {
    this.activeContract.set(id as SettingContractId);
    this.storage.writeJson(CONTRACT_STORAGE_KEY, id, 'local');
  }

  selectPreset(id: string): void {
    this.presets.select(this.activeContract(), id);
  }

  update(event: FieldChangeEvent): void {
    this.slicer.updateSettings({ [event.key]: event.value });
  }
}
