import { Component, ViewChild, computed, effect, inject, signal } from '@angular/core';
import type { ElementRef, OnDestroy } from '@angular/core';
import { Router, RouterLink } from '@angular/router';
import type { PrinterProfile } from '../../models/printer.model';
import { ListHistory } from '../../components/list-history/list-history';
import { NotificationService } from '../../services/notifications';
import {
  PrinterConnectionService,
  type PrinterProbeState,
} from '../../services/printer-connection';
import { PrintersStore } from '../../services/profiles/printers-store';
import { Slicer } from '../../services/slicer';
import { Icon } from '../../shared/icon/icon';
import { Button } from '../../ui/button/button';
import { EmptyState } from '../../ui/empty-state/empty-state';
import { SectionHeader } from '../../ui/section-header/section-header';

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
  imports: [RouterLink, ListHistory, Icon, Button, EmptyState, SectionHeader],
  templateUrl: './home.component.html',
  styleUrl: './home.component.scss',
})
export class HomeDashboard implements OnDestroy {
  private readonly router = inject(Router);
  private readonly printersStore = inject(PrintersStore);
  private readonly printerConn = inject(PrinterConnectionService);
  private readonly slicer = inject(Slicer);
  private readonly notifications = inject(NotificationService);

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
    const file = input.files?.[0];
    input.value = '';
    if (!file) {
      return;
    }
    await this.openWorkplateFromFile(file);
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
    const file = event.dataTransfer?.files?.[0] ?? null;
    if (file) {
      void this.openWorkplateFromFile(file);
    }
  }

  /** Whether the current drag carries files (ignore text/element drags). */
  private dragHasFiles(event: DragEvent): boolean {
    return Array.from(event.dataTransfer?.types ?? []).includes('Files');
  }

  /** Validate a picked/dropped model and open it as a fresh workplace. */
  private async openWorkplateFromFile(file: File): Promise<void> {
    if (!/\.(stl|obj|3mf)$/i.test(file.name)) {
      this.notifications.error('Unsupported file', 'Use an STL, OBJ, or 3MF model.');
      return;
    }
    try {
      const workplate = await this.slicer.startWorkplate(file);
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
    try {
      const response = await fetch(HomeDashboard.BENCHY_URL);
      if (!response.ok) {
        throw new Error(`HTTP ${response.status}`);
      }
      const blob = await response.blob();
      const file = new File([blob], '3DBenchy.stl', { type: 'model/stl' });
      const workplate = await this.slicer.startWorkplate(file);
      await this.router.navigate(['/slice', workplate.requestUuid], {
        state: workplate.uploadMeta ? { uploadMeta: workplate.uploadMeta } : undefined,
      });
    } catch {
      this.notifications.error('Could not load 3DBenchy', 'Check your connection and try again.');
    } finally {
      this.benchyLoading.set(false);
    }
  }

  private toDashboardPrinter(printer: PrinterProfile): DashboardPrinter {
    const connection = printer.connection;
    const model = `${printer.vendor} ${printer.model}`.trim();
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
