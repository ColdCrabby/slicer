import { Injectable, signal } from '@angular/core';

/**
 * Whether the library is open as a flyout over the plate.
 *
 * With a plate on screen, opening the library is almost always about putting
 * something on it — so the rail's Library button opens a panel beside the
 * plate instead of leaving it. Everywhere else the button goes to the full
 * page. Kept free of imports: the nav rail reads it, and the rail is in the
 * initial download.
 */
@Injectable({ providedIn: 'root' })
export class LibraryFlyout {
  readonly #open = signal(false);
  readonly open = this.#open.asReadonly();

  toggle(): void {
    this.#open.update((open) => !open);
  }

  close(): void {
    this.#open.set(false);
  }
}
