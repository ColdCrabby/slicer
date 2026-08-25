import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import { Icon } from '../../shared/icon/icon';

/**
 * Minimal centered modal: scrim + solid card with a titled header, a close
 * button, and a scrolling `<ng-content>` body. For lightweight overlays (e.g.
 * the catalog browser) that don't need the multi-step wizard chrome.
 */
@Component({
  selector: 'nexus-modal-shell',
  standalone: true,
  imports: [Icon],
  template: `
    <div class="modal__scrim" (click)="close.emit()"></div>
    <div class="modal" role="dialog" aria-modal="true" [attr.aria-label]="title()">
      <header class="modal__header">
        <h2 class="modal__title">{{ title() }}</h2>
        <button class="modal__close" type="button" aria-label="Close" (click)="close.emit()">
          <nexus-icon name="xmark" />
        </button>
      </header>
      <div class="modal__body">
        <ng-content />
      </div>
    </div>
  `,
  styleUrl: './modal-shell.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ModalShell {
  readonly title = input.required<string>();
  readonly close = output<void>();
}
