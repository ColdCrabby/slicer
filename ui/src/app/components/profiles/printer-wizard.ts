import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
  output,
  signal,
} from '@angular/core';
import {
  makePrinter,
  PRINTER_GCODE_FLAVORS,
  type BedShape,
  type PrinterGcodeFlavor,
  type PrinterProfile,
} from '../../models/printer.model';
import { CloudCatalog, toUserCopy } from '../../services/catalog/cloud-catalog';
import { PrintersStore } from '../../services/profiles/printers-store';
import { Icon } from '../../shared/icon/icon';
import { NumberInput } from '../../ui/number-input/number-input';
import { Segmented } from '../../ui/segmented/segmented';
import { Select } from '../../ui/select/select';
import { Switch } from '../../ui/switch/switch';
import { FieldRow } from '../../ui/field-row/field-row';
import { WizardShell } from '../../ui/wizard/wizard-shell';
import { CatalogPicker, type CatalogEntryVm } from './catalog-picker';
import { paramNum, paramStr } from '../../models/params-access';

const STEPS = ['Start', 'Basics', 'Build volume', 'Hardware'] as const;

/**
 * Guided, multi-step flow for adding a printer. Step 0 lets the user seed from
 * a cloud catalog preset or start from scratch; the remaining steps collect the
 * hardware details. Emits the finished profile — the host store decides how to
 * persist and select it.
 */
@Component({
  selector: 'nexus-printer-wizard',
  standalone: true,
  imports: [WizardShell, CatalogPicker, FieldRow, NumberInput, Select, Switch, Segmented, Icon],
  templateUrl: './printer-wizard.html',
  styleUrl: './printer-wizard.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class PrinterWizard {
  private readonly catalog = inject(CloudCatalog);
  private readonly store = inject(PrintersStore);

  readonly completed = output<PrinterProfile>();
  readonly cancelled = output<void>();

  protected readonly steps = STEPS;
  protected readonly index = signal(0);
  protected readonly draft = signal<PrinterProfile>(makePrinter());

  protected readonly bedShapeOptions = [
    { value: 'rectangular', label: 'Rectangular' },
    { value: 'circular', label: 'Circular (delta)' },
  ];
  protected readonly flavorOptions = PRINTER_GCODE_FLAVORS;

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

  protected readonly canProceed = computed(() => {
    if (this.index() === 0) {
      return false; // Step 0 advances via an explicit choice, not Next.
    }
    return this.draft().name.trim().length > 0;
  });

  constructor() {
    void this.catalog.load();
  }

  protected readonly pnum = paramNum;
  protected readonly pstr = paramStr;

  protected patch(patch: Partial<PrinterProfile>): void {
    this.draft.update((d) => ({ ...d, ...patch }));
  }

  /** Merge a partial `SlicingParams` into the draft's `params` bundle. */
  protected patchParams(patch: Record<string, unknown>): void {
    this.draft.update((d) => ({
      ...d,
      params: { ...((d.params as Record<string, unknown>) ?? {}), ...patch },
    }));
  }

  protected patchName(event: Event): void {
    this.patch({ name: (event.target as HTMLInputElement).value });
  }

  protected patchVendor(event: Event): void {
    this.patch({ vendor: (event.target as HTMLInputElement).value });
  }

  protected patchModel(event: Event): void {
    this.patch({ model: (event.target as HTMLInputElement).value });
  }

  protected startFromScratch(): void {
    this.draft.set(makePrinter());
    this.index.set(1);
  }

  protected startFromCatalog(id: string): void {
    const entry = this.catalog.printers().find((p) => p.id === id);
    if (entry) {
      this.draft.set(toUserCopy(entry));
      this.index.set(1);
    }
  }

  protected retryCatalog(): void {
    void this.catalog.load(true);
  }

  protected back(): void {
    this.index.update((i) => Math.max(0, i - 1));
  }

  protected next(): void {
    this.index.update((i) => Math.min(this.steps.length - 1, i + 1));
  }

  protected finish(): void {
    this.completed.emit(this.draft());
  }

  protected setBedShape(value: string): void {
    this.patch({ bed_shape: value as BedShape });
  }

  protected setFlavor(value: string): void {
    this.patchParams({ gcode_flavor: value as PrinterGcodeFlavor });
  }
}
