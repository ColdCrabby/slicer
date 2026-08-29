import { Directive, ElementRef, NgZone, OnDestroy, inject, output } from '@angular/core';

/**
 * How long a touch must be held before it counts as a long-press. Matches the
 * ~0.5s the native iOS/iPadOS and Android context menus use, so the gesture
 * feels the same in the browser without any custom CSS animation.
 */
const LONG_PRESS_MS = 500;
/** Movement (px) that reclassifies a press as a scroll/drag and cancels it. */
const MOVE_CANCEL_PX = 10;
/**
 * Window after a long-press fires during which a follow-up native `contextmenu`
 * (some browsers, e.g. Android Chrome, emit both) is treated as a duplicate.
 */
const DEDUPE_MS = 700;

/**
 * Opens a context menu on right-click *and* touch long-press.
 *
 * The `contextmenu` DOM event is all a desktop mouse (or a browser that fires it
 * on long-press) needs, but iOS/iPadOS Safari never dispatches `contextmenu` for
 * a long-press on a normal element — so those menus simply never appeared on an
 * iPad. This directive layers a pointer-based long-press recogniser on top of the
 * native event to close that gap while leaning on native behaviour everywhere it
 * already works:
 *
 * - **Mouse:** ignored here; the browser's `contextmenu` event drives the menu.
 * - **Touch / pen:** a held press (no scroll) for {@link LONG_PRESS_MS} opens the
 *   menu at the finger, matching the native long-press timing.
 * - **De-duplication:** browsers that emit *both* a long-press and a
 *   `contextmenu` only open one menu, and the OS menu is always suppressed.
 *
 * Emits the originating pointer/mouse event so the host can forward it straight
 * to {@link ContextMenuService.open}.
 */
@Directive({
  selector: '[nexusContextMenu]',
  standalone: true,
  // Tagged with a class rather than styled through `[nexusContextMenu]`: that
  // selector never matches, because `(nexusContextMenu)="…"` is an *output
  // binding* and Angular does not emit it as a DOM attribute. The class is what
  // lets `styles/base/_reset.scss` suppress iOS's own long-press callout.
  host: { class: 'nexus-context-target' },
})
export class ContextMenuTrigger implements OnDestroy {
  /** Fires when a context menu is requested (right-click or touch long-press). */
  readonly nexusContextMenu = output<MouseEvent>();

  readonly #host = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  readonly #zone = inject(NgZone);

  #timer: ReturnType<typeof setTimeout> | null = null;
  #startX = 0;
  #startY = 0;
  #lastFire = 0;

  constructor() {
    this.#host.addEventListener('contextmenu', this.#onContextMenu);
    this.#host.addEventListener('pointerdown', this.#onPointerDown);
  }

  ngOnDestroy(): void {
    this.#host.removeEventListener('contextmenu', this.#onContextMenu);
    this.#host.removeEventListener('pointerdown', this.#onPointerDown);
    this.#cancel();
  }

  /**
   * Native path: desktop right-click, or a browser that raises `contextmenu` on
   * long-press. Always swallow the OS menu; the de-dupe guard stops a press that
   * already opened our menu from opening a second one.
   */
  readonly #onContextMenu = (event: MouseEvent): void => {
    event.preventDefault();
    this.#fire(event);
  };

  readonly #onPointerDown = (event: PointerEvent): void => {
    // A mouse right-click already arrives via `contextmenu`; only touch and pen
    // need the long-press timer (this is the path iPadOS Safari relies on).
    if (event.pointerType === 'mouse') {
      return;
    }
    this.#startX = event.clientX;
    this.#startY = event.clientY;
    this.#clearTimer();
    this.#zone.runOutsideAngular(() => {
      window.addEventListener('pointermove', this.#onPointerMove, { passive: true });
      window.addEventListener('pointerup', this.#onPointerEnd, { passive: true });
      window.addEventListener('pointercancel', this.#onPointerEnd, { passive: true });
      window.addEventListener('scroll', this.#onPointerEnd, { capture: true, passive: true });
    });
    this.#timer = setTimeout(() => {
      this.#stopTracking();
      this.#suppressTrailingClick();
      this.#zone.run(() => this.#fire(event));
    }, LONG_PRESS_MS);
  };

  readonly #onPointerMove = (event: PointerEvent): void => {
    if (
      Math.abs(event.clientX - this.#startX) > MOVE_CANCEL_PX ||
      Math.abs(event.clientY - this.#startY) > MOVE_CANCEL_PX
    ) {
      this.#cancel();
    }
  };

  readonly #onPointerEnd = (): void => this.#cancel();

  #fire(event: MouseEvent): void {
    const now = performance.now();
    if (now - this.#lastFire < DEDUPE_MS) {
      return;
    }
    this.#lastFire = now;
    this.#clearTimer();
    this.nexusContextMenu.emit(event);
  }

  /**
   * Lifting the finger after a long-press still synthesises a `click`. Swallow
   * that one click wherever it lands.
   *
   * Restricting this to the host is not enough: the menu opens in a body-level
   * layer at the pointer, so on touch the click can land on a *menu item* and
   * activate it the instant the finger lifts — the user would never get to
   * choose. Native long-press menus behave the same way: the lift only opens
   * the menu, and a second deliberate tap picks an item.
   */
  #suppressTrailingClick(): void {
    const cleanup = (): void => {
      window.removeEventListener('click', swallow, true);
      clearTimeout(fallback);
    };
    const swallow = (event: Event): void => {
      event.stopPropagation();
      event.preventDefault();
      cleanup();
    };
    const fallback = setTimeout(cleanup, DEDUPE_MS);
    window.addEventListener('click', swallow, true);
  }

  #cancel(): void {
    this.#stopTracking();
    this.#clearTimer();
  }

  #stopTracking(): void {
    window.removeEventListener('pointermove', this.#onPointerMove);
    window.removeEventListener('pointerup', this.#onPointerEnd);
    window.removeEventListener('pointercancel', this.#onPointerEnd);
    window.removeEventListener('scroll', this.#onPointerEnd, {
      capture: true,
    } as EventListenerOptions);
  }

  #clearTimer(): void {
    if (this.#timer !== null) {
      clearTimeout(this.#timer);
      this.#timer = null;
    }
  }
}
