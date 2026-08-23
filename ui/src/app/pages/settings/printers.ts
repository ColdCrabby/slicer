import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import {
  PRINTER_GCODE_FLAVORS,
  type BedShape,
  type PrinterGcodeFlavor,
  type PrinterProfile,
} from '../../models/printer.model';
import { PROFILE_SOURCE_LABELS } from '../../models/profile-source';
import { CloudCatalog } from '../../services/catalog/cloud-catalog';
import { ActiveSelection } from '../../services/profiles/active-selection';
import {
  matchesAllLabels,
  toggledFilter,
  toggledLabelIds,
} from '../../services/profiles/label-filtering';
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
  private readonly catalog = inject(CloudCatalog);

  protected readonly sourceLabels = PROFILE_SOURCE_LABELS;
  protected readonly flavorOptions = PRINTER_GCODE_FLAVORS;
  protected readonly bedShapeOptions = [
    { value: 'rectangular', label: 'Rectangular' },
    { value: 'circular', label: 'Circular (delta)' },
  ];

  protected readonly wizardOpen = signal(false);
  protected readonly catalogOpen = signal(false);
  protected readonly editingId = signal<string | null>(null);
  protected readonly confirmDeleteId = signal<string | null>(null);
  protected readonly labelFilter = signal<string[]>([]);

  /** Printers narrowed by the active label filter. */
  protected readonly visibleItems = computed(() =>
    this.store.items().filter((p) => matchesAllLabels(p, this.labelFilter())),
  );

  protected labelsOf(item: PrinterProfile) {
    return this.labels.resolve(item.labelIds);
  }

  protected toggleFilter(id: string): void {
    this.labelFilter.update((f) => toggledFilter(f, id));
  }

  protected clearFilter(): void {
    this.labelFilter.set([]);
  }

  protected toggleLabel(id: string, labelId: string): void {
    const item = this.store.getById(id);
    if (item) {
      this.store.update(id, { labelIds: toggledLabelIds(item.labelIds, labelId) });
    }
  }

  protected readonly catalogStatus = this.catalog.status;
  protected readonly catalogEntries = computed<CatalogEntryVm[]>(() =>
    this.catalog.printers().map((p) => ({
      id: p.id,
      name: p.name,
      vendor: p.vendor,
      meta: `${p.bedWidth}×${p.bedDepth} mm · ${p.nozzleDiameter} mm`,
      icon: 'printer',
      imported: this.store.items().some((item) => item.basedOn === p.id),
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

  protected update(id: string, patch: Partial<PrinterProfile>): void {
    this.store.update(id, patch);
  }

  protected rename(id: string, event: Event): void {
    const name = (event.target as HTMLInputElement).value.trim();
    if (name) {
      this.store.update(id, { name });
    }
  }

  protected setBedShape(id: string, value: string): void {
    this.store.update(id, { bedShape: value as BedShape });
  }

  protected setFlavor(id: string, value: string): void {
    this.store.update(id, { gcodeFlavor: value as PrinterGcodeFlavor });
  }
}
