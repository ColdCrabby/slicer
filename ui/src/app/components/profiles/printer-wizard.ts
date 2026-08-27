import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import {
  makePrinter,
  PRINTER_CONNECTION_LABELS,
  PRINTER_GCODE_FLAVORS,
  type BedShape,
  type PrinterGcodeFlavor,
  type PrinterProfile,
} from '../../models/printer.model';
import { CloudCatalog, toUserCopy } from '../../services/catalog/cloud-catalog';
import { ActiveSelection } from '../../services/profiles/active-selection';
import { PrintersStore } from '../../services/profiles/printers-store';
import {
  PrinterConnectionService,
  type PrinterDetectionResult,
} from '../../services/printer-connection';
import { defaultGcodeTemplateIdForFlavor, gcodeTemplatePatch } from '../../models/gcode-templates';
import { Icon } from '../../shared/icon/icon';
import { Button } from '../../ui/button/button';
import { NumberInput } from '../../ui/number-input/number-input';
import { Segmented } from '../../ui/segmented/segmented';
import { Select } from '../../ui/select/select';
import { Switch } from '../../ui/switch/switch';
import { FieldRow } from '../../ui/field-row/field-row';
import { WizardShell } from '../../ui/wizard/wizard-shell';
import { CatalogPicker, type CatalogEntryVm } from './catalog-picker';
import { paramNum, paramStr } from '../../models/params-access';

const STEPS = ['Start', 'Basics', 'Build volume', 'Hardware'] as const;
const KLIPPAIN_TEMPLATE_ID = 'klippain';
const KLIPPAIN_REPO_URL = 'https://github.com/Frix-x/klippain';
const KLIPPAIN_README_URL = 'https://github.com/Frix-x/klippain/blob/main/README.md';

type KlipperMacroChoice = 'standard' | 'klippain';

function normalizedFlavor(value: string | undefined): PrinterGcodeFlavor | undefined {
  const normalized = value?.trim().toLowerCase();
  if (normalized === 'marlin' || normalized === 'klipper') {
    return normalized;
  }
  return undefined;
}

/**
 * Guided, multi-step flow for adding a printer. Step 0 lets the user seed from
 * a cloud catalog preset or start from scratch; the remaining steps collect the
 * hardware details. Emits the finished profile — the host store decides how to
 * persist and select it.
 */
@Component({
  selector: 'nexus-printer-wizard',
  standalone: true,
  imports: [
    WizardShell,
    CatalogPicker,
    FieldRow,
    NumberInput,
    Select,
    Switch,
    Segmented,
    Icon,
    Button,
  ],
  templateUrl: './printer-wizard.html',
  styleUrl: './printer-wizard.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class PrinterWizard {
  private readonly catalog = inject(CloudCatalog);
  private readonly store = inject(PrintersStore);
  private readonly active = inject(ActiveSelection);
  private readonly printerConn = inject(PrinterConnectionService);
  private readonly router = inject(Router);

  protected readonly steps = STEPS;
  protected readonly index = signal(0);
  protected readonly draft = signal<PrinterProfile>(makePrinter());

  /** “Detect from URL” state for the Start step. */
  protected readonly detectHost = signal('');
  protected readonly detecting = signal(false);
  protected readonly detectResult = signal<PrinterDetectionResult | null>(null);
  protected readonly klipperMacroChoice = signal<KlipperMacroChoice | null>(null);

  /** Human-readable summary of a successful detection, for the review card. */
  protected readonly detectionRows = computed<{ label: string; value: string }[]>(() => {
    const r = this.detectResult();
    if (!r?.reachable) {
      return [];
    }
    const rows: { label: string; value: string }[] = [];
    rows.push({ label: 'Connection', value: PRINTER_CONNECTION_LABELS[r.kind] });
    if (r.name) {
      rows.push({ label: 'Name', value: r.name });
    }
    if (r.vendor) {
      rows.push({ label: 'Firmware', value: r.vendor });
    }
    if (r.bedWidth != null) {
      const bed =
        r.bedShape === 'circular'
          ? `⌀ ${r.bedWidth} mm`
          : `${r.bedWidth} × ${r.bedDepth ?? r.bedWidth} mm`;
      rows.push({ label: 'Bed', value: bed });
    }
    if (r.bedHeight != null) {
      rows.push({ label: 'Max height', value: `${r.bedHeight} mm` });
    }
    if (r.nozzleDiameterMm != null) {
      rows.push({ label: 'Nozzle', value: `${r.nozzleDiameterMm} mm` });
    }
    if (r.originAtCenter) {
      rows.push({ label: 'Kinematics', value: 'Delta (center origin)' });
    }
    return rows;
  });

  /** True when a reachable printer left some hardware fields unknown. */
  protected readonly detectionMissing = computed(() => {
    const r = this.detectResult();
    return !!r?.reachable && (r.bedWidth == null || r.nozzleDiameterMm == null);
  });

  /** True when detection identified a Klipper host. */
  protected readonly detectedKlipper = computed(() => {
    const result = this.detectResult();
    if (!result?.reachable) {
      return false;
    }
    return normalizedFlavor(result.firmware) === 'klipper';
  });

  /** Block adding until a Klipper profile (standard/Klippain) is chosen. */
  protected readonly needsKlipperFlavorChoice = computed(
    () => this.detectedKlipper() && this.klipperMacroChoice() == null,
  );

  protected readonly bedShapeOptions = [
    { value: 'rectangular', label: 'Rectangular' },
    { value: 'circular', label: 'Circular (delta)' },
  ];
  protected readonly klipperMacroOptions = [
    {
      value: 'standard',
      label: 'Standard Klipper',
      description: 'PRINT_START / PRINT_END macros.',
    },
    {
      value: 'klippain',
      label: 'Klippain',
      description: 'START_PRINT / END_PRINT + _ON_LAYER_CHANGE macros.',
    },
  ];
  protected readonly klippainRepoUrl = KLIPPAIN_REPO_URL;
  protected readonly klippainReadmeUrl = KLIPPAIN_README_URL;
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
  protected patchParams(patch: object): void {
    this.draft.update((d) => ({
      ...d,
      params: {
        ...((d.params as Record<string, unknown>) ?? {}),
        ...(patch as Record<string, unknown>),
      },
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

  protected setDetectHost(event: Event): void {
    this.detectHost.set((event.target as HTMLInputElement).value);
  }

  /**
   * Probe the typed URL, then — on a reachable printer — prefill the draft from
   * whatever the engine could learn (kind, bed volume, nozzle, kinematics) and
   * show a review card summarising the findings. An unreachable host stays on
   * the Start step with an explanatory message.
   */
  protected async detect(): Promise<void> {
    const host = this.detectHost().trim();
    if (!host || this.detecting()) {
      return;
    }
    this.detecting.set(true);
    this.detectResult.set(null);
    this.klipperMacroChoice.set(null);
    try {
      const result = await this.printerConn.detectPrinter(host);
      this.detectResult.set(result);
      if (result.reachable) {
        this.applyDetection(result, host);
      }
    } finally {
      this.detecting.set(false);
    }
  }

  /** Accept the detected settings and move on to review the Basics step. */
  protected continueFromDetection(): void {
    if (this.needsKlipperFlavorChoice()) {
      return;
    }
    this.index.set(1);
  }

  /** Add the detected printer and open its editor scrolled to the G-code block. */
  protected finishAndConfigureGcode(): void {
    if (this.needsKlipperFlavorChoice()) {
      return;
    }
    const printer = this.persist();
    void this.router.navigate(['/settings/printers'], {
      queryParams: { configure: printer.id, focus: 'gcode' },
    });
  }

  /** Discard the detection and return to the manual "start" options. */
  protected startOver(): void {
    this.detectResult.set(null);
    this.detectHost.set('');
    this.klipperMacroChoice.set(null);
    this.draft.set(makePrinter());
  }

  /** Pick the macro convention for detected Klipper hosts. */
  protected setKlipperMacroChoice(value: string): void {
    if (value !== 'standard' && value !== 'klippain') {
      return;
    }
    this.klipperMacroChoice.set(value);
    this.applyDetectedKlipperTemplate(value);
  }

  /** Merge a successful detection into a fresh draft, keeping sane defaults. */
  private applyDetection(result: PrinterDetectionResult, host: string): void {
    const base = makePrinter();
    const params = { ...((base.params as Record<string, unknown>) ?? {}) };
    const flavor = normalizedFlavor(result.firmware);
    this.klipperMacroChoice.set(null);

    if (flavor === 'klipper') {
      // Do not assume a Klipper macro convention: ask whether this host uses
      // Klippain before choosing the template.
      params['gcode_flavor'] = 'klipper';
    } else {
      // Non-Klipper printers can keep the automatic firmware-appropriate
      // defaults (Marlin M-codes, etc.).
      const templatePatch = gcodeTemplatePatch(defaultGcodeTemplateIdForFlavor(flavor));
      if (templatePatch) {
        Object.assign(params, templatePatch);
      }
    }
    if (result.nozzleDiameterMm != null) {
      params['nozzle_diameter_mm'] = result.nozzleDiameterMm;
    }
    this.draft.set({
      ...base,
      name: result.name?.trim() || result.vendor || base.name,
      vendor: result.vendor ?? base.vendor,
      model: result.model ?? base.model,
      bed_shape: result.bedShape ?? base.bed_shape,
      bed_width: result.bedWidth ?? base.bed_width,
      bed_depth: result.bedDepth ?? base.bed_depth,
      bed_height: result.bedHeight ?? base.bed_height,
      origin_at_center: result.originAtCenter ?? base.origin_at_center,
      connection: { kind: result.kind, host, connected: false },
      params,
    });
  }

  private applyDetectedKlipperTemplate(choice: KlipperMacroChoice): void {
    const templateId =
      choice === 'klippain' ? KLIPPAIN_TEMPLATE_ID : defaultGcodeTemplateIdForFlavor('klipper');
    const templatePatch = gcodeTemplatePatch(templateId);
    if (templatePatch) {
      this.patchParams(templatePatch);
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

  protected goto(index: number): void {
    this.index.set(index);
  }

  protected finish(): void {
    this.persist();
    void this.router.navigate(['/settings/printers']);
  }

  /** Create the printer, then open its editor scrolled to the advanced sections. */
  protected finishAndConfigure(): void {
    const printer = this.persist();
    void this.router.navigate(['/settings/printers'], { queryParams: { configure: printer.id } });
  }

  /** Persist the draft and make it the active printer; returns the saved profile. */
  private persist(): PrinterProfile {
    const printer = this.draft();
    this.store.add(printer);
    this.active.selectPrinter(printer.id);
    return printer;
  }

  protected cancel(): void {
    void this.router.navigate(['/settings/printers']);
  }

  protected setBedShape(value: string): void {
    this.patch({ bed_shape: value as BedShape });
  }

  protected setFlavor(value: string): void {
    this.patchParams({ gcode_flavor: value as PrinterGcodeFlavor });
  }
}
