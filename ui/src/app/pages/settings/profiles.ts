import {
  ChangeDetectionStrategy,
  Component,
  afterNextRender,
  computed,
  inject,
  signal,
} from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { type PrintProfile } from '../../models/print-profile.model';
import { PROFILE_SOURCE_LABELS } from '../../models/profile-source';
import { SETTING_CONTRACTS } from '../../models/setting-contract';
import globalSettingsSchema from '../../../schemas/slicer-engine-global-settings-v1.json';
import { parseSchema } from '../../schema-form/models/schema-parser';
import type { SchemaGroup } from '../../schema-form/models/field-def';
import { CloudCatalog } from '../../services/catalog/cloud-catalog';
import { ContextMenuService } from '../../services/context-menu/context-menu.service';
import { ContextMenuTrigger } from '../../services/context-menu/context-menu-trigger';
import type { ContextMenuItem } from '../../services/context-menu/context-menu.model';
import { Dialog } from '../../services/dialog';
import { ActiveSelection } from '../../services/profiles/active-selection';
import { matchesAllLabels, toggledLabelIds } from '../../services/profiles/label-filtering';
import { paramNum } from '../../models/params-access';
import { LabelFilterStore } from '../../services/profiles/label-filter-store';
import { LabelsStore } from '../../services/profiles/labels-store';
import { PrintProfilesStore } from '../../services/profiles/print-profiles-store';
import { Icon } from '../../shared/icon/icon';
import { Badge } from '../../shared/badge/badge';
import { CatalogPicker, type CatalogEntryVm } from '../../components/profiles/catalog-picker';
import { ParamField } from '../../components/profiles/param-field';
import { LabelFilterBar } from '../../components/labels/label-filter-bar';
import { LabelPicker } from '../../components/labels/label-picker';
import { focusConfigureTarget } from './configure-scroll';
import { Button } from '../../ui/button/button';
import { EmptyState } from '../../ui/empty-state/empty-state';
import { FieldRow } from '../../ui/field-row/field-row';
import { IconButton } from '../../ui/icon-button/icon-button';
import { ModalShell } from '../../ui/modal-shell/modal-shell';
import { SectionHeader } from '../../ui/section-header/section-header';
import { Segmented } from '../../ui/segmented/segmented';

/**
 * The `SlicingParams` sub-schema extracted from the generated global-settings
 * schema, so the profile editor can render every process parameter dynamically
 * (the same schema the slice-page settings sidebar consumes). Any new
 * `SlicingParams` field appears automatically — no hand-maintained field list.
 */
const SLICING_PARAMS_SCHEMA = {
  ...(globalSettingsSchema.$defs.SlicingParams as Record<string, unknown>),
  $defs: globalSettingsSchema.$defs as Record<string, unknown>,
};

/** `x-group` names owned by the Process contract, in display order. */
const PROCESS_GROUPS = SETTING_CONTRACTS.find((c) => c.id === 'process')!.groups;

/**
 * The process-parameter groups rendered in the editor, in the Process
 * contract's display order. Parsed once from the schema (it never changes at
 * runtime); groups owned by other contracts (Hardware, Temperature, …) are
 * left out so the print-profile editor only shows process settings.
 */
const PARAM_GROUPS: SchemaGroup[] = (() => {
  const order = new Map(PROCESS_GROUPS.map((name, index) => [name, index]));
  return parseSchema(SLICING_PARAMS_SCHEMA)
    .groups.filter((g) => order.has(g.name))
    .sort((a, b) => order.get(a.name)! - order.get(b.name)!);
})();

@Component({
  selector: 'nexus-settings-profiles',
  imports: [
    SectionHeader,
    EmptyState,
    Button,
    IconButton,
    Icon,
    Badge,
    RouterLink,
    CatalogPicker,
    ModalShell,
    FieldRow,
    ParamField,
    Segmented,
    LabelFilterBar,
    LabelPicker,
    ContextMenuTrigger,
  ],
  templateUrl: './profiles.html',
  styleUrl: './profiles.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ProfilesSettings {
  protected readonly store = inject(PrintProfilesStore);
  protected readonly active = inject(ActiveSelection);
  protected readonly labels = inject(LabelsStore);
  private readonly filterStore = inject(LabelFilterStore);
  private readonly catalog = inject(CloudCatalog);
  private readonly contextMenu = inject(ContextMenuService);
  private readonly dialog = inject(Dialog);
  private readonly route = inject(ActivatedRoute);

  protected readonly sourceLabels = PROFILE_SOURCE_LABELS;

  /**
   * Flat, sticky process-parameter sections rendered in the editor. Every
   * field is always shown here — a profile is where you *author* presets (e.g.
   * dialling in support settings you keep off by default), so unlike the live
   * slice sidebar the editor never hides gated-off fields.
   */
  protected readonly paramGroups = PARAM_GROUPS;

  protected readonly groupByOptions = [
    { value: 'label', label: 'Labels' },
    { value: 'none', label: 'None' },
  ];

  protected readonly catalogOpen = signal(false);
  /** Which profile's editor is open in the detail pane. */
  protected readonly selectedId = signal<string | null>(this.active.profile()?.id ?? null);
  protected readonly search = signal('');
  protected readonly groupBy = signal<'label' | 'none'>('label');
  protected readonly labelFilter = this.filterStore.selectedIds;

  /** Typed-name delete challenge state (high-impact delete — design language). */
  protected readonly deleteArmed = signal(false);
  protected readonly deleteText = signal('');

  /** Print profiles narrowed by the active label filter and the search query. */
  protected readonly filtered = computed(() => {
    const q = this.search().trim().toLowerCase();
    return this.store
      .items()
      .filter(
        (p) => matchesAllLabels(p, this.labelFilter()) && (!q || p.name.toLowerCase().includes(q)),
      );
  });

  /** The filtered profiles bucketed into titled groups per the group-by mode. */
  protected readonly groups = computed<{ key: string; title: string; items: PrintProfile[] }[]>(
    () => {
      const items = this.filtered();
      if (this.groupBy() === 'none') {
        return [{ key: 'all', title: '', items: sortByName(items) }];
      }
      // Group by label.
      const groups: { key: string; title: string; items: PrintProfile[] }[] = [];
      for (const label of this.labels.items()) {
        const members = items.filter((p) => p.label_ids?.includes(label.id));
        if (members.length) {
          groups.push({ key: label.id, title: label.name, items: sortByName(members) });
        }
      }
      const unlabeled = items.filter((p) => !p.label_ids?.length);
      if (unlabeled.length) {
        groups.push({ key: '__none', title: 'Unlabeled', items: sortByName(unlabeled) });
      }
      return groups;
    },
  );

  protected readonly selected = computed(() => {
    const id = this.selectedId();
    return id ? (this.store.getById(id) ?? null) : null;
  });

  /** Whether the typed name matches the selected profile's name exactly. */
  protected readonly deleteReady = computed(() => {
    const p = this.selected();
    return !!p && this.deleteText().trim() === p.name.trim();
  });

  constructor() {
    // Arriving from the wizard's "Add & configure": open the new profile and
    // scroll to the full editor so the user can keep tuning it.
    const configureId = this.route.snapshot.queryParamMap.get('configure');
    if (configureId && this.store.getById(configureId)) {
      this.select(configureId);
      afterNextRender(() => focusConfigureTarget('configure-target'));
    }
  }

  protected setSearch(event: Event): void {
    this.search.set((event.target as HTMLInputElement).value);
  }

  protected clearSearch(): void {
    this.search.set('');
  }

  protected setGroupBy(value: string): void {
    this.groupBy.set(value as 'label' | 'none');
  }

  protected isDefault(id: string): boolean {
    return this.active.profile()?.id === id;
  }

  protected toggleFilter(id: string): void {
    this.filterStore.toggle(id);
  }

  protected clearFilter(): void {
    this.filterStore.clear();
  }

  protected toggleLabel(id: string, labelId: string): void {
    const item = this.store.getById(id);
    if (item) {
      this.store.update(id, { label_ids: toggledLabelIds(item.label_ids, labelId) });
    }
  }

  protected readonly catalogStatus = this.catalog.status;
  protected readonly catalogEntries = computed<CatalogEntryVm[]>(() =>
    this.catalog.profiles().map((p) => {
      const params = (p.params as Record<string, unknown>) ?? {};
      const layer = Number(params['layer_height'] ?? 0);
      const infill = Number(params['infill_density'] ?? 0);
      return {
        id: p.id,
        name: p.name,
        vendor: p.quality ?? 'standard',
        meta: `${layer} mm · ${Math.round(infill * 100)}% infill`,
        icon: 'menu-scale',
        imported: this.store.items().some((item) => item.based_on === p.id),
      };
    }),
  );

  protected infillPct(fraction: number): number {
    return Math.round(fraction * 100);
  }

  protected openCatalog(): void {
    void this.catalog.load();
    this.catalogOpen.set(true);
  }

  protected retryCatalog(): void {
    void this.catalog.load(true);
  }

  protected importFromCatalog(id: string): void {
    const entry = this.catalog.profiles().find((p) => p.id === id);
    if (!entry) {
      return;
    }
    const copy = this.store.importFromCatalog(entry);
    this.active.selectProfile(copy.id);
    this.select(copy.id);
  }

  /** Open a profile in the detail pane. */
  protected select(id: string): void {
    this.selectedId.set(id);
    this.disarmDelete();
  }

  /** Make the selected profile the default used for slicing. */
  protected setDefault(id: string): void {
    this.active.selectProfile(id);
  }

  protected duplicate(id: string): void {
    const copy = this.store.duplicate(id);
    if (copy) {
      this.select(copy.id);
    }
  }

  /** Right-click a profile card: quick actions mirroring the detail-pane buttons. */
  protected onContextMenu(event: MouseEvent, profile: PrintProfile): void {
    const items: ContextMenuItem[] = [
      {
        label: 'Set as default',
        icon: 'star',
        disabled: this.isDefault(profile.id),
        action: () => this.setDefault(profile.id),
      },
      { label: 'Duplicate', icon: 'copy', action: () => this.duplicate(profile.id) },
    ];
    if (profile.source !== 'builtin') {
      items.push({ separator: true, label: '' });
      items.push({
        label: 'Delete\u2026',
        icon: 'trash',
        danger: true,
        action: () => this.confirmDeleteFromContextMenu(profile),
      });
    }
    void this.contextMenu.open(event, items);
  }

  protected toggleDelete(): void {
    if (this.deleteArmed()) {
      this.disarmDelete();
      return;
    }
    this.armDelete();
  }

  protected armDelete(): void {
    this.deleteArmed.set(true);
    this.deleteText.set('');
  }

  protected disarmDelete(): void {
    this.deleteArmed.set(false);
    this.deleteText.set('');
  }

  protected setDeleteText(event: Event): void {
    this.deleteText.set((event.target as HTMLInputElement).value);
  }

  /** Delete the selected profile once its name has been typed to confirm. */
  protected confirmDelete(): void {
    const profile = this.selected();
    if (!profile || !this.deleteReady()) {
      return;
    }
    this.deleteProfileById(profile.id);
  }

  private confirmDeleteFromContextMenu(profile: PrintProfile): void {
    this.dialog
      .confirm({
        title: `Delete profile "${profile.name}"?`,
        message: 'This print profile will be permanently deleted.',
        type: 'danger',
        confirmLabel: 'Delete',
      })
      .subscribe((confirmed) => {
        if (!confirmed) {
          return;
        }
        this.deleteProfileById(profile.id);
      });
  }

  private deleteProfileById(id: string): void {
    this.store.remove(id);
    this.disarmDelete();
    if (this.selectedId() === id) {
      this.selectedId.set(this.store.items()[0]?.id ?? null);
    }
  }

  protected readonly pnum = paramNum;

  protected update(id: string, patch: Partial<PrintProfile>): void {
    this.store.update(id, patch);
  }

  /** Merge a partial `SlicingParams` into a stored profile's `params` bundle. */
  protected updateParams(id: string, patch: Record<string, unknown>): void {
    const item = this.store.getById(id);
    if (item) {
      this.store.update(id, {
        params: { ...((item.params as Record<string, unknown>) ?? {}), ...patch },
      });
    }
  }

  /** A profile's `params` bag as a plain record for the field controls. */
  protected paramsOf(profile: PrintProfile): Record<string, unknown> {
    return (profile.params as Record<string, unknown>) ?? {};
  }

  /**
   * Sibling values for a process field's cross-contract notices: this profile's
   * own params over the **active printer's and filament's**, matching the
   * engine's `printer → filament → process` merge order.
   *
   * A process setting can depend on the machine or the material, and both live
   * on profiles the user is not currently looking at — see
   * `ui-design-language.instructions.md`, "Cross-contract dependencies".
   */
  protected siblingsFor(profile: PrintProfile): Record<string, unknown> {
    return {
      ...((this.active.printer()?.params as Record<string, unknown>) ?? {}),
      ...((this.active.filament()?.params as Record<string, unknown>) ?? {}),
      ...this.paramsOf(profile),
    };
  }

  /** Apply a single param field edit (templates can't build computed keys). */
  protected setParam(id: string, key: string, value: unknown): void {
    this.updateParams(id, { [key]: value });
  }

  protected rename(id: string, event: Event): void {
    const name = (event.target as HTMLInputElement).value.trim();
    if (name) {
      this.store.update(id, { name });
    }
  }
}

/** Case-insensitive sort by display name (non-mutating). */
function sortByName(items: readonly PrintProfile[]): PrintProfile[] {
  return [...items].sort((a, b) => a.name.localeCompare(b.name));
}
