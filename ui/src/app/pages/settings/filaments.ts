import {
  ChangeDetectionStrategy,
  Component,
  afterNextRender,
  computed,
  inject,
  signal,
} from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';
import {
  FILAMENT_MATERIAL_LABELS,
  FILAMENT_MATERIALS,
  MATERIAL_DENSITY,
  MATERIAL_PARAMS,
  type FilamentMaterial,
  type FilamentProfile,
} from '../../models/filament.model';
import { PROFILE_SOURCE_LABELS } from '../../models/profile-source';
import { SETTING_CONTRACTS } from '../../models/setting-contract';
import globalSettingsSchema from '../../../schemas/slicer-engine-global-settings-v1.json';
import { parseSchema } from '../../schema-form/models/schema-parser';
import type { SchemaGroup } from '../../schema-form/models/field-def';
import { CloudCatalog } from '../../services/catalog/cloud-catalog';
import { ContextMenuService } from '../../services/context-menu/context-menu.service';
import type { ContextMenuItem } from '../../services/context-menu/context-menu.model';
import { Dialog } from '../../services/dialog';
import { ActiveSelection } from '../../services/profiles/active-selection';
import { matchesAllLabels, toggledLabelIds } from '../../services/profiles/label-filtering';
import { paramNum } from '../../models/params-access';
import { LabelFilterStore } from '../../services/profiles/label-filter-store';
import { LabelsStore } from '../../services/profiles/labels-store';
import { FilamentsStore } from '../../services/profiles/filaments-store';
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
import { NumberInput } from '../../ui/number-input/number-input';
import { SectionHeader } from '../../ui/section-header/section-header';
import { Segmented } from '../../ui/segmented/segmented';
import { Select } from '../../ui/select/select';
import { ColorPicker } from '../../ui/color-picker/color-picker';

/**
 * The `SlicingParams` sub-schema extracted from the generated global-settings
 * schema, so the filament editor can render every temperature/cooling parameter
 * dynamically (the same schema the slice-page settings sidebar consumes). Any
 * new `SlicingParams` field in those groups appears automatically — no
 * hand-maintained field list.
 */
const SLICING_PARAMS_SCHEMA = {
  ...(globalSettingsSchema.$defs.SlicingParams as Record<string, unknown>),
  $defs: globalSettingsSchema.$defs as Record<string, unknown>,
};

/** `x-group` names owned by the Filament contract, in display order. */
const FILAMENT_GROUPS = SETTING_CONTRACTS.find((c) => c.id === 'filament')!.groups;

/**
 * The filament-parameter groups rendered in the editor, in the Filament
 * contract's display order (`Temperature`, `Cooling`). Parsed once from the
 * schema (it never changes at runtime); groups owned by other contracts
 * (Hardware, Extrusion, …) are left out so the filament editor only shows
 * material temperature/cooling settings.
 *
 * Each group's fields are filtered to those `nexus-param-field` can actually
 * render (enum → select, boolean → switch, everything else → number). Plain
 * `string` and array fields are dropped, which excludes `filament_type` and
 * `fan_configs` automatically. `filament_diameter_mm` (Hardware) stays a
 * bespoke "Diameter" row under Identity, so no param key renders twice.
 */
const PARAM_GROUPS: SchemaGroup[] = (() => {
  const order = new Map(FILAMENT_GROUPS.map((name, index) => [name, index]));
  return parseSchema(SLICING_PARAMS_SCHEMA)
    .groups.filter((g) => order.has(g.name))
    .map((g) => ({
      ...g,
      fields: g.fields.filter(
        (f) =>
          !!f.enumOptions?.length ||
          f.type === 'boolean' ||
          f.type === 'number' ||
          f.type === 'integer',
      ),
    }))
    .filter((g) => g.fields.length > 0)
    .sort((a, b) => order.get(a.name)! - order.get(b.name)!);
})();

@Component({
  selector: 'nexus-settings-filaments',
  imports: [
    SectionHeader,
    EmptyState,
    Button,
    IconButton,
    Icon,
    Badge,
    RouterLink,
    CatalogPicker,
    ParamField,
    ModalShell,
    FieldRow,
    NumberInput,
    Select,
    ColorPicker,
    Segmented,
    LabelFilterBar,
    LabelPicker,
  ],
  templateUrl: './filaments.html',
  styleUrl: './filaments.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class FilamentsSettings {
  protected readonly store = inject(FilamentsStore);
  protected readonly active = inject(ActiveSelection);
  protected readonly labels = inject(LabelsStore);
  private readonly filterStore = inject(LabelFilterStore);
  private readonly catalog = inject(CloudCatalog);
  private readonly contextMenu = inject(ContextMenuService);
  private readonly dialog = inject(Dialog);
  private readonly route = inject(ActivatedRoute);

  protected readonly sourceLabels = PROFILE_SOURCE_LABELS;
  protected readonly materialOptions = FILAMENT_MATERIALS.map((m) => ({
    value: m,
    label: FILAMENT_MATERIAL_LABELS[m],
  }));
  protected readonly groupByOptions = [
    { value: 'category', label: 'Material' },
    { value: 'label', label: 'Labels' },
    { value: 'none', label: 'None' },
  ];

  protected readonly catalogOpen = signal(false);
  /** Which filament's editor is open in the detail pane. */
  protected readonly selectedId = signal<string | null>(this.active.filament()?.id ?? null);
  protected readonly search = signal('');
  protected readonly groupBy = signal<'category' | 'label' | 'none'>('category');
  protected readonly labelFilter = this.filterStore.selectedIds;

  /** Typed-name delete challenge state (high-impact delete — design language). */
  protected readonly deleteArmed = signal(false);
  protected readonly deleteText = signal('');

  /** Filaments narrowed by the active label filter and the search query. */
  protected readonly filtered = computed(() => {
    const q = this.search().trim().toLowerCase();
    return this.store
      .items()
      .filter(
        (f) =>
          matchesAllLabels(f, this.labelFilter()) &&
          (!q || `${f.name} ${f.vendor ?? ''} ${f.material}`.toLowerCase().includes(q)),
      );
  });

  /** The filtered filaments bucketed into titled groups per the group-by mode. */
  protected readonly groups = computed<{ key: string; title: string; items: FilamentProfile[] }[]>(
    () => {
      const items = this.filtered();
      switch (this.groupBy()) {
        case 'none':
          return [{ key: 'all', title: '', items: sortByName(items) }];
        case 'label': {
          const groups: { key: string; title: string; items: FilamentProfile[] }[] = [];
          for (const label of this.labels.items()) {
            const members = items.filter((f) => f.label_ids?.includes(label.id));
            if (members.length) {
              groups.push({ key: label.id, title: label.name, items: sortByName(members) });
            }
          }
          const unlabeled = items.filter((f) => !f.label_ids?.length);
          if (unlabeled.length) {
            groups.push({ key: '__none', title: 'Unlabeled', items: sortByName(unlabeled) });
          }
          return groups;
        }
        default: {
          // Group by material, ordered by the canonical material list.
          return FILAMENT_MATERIALS.map((material) => ({
            key: material,
            title: FILAMENT_MATERIAL_LABELS[material],
            items: sortByName(items.filter((f) => f.material === material)),
          })).filter((g) => g.items.length > 0);
        }
      }
    },
  );

  protected readonly selected = computed(() => {
    const id = this.selectedId();
    return id ? (this.store.getById(id) ?? null) : null;
  });

  /** Whether the typed name matches the selected filament's name exactly. */
  protected readonly deleteReady = computed(() => {
    const f = this.selected();
    return !!f && this.deleteText().trim() === f.name.trim();
  });

  constructor() {
    // Arriving from the wizard's "Add & configure": open the new filament and
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
    this.groupBy.set(value as 'category' | 'label' | 'none');
  }

  protected isDefault(id: string): boolean {
    return this.active.filament()?.id === id;
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
    this.catalog.filaments().map((f) => ({
      id: f.id,
      name: f.name,
      vendor: f.vendor,
      meta: `${f.material} · ${(f.params as Record<string, unknown>)?.['nozzle_temp']}°C`,
      color: f.color,
      imported: this.store.items().some((item) => item.based_on === f.id),
    })),
  );

  protected openCatalog(): void {
    void this.catalog.load();
    this.catalogOpen.set(true);
  }

  protected retryCatalog(): void {
    void this.catalog.load(true);
  }

  protected importFromCatalog(id: string): void {
    const entry = this.catalog.filaments().find((f) => f.id === id);
    if (!entry) {
      return;
    }
    const copy = this.store.importFromCatalog(entry);
    this.active.selectFilament(copy.id);
    this.select(copy.id);
  }

  /** Open a filament in the detail pane. */
  protected select(id: string): void {
    this.selectedId.set(id);
    this.disarmDelete();
  }

  /** Make the selected filament the default used for slicing. */
  protected setDefault(id: string): void {
    this.active.selectFilament(id);
  }

  protected duplicate(id: string): void {
    const copy = this.store.duplicate(id);
    if (copy) {
      this.select(copy.id);
    }
  }

  /** Right-click a filament card: quick actions mirroring the detail-pane buttons. */
  protected onContextMenu(event: MouseEvent, filament: FilamentProfile): void {
    const items: ContextMenuItem[] = [
      {
        label: 'Set as default',
        icon: 'star',
        disabled: this.isDefault(filament.id),
        action: () => this.setDefault(filament.id),
      },
      { label: 'Duplicate', icon: 'copy', action: () => this.duplicate(filament.id) },
    ];
    if (filament.source !== 'builtin') {
      items.push({ separator: true, label: '' });
      items.push({
        label: 'Delete\u2026',
        icon: 'trash',
        danger: true,
        action: () => this.confirmDeleteFromContextMenu(filament),
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

  /** Delete the selected filament once its name has been typed to confirm. */
  protected confirmDelete(): void {
    const filament = this.selected();
    if (!filament || !this.deleteReady()) {
      return;
    }
    this.deleteFilamentById(filament.id);
  }

  private confirmDeleteFromContextMenu(filament: FilamentProfile): void {
    this.dialog
      .confirm({
        title: `Delete filament "${filament.name}"?`,
        message: 'This filament profile will be permanently deleted.',
        type: 'danger',
        confirmLabel: 'Delete',
      })
      .subscribe((confirmed) => {
        if (!confirmed) {
          return;
        }
        this.deleteFilamentById(filament.id);
      });
  }

  private deleteFilamentById(id: string): void {
    this.store.remove(id);
    this.disarmDelete();
    if (this.selectedId() === id) {
      this.selectedId.set(this.store.items()[0]?.id ?? null);
    }
  }

  protected readonly pnum = paramNum;
  /**
   * Filament-parameter sections rendered in the editor. Every field is always
   * shown — the profile editor is where presets are authored, so it never
   * hides gated-off fields (unlike the live slice sidebar).
   */
  protected readonly paramGroups = PARAM_GROUPS;

  protected update(id: string, patch: Partial<FilamentProfile>): void {
    this.store.update(id, patch);
  }

  /** Merge a partial `SlicingParams` into a stored filament's `params` bundle. */
  protected updateParams(id: string, patch: Record<string, unknown>): void {
    const item = this.store.getById(id);
    if (item) {
      this.store.update(id, {
        params: { ...((item.params as Record<string, unknown>) ?? {}), ...patch },
      });
    }
  }

  /** A filament's `params` bag as a plain record for the field controls. */
  protected paramsOf(filament: FilamentProfile): Record<string, unknown> {
    return (filament.params as Record<string, unknown>) ?? {};
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

  protected setColor(id: string, color: string): void {
    this.store.update(id, { color });
  }

  protected setMaterial(id: string, value: string): void {
    const material = value as FilamentMaterial;
    const current = this.store.getById(id);
    this.store.update(id, {
      material,
      density_g_cm3: MATERIAL_DENSITY[material],
      params: {
        ...((current?.params as Record<string, unknown>) ?? {}),
        ...MATERIAL_PARAMS[material],
      },
    });
  }
}

/** Case-insensitive sort by display name (non-mutating). */
function sortByName(items: readonly FilamentProfile[]): FilamentProfile[] {
  return [...items].sort((a, b) => a.name.localeCompare(b.name));
}
