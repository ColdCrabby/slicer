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
import { CloudCatalog, catalogSpecOf, toUserCopy } from '../../services/catalog/cloud-catalog';
import { ActiveSelection } from '../../services/profiles/active-selection';
import { PrintersStore } from '../../services/profiles/printers-store';
import { NotificationService } from '../../services/notifications';
import {
  PrinterConnectionService,
  type DetectionQuestion,
  type PrinterDetectionResult,
} from '../../services/printer-connection';
import {
  customGcodeTemplatePatch,
  defaultGcodeTemplateIdForFlavor,
  gcodeTemplatePatch,
} from '../../models/gcode-templates';
import {
  optionDescription,
  optionLabel,
  optionProfilePatch,
  optionTemplateId,
  questionCopy,
} from './detection-questions';
import {
  Icon,
  Button,
  NumberInput,
  Segmented,
  Select,
  Switch,
  FieldRow,
  WizardShell,
} from '@coldcrabby/ui';
import { CatalogPicker, type CatalogEntryVm } from './catalog-picker';
import { paramNum, paramStr } from '../../models/params-access';

/** Display names for the G-code dialects detection reports. */
const FIRMWARE_LABELS: Readonly<Record<string, string>> = {
  klipper: 'Klipper',
  marlin: 'Marlin',
};

/** Steps for a printer entered by hand or seeded from a catalog preset. */
const MANUAL_STEPS = ['Start', 'Basics', 'Build volume', 'Hardware'] as const;

/**
 * A question the wizard is showing, paired with the wording for it.
 *
 * Questions whose copy this build doesn't recognise are dropped rather than
 * rendered as raw ids — their suggested answer is still applied, so an older UI
 * against a newer engine produces a correct profile, just without the prompt.
 */
interface WizardQuestion {
  readonly question: DetectionQuestion;
  readonly copy: NonNullable<ReturnType<typeof questionCopy>>;
}

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
  private readonly notifications = inject(NotificationService);
  private readonly router = inject(Router);

  protected readonly index = signal(0);
  protected readonly draft = signal<PrinterProfile>(makePrinter());

  /** “Detect from URL” state for the Start step. */
  protected readonly detectHost = signal('');
  protected readonly detecting = signal(false);
  protected readonly detectResult = signal<PrinterDetectionResult | null>(null);
  /** Chosen option id per question id. Seeded with every suggestion. */
  protected readonly answers = signal<Record<string, string>>({});

  /**
   * Questions worth putting to the user: the ones the config did not settle,
   * and that this build has wording for.
   */
  protected readonly openQuestions = computed<WizardQuestion[]>(() => {
    const result = this.detectResult();
    if (!result?.reachable) {
      return [];
    }
    return (result.questions ?? [])
      .filter((question) => !question.certain)
      .map((question) => ({ question, copy: questionCopy(question.id) }))
      .filter((entry): entry is WizardQuestion => entry.copy != null);
  });

  /**
   * After a detection the wizard asks only what it could not work out: a
   * review of what it read, a step per open question, and a finish. A printer
   * entered by hand keeps the full manual form.
   */
  protected readonly steps = computed<readonly string[]>(() => {
    if (!this.detectResult()?.reachable) {
      return MANUAL_STEPS;
    }
    return ['Detected', ...this.openQuestions().map((entry) => entry.copy.step), 'Ready'];
  });

  /** The question shown on the current step, if the current step is one. */
  protected readonly currentQuestion = computed<WizardQuestion | null>(
    () => this.openQuestions()[this.index() - 1] ?? null,
  );

  /** Everything detection read off the printer, for the "what we read" panel. */
  protected readonly findings = computed(() => this.detectResult()?.findings ?? []);

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
    // Vendor is the machine's maker; the firmware it runs is its own row.
    if (r.model || r.vendor) {
      rows.push({ label: 'Machine', value: r.model || (r.vendor as string) });
    }
    if (r.firmware) {
      rows.push({ label: 'Firmware', value: FIRMWARE_LABELS[r.firmware] ?? r.firmware });
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

  /**
   * True when a reachable printer left the two settings a profile is useless
   * without at their defaults, so the manual steps are worth walking.
   */
  protected readonly detectionMissing = computed(() => {
    const r = this.detectResult();
    return !!r?.reachable && (r.bedWidth == null || r.nozzleDiameterMm == null);
  });

  protected readonly bedShapeOptions = [
    { value: 'rectangular', label: 'Rectangular' },
    { value: 'circular', label: 'Circular (delta)' },
  ];
  protected readonly flavorOptions = PRINTER_GCODE_FLAVORS;

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

  protected readonly canProceed = computed(() => {
    // The manual flow's first step advances via an explicit choice, not Next.
    if (this.index() === 0 && !this.detectResult()?.reachable) {
      return false;
    }
    return this.draft().name.trim().length > 0;
  });

  constructor() {
    void this.catalog.loadPrinters();
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
  /**
   * Fetch the full preset behind `id` (real slicing params, not just the
   * browsed summary) and seed the draft from it. The catalog picker shows a
   * busy state on this entry's pick button for the duration.
   */
  protected async startFromCatalog(id: string): Promise<void> {
    const base = this.catalog.printers().find((p) => p.id === id);
    if (!base || this.importingId()) {
      return;
    }
    this.importingId.set(id);
    try {
      const full = await this.catalog.printerDetail(base);
      this.draft.set(toUserCopy(full));
      this.index.set(1);
    } catch (error) {
      this.notifications.error(
        'Could not load preset',
        error instanceof Error ? error.message : 'The preset details could not be fetched.',
      );
    } finally {
      this.importingId.set(null);
    }
  }

  protected loadMoreCatalog(): void {
    void this.catalog.loadMorePrinters();
  }

  protected setDetectHost(event: Event): void {
    this.detectHost.set((event.target as HTMLInputElement).value);
  }

  /**
   * Probe the typed URL and, on a reachable printer, build a finished profile
   * from everything the engine could read off it — then rebuild the wizard's
   * steps around whatever the config left open. An unreachable host stays on
   * the Start step with an explanatory message.
   */
  protected async detect(): Promise<void> {
    const host = this.detectHost().trim();
    if (!host || this.detecting()) {
      return;
    }
    this.detecting.set(true);
    this.detectResult.set(null);
    this.answers.set({});
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

  /** Walk the detected printer through whatever is still open. */
  protected continueFromDetection(): void {
    this.index.set(1);
  }

  /** Skip the questions and add the printer as detection left it. */
  protected addDetected(): void {
    this.finish();
  }

  /** Add the detected printer and open its editor scrolled to the G-code block. */
  protected finishAndConfigureGcode(): void {
    const printer = this.persist();
    void this.router.navigate(['/settings/printers'], {
      queryParams: { configure: printer.id, focus: 'gcode' },
    });
  }

  /** Fall back to the manual steps for a printer we could only half read. */
  protected reviewManually(): void {
    this.detectResult.set(null);
    this.index.set(1);
  }

  /** Discard the detection and return to the manual "start" options. */
  protected startOver(): void {
    this.detectResult.set(null);
    this.detectHost.set('');
    this.answers.set({});
    this.draft.set(makePrinter());
    this.index.set(0);
  }

  /** The option currently chosen for a question. */
  protected answerFor(questionId: string): string | null {
    return this.answers()[questionId] ?? null;
  }

  /** Options for a question, in the shape the segmented control takes. */
  protected optionsFor(
    entry: WizardQuestion,
  ): { value: string; label: string; description?: string }[] {
    return entry.question.options.map((option) => ({
      value: option.id,
      label: optionLabel(entry.question, option),
      description: optionDescription(entry.question, option),
    }));
  }

  /** Record an answer and apply what it implies. */
  protected answer(questionId: string, optionId: string): void {
    const entry = this.openQuestions().find((q) => q.question.id === questionId);
    if (!entry) {
      return;
    }
    this.answers.update((answers) => ({ ...answers, [questionId]: optionId }));
    this.applyAnswer(entry.question, optionId);
  }

  /**
   * Merge a successful detection into a fresh draft.
   *
   * The draft is left **finished**: every fact the printer reported is applied,
   * and so is the suggested answer to every open question. The question steps
   * that follow refine a profile the user could already add, which is what lets
   * the wizard offer "Add printer" from the first step on.
   */
  private applyDetection(result: PrinterDetectionResult, host: string): void {
    const base = makePrinter();
    const flavor = normalizedFlavor(result.firmware);
    const params: Record<string, unknown> = {
      ...((base.params as Record<string, unknown>) ?? {}),
      // Non-Klipper printers get the firmware-appropriate template outright;
      // for Klipper the macro convention decides it, and the engine reports
      // that as a question — settled or not.
      ...(flavor === 'klipper'
        ? { gcode_flavor: 'klipper' }
        : (gcodeTemplatePatch(defaultGcodeTemplateIdForFlavor(flavor)) ?? {})),
      // Everything read off the machine's own config.
      ...(result.params ?? {}),
    };

    this.draft.set({
      ...base,
      name: result.name?.trim() || result.model || base.name,
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

    // Apply every suggestion — including the ones the config settled outright,
    // which never become steps.
    const answers: Record<string, string> = {};
    for (const question of result.questions ?? []) {
      answers[question.id] = question.suggested;
      this.applyAnswer(question, question.suggested);
    }
    this.answers.set(answers);
    this.index.set(0);
  }

  /**
   * Apply one answer: its slicing params, plus the profile-level effects a
   * params bag cannot carry (a machine's identity, its plate orientation, the
   * G-code its start macros expect).
   */
  private applyAnswer(question: DetectionQuestion, optionId: string): void {
    const option = question.options.find((candidate) => candidate.id === optionId);
    if (option?.params) {
      this.patchParams(option.params);
    }

    const templateId = optionTemplateId(question.id, optionId);
    if (templateId) {
      const patch = gcodeTemplatePatch(templateId);
      if (patch) {
        this.patchParams(patch);
      }
    } else if (question.id === 'macro_convention') {
      // "Leave it to me" — write no start/end G-code rather than commands this
      // printer has no macro for.
      this.patchParams({
        ...customGcodeTemplatePatch(),
        start_gcode: '',
        end_gcode: '',
        layer_gcode: '',
      });
    }

    if (question.id === 'machine_identity' && optionId === 'other') {
      this.patch({ vendor: '', model: '' });
    }

    const profilePatch = optionProfilePatch(question.id, optionId);
    if (profilePatch) {
      this.patch(profilePatch);
    }
  }

  protected onCatalogSearch(query: string): void {
    void this.catalog.searchPrinters(query);
  }

  protected retryCatalog(): void {
    void this.catalog.loadPrinters(true, this.catalog.printersQuery());
  }

  protected back(): void {
    this.index.update((i) => Math.max(0, i - 1));
  }

  protected next(): void {
    this.index.update((i) => Math.min(this.steps().length - 1, i + 1));
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
