import { Component, ViewChild, computed, effect, inject, signal } from '@angular/core';
import type { ElementRef, OnDestroy } from '@angular/core';
import { Router, RouterLink } from '@angular/router';
import { Panel } from '../../ui/panel/panel';
import type { PrinterProfile } from '../../models/printer.model';
import { ListHistory } from '../../components/list-history/list-history';
import {
  PrinterConnectionService,
  type PrinterProbeState,
} from '../../services/printer-connection';
import { PrintersStore } from '../../services/profiles/printers-store';
import { Slicer } from '../../services/slicer';
import { MODEL_FILE_ACCEPT } from '../../services/model-source';
import { WorkplateObjects } from '../../services/workplate-objects';
import {
  Icon,
  Button,
  EmptyState,
  InlineNotice,
  type InlineNoticeTone,
  SectionHeader,
} from '@coldcrabby/ui';

interface DashboardPrinter {
  id: string;
  name: string;
  model: string;
  /** Live connectivity state driving the status dot colour. */
  state: PrinterProbeState;
  statusLabel: string;
  /** Longer detail shown as a tooltip. */
  message?: string;
}

@Component({
  selector: 'nexus-home-dashboard',
  standalone: true,
  imports: [Panel, RouterLink, ListHistory, Icon, Button, EmptyState, InlineNotice, SectionHeader],
  templateUrl: './home.component.html',
  styleUrl: './home.component.scss',
})
export class HomeDashboard implements OnDestroy {
  private readonly router = inject(Router);
  private readonly printersStore = inject(PrintersStore);
  private readonly printerConn = inject(PrinterConnectionService);
  private readonly slicer = inject(Slicer);
  private readonly workplate = inject(WorkplateObjects);
  protected readonly modelFileAccept = MODEL_FILE_ACCEPT;

  /** Re-probe printers periodically so the dashboard reflects live status. */
  private readonly pollTimer = setInterval(
    () => this.printerConn.checkAll(this.printersStore.items()),
    PrinterConnectionService.POLL_INTERVAL_MS,
  );

  /** Canonical single-part 3DBenchy STL, served with permissive CORS by GitHub raw. */
  private static readonly BENCHY_URL =
    'https://raw.githubusercontent.com/CreativeTools/3DBenchy/master/Single-part/3DBenchy.stl';

  /** True while the demo model is being fetched over the network. */
  protected readonly benchyLoading = signal(false);

  /**
   * Why the last attempt to open something did not work, shown under the tiles
   * that start one.
   *
   * There is no scene here to speak over, and the corner of the window is a
   * long way from the button that was just pressed — so the page answers where
   * it was asked.
   */
  protected readonly openError = signal<{
    tone: InlineNoticeTone;
    title: string;
    text: string;
  } | null>(null);

  /** True while a file is being dragged over the dashboard (shows the drop overlay). */
  protected readonly dragActive = signal(false);
  // dragenter/leave fire for every descendant; count depth so nested children
  // don't prematurely clear the overlay.
  private dragDepth = 0;

  @ViewChild('quickFileInput') private quickFileInput!: ElementRef<HTMLInputElement>;

  protected readonly printers = computed<DashboardPrinter[]>(() =>
    this.printersStore.items().map((printer) => this.toDashboardPrinter(printer)),
  );

  constructor() {
    // Probe printers whenever the set of configured printers changes, on first
    // render, and again once the cloud server link comes up (so cloud-mode
    // probes run server-side instead of falling back to a browser request).
    effect(() => {
      const printers = this.printersStore.items();
      // Establish a reactive dependency on server connectivity.
      this.printerConn.serverConnected();
      this.printerConn.checkAll(printers);
    });
  }

  ngOnDestroy(): void {
    clearInterval(this.pollTimer);
  }

  openModel(): void {
    this.quickFileInput.nativeElement.click();
  }

  /**
   * Discard the current workplate (file + scene) and open a clean plate. The
   * route change alone is not enough — the slicer/scene singletons would carry
   * the previous model over into the "empty" plate.
   */
  async openEmptyWorkplate(): Promise<void> {
    await this.slicer.resetWorkplate();
    await this.router.navigate(['/slice', 'new']);
  }

  async onQuickFileSelected(event: Event): Promise<void> {
    const input = event.target as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    input.value = '';
    if (files.length === 0) {
      return;
    }
    await this.openWorkplateFromFiles(files);
  }

  onDragEnter(event: DragEvent): void {
    if (!this.dragHasFiles(event)) {
      return;
    }
    event.preventDefault();
    this.dragDepth += 1;
    this.dragActive.set(true);
  }

  onDragOver(event: DragEvent): void {
    if (!this.dragHasFiles(event)) {
      return;
    }
    event.preventDefault();
  }

  onDragLeave(event: DragEvent): void {
    if (!this.dragHasFiles(event)) {
      return;
    }
    event.preventDefault();
    this.dragDepth = Math.max(0, this.dragDepth - 1);
    if (this.dragDepth === 0) {
      this.dragActive.set(false);
    }
  }

  onDrop(event: DragEvent): void {
    event.preventDefault();
    this.dragDepth = 0;
    this.dragActive.set(false);
    const files = Array.from(event.dataTransfer?.files ?? []);
    if (files.length > 0) {
      void this.openWorkplateFromFiles(files);
    }
  }

  /** Whether the current drag carries files (ignore text/element drags). */
  private dragHasFiles(event: DragEvent): boolean {
    return Array.from(event.dataTransfer?.types ?? []).includes('Files');
  }

  /**
   * Open a fresh workplate from the picked/dropped models.
   *
   * The first valid model opens the plate; any others are queued and added by
   * the slice viewer once the scene exists, so dropping a whole batch of parts
   * plates all of them instead of silently keeping one.
   */
  private async openWorkplateFromFiles(files: readonly File[]): Promise<void> {
    this.openError.set(null);
    const models = files.filter((f) => /\.(stl|obj|3mf)$/i.test(f.name));
    if (models.length === 0) {
      this.openError.set({
        tone: 'danger',
        title: 'Unsupported file',
        text: 'Use an STL, OBJ, or 3MF model.',
      });
      return;
    }
    if (models.length < files.length) {
      // A warning, not a failure: the plate still opens with what could be
      // used. Calling it an error overstated what happened.
      this.openError.set({
        tone: 'warning',
        title: 'Some files were skipped',
        text: 'Only STL, OBJ, and 3MF models can be plated.',
      });
    }

    const [first, ...rest] = models;
    try {
      const workplate = await this.slicer.startWorkplate(first);
      // Queue only after the plate is created — `startWorkplate` resets the
      // scene, which would otherwise discard these before they are added.
      this.workplate.queuePending(rest);
      await this.router.navigate(['/slice', workplate.requestUuid], {
        state: workplate.uploadMeta ? { uploadMeta: workplate.uploadMeta } : undefined,
      });
    } catch {
      // Errors are tracked by the slicer/file services and surfaced in the UI.
    }
  }

  /**
   * Fetch the canonical 3DBenchy STL and open it as a fresh workplate so users
   * can demo slicing without hunting for a model of their own.
   */
  async loadBenchy(): Promise<void> {
    if (this.benchyLoading()) {
      return;
    }
    this.benchyLoading.set(true);
    this.openError.set(null);
    try {
      const blob = await fetchDemoModel(HomeDashboard.BENCHY_URL);
      const file = new File([blob], '3DBenchy.stl', { type: 'model/stl' });
      const workplate = await this.slicer.startWorkplate(file);
      await this.router.navigate(['/slice', workplate.requestUuid], {
        state: workplate.uploadMeta ? { uploadMeta: workplate.uploadMeta } : undefined,
      });
    } catch {
      this.openError.set({
        tone: 'danger',
        title: 'Could not load 3DBenchy',
        text: 'Check your connection and try again.',
      });
    } finally {
      this.benchyLoading.set(false);
    }
  }

  private toDashboardPrinter(printer: PrinterProfile): DashboardPrinter {
    const connection = printer.connection;
    // Catalog models often already carry the vendor ("Voron 2.4"), which
    // prefixing again turned into "Voron Voron 2.4".
    const vendor = printer.vendor?.trim() ?? '';
    const name = printer.model?.trim() ?? '';
    const model =
      vendor && !name.toLowerCase().startsWith(vendor.toLowerCase())
        ? `${vendor} ${name}`.trim()
        : name || vendor;
    if (!connection || connection.kind === 'none') {
      return {
        id: printer.id,
        name: printer.name,
        model,
        state: 'local',
        statusLabel: 'Local profile',
      };
    }
    const live = this.printerConn.statuses()[printer.id];
    return {
      id: printer.id,
      name: printer.name,
      model,
      state: live?.state ?? 'unknown',
      statusLabel: live?.label ?? 'Not checked',
      message: live?.message,
    };
  }
}

/**
 * Fetch the demo model, keeping a copy so the demo still opens offline.
 *
 * The model lives on GitHub, which a desktop on a train or a self-hosted slicer
 * on an air-gapped network cannot reach. The first successful fetch is stored in
 * the Cache API; afterwards a failed fetch falls back to it.
 */
async function fetchDemoModel(url: string): Promise<Blob> {
  const cache = await globalThis.caches?.open('slicer-demo-models').catch(() => undefined);
  try {
    const response = await fetch(url);
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }
    await cache?.put(url, response.clone()).catch(() => undefined);
    return await response.blob();
  } catch (error) {
    const cached = await cache?.match(url);
    if (cached) {
      return cached.blob();
    }
    throw error;
  }
}
