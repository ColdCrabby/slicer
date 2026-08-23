import { Component, inject } from '@angular/core';
import { Router } from '@angular/router';
import { RuntimeHistorySession } from '../../runtime/domain/history-models';
import { History } from '../../services/history';
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

  navigate(session: RuntimeHistorySession): void {
    void this.#router.navigate(['/slice', session.request_uuid]);
  }

  download(session: RuntimeHistorySession): void {
    this.history.download(session);
  }
}
