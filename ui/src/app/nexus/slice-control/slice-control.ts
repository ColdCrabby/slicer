import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { GcodePreview } from '../../services/gcode-preview';
import { PrinterConnectionService } from '../../services/printer-connection';
import { ActiveSelection } from '../../services/profiles/active-selection';
import { formatDuration, PHASE_LABELS, Slicer } from '../../services/slicer';
import { Icon } from '../../shared/icon/icon';
import { TooltipDirective } from '../../shared/tooltip/tooltip.directive';

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

  protected readonly sendTooltip = computed(() => {
    const printer = this.active.printer();
    return printer ? `Send G-code to ${printer.name}` : 'Send G-code to the printer';
  });

  /** Upload the sliced G-code to the active printer (does not auto-start). */
  sendToPrinter(): void {
    const printer = this.active.printer();
    const uuid = this.slicer.currentRequestUuid();
    if (!printer || !uuid) {
      return;
    }
    this.printerConn.sendToPrinter(printer, uuid, { start: false });
  }
}
