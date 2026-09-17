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
  questionHeadline,
  questionStep,
} from './detection-questions';
import {
  Icon,
  Button,
  InlineNotice,
  NumberInput,
  RadioGroup,
  Segmented,
  Select,
  Switch,
  FieldRow,
} from '@coldcrabby/ui';
import { WizardChrome, type WizardAction } from './wizard-chrome';
import { WizardRoute } from './wizard-route';
import { WizardName } from './wizard-name';
import { CatalogPicker, type CatalogEntryVm } from './catalog-picker';
import { paramNum, paramStr } from '../../models/params-access';

/** Display names for the G-code dialects detection reports. */
const FIRMWARE_LABELS: Readonly<Record<string, string>> = {
  klipper: 'Klipper',
  marlin: 'Marlin',
};

/** Steps for a printer entered by hand or seeded from a catalog preset. */
const MANUAL_STEPS = ['Where to start', 'Basics', 'Build volume', 'Hardware'] as const;

/**
 * A question the wizard is showing, paired with the wording for it.
 *
 * Questions whose copy this build doesn't recognise are dropped rather than
 * rendered as raw ids — their suggested answer is still applied, so an older UI
 * against a newer engine produces a correct profile, just without the prompt.
 */
/** One thing the detection concluded, and the config section behind it. */
interface Reading {
  readonly label: string;
  readonly value: string;
  /** Absent for a conclusion no single section states — the model, say. */
  readonly source?: string;
}

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
    WizardChrome,
    WizardRoute,
    WizardName,
    CatalogPicker,
    FieldRow,
    InlineNotice,
    NumberInput,
    RadioGroup,
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

  protected readonly index = signal(0);
  protected readonly draft = signal<PrinterProfile>(makePrinter());

  /**
   * Which route on the first screen is unfolded, if any.
   *
   * Detection opens by default because it is the recommended path and costs one
   * input row; the catalog stays folded so its search field, spinner and
   * unavailable-notice are not the largest thing on a screen that is meant to
   * pose a single question.
   */
  protected readonly openRoute = signal<'detect' | 'preset' | null>('detect');

  protected toggleRoute(route: 'detect' | 'preset'): void {
    this.openRoute.update((current) => (current === route ? null : route));
  }

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
    return [
      'What we found',
      ...this.openQuestions().map((entry) => questionStep(entry.question, entry.copy)),
      'Name and add',
    ];
  });

  /** The question shown on the current step, if the current step is one. */
  protected readonly currentQuestion = computed<WizardQuestion | null>(
    () => this.openQuestions()[this.index() - 1] ?? null,
  );

  /**
   * Everything the detection concluded, as one list.
   *
   * There used to be two: a summary grid of the headline facts, and a folded
   * table repeating most of them with their provenance. Same values, twice,
   * and the one that answered "where did this come from?" was the one hidden
   * behind a disclosure. So there is now a single list, always open, and a
   * reading carries its own config section.
   *
   * Identity comes first because it is what the user checks — is this the right
   * machine? — and it has no config section of its own: the connection kind is
   * how we reached the host, and the model is a fingerprint rather than
   * something the config states.
   */
  protected readonly readings = computed<Reading[]>(() => {
    const r = this.detectResult();
    if (!r?.reachable) {
      return [];
    }

    const rows: Reading[] = [{ label: 'Connection', value: PRINTER_CONNECTION_LABELS[r.kind] }];
    // Vendor is the machine's maker; the firmware it runs is its own row.
    if (r.model || r.vendor) {
      rows.push({ label: 'Machine', value: r.model || (r.vendor as string) });
    }
    if (r.firmware) {
      rows.push({ label: 'Firmware', value: FIRMWARE_LABELS[r.firmware] ?? r.firmware });
    }

    const findings = r.findings ?? [];
    if (findings.length) {
      rows.push(...findings.map((f) => ({ label: f.label, value: f.value, source: f.source })));
      return rows;
    }

    // No findings means the heavy `configfile` query never landed. The toolhead
    // probe still knows the build volume, so state it — sourceless, because
    // nothing read it out of a named section.
    if (r.bedWidth != null) {
      rows.push({
        label: 'Bed size',
        value:
          r.bedShape === 'circular'
            ? `⌀ ${r.bedWidth} mm`
            : `${r.bedWidth} × ${r.bedDepth ?? r.bedWidth} mm`,
      });
    }
    if (r.bedHeight != null) {
      rows.push({ label: 'Max height', value: `${r.bedHeight} mm` });
    }
    if (r.nozzleDiameterMm != null) {
      rows.push({ label: 'Nozzle diameter', value: `${r.nozzleDiameterMm} mm` });
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

  /**
   * Why the last catalog import failed. Rendered by the picker, beside the
   * button that was pressed — an error about a control the user is looking at
   * does not belong in a floating message somewhere else.
   */
  protected readonly importError = signal<string | null>(null);
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

  protected readonly named = computed(() => this.draft().name.trim().length > 0);
  protected readonly isLast = computed(() => this.index() === this.steps().length - 1);

  /**
   * The footer's actions for the current step, primary last.
   *
   * There is exactly one action row, and it lives in the footer. The detected
   * card used to carry three buttons of its own while the shell's footer
   * offered Next beside them — two rows, five buttons, and the two most
   * prominent did the same thing.
   *
   * After a detection the draft is already a finished, addable profile, so
   * every step from the first offers "Add now" alongside the step that refines
   * it. That is the honest shape of this flow: nothing below the first screen
   * is required.
   */
  protected readonly actions = computed<WizardAction[]>(() => {
    const detected = !!this.detectResult()?.reachable;

    if (this.index() === 0 && !detected) {
      // Every choice on the manual start screen advances by being chosen, so a
      // Next here could only ever be a disabled button nobody can satisfy.
      return [];
    }

    if (this.isLast()) {
      return [
        { id: 'configure', label: 'Add & configure', disabled: !this.named() },
        { id: 'finish', label: 'Add printer', disabled: !this.named() },
      ];
    }

    if (!detected) {
      return [{ id: 'next', label: 'Next', icon: 'nav-arrow-right', disabled: !this.named() }];
    }

    const open = this.openQuestions().length;
    if (this.index() === 0) {
      return [
        { id: 'finish', label: 'Add as-is' },
        open > 0
          ? {
              id: 'next',
              label: open === 1 ? 'Answer 1 question' : `Answer ${open} questions`,
              icon: 'nav-arrow-right',
            }
          : { id: 'gcode', label: 'Add & check G-code', icon: 'nav-arrow-right' },
      ];
    }

    return [
      { id: 'finish', label: 'Add now' },
      { id: 'next', label: 'Next', icon: 'nav-arrow-right' },
    ];
  });

  /** Route a footer press to the method behind it. */
  protected onAction(id: string): void {
    switch (id) {
      case 'next':
        this.next();
        break;
      case 'finish':
        this.finish();
        break;
      case 'configure':
        this.finishAndConfigure();
        break;
      case 'gcode':
        this.finishAndConfigureGcode();
        break;
    }
  }

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
    this.importError.set(null);
    try {
      const full = await this.catalog.printerDetail(base);
      this.draft.set(toUserCopy(full));
      this.index.set(1);
    } catch (error) {
      this.importError.set(
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

  /** Add the detected printer and open its editor scrolled to the G-code block. */
  private finishAndConfigureGcode(): void {
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

  /**
   * Options for a question, as option cards.
   *
   * Cards rather than a segmented control because the labels are terms out of a
   * config file — `PRINT_START / PRINT_END` against `START_PRINT / END_PRINT`
   * separates nothing by name — and because a segmented control shows a
   * description only as a hover tooltip, which is no description at all on a
   * touchscreen.
   */
  protected optionsFor(
    entry: WizardQuestion,
  ): { value: string; label: string; description?: string }[] {
    return entry.question.options.map((option) => ({
      value: option.id,
      label: optionLabel(entry.question, option),
      description: optionDescription(entry.question, option),
    }));
  }

  /** The question as a sentence, with whatever the engine named filled in. */
  protected headlineFor(entry: WizardQuestion): string {
    return questionHeadline(entry.question, entry.copy);
  }

  /** The config sections behind a question, for the "where we read this" line. */
  protected sourcesFor(entry: WizardQuestion): readonly string[] {
    return entry.question.sources ?? [];
  }

  /**
   * One line of what this machine is, under its name on the final step — the
   * two facts that tell the user they are naming the right thing.
   */
  protected readonly machineLine = computed(() => {
    const r = this.detectResult();
    if (!r?.reachable) {
      return '';
    }
    const bed =
      r.bedWidth == null
        ? null
        : r.bedShape === 'circular'
          ? `⌀ ${r.bedWidth} mm`
          : `${r.bedWidth} × ${r.bedDepth ?? r.bedWidth} mm`;
    return [r.model || r.vendor, bed].filter(Boolean).join(' · ');
  });

  /** The decisions the user has made, recapped on the final step. */
  protected readonly answerSummary = computed<{ step: string; choice: string }[]>(() =>
    this.openQuestions().map((entry) => {
      const chosen = entry.question.options.find(
        (option) => option.id === this.answerFor(entry.question.id),
      );
      return {
        step: questionStep(entry.question, entry.copy),
        choice: chosen ? optionLabel(entry.question, chosen) : '—',
      };
    }),
  );

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
