import { DOCUMENT, Injectable, inject } from '@angular/core';

/** How long a press must be held before it starts repeating. */
const HOLD_MS = 400;

/** Interval of the first repeat, and the floor it accelerates towards. */
const START_MS = 220;
const MIN_MS = 35;

/** Each repeat is this much quicker than the one before it. */
const DECAY = 0.82;

/**
 * Buttons that keep stepping while they are held down.
 *
 * Two families, because they are the same control: the shared number field's
 * `+` / `−`, and the G-code preview's layer and progress arrows. Both mean
 * "nudge this value by one", and in both a value far from where you are is
 * reached by holding rather than by tapping thirty times.
 *
 * **Keep in sync with the matching rule in `styles/base/_reset.scss`**, which
 * suppresses iOS's long-press callout on exactly these elements. The G-code arm
 * is scoped to its component rather than a bare `.step-btn`, so an unrelated
 * class of the same name never silently acquires a repeat.
 */
export const REPEAT_BUTTON_SELECTOR = 'nexus-number-input .step, nexus-slice-segment-bar .step-btn';

/**
 * Hold a stepper to keep stepping, faster the longer you hold.
 *
 * Settings are full of values that live a long way from their default — a skirt
 * distance near 200, a bed temperature at 100 — and reaching one a click at a
 * time is what sends people to the keyboard for a number they were happy to
 * nudge. Scrubbing to layer 180 one arrow-tap at a time is the same problem.
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
    this.document.addEventListener('contextmenu', this.onContextMenu, { capture: true });
    this.document.defaultView?.addEventListener('blur', this.stop);
  }

  private readonly onDown = (event: PointerEvent): void => {
    if (event.button !== 0) {
      return;
    }
    const target = event.target as HTMLElement | null;
    const button = target?.closest<HTMLButtonElement>(REPEAT_BUTTON_SELECTOR) ?? null;
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

  /**
   * A held press *is* the gesture here, and a touchscreen reads a held press as
   * a request for a context menu — Android Chrome raises `contextmenu` at about
   * 500 ms, barely after the repeat starts, which killed the hold the moment it
   * began. Swallow it over a repeat button: neither the OS menu nor a right
   * click has anything to offer on a `+`, and the hold survives.
   *
   * iOS never raises `contextmenu` for a long press at all; there the callout
   * bar is what appears, and `_reset.scss` is what suppresses it.
   *
   * Anywhere else a context menu still ends the repeat, because the menu is
   * then what the press meant.
   */
  private readonly onContextMenu = (event: MouseEvent): void => {
    const target = event.target as HTMLElement | null;
    if (target?.closest(REPEAT_BUTTON_SELECTOR)) {
      event.preventDefault();
      return;
    }
    this.stop();
  };

  private readonly stop = (): void => {
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    this.button = null;
  };
}
