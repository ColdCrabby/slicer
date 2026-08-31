import { DOCUMENT, Injectable, computed, inject, signal } from '@angular/core';

/**
 * The one definition of "this is a phone", for TypeScript.
 *
 * Must stay identical to the `handheld()` mixin in
 * `styles/_breakpoints.scss` — CSS switches the layout over and this service
 * switches the *behaviour* (which controls exist, whether the settings column
 * may dock), so a disagreement shows up as chrome that is styled for one
 * layout and wired for the other.
 */
export const HANDHELD_MEDIA_QUERY =
  '(max-width: 640px), (max-width: 950px) and (max-height: 500px) and (orientation: landscape)';

/** Marks `<html>` while the handheld query matches, for global style overrides. */
export const HANDHELD_CLASS = 'is-handheld';

/**
 * Tracks whether the app is running on a phone-sized viewport.
 *
 * Phones are not merely "small desktops": there is no hover, no room for a
 * docked settings column beside the scene, and the bottom of the screen is the
 * only comfortable place to put a primary action. Components ask this before
 * offering pointer-shaped affordances (a drag gizmo cube, a resize handle) or
 * niche controls that would push the essentials off-screen.
 *
 * The `<html>` class it maintains is what lets a global stylesheet reach into
 * the shared `@coldcrabby/ui` components, whose `:host` rules outrank a plain
 * element selector. Layout itself is driven by media queries rather than this
 * class, so a phone lays out correctly before any script runs.
 */
@Injectable({ providedIn: 'root' })
export class Viewport {
  private readonly document = inject(DOCUMENT);
  private readonly matches = signal(this.query()?.matches ?? false);

  /** True on phone-sized viewports, in either orientation. Reactive. */
  readonly isHandheld = computed(() => this.matches());

  constructor() {
    const query = this.query();
    query?.addEventListener('change', (event) => {
      this.matches.set(event.matches);
      this.syncClass(event.matches);
    });
    this.syncClass(this.matches());
  }

  private query(): MediaQueryList | null {
    const view = this.document.defaultView;
    return view?.matchMedia ? view.matchMedia(HANDHELD_MEDIA_QUERY) : null;
  }

  private syncClass(handheld: boolean): void {
    this.document.documentElement.classList.toggle(HANDHELD_CLASS, handheld);
  }
}
