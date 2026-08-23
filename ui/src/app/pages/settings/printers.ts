import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
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
import { CloudCatalog } from '../../services/catalog/cloud-catalog';
import { PrinterConnectionService } from '../../services/printer-connection';
import { ActiveSelection } from '../../services/profiles/active-selection';
import { matchesAllLabels, toggledLabelIds } from '../../services/profiles/label-filtering';
import { paramNum, paramStr } from '../../models/params-access';
import { LabelFilterStore } from '../../services/profiles/label-filter-store';
import { LabelsStore } from '../../services/profiles/labels-store';
import { PrintersStore } from '../../services/profiles/printers-store';
import { Icon } from '../../shared/icon/icon';
import { PrinterWizard } from '../../components/profiles/printer-wizard';
import { CatalogPicker, type CatalogEntryVm } from '../../components/profiles/catalog-picker';
import { LabelChip } from '../../components/labels/label-chip';
import { LabelFilterBar } from '../../components/labels/label-filter-bar';
import { LabelPicker } from '../../components/labels/label-picker';
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
    PrinterWizard,
    CatalogPicker,
    ModalShell,
    FieldRow,
    NumberInput,
    Select,
    Switch,
    Segmented,
    LabelChip,
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

  protected readonly sourceLabels = PROFILE_SOURCE_LABELS;
  protected readonly flavorOptions = PRINTER_GCODE_FLAVORS;
  protected readonly connectionKindOptions = PRINTER_CONNECTION_KINDS.map((kind) => ({
    value: kind,
    label: PRINTER_CONNECTION_LABELS[kind],
  }));
  protected readonly bedShapeOptions = [
    { value: 'rectangular', label: 'Rectangular' },
    { value: 'circular', label: 'Circular (delta)' },
  ];

  protected readonly wizardOpen = signal(false);
  protected readonly catalogOpen = signal(false);
  protected readonly editingId = signal<string | null>(null);
  protected readonly confirmDeleteId = signal<string | null>(null);
  protected readonly labelFilter = this.filterStore.selectedIds;

  /** Printers narrowed by the active label filter. */
  protected readonly visibleItems = computed(() =>
    this.store.items().filter((p) => matchesAllLabels(p, this.labelFilter())),
  );

  protected labelsOf(item: PrinterProfile) {
    return this.labels.resolve(item.label_ids);
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

  protected readonly editing = computed(() => {
    const id = this.editingId();
    return id ? (this.store.getById(id) ?? null) : null;
  });

  protected openWizard(): void {
    this.wizardOpen.set(true);
  }

  protected onWizardCompleted(printer: PrinterProfile): void {
    this.store.add(printer);
    this.active.selectPrinter(printer.id);
    this.editingId.set(printer.id);
    this.wizardOpen.set(false);
  }

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
  }

  protected selectActive(id: string): void {
    this.active.selectPrinter(id);
  }

  protected toggleEditor(id: string): void {
    this.editingId.update((current) => (current === id ? null : id));
  }

  protected duplicate(id: string): void {
    const copy = this.store.duplicate(id);
    if (copy) {
      this.editingId.set(copy.id);
    }
  }

  protected requestDelete(id: string): void {
    if (this.confirmDeleteId() === id) {
      this.store.remove(id);
      this.confirmDeleteId.set(null);
      if (this.editingId() === id) {
        this.editingId.set(null);
      }
    } else {
      this.confirmDeleteId.set(id);
      setTimeout(() => {
        if (this.confirmDeleteId() === id) {
          this.confirmDeleteId.set(null);
        }
      }, 3000);
    }
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
