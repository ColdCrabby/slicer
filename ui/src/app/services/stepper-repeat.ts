import { DOCUMENT, Injectable, inject } from '@angular/core';

/** How long a press must be held before it starts repeating. */
const HOLD_MS = 400;

/** Interval of the first repeat, and the floor it accelerates towards. */
const START_MS = 220;
const MIN_MS = 35;

/** Each repeat is this much quicker than the one before it. */
const DECAY = 0.82;

/**
 * Hold a number field's `+` / `−` to keep stepping, faster the longer you hold.
 *
 * Settings are full of values that live a long way from their default — a skirt
 * distance near 200, a bed temperature at 100 — and reaching one a click at a
 * time is what sends people to the keyboard for a number they were happy to
 * nudge.
 *
 * **Why a document-level listener rather than a directive.** `nexus-number-input`
 * is vendored from the shared UI library, whose checkout this repo hard-resets
 * on install, so its template cannot be edited here; and a directive would have
 * to be added to the imports of every component that renders a number field,
 * where the next one added would silently miss out. One listener covers every
 * call site, present and future. It repeats the button's own `click`, so the
 * step size, the clamping and the Shift/Alt modifiers all stay where they are
 * defined — this adds a cadence and nothing else.
 */
@Injectable({ providedIn: 'root' })
export class StepperRepeat {
  private readonly document = inject(DOCUMENT);

  private button: HTMLButtonElement | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private delay = START_MS;
  private modifiers = { shiftKey: false, altKey: false };

  constructor() {
    // Capture, so a press is seen before the component's own handler runs and
    // whatever it does to the DOM.
    this.document.addEventListener('pointerdown', this.onDown, { capture: true });
    // A press can end anywhere — off the button, outside the window — and any
    // ending stops the repeat.
    this.document.addEventListener('pointerup', this.stop, { capture: true });
    this.document.addEventListener('pointercancel', this.stop, { capture: true });
    this.document.addEventListener('contextmenu', this.stop, { capture: true });
    this.document.defaultView?.addEventListener('blur', this.stop);
  }

  private readonly onDown = (event: PointerEvent): void => {
    if (event.button !== 0) {
      return;
    }
    const target = event.target as HTMLElement | null;
    const button = target?.closest<HTMLButtonElement>('nexus-number-input .step') ?? null;
    if (!button || button.disabled) {
      return;
    }
    this.button = button;
    this.modifiers = { shiftKey: event.shiftKey, altKey: event.altKey };
    this.delay = START_MS;
    this.timer = setTimeout(this.tick, HOLD_MS);
  };

  private readonly tick = (): void => {
    const button = this.button;
    // A stepper disables itself at the limit; stopping there is what keeps a
    // held press from spinning against `min` or `max` forever.
    if (!button || button.disabled || !button.isConnected) {
      this.stop();
      return;
    }
    button.dispatchEvent(new MouseEvent('click', { bubbles: true, ...this.modifiers }));
    this.delay = Math.max(MIN_MS, this.delay * DECAY);
    this.timer = setTimeout(this.tick, this.delay);
  };

  private readonly stop = (): void => {
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    this.button = null;
  };
}
