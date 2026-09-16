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
  PRINTER_CONNECTION_KINDS,
  PRINTER_CONNECTION_LABELS,
  PRINTER_GCODE_FLAVORS,
  type BedShape,
  type PrinterConnection,
  type PrinterConnectionKind,
  type PrinterGcodeFlavor,
  type PrinterProfile,
} from '../../models/printer.model';
import { PROFILE_SOURCE_LABELS } from '../../models/profile-source';
import { SETTING_CONTRACTS } from '../../models/setting-contract';
import globalSettingsSchema from '../../../schemas/slicer-engine-global-settings-v1.json';
import { controlFor } from '../../schema-form/models/field-control';
import { parseSchema } from '../../schema-form/models/schema-parser';
import type { FieldDef, SchemaGroup } from '../../schema-form/models/field-def';
import {
  CUSTOM_TEMPLATE_ID,
  GCODE_PLACEHOLDER_HINT,
  GCODE_TEMPLATE_OPTIONS,
  customGcodeTemplatePatch,
  gcodeTemplatePatch,
  gcodeTemplateStatus,
  type GcodeTemplateStatus,
} from '../../models/gcode-templates';
import { CloudCatalog, catalogSpecOf } from '../../services/catalog/cloud-catalog';
import { ContextMenuService } from '../../services/context-menu/context-menu.service';
import { ContextMenuTrigger } from '../../services/context-menu/context-menu-trigger';
import type { ContextMenuItem } from '../../services/context-menu/context-menu.model';
import { Dialog } from '../../services/dialog';
import { NotificationService } from '../../services/notifications';
import { PrinterConnectionService } from '../../services/printer-connection';
import {
  FILAMENT_MATERIALS,
  FILAMENT_MATERIAL_LABELS,
  MATERIAL_PARAMS,
  type FilamentMaterial,
} from '../../models/filament.model';
import { ActiveSelection } from '../../services/profiles/active-selection';
import { matchesAnyLabel, toggledLabelIds } from '../../services/profiles/label-filtering';
import { paramNum, paramStr } from '../../models/params-access';
import { LabelFilterStore } from '../../services/profiles/label-filter-store';
import { LabelsStore } from '../../services/profiles/labels-store';
import { PrintersStore } from '../../services/profiles/printers-store';
import { correctionsFor, withCorrections } from '../../services/profiles/material-corrections';
import {
  Icon,
  Badge,
  Button,
  EmptyState,
  FieldRow,
  IconButton,
  ModalShell,
  NumberInput,
  SectionHeader,
  Segmented,
  Select,
  Switch,
} from '@coldcrabby/ui';
import { CatalogPicker, type CatalogEntryVm } from '../../components/profiles/catalog-picker';
import { FieldShell } from '../../components/profiles/field-shell';
import { ParamField } from '../../components/profiles/param-field';
import { ColumnResizer } from '../../components/profiles/column-resizer';
import { ProfileOutline } from '../../components/profiles/profile-outline';
import { CodeEditor } from '../../components/code-editor/code-editor';
import { LabelFilterBar } from '../../components/labels/label-filter-bar';
import { LabelPicker } from '../../components/labels/label-picker';
import { focusConfigureTarget } from './configure-scroll';
import { LabelPickerPanel } from '../../components/labels/label-picker-panel';

/**
 * The `SlicingParams` sub-schema extracted from the generated global-settings
 * schema, so the printer editor can render its slice-parameter groups
 * dynamically (the same schema the slice-page settings sidebar consumes). Any
 * new `SlicingParams` field appears automatically — no hand-maintained rows.
 */
const SLICING_PARAMS_SCHEMA = {
  ...(globalSettingsSchema.$defs.SlicingParams as Record<string, unknown>),
  $defs: globalSettingsSchema.$defs as Record<string, unknown>,
};

/**
 * `x-group` names schema-driven in the printer editor, in display order.
 *
 * The Printer contract owns `['Hardware', 'Retraction', 'Output']`, but `Output`
 * is left out here: its `gcode_flavor` is already the bespoke "Firmware" select
 * and its `*_gcode` fields are multiline strings edited through the dedicated
 * G-code editor block — both need typed widgets `nexus-param-field` can't
 * provide. So the printer only schema-drives `Hardware` and `Retraction`.
 */
const PRINTER_PARAM_GROUPS = SETTING_CONTRACTS.find((c) => c.id === 'printer')!.groups.filter(
  (name) => name === 'Hardware' || name === 'Retraction',
);

/**
 * Params the printer editor does not offer.
 *
 * `resolve` writes both from the chosen printer profile on every slice, so a
 * box here would accept an edit and then quietly discard it. The machine's
 * vendor and model are shown with its name in the header instead, where they
 * read as what they are: a description of the printer, not a setting.
 */
const DERIVED_PARAM_KEYS = new Set(['printer_vendor', 'printer_model']);

/**
 * The slice-parameter groups rendered in the printer editor, in contract
 * display order. Parsed once from the schema (it never changes at runtime).
 *
 * `nexus-param-field` renders every control in the shared taxonomy except
 * `array` — fan curves and pause triggers have editors of their own — so that
 * and {@link DERIVED_PARAM_KEYS} are all that is filtered out. Groups left with
 * no renderable field are dropped entirely.
 */
/**
 * The settings a machine may correct per material, in schema order.
 *
 * Read from the schema's `x-per-machine-material` annotations, so the engine's
 * `PER_MACHINE_MATERIAL_KEYS` stays the only list of them and a new one appears
 * here with no change.
 */
const CORRECTABLE_FIELDS: FieldDef[] = parseSchema(SLICING_PARAMS_SCHEMA).fields.filter(
  (field) => field.perMachineMaterial,
);

/**
 * The setting a brand-new material correction starts on.
 *
 * Named rather than taken from the head of the list: schema order is the right
 * order for the *picker*, but it is arbitrary as a starting point, and landing
 * someone on a fan ceiling when what they came to correct is flow makes the
 * feature read as the wrong thing. Melt rate is the reason most machines need a
 * correction at all.
 */
const FIRST_CORRECTION_KEY = 'max_volumetric_speed';

/**
 * Heading of the entry row's own section.
 *
 * Named because every material's section is titled "<Material> corrections" and
 * this one would otherwise match the same suffix — jumping to the corrections
 * would land on the control that asked to jump.
 */
/** How long an armed "Remove all" waits before disarming itself. */
const REMOVE_CONFIRM_MS = 4000;

const PARAM_GROUPS: SchemaGroup[] = (() => {
  const order = new Map<string, number>(PRINTER_PARAM_GROUPS.map((name, index) => [name, index]));
  return parseSchema(SLICING_PARAMS_SCHEMA)
    .groups.filter((g) => order.has(g.name))
    .map((g) => ({
      ...g,
      fields: g.fields.filter((f) => !DERIVED_PARAM_KEYS.has(f.key) && controlFor(f) !== 'array'),
    }))
    .filter((g) => g.fields.length > 0)
    .sort((a, b) => (order.get(a.name) ?? 0) - (order.get(b.name) ?? 0));
})();

@Component({
  selector: 'nexus-settings-printers',
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
    FieldShell,
    CodeEditor,
    ModalShell,
    FieldRow,
    NumberInput,
    Select,
    Switch,
    Segmented,
    LabelFilterBar,
    LabelPicker,
    ContextMenuTrigger,
    ProfileOutline,
    ColumnResizer,
  ],
  templateUrl: './printers.html',
  styleUrl: './printers.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class PrintersSettings {
  protected readonly store = inject(PrintersStore);
  protected readonly active = inject(ActiveSelection);
  protected readonly labels = inject(LabelsStore);
  private readonly filterStore = inject(LabelFilterStore);
  private readonly catalog = inject(CloudCatalog);
  private readonly contextMenu = inject(ContextMenuService);
  private readonly dialog = inject(Dialog);
  private readonly notifications = inject(NotificationService);
  private readonly printerConn = inject(PrinterConnectionService);
  private readonly route = inject(ActivatedRoute);

  protected readonly sourceLabels = PROFILE_SOURCE_LABELS;
  protected readonly flavorOptions = PRINTER_GCODE_FLAVORS;
  protected readonly gcodeTemplateOptions = GCODE_TEMPLATE_OPTIONS;
  protected readonly gcodePlaceholderHint = GCODE_PLACEHOLDER_HINT;
  protected readonly connectionKindOptions = PRINTER_CONNECTION_KINDS.map((kind) => ({
    value: kind,
    label: PRINTER_CONNECTION_LABELS[kind],
  }));
  protected readonly bedShapeOptions = [
    { value: 'rectangular', label: 'Rectangular' },
    { value: 'circular', label: 'Circular (delta)' },
  ];
  protected readonly groupByOptions = [
    { value: 'category', label: 'Vendor' },
    { value: 'label', label: 'Labels' },
    { value: 'none', label: 'None' },
  ];

  protected readonly catalogOpen = signal(false);
  /** Which printer's editor is open in the detail pane. */
  protected readonly selectedId = signal<string | null>(this.active.printer()?.id ?? null);
  protected readonly search = signal('');
  protected readonly groupBy = signal<'category' | 'label' | 'none'>('category');
  protected readonly labelFilter = this.filterStore.selectedIds;

  /**
   * Inline two-step delete — the design language's default for a routine
   * destructive action. This used to be a typed-name challenge, which is
   * reserved for irreversible data loss; a profile is a handful of settings the
   * user can recreate, and typing its name out to remove one was friction
   * without a matching risk.
   */
  protected readonly deleteArmed = signal(false);

  /** Printers narrowed by the active label filter and the search query. */
  protected readonly filtered = computed(() => {
    const q = this.search().trim().toLowerCase();
    return this.store
      .items()
      .filter(
        (p) =>
          matchesAnyLabel(p, this.labelFilter()) &&
          (!q || `${p.name} ${p.vendor ?? ''} ${p.model ?? ''}`.toLowerCase().includes(q)),
      );
  });

  /** The filtered printers bucketed into titled groups per the group-by mode. */
  protected readonly groups = computed<{ key: string; title: string; items: PrinterProfile[] }[]>(
    () => {
      const items = this.filtered();
      switch (this.groupBy()) {
        case 'none':
          return [{ key: 'all', title: '', items: sortByName(items) }];
        case 'label': {
          const groups: { key: string; title: string; items: PrinterProfile[] }[] = [];
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
        }
        default: {
          const byVendor = new Map<string, PrinterProfile[]>();
          for (const p of items) {
            const key = p.vendor?.trim() || 'Other';
            const bucket = byVendor.get(key);
            if (bucket) bucket.push(p);
            else byVendor.set(key, [p]);
          }
          return [...byVendor.entries()]
            .sort(([a], [b]) => (a === 'Other' ? 1 : b === 'Other' ? -1 : a.localeCompare(b)))
            .map(([key, bucket]) => ({ key, title: key, items: sortByName(bucket) }));
        }
      }
    },
  );

  protected readonly selected = computed(() => {
    const id = this.selectedId();
    return id ? (this.store.getById(id) ?? null) : null;
  });

  constructor() {
    // Arriving from a wizard's "Add & configure": open the new printer and
    // scroll to the sections the wizard doesn't cover. `focus=gcode` jumps
    // straight to the G-code block (the meaningful review step after detection).
    const configureId = this.route.snapshot.queryParamMap.get('configure');
    if (configureId && this.store.getById(configureId)) {
      this.select(configureId);
      const anchor =
        this.route.snapshot.queryParamMap.get('focus') === 'gcode'
          ? 'gcode-target'
          : 'configure-target';
      afterNextRender(() => focusConfigureTarget(anchor));
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
    return this.active.printer()?.id === id;
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

  protected readonly catalogStatus = this.catalog.printersStatus;
  protected readonly catalogHasMore = this.catalog.printersHasMore;
  protected readonly catalogLoadingMore = this.catalog.printersLoadingMore;
  /** Id of the catalog entry currently being fetched for import, if any. */
  protected readonly importingId = signal<string | null>(null);
  protected readonly catalogEntries = computed<CatalogEntryVm[]>(() =>
    this.catalog.printers().map((p) => ({
      id: p.id,
      name: p.name,
      vendor: p.vendor,
      meta:
        catalogSpecOf(p) ??
        `${p.bed_width}×${p.bed_depth} mm · ${(p.params as Record<string, unknown>)?.['nozzle_diameter_mm']} mm`,
      icon: 'printer',
      imported: this.store.items().some((item) => item.based_on === p.id),
    })),
  );

  protected readonly editing = computed(() => this.selected());

  protected openCatalog(): void {
    void this.catalog.loadPrinters();
    this.catalogOpen.set(true);
  }

  protected onCatalogSearch(query: string): void {
    void this.catalog.searchPrinters(query);
  }

  protected retryCatalog(): void {
    void this.catalog.loadPrinters(true, this.catalog.printersQuery());
  }

  protected loadMoreCatalog(): void {
    void this.catalog.loadMorePrinters();
  }

  /**
   * Fetch the full preset behind `id` (real slicing params, not just the
   * browsed summary) and import it. The catalog picker shows a busy state on
   * this entry's pick button for the duration.
   */
  protected async importFromCatalog(id: string): Promise<void> {
    const base = this.catalog.printers().find((p) => p.id === id);
    if (!base || this.importingId()) {
      return;
    }
    this.importingId.set(id);
    try {
      const full = await this.catalog.printerDetail(base);
      const copy = this.store.importFromCatalog(full);
      this.active.selectPrinter(copy.id);
      this.select(copy.id);
    } catch (error) {
      this.notifications.error(
        'Import failed',
        error instanceof Error ? error.message : 'The preset details could not be fetched.',
      );
    } finally {
      this.importingId.set(null);
    }
  }

  /** Open a printer in the detail pane and refresh its live status. */
  protected select(id: string): void {
    this.selectedId.set(id);
    this.disarmDelete();
    const printer = this.store.getById(id);
    if (printer && (printer.connection?.kind ?? 'none') !== 'none') {
      this.printerConn.check(printer);
    }
  }

  /** Make the selected printer the default used for slicing. */
  protected setDefault(id: string): void {
    this.active.selectPrinter(id);
  }

  protected duplicate(id: string): void {
    const copy = this.store.duplicate(id);
    if (copy) {
      this.select(copy.id);
    }
  }

  /** Right-click a printer card: quick actions mirroring the detail-pane buttons. */
  protected onContextMenu(event: MouseEvent, printer: PrinterProfile): void {
    const items: ContextMenuItem[] = [
      {
        label: 'Set as default',
        icon: 'star',
        disabled: this.isDefault(printer.id),
        action: () => this.setDefault(printer.id),
      },
      { label: 'Duplicate', icon: 'copy', action: () => this.duplicate(printer.id) },
    ];
    if ((printer.connection?.kind ?? 'none') !== 'none') {
      items.push({
        label: 'Test connection',
        icon: 'wifi',
        action: () => this.testConnection(printer),
      });
    }
    items.push(this.labelSubmenu(printer));
    if (printer.source !== 'builtin') {
      items.push({ separator: true, label: '' });
      items.push({
        label: 'Delete\u2026',
        icon: 'trash',
        danger: true,
        action: () => this.confirmDeleteFromContextMenu(printer),
      });
    }
    void this.contextMenu.open(event, items);
  }

  /**
   * The labels, as a flyout on the profile's own context menu.
   *
   * Assigning the same label across a shelf of profiles is what labels are for,
   * and doing it from the card is one gesture instead of selecting each one and
   * scrolling to its Labels row.
   *
   * The flyout hosts the same picker the detail pane uses — coloured dots,
   * search, and "create this one" for a name that does not exist yet — because
   * a row of plain text is not a label, and a shelf of twenty needs filtering.
   * `submenu` carries the same labels as plain rows for the OS-drawn menus on
   * desktop and iOS, which can only show rows.
   */
  private labelSubmenu(item: { id: string; label_ids?: string[] }): ContextMenuItem {
    const owned = new Set(item.label_ids ?? []);
    const labels = this.labels.items();
    return {
      label: 'Labels',
      icon: 'label',
      submenu: labels.map((label) => ({
        label: label.name,
        checked: owned.has(label.id),
        action: () => this.toggleLabel(item.id, label.id),
      })),
      submenuPanel: {
        component: LabelPickerPanel,
        inputs: { assignedIds: () => this.store.getById(item.id)?.label_ids ?? [] },
        outputs: { toggle: (labelId: string) => this.toggleLabel(item.id, labelId) },
      },
    };
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
  }

  protected disarmDelete(): void {
    this.deleteArmed.set(false);
  }

  /** Delete the selected printer once its name has been typed to confirm. */
  protected confirmDelete(): void {
    const printer = this.selected();
    if (!printer) {
      return;
    }
    this.deletePrinterById(printer.id);
  }

  protected readonly pnum = paramNum;
  protected readonly pstr = paramStr;

  /**
   * Slice-parameter groups (Hardware, Retraction) rendered from the schema.
   * Every field is always shown — the profile editor authors presets, so it
   * never hides gated-off fields (unlike the live slice sidebar).
   */
  protected readonly paramGroups = PARAM_GROUPS;

  protected update(id: string, patch: Partial<PrinterProfile>): void {
    this.store.update(id, patch);
  }

  /** Merge a partial `SlicingParams` into a stored printer's `params` bundle. */
  protected updateParams(id: string, patch: Record<string, unknown>): void {
    const item = this.store.getById(id);
    if (item) {
      this.store.update(id, {
        params: { ...((item.params as Record<string, unknown>) ?? {}), ...patch },
      });
    }
  }

  /** A printer's `params` bag as a plain record for the field controls. */
  protected paramsOf(printer: PrinterProfile): Record<string, unknown> {
    return (printer.params as Record<string, unknown>) ?? {};
  }

  /** Apply a single param field edit (templates can't build computed keys). */
  protected setParam(id: string, key: string, value: unknown): void {
    this.updateParams(id, { [key]: value });
  }

  /**
   * This machine's per-material corrections, one section per material family.
   *
   * A correction is normally *captured* — changed on a plate that printed
   * wrong, then synced with "this printer only" — but these pages are the
   * surface that manages everything the slicer has, so each one is fully
   * editable here: change a value, stop correcting one setting, correct
   * another, or drop the material entirely.
   *
   * `fields` holds only the settings this material actually corrects, and
   * `addable` the rest of the eligible set. An empty correction is not a
   * setting left blank — it is a measurement nobody took, and the material's
   * own value standing is the right answer until they do.
   */
  protected materialCorrections(printer: PrinterProfile): {
    material: string;
    label: string;
    fields: FieldDef[];
    addable: { value: string; label: string }[];
  }[] {
    const overlays = (printer.material_overlays ?? {}) as Record<string, Record<string, unknown>>;
    return Object.entries(overlays)
      .filter(([, params]) => Object.keys(params ?? {}).length > 0)
      .map(([material, params]) => ({
        material,
        label: FILAMENT_MATERIAL_LABELS[material as FilamentMaterial] ?? material,
        fields: CORRECTABLE_FIELDS.filter((field) => field.key in params),
        addable: CORRECTABLE_FIELDS.filter((field) => !(field.key in params)).map((field) => ({
          value: field.key,
          label: field.title ?? field.key,
        })),
      }));
  }

  /** Begin correcting a material, and take the user to the section it creates. */
  protected startMaterialCorrection(id: string, material: string): void {
    if (!material) {
      return;
    }
    this.addMaterialCorrection(id, material);
    this.revealMaterial(material);
  }

  /**
   * Bring a material's card into view after adding it.
   *
   * Found by the card's own `data-material` rather than by its heading text:
   * the heading is a translated label and the attribute is the key, and reading
   * the key is what keeps this from breaking the day a label is reworded.
   */
  private revealMaterial(material: string): void {
    setTimeout(() => {
      document
        .querySelector(`.mgr__correction-card[data-material="${material}"]`)
        ?.scrollIntoView({ behavior: 'smooth', block: 'center' });
    });
  }

  /** Material families this machine has no correction for yet. */
  protected addableMaterials(printer: PrinterProfile): { value: string; label: string }[] {
    const overlays = (printer.material_overlays ?? {}) as Record<string, Record<string, unknown>>;
    return FILAMENT_MATERIALS.filter(
      (material) => Object.keys(overlays[material] ?? {}).length === 0,
    ).map((material) => ({ value: material, label: FILAMENT_MATERIAL_LABELS[material] }));
  }

  protected correctionValue(printer: PrinterProfile, material: string, key: string): unknown {
    return correctionsFor(printer, material)[key];
  }

  /**
   * What a correction's controls read their neighbours from: the machine's own
   * params with the correction laid over them, which is the order they resolve
   * in. A unit toggle asking what the nozzle is must get the printer's answer,
   * not nothing.
   */
  protected correctionSiblings(printer: PrinterProfile, material: string): Record<string, unknown> {
    return { ...this.paramsOf(printer), ...correctionsFor(printer, material) };
  }

  protected setCorrection(id: string, material: string, key: string, value: unknown): void {
    this.patchCorrection(id, material, (params) => ({ ...params, [key]: value }));
  }

  /** Start correcting one more setting, from the value it is a deviation from. */
  protected addCorrection(id: string, material: string, key: string): void {
    if (!key) {
      return;
    }
    this.patchCorrection(id, material, (params) => ({
      ...params,
      [key]: this.seedFor(id, material, key),
    }));
  }

  /**
   * What a fresh correction starts at: the value it is a correction *of*.
   *
   * The material's own generic figure first — a PETG correction opening at
   * PETG's 12 mm³/s says plainly what is being adjusted — then whatever the
   * machine itself carries, then the engine's default. Starting at zero would
   * be a correction that means "none" in most of these fields, which is a
   * worse first impression than a number that is merely not yours yet.
   */
  private seedFor(id: string, material: string, key: string): unknown {
    const fromMaterial = MATERIAL_PARAMS[material as FilamentMaterial]?.[key];
    if (fromMaterial !== undefined) {
      return fromMaterial;
    }
    const printer = this.store.items().find((p) => p.id === id);
    const fromPrinter = printer ? this.paramsOf(printer)[key] : undefined;
    return fromPrinter ?? CORRECTABLE_FIELDS.find((f) => f.key === key)?.default ?? 0;
  }

  /** Stop correcting one setting — the material's own value stands again. */
  protected removeCorrection(id: string, material: string, key: string): void {
    this.patchCorrection(id, material, (params) => {
      const next = { ...params };
      delete next[key];
      return next;
    });
  }

  /** Begin correcting a material this machine had nothing to say about. */
  protected addMaterialCorrection(id: string, material: string): void {
    if (!material) {
      return;
    }
    this.patchCorrection(id, material, () => ({
      [FIRST_CORRECTION_KEY]: this.seedFor(id, material, FIRST_CORRECTION_KEY),
    }));
  }

  /**
   * Rewrite one material's corrections, leaving every other material alone and
   * dropping a material left with nothing. Both rules live in
   * `material-corrections`, shared with the write-back dialog.
   */
  private patchCorrection(
    id: string,
    material: string,
    edit: (params: Record<string, unknown>) => Record<string, unknown>,
  ): void {
    const printer = this.store.items().find((p) => p.id === id);
    if (!printer) {
      return;
    }
    const next = edit(correctionsFor(printer, material));
    this.store.update(id, {
      material_overlays: withCorrections(printer, material, next),
    } as Partial<PrinterProfile>);
  }

  /**
   * The material whose "Remove all" is armed, if any.
   *
   * Dropping a material's corrections throws away measurements — a flow rate
   * somebody found by printing the thing badly first — and there is no undo
   * behind it. Cheap to redo is not the same as cheap to lose, so it arms on the
   * first press and acts on the second, the same two-step the profile delete and
   * the slice panel's "Reset all" use.
   */
  protected readonly removeArmed = signal<string | null>(null);
  private removeTimer: ReturnType<typeof setTimeout> | null = null;

  /** Arm on the first press, drop the material's corrections on the second. */
  protected removeAll(id: string, material: string): void {
    if (this.removeArmed() !== material) {
      this.disarmRemove();
      this.removeArmed.set(material);
      this.removeTimer = setTimeout(() => this.disarmRemove(), REMOVE_CONFIRM_MS);
      return;
    }
    this.disarmRemove();
    this.patchCorrection(id, material, () => ({}));
  }

  /** Disarm — on blur, or when the armed button has sat untouched long enough. */
  protected disarmRemove(): void {
    if (this.removeTimer !== null) {
      clearTimeout(this.removeTimer);
      this.removeTimer = null;
    }
    this.removeArmed.set(null);
  }

  protected rename(id: string, event: Event): void {
    const name = (event.target as HTMLInputElement).value.trim();
    if (name) {
      this.store.update(id, { name });
    }
  }

  protected setBedShape(id: string, value: string): void {
    this.store.update(id, { bed_shape: value as BedShape });
  }

  protected setFlavor(id: string, value: string): void {
    this.updateParams(id, { gcode_flavor: value as PrinterGcodeFlavor });
  }

  // ── G-code templates ────────────────────────────────────────────────────────

  /** The template the printer is currently based on, or `custom`. */
  protected gcodeTemplateId(printer: PrinterProfile): string {
    return gcodeTemplateStatus(printer.params).id;
  }

  /**
   * How the printer's G-code relates to its chosen template (`custom` / `synced`
   * / `modified` / `updated`) — drives the "Modified from …" banner and the
   * reset button.
   */
  protected gcodeTemplateStatus(printer: PrinterProfile): GcodeTemplateStatus {
    return gcodeTemplateStatus(printer.params);
  }

  /** Apply a preset's start/end/layer blocks (and flavor) to the printer. */
  protected applyGcodeTemplate(id: string, templateId: string): void {
    if (templateId === CUSTOM_TEMPLATE_ID) {
      // Detach from any template but keep the blocks the user already has.
      this.updateParams(id, customGcodeTemplatePatch());
      return;
    }
    const patch = gcodeTemplatePatch(templateId);
    if (patch) {
      const merged: Record<string, unknown> = { ...patch };
      this.updateParams(id, merged);
    }
  }

  /**
   * Restore the printer's blocks to its chosen template's current definition —
   * used both to discard edits (`modified`) and to adopt an upstream update
   * (`updated`). No-op for `custom`.
   */
  protected resetToTemplate(id: string): void {
    const printer = this.store.getById(id);
    if (!printer) {
      return;
    }
    const status = gcodeTemplateStatus(printer.params);
    if (!status.template) {
      return;
    }
    const patch = gcodeTemplatePatch(status.template.id);
    if (patch) {
      this.updateParams(id, { ...patch });
    }
  }

  protected setStartGcode(id: string, value: string): void {
    this.updateParams(id, { start_gcode: value });
  }

  protected setEndGcode(id: string, value: string): void {
    this.updateParams(id, { end_gcode: value });
  }

  protected setLayerGcode(id: string, value: string): void {
    this.updateParams(id, { layer_gcode: value });
  }

  // ── Connection ────────────────────────────────────────────────────────────

  /** Live connectivity status for a printer's card. */
  protected connectionStatus(id: string) {
    return this.printerConn.statusFor(id);
  }

  /** Probe the printer now and reflect the result in its status badge. */
  protected testConnection(printer: PrinterProfile): void {
    this.printerConn.check(printer);
  }

  protected setConnectionKind(id: string, value: string): void {
    // Reset the stale `connected` flag; live status is owned by the probe.
    this.updateConnection(id, { kind: value as PrinterConnectionKind, connected: false });
    const printer = this.store.getById(id);
    if (printer && value !== 'none') {
      this.printerConn.check(printer);
    }
  }

  protected setConnectionHost(id: string, event: Event): void {
    const host = (event.target as HTMLInputElement).value.trim();
    this.updateConnection(id, { host: host || undefined });
  }

  protected setConnectionPort(id: string, event: Event): void {
    const raw = (event.target as HTMLInputElement).value.trim();
    const port = raw ? Number.parseInt(raw, 10) : NaN;
    this.updateConnection(id, {
      port: Number.isFinite(port) && port > 0 ? port : undefined,
    });
  }

  protected setConnectionApiKey(id: string, event: Event): void {
    const key = (event.target as HTMLInputElement).value;
    this.updateConnection(id, { api_key: key || undefined });
  }

  private confirmDeleteFromContextMenu(printer: PrinterProfile): void {
    this.dialog
      .confirm({
        title: `Delete printer "${printer.name}"?`,
        message: 'This printer profile will be permanently deleted.',
        type: 'danger',
        confirmLabel: 'Delete',
      })
      .subscribe((confirmed) => {
        if (!confirmed) {
          return;
        }
        this.deletePrinterById(printer.id);
      });
  }

  private deletePrinterById(id: string): void {
    this.store.remove(id);
    this.disarmDelete();
    if (this.selectedId() === id) {
      this.selectedId.set(this.store.items()[0]?.id ?? null);
    }
  }

  /** Merge a partial connection into a stored printer's `connection` block. */
  private updateConnection(id: string, patch: Partial<PrinterConnection>): void {
    const item = this.store.getById(id);
    if (!item) {
      return;
    }
    const current = item.connection ?? { kind: 'none', connected: false };
    this.store.update(id, { connection: { ...current, ...patch } });
  }
}

/** Case-insensitive sort by display name (non-mutating). */
function sortByName(items: readonly PrinterProfile[]): PrinterProfile[] {
  return [...items].sort((a, b) => a.name.localeCompare(b.name));
}
