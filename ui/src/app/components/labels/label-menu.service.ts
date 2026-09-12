import { Injectable, inject } from '@angular/core';
import type { OutputRefSubscription } from '@angular/core';
import { FloatingService, type FloatingComponentRef, type FloatingReference } from '@coldcrabby/ui';
import { LabelPickerPanel } from './label-picker-panel';

/**
 * Opens the label list at a point on screen.
 *
 * Labels reached the context menu as one item per label at first, which does
 * not survive contact with a real shelf: a menu cannot search, cannot create,
 * and twenty labels make a twenty-item menu. This opens the same searchable
 * panel the profile detail pane uses, anchored where the user right-clicked —
 * the shape Finder uses for tags, and the one component we already had.
 *
 * The panel stays open across toggles: assigning three labels to a printer is
 * one gesture and three clicks, not three trips through a menu.
 */
@Injectable({ providedIn: 'root' })
export class LabelMenuService {
  readonly #floating = inject(FloatingService);

  #openRef: FloatingComponentRef<LabelPickerPanel> | null = null;
  #openSub: OutputRefSubscription | null = null;

  /**
   * Show the label panel at `(x, y)`.
   *
   * `assigned` is read on every change rather than passed once, so the ticks
   * follow the profile as the user toggles them.
   */
  open(
    at: { x: number; y: number },
    assigned: () => readonly string[],
    onToggle: (labelId: string) => void,
  ): void {
    this.close();

    const reference: FloatingReference = {
      getBoundingClientRect: () =>
        ({
          x: at.x,
          y: at.y,
          top: at.y,
          left: at.x,
          right: at.x,
          bottom: at.y,
          width: 0,
          height: 0,
          toJSON: () => ({}),
        }) as DOMRect,
    };

    const ref = this.#floating.openComponent(LabelPickerPanel, {
      reference,
      interactive: true,
      panelClass: 'nexus-floating--fit',
      options: { placement: 'right-start', offset: 4, padding: 8, size: true },
      onOutsidePointer: () => this.close(),
      onEscape: () => this.close(),
    });

    ref.setInput('assignedIds', assigned());
    this.#openSub = ref.instance.toggle.subscribe((labelId: string) => {
      onToggle(labelId);
      // Re-read rather than tracking locally: the store is the truth, and a
      // label created from inside the panel is assigned by the same path.
      ref.setInput('assignedIds', assigned());
    });
    this.#openRef = ref;
  }

  close(): void {
    this.#openSub?.unsubscribe();
    this.#openSub = null;
    this.#openRef?.close();
    this.#openRef = null;
  }
}
