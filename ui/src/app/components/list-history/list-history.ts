import { Component, inject } from '@angular/core';
import { Router } from '@angular/router';
import type { RuntimeHistorySession } from '../../runtime/domain/history-models';
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

  /** Custom workplate name if the user set one, otherwise the source filename. */
  displayName(session: RuntimeHistorySession): string {
    return (
      this.#workplateNames.nameFor(session.request_uuid) ??
      session.original_filename ??
      'unknown.stl'
    );
  }

  navigate(session: RuntimeHistorySession): void {
    void this.#router.navigate(['/slice', session.request_uuid]);
  }

  download(session: RuntimeHistorySession): void {
    this.history.download(session);
  }
}
