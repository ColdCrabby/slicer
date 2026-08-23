import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import {
  makePrinter,
  PRINTER_CONNECTION_LABELS,
  PrinterConnectionKind,
} from '../../models/printer.model';
import { PrintersStore } from '../../services/profiles/printers-store';
import { Icon } from '../../shared/icon/icon';
import { Button } from '../../ui/button/button';
import { EmptyState } from '../../ui/empty-state/empty-state';
import { IconButton } from '../../ui/icon-button/icon-button';
import { SectionHeader } from '../../ui/section-header/section-header';

@Component({
  selector: 'nexus-settings-printers',
  imports: [SectionHeader, EmptyState, Button, IconButton, Icon],
  templateUrl: './printers.html',
  styleUrl: './printers.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class PrintersSettings {
  protected readonly store = inject(PrintersStore);

  add(): void {
    this.store.add(makePrinter());
  }

  remove(id: string): void {
    this.store.remove(id);
  }

  rename(id: string, event: Event): void {
    const name = (event.target as HTMLInputElement).value.trim();
    if (name) {
      this.store.update(id, { name });
    }
  }

  connectionLabel(kind: PrinterConnectionKind): string {
    return PRINTER_CONNECTION_LABELS[kind];
  }
}
