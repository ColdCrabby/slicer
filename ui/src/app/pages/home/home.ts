import { Component, ViewChild, inject, signal } from '@angular/core';
import type { ElementRef } from '@angular/core';
import { Router, RouterLink } from '@angular/router';
import { ListHistory } from '../../components/list-history/list-history';
import { GcodePreview } from '../../services/gcode-preview';
import { NotificationService } from '../../services/notifications';
import { SceneEngine } from '../../services/scene-engine';
import { Slicer } from '../../services/slicer';
import { ViewerControl } from '../../services/viewer-control';
import { Icon } from '../../shared/icon/icon';
import { Button } from '../../ui/button/button';
import { EmptyState } from '../../ui/empty-state/empty-state';
import { SectionHeader } from '../../ui/section-header/section-header';

/** Placeholder machine shown until the real printer store lands (mock). */
interface MockPrinter {
  id: string;
  name: string;
  model: string;
  status: 'ready' | 'offline';
}

@Component({
  selector: 'nexus-home-dashboard',
  standalone: true,
  imports: [RouterLink, ListHistory, Icon, Button, EmptyState, SectionHeader],
  templateUrl: './home.component.html',
  styleUrl: './home.component.scss',
})
export class HomeDashboard {
  private readonly router = inject(Router);
  private readonly slicer = inject(Slicer);
  private readonly sceneEngine = inject(SceneEngine);
  private readonly gcodePreview = inject(GcodePreview);
  private readonly viewerControl = inject(ViewerControl);
  private readonly notifications = inject(NotificationService);

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

  // Mocked until the Printers store (Phase F) is built.
  protected readonly printers: MockPrinter[] = [
    { id: 'p1', name: 'Workshop MK4', model: 'Prusa MK4', status: 'ready' },
    { id: 'p2', name: 'Garage Ender', model: 'Creality Ender 3', status: 'offline' },
  ];

  openModel(): void {
    this.quickFileInput.nativeElement.click();
  }

  /**
   * Discard the current workplate (file + scene) and open a clean plate. The
   * route change alone is not enough — the slicer/scene singletons would carry
   * the previous model over into the "empty" plate.
   */
  async openEmptyWorkplate(): Promise<void> {
    this.slicer.reset();
    this.gcodePreview.clear();
    this.viewerControl.viewMode.set('model');
    await this.sceneEngine.clear();
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
}
