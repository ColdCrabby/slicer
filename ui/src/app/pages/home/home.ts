import { Component, ElementRef, ViewChild, inject } from '@angular/core';
import { Router, RouterLink } from '@angular/router';
import { ConnectionState } from '../../components/connection-state/connection-state';
import { ListHistory } from '../../components/list-history/list-history';
import { Slicer } from '../../services/slicer';
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
  imports: [RouterLink, ListHistory, ConnectionState, Icon, Button, EmptyState, SectionHeader],
  templateUrl: './home.component.html',
  styleUrl: './home.component.scss',
})
export class HomeDashboard {
  private readonly router = inject(Router);
  private readonly slicer = inject(Slicer);

  @ViewChild('quickFileInput') private quickFileInput!: ElementRef<HTMLInputElement>;

  // Mocked until the Printers store (Phase F) is built.
  protected readonly printers: MockPrinter[] = [
    { id: 'p1', name: 'Workshop MK4', model: 'Prusa MK4', status: 'ready' },
    { id: 'p2', name: 'Garage Ender', model: 'Creality Ender 3', status: 'offline' },
  ];

  openModel(): void {
    this.quickFileInput.nativeElement.click();
  }

  async onQuickFileSelected(event: Event): Promise<void> {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    input.value = '';
    if (!file || !/\.(stl|obj|3mf)$/i.test(file.name)) {
      return;
    }
    try {
      const workplate = await this.slicer.startWorkplate(file);
      this.router.navigate(['/slice', workplate.requestUuid], {
        state: workplate.uploadMeta ? { uploadMeta: workplate.uploadMeta } : undefined,
      });
    } catch {
      // Errors are tracked by the slicer/file services and surfaced in the UI.
    }
  }
}
