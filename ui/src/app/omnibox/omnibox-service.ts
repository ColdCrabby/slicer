import { Injectable, signal } from '@angular/core';

/**
 * Whether the omnibox palette is on screen, and what it should start with.
 *
 * Kept as its own tiny service — no providers, no imports beyond signals — so
 * the eager graph pays nothing for it: the shell watches it for the palette's
 * `@defer` trigger, and the keyboard shortcuts open it, while everything the
 * palette actually *is* stays in its deferred chunk until it is needed.
 */
@Injectable({ providedIn: 'root' })
export class OmniboxService {
  private readonly isOpen = signal(false);

  readonly open = this.isOpen.asReadonly();

  /** A query to start with, taken once — `settings` for a scoped hand-off. */
  #prefill: string | null = null;

  show(prefill?: string): void {
    if (prefill != null) {
      this.#prefill = prefill;
    }
    this.isOpen.set(true);
  }

  hide(): void {
    // An open that never happened must not dress the next one. A prefill set
    // for a palette that was dismissed before it could ask — the chunk still
    // loading, a second press of `e` — would otherwise surface as somebody
    // else's query on a later bare open.
    this.#prefill = null;
    this.isOpen.set(false);
  }

  toggle(): void {
    this.isOpen() ? this.hide() : this.show();
  }

  /** Consume the pending prefill, if there is one; the palette asks on open. */
  takePrefill(): string | null {
    const prefill = this.#prefill;
    this.#prefill = null;
    return prefill;
  }
}
