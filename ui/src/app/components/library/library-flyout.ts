import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { RouterLink } from '@angular/router';
import { Icon, IconButton } from '@coldcrabby/ui';
import { ObjectLibrary, type LibraryEntry } from '../../services/library';
import { LibraryActions } from '../../services/library/library-actions';
import { LibraryFlyout as LibraryFlyoutState } from '../../services/library/library-flyout';
import { LibraryBrowser } from './library-browser';

/** A second press on the same card inside this window is the same press. */
const REPEAT_MS = 600;

/**
 * The library beside the plate: pick a model and it lands on the plate.
 *
 * Opened from the rail while a plate is on screen, because that is when the
 * library is wanted for one thing — putting a model down — and the full page
 * would take the plate away to do it. One pick adds; looking closer is a
 * link away, on the full page.
 */
@Component({
  selector: 'nexus-library-flyout',
  imports: [Icon, IconButton, LibraryBrowser, RouterLink],
  template: `
    <header class="flyout-header">
      <h2 class="flyout-title">Library</h2>
      <a
        nexusIconButton
        routerLink="/library"
        aria-label="Open the full library"
        (click)="state.close()"
      >
        <nexus-icon name="open-new-window" />
      </a>
      <button nexusIconButton type="button" aria-label="Close library" (click)="state.close()">
        <nexus-icon name="xmark" />
      </button>
    </header>

    @if (library.entries().length > 0) {
      <p class="flyout-hint">Pick a model to add it to the plate.</p>
      <nexus-library-browser variant="flyout" (pick)="add($event)" (activate)="add($event)" />
    } @else if (library.loaded()) {
      <p class="flyout-hint">
        Nothing here yet. Every model you put on a plate is kept in the library, ready for the next
        one.
      </p>
    }
  `,
  styleUrl: './library-flyout.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    role: 'dialog',
    'aria-label': 'Library',
    '(keydown.escape)': 'state.close()',
  },
})
export class LibraryFlyout {
  protected readonly library = inject(ObjectLibrary);
  protected readonly state = inject(LibraryFlyoutState);
  readonly #actions = inject(LibraryActions);

  #last: { id: string; at: number } | null = null;

  constructor() {
    // The page rescans folders; opening beside a plate should be instant.
    if (!this.library.loaded()) {
      void this.library.refresh({ scan: false });
    }
  }

  /**
   * Add a model to the plate. A double-click is also two clicks, so a repeat
   * on the same card straight after is dropped rather than adding it twice —
   * a deliberate second copy is still a click away a moment later.
   */
  protected add(entry: LibraryEntry): void {
    const now = performance.now();
    if (this.#last?.id === entry.id && now - this.#last.at < REPEAT_MS) {
      return;
    }
    this.#last = { id: entry.id, at: now };
    void this.#actions.addToPlate(entry);
  }
}
