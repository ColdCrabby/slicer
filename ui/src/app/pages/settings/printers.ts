import { ChangeDetectionStrategy, Component, afterNextRender, computed, inject, signal } from '@angular/core';
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
import {
  CUSTOM_TEMPLATE_ID,
  GCODE_PLACEHOLDER_HINT,
  GCODE_TEMPLATE_OPTIONS,
  customGcodeTemplatePatch,
  gcodeTemplatePatch,
  gcodeTemplateStatus,
  type GcodeTemplateStatus,
} from '../../models/gcode-templates';
import { CloudCatalog } from '../../services/catalog/cloud-catalog';
import { PrinterConnectionService } from '../../services/printer-connection';
import { ActiveSelection } from '../../services/profiles/active-selection';
import { matchesAllLabels, toggledLabelIds } from '../../services/profiles/label-filtering';
import { paramNum, paramStr } from '../../models/params-access';
import { LabelFilterStore } from '../../services/profiles/label-filter-store';
import { LabelsStore } from '../../services/profiles/labels-store';
import { PrintersStore } from '../../services/profiles/printers-store';
import { Icon } from '../../shared/icon/icon';
import { Badge } from '../../shared/badge/badge';
import { CatalogPicker, type CatalogEntryVm } from '../../components/profiles/catalog-picker';
import { CodeEditor } from '../../components/code-editor/code-editor';
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
import { Switch } from '../../ui/switch/switch';

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
    CodeEditor,
    ModalShell,
    FieldRow,
    NumberInput,
    Select,
    Switch,
    Segmented,
    LabelFilterBar,
    LabelPicker,
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

  /** Typed-name delete challenge state (high-impact delete — design language). */
  protected readonly deleteArmed = signal(false);
  protected readonly deleteText = signal('');

  /** Printers narrowed by the active label filter and the search query. */
  protected readonly filtered = computed(() => {
    const q = this.search().trim().toLowerCase();
    return this.store.items().filter(
      (p) =>
        matchesAllLabels(p, this.labelFilter()) &&
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

  /** Whether the typed name matches the selected printer's name exactly. */
  protected readonly deleteReady = computed(() => {
    const p = this.selected();
    return !!p && this.deleteText().trim() === p.name.trim();
  });

  constructor() {
    // Arriving from a wizard's "Add & configure": open the new printer and
    // scroll to the sections (connection, G-code) the wizard doesn't cover.
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

  protected readonly catalogStatus = this.catalog.status;
  protected readonly catalogEntries = computed<CatalogEntryVm[]>(() =>
    this.catalog.printers().map((p) => ({
      id: p.id,
      name: p.name,
      vendor: p.vendor,
      meta: `${p.bed_width}×${p.bed_depth} mm · ${(p.params as Record<string, unknown>)?.['nozzle_diameter_mm']} mm`,
      icon: 'printer',
      imported: this.store.items().some((item) => item.based_on === p.id),
    })),
  );

  protected readonly editing = computed(() => this.selected());

  protected openCatalog(): void {
    void this.catalog.load();
    this.catalogOpen.set(true);
  }

  protected retryCatalog(): void {
    void this.catalog.load(true);
  }

  protected importFromCatalog(id: string): void {
    const entry = this.catalog.printers().find((p) => p.id === id);
    if (!entry) {
      return;
    }
    const copy = this.store.importFromCatalog(entry);
    this.active.selectPrinter(copy.id);
    this.select(copy.id);
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

  /** Delete the selected printer once its name has been typed to confirm. */
  protected confirmDelete(): void {
    const printer = this.selected();
    if (!printer || !this.deleteReady()) {
      return;
    }
    this.store.remove(printer.id);
    this.disarmDelete();
    this.selectedId.set(this.store.items()[0]?.id ?? null);
  }

  protected readonly pnum = paramNum;
  protected readonly pstr = paramStr;

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
