import {
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import type { ElementRef, TemplateRef } from '@angular/core';
import { GcodePreview } from '../../services/gcode-preview';
import { PrinterConnectionService } from '../../services/printer-connection';
import { ActiveSelection } from '../../services/profiles/active-selection';
import { BrowserStorage } from '../../services/browser-storage';
import { formatDuration, PHASE_LABELS, Slicer } from '../../services/slicer';
import { FloatingService } from '../../shared/floating';
import type { FloatingRef } from '../../shared/floating';
import { Icon } from '../../shared/icon/icon';
import { TooltipDirective } from '../../shared/tooltip/tooltip.directive';

/** The action the result split button runs on the sliced G-code. */
type SliceAction = 'download' | 'upload' | 'print';

/** localStorage key remembering the user's preferred default action. */
const PRIMARY_ACTION_KEY = 'nexus.slice.primary-action';

@Component({
  selector: 'nexus-slice-control',
  imports: [Icon, TooltipDirective],
  templateUrl: './slice-control.html',
  styleUrl: './slice-control.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SliceControl {
  protected readonly slicer = inject(Slicer);
  private readonly preview = inject(GcodePreview);
  private readonly active = inject(ActiveSelection);
  private readonly printerConn = inject(PrinterConnectionService);
  private readonly storage = inject(BrowserStorage);
  private readonly floating = inject(FloatingService);

  /** Busy = a job is in flight (upload or slice). */
  protected readonly isActive = computed(() => {
    const s = this.slicer.status();
    return s === 'uploading' || s === 'slicing';
  });

  /** The progress rail is shown from job start through completion / error. */
  protected readonly showProgress = computed(() => {
    const s = this.slicer.status();
    return s === 'uploading' || s === 'slicing' || s === 'done' || s === 'error';
  });

  protected readonly isDone = computed(() => this.slicer.status() === 'done');
  protected readonly isError = computed(() => this.slicer.status() === 'error');

  /** Preview drifted from the current scene/settings — hint (never forces) a reslice. */
  protected readonly isStale = computed(
    () => this.slicer.previewStale() && !this.isActive() && !this.isError(),
  );

  /**
   * Disable width animation at the bounds so the bar snaps at reset/end.
   * Avoids tweening 100 → 0 on re-slice and redundant 99 → 100 motion.
   */
  protected readonly disableProgressTransition = computed(() => {
    const progress = this.slicer.sliceProgress();
    return progress === 0 || progress === 100;
  });

  protected readonly canSlice = computed(() => {
    const s = this.slicer.status();
    return (
      (s === 'idle' || s === 'ready' || s === 'done' || s === 'error') &&
      this.slicer.selectedFile() !== null
    );
  });

  /** Primary-button label, reflecting the current job state. */
  protected readonly ctaLabel = computed(() => {
    const s = this.slicer.status();
    if (s === 'uploading') return 'Uploading';
    if (s === 'slicing') return 'Slicing';
    return this.isDone() ? 'Re-Slice' : 'Slice';
  });

  protected readonly ctaTooltip = computed(() => {
    if (this.isActive()) return 'Slicing in progress…';
    if (this.isStale()) return 'Scene changed — re-slice to refresh the preview';
    return this.canSlice() ? 'Slice and generate G-code' : 'Add a model first';
  });

  /** Coarse state token used for status-line styling. */
  protected readonly statusState = computed<'idle' | 'busy' | 'done' | 'error' | 'stale'>(() => {
    if (this.isError()) return 'error';
    if (this.isActive()) return 'busy';
    if (this.isStale()) return 'stale';
    if (this.isDone()) return 'done';
    return 'idle';
  });

  /**
   * Always-present status line. The height is reserved in CSS so switching
   * between states never reflows the card (no jerk on first / repeat slices).
   */
  protected readonly statusLine = computed(() => {
    const s = this.slicer.status();
    if (s === 'error') return 'Slice failed — check the status panel';
    if (s === 'uploading') return 'Uploading model…';
    if (s === 'slicing') {
      const phase = this.slicer.currentPhase();
      return phase ? (PHASE_LABELS[phase] ?? phase) : 'Preparing…';
    }
    if (this.isStale()) return 'Scene changed — re-slice to update';
    if (s === 'done') {
      const n = this.preview.layerCount();
      const elapsed = this.slicer.totalElapsedMs();
      const time = elapsed != null ? ` · ${formatDuration(elapsed)}` : '';
      return n > 0 ? `Sliced · ${n} layers${time}` : `Slice complete${time}`;
    }
    return this.slicer.selectedFile() ? 'Ready to slice' : 'Add a model to begin';
  });

  slice(): void {
    void this.slicer.slice();
  }

  download(): void {
    this.slicer.downloadGcode();
  }

  /** Whether the active printer has a network connection and a slice is ready. */
  protected readonly canSendToPrinter = computed(() => {
    const printer = this.active.printer();
    const connected = !!printer?.connection && printer.connection.kind !== 'none';
    return this.isDone() && connected && !!this.slicer.currentRequestUuid();
  });

  // ── Result action split button (download / upload / print) ────────────────

  private readonly caretEl = viewChild<ElementRef<HTMLElement>>('caret');
  private readonly menuTpl = viewChild<TemplateRef<unknown>>('menuTpl');
  private menuRef: FloatingRef | null = null;

  private readonly actionMeta: Record<
    SliceAction,
    { label: string; description: string; icon: string }
  > = {
    download: { label: 'Download G-code', description: 'Save the .gcode file', icon: 'download' },
    upload: { label: 'Just upload', description: 'Copy the G-code to the printer', icon: 'upload' },
    print: { label: 'Upload & print', description: 'Upload, then start the print', icon: 'printer' },
  };

  /** Remembered default action, persisted to localStorage across sessions. */
  protected readonly primaryAction = signal<SliceAction>(this.readPrimaryAction());

  protected readonly menuOpen = signal(false);

  private readonly downloadAvailable = computed(() => !!this.slicer.gcodeDownloadUrl());

  /**
   * The action the primary button runs: the remembered default when it is
   * currently possible, otherwise the first available fallback.
   */
  protected readonly effectiveAction = computed<SliceAction | null>(() => {
    for (const action of [this.primaryAction(), 'download', 'upload', 'print'] as SliceAction[]) {
      if (this.isActionAvailable(action)) return action;
    }
    return null;
  });

  /** Menu rows: every action, flagged with availability and selection. */
  protected readonly actions = computed(() =>
    (['download', 'upload', 'print'] as SliceAction[]).map((value) => ({
      value,
      ...this.actionMeta[value],
      available: this.isActionAvailable(value),
      selected: value === this.effectiveAction(),
    })),
  );

  protected readonly showActions = computed(() => this.effectiveAction() !== null);

  protected readonly primaryIcon = computed(() => {
    const action = this.effectiveAction();
    return action ? this.actionMeta[action].icon : 'download';
  });

  protected readonly primaryTooltip = computed(() => {
    const action = this.effectiveAction();
    if (action === 'download' || action === null) return 'Download G-code';
    const printer = this.active.printer();
    const target = printer ? printer.name : 'the printer';
    return action === 'print' ? `Upload & print on ${target}` : `Upload G-code to ${target}`;
  });

  constructor() {
    inject(DestroyRef).onDestroy(() => this.closeMenu());
  }

  private readPrimaryAction(): SliceAction {
    const raw = this.storage.get(PRIMARY_ACTION_KEY)();
    return raw === 'upload' || raw === 'print' ? raw : 'download';
  }

  private isActionAvailable(action: SliceAction): boolean {
    return action === 'download' ? this.downloadAvailable() : this.canSendToPrinter();
  }

  /** Run the current default action. */
  protected runPrimary(): void {
    const action = this.effectiveAction();
    if (action) this.run(action);
  }

  /** Pick an action from the menu: remember it as the default. */
  protected pick(action: SliceAction): void {
    if (!this.isActionAvailable(action)) return;
    this.primaryAction.set(action);
    this.storage.write(PRIMARY_ACTION_KEY, action);
    this.closeMenu();
  }

  private run(action: SliceAction): void {
    if (action === 'download') {
      this.download();
      return;
    }
    const printer = this.active.printer();
    const uuid = this.slicer.currentRequestUuid();
    if (!printer || !uuid) return;
    this.printerConn.sendToPrinter(printer, uuid, {
      start: action === 'print',
      filename: this.gcodeFilename(),
    });
  }

  /** A printer-friendly `<model>.gcode` name, falling back to the engine default. */
  private gcodeFilename(): string | undefined {
    const name = this.slicer.selectedFile()?.name;
    if (!name) return undefined;
    const base = name.replace(/\.[^./\\]+$/, '').trim();
    return base ? `${base}.gcode` : undefined;
  }

  protected toggleMenu(): void {
    this.menuOpen() ? this.closeMenu() : this.openMenu();
  }

  private openMenu(): void {
    const trigger = this.caretEl()?.nativeElement;
    const tpl = this.menuTpl();
    if (!trigger || !tpl) {
      return;
    }
    this.menuOpen.set(true);
    this.menuRef = this.floating.openTemplate(
      tpl,
      {},
      {
        reference: trigger,
        interactive: true,
        panelClass: 'nexus-floating--fit',
        originElement: trigger,
        options: { placement: 'bottom-end', offset: 4, padding: 8 },
        onOutsidePointer: () => this.closeMenu(),
        onEscape: () => this.closeMenu(),
      },
    );
  }

  private closeMenu(): void {
    this.menuOpen.set(false);
    this.menuRef?.close();
    this.menuRef = null;
  }
}
