import { Component, inject } from '@angular/core';
import { Router } from '@angular/router';
import type { RuntimeHistorySession } from '../../runtime/domain/history-models';
import { ContextMenuService } from '../../services/context-menu/context-menu.service';
import type { ContextMenuItem } from '../../services/context-menu/context-menu.model';
import { History } from '../../services/history';
import { WorkplateNames } from '../../services/workplate-names';
import { Icon } from '../../shared/icon/icon';
import { Button } from '../../ui/button/button';

@Component({
  selector: 'nexus-list-history',
  standalone: true,
  imports: [Button, Icon],
  templateUrl: './list-history.component.html',
  styleUrl: './list-history.component.scss',
})
export class ListHistory {
  protected readonly history = inject(History);
  readonly #router = inject(Router);
  readonly #workplateNames = inject(WorkplateNames);
  readonly #contextMenu = inject(ContextMenuService);

  /** Custom workplate name, or a default derived from the first uploaded model. */
  displayName(session: RuntimeHistorySession): string {
    return this.#workplateNames.displayNameFor(session.request_uuid, session.original_filename);
  }

  navigate(session: RuntimeHistorySession): void {
    void this.#router.navigate(['/slice', session.request_uuid]);
  }

  download(session: RuntimeHistorySession): void {
    this.history.download(session);
  }

  onContextMenu(event: MouseEvent, session: RuntimeHistorySession): void {
    const items: ContextMenuItem[] = [
      { label: 'Open', icon: 'open-new-window', action: () => this.navigate(session) },
      { label: 'Download G-code', icon: 'download', action: () => this.download(session) },
      { separator: true, label: '' },
      { label: 'Rename…', icon: 'edit-pencil', action: () => this.#rename(session) },
      { label: 'Copy UUID', icon: 'copy', action: () => this.#copyUuid(session) },
    ];
    void this.#contextMenu.open(event, items);
  }

  #rename(session: RuntimeHistorySession): void {
    const next = window.prompt('Workplate name', this.displayName(session));
    if (next !== null) {
      this.#workplateNames.setName(session.request_uuid, next);
    }
  }

  #copyUuid(session: RuntimeHistorySession): void {
    void navigator.clipboard?.writeText(session.request_uuid);
  }
}
