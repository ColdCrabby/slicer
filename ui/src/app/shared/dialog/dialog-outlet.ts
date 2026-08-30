import { NgComponentOutlet } from '@angular/common';
import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  ViewEncapsulation,
  effect,
  inject,
} from '@angular/core';
import { Dialog } from '../../services/dialog';
import { Button } from '@coldcrabby/ui';

@Component({
  selector: 'nexus-dialog-outlet',
  standalone: true,
  imports: [NgComponentOutlet, Button],
  templateUrl: './dialog-outlet.html',
  styleUrl: './dialog-outlet.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
  // ViewEncapsulation.None is required so that `dialog::backdrop` is not
  // mangled by Angular's emulated encapsulation (::backdrop is a
  // pseudo-element and cannot carry an attribute selector).
  encapsulation: ViewEncapsulation.None,
})
export class DialogOutlet {
  readonly #service = inject(Dialog);
  readonly #host = inject(ElementRef<HTMLElement>);

  readonly dialog = this.#service.activeDialog;

  constructor() {
    effect(() => {
      const d = this.dialog();
      const el = this.#nativeDialog;
      if (!el) {
        return;
      }

      if (!d) {
        if (el.open) {
          el.close();
        }
        return;
      }

      // Only open when a fresh (non-leaving) dialog arrives.
      if (!d.isLeaving && !el.open) {
        el.showModal();
      }
    });
  }

  get #nativeDialog(): HTMLDialogElement | null {
    return this.#host.nativeElement.querySelector('dialog');
  }

  confirm(): void {
    this.#service.resolve(true);
  }

  cancel(): void {
    this.#service.resolve(false);
  }

  /**
   * Intercept the native `cancel` event (fired when the user presses Escape)
   * so we can run the leave animation before closing.
   */
  onNativeCancel(event: Event): void {
    event.preventDefault();
    this.cancel();
  }

  /**
   * Clicks on the `::backdrop` pseudo-element bubble up as a click on the
   * `<dialog>` element itself, with `target === currentTarget`.
   */
  onDialogClick(event: MouseEvent): void {
    if (event.target === event.currentTarget) {
      this.cancel();
    }
  }
}
