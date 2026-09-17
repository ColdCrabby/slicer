import { DOCUMENT, Injectable, computed, inject, signal, type WritableSignal } from '@angular/core';

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

/**
 * Touch (or stylus) as the primary input. Mirrors the `coarse-pointer()` mixin.
 *
 * Deliberately independent of width: an iPad Pro is wider than most laptops and
 * still has no cursor to hover with. It says nothing about *precision* — see
 * {@link Viewport.isFingertip} for that.
 */
export const COARSE_POINTER_MEDIA_QUERY = '(pointer: coarse)';

/**
 * Viewports that cannot leave the floating scene chrome permanently open.
 * Mirrors the `compact()` mixin — keep the three in sync.
 */
export const COMPACT_MEDIA_QUERY = '(max-width: 1024px), (pointer: coarse)';

/** Marks `<html>` while the handheld query matches, for global style overrides. */
export const HANDHELD_CLASS = 'is-handheld';

/** Marks `<html>` while the pointer is coarse, for the same reason. */
export const COARSE_POINTER_CLASS = 'is-coarse-pointer';

/** Marks `<html>` while a stylus is the pointer in use. */
export const STYLUS_CLASS = 'is-stylus';

/**
 * How long a pen contact keeps its claim on the pointer after it lifts.
 *
 * A hand resting on the glass mid-stroke fires `touch` events of its own, and
 * demoting on the first of those would resize the entire interface under a
 * Pencil that never left the screen. The viewer's palm rejection answers the
 * same question the same way — a pen outranks a touch that overlaps it — and
 * this is the chrome's copy of that rule, with a window wide enough to span the
 * pauses inside one drawing session.
 */
const STYLUS_HOLD_MS = 1500;

/**
 * Tracks how much room and how much pointer precision the app has to work with.
 *
 * Phones are not merely "small desktops": there is no hover, no room for a
 * docked settings column beside the scene, and the bottom of the screen is the
 * only comfortable place to put a primary action. Components ask this before
 * offering pointer-shaped affordances (a drag gizmo cube, a resize handle) or
 * niche controls that would push the essentials off-screen.
 *
 * Tablets are the case that needs three answers rather than one. An iPad has a
 * desktop's width and a phone's input, so a single "is it small?" flag gets it
 * wrong either way: treat it as a phone and it loses the docked column it has
 * ample room for; treat it as a desktop — which is what a lone `isHandheld`
 * did — and every panel hovering over the plate stays open forever, with no
 * cursor to dismiss it and targets sized for a mouse. Hence the split:
 *
 * | Signal            | Answers                                        |
 * | ----------------- | ---------------------------------------------- |
 * | `isHandheld`      | may the layout keep its desktop shape?         |
 * | `isCompact`       | must chrome floating over the scene fold away? |
 * | `isCoarsePointer` | is there a cursor to hover and right-click with? |
 * | `isFingertip`     | how big must a target be?                      |
 *
 * The last two look like one question and are not. A Pencil is coarse — it has
 * no hover state and no modifier keys, so the affordances that stand in for
 * those still belong on screen — and it is also the most accurate pointer the
 * app ever sees, so the targets it aims at should be no larger than a mouse's.
 *
 * The `<html>` classes it maintains are what let a global stylesheet reach into
 * the shared `@coldcrabby/ui` components, whose `:host` rules outrank a plain
 * element selector. Layout itself is driven by media queries rather than these
 * classes, so a phone lays out correctly before any script runs.
 */
@Injectable({ providedIn: 'root' })
export class Viewport {
  private readonly document = inject(DOCUMENT);

  /**
   * Live `MediaQueryList`s, retained deliberately.
   *
   * A `MediaQueryList` is only kept alive by something holding a reference to
   * it — registering a `change` listener is not enough, and a collected list
   * stops firing. Dropping these on the floor leaves the signals frozen at
   * whatever they read during construction, which looks exactly like "the
   * breakpoint does not work" the first time the window is resized.
   */
  private readonly lists: MediaQueryList[] = [];

  /** When a pen last touched or hovered the glass, for {@link STYLUS_HOLD_MS}. */
  private lastPenMs = 0;

  private readonly stylusActive = signal(false);
  private readonly handheldMatches = signal(this.query(HANDHELD_MEDIA_QUERY)?.matches ?? false);
  private readonly coarseMatches = signal(this.query(COARSE_POINTER_MEDIA_QUERY)?.matches ?? false);
  private readonly compactMatches = signal(this.query(COMPACT_MEDIA_QUERY)?.matches ?? false);

  /** True on phone-sized viewports, in either orientation. Reactive. */
  readonly isHandheld = computed(() => this.handheldMatches());

  /**
   * True where a finger (or pencil) is the pointer — every iPhone and iPad, and
   * a touchscreen laptop. Ask this to size a target, never to choose a layout.
   */
  readonly isCoarsePointer = computed(() => this.coarseMatches());

  /**
   * True while a stylus is the pointer in use — an Apple Pencil, a Surface Pen.
   *
   * `pointer: coarse` cannot answer this: iPadOS reports coarse whether the
   * glass is being touched by a fingertip or by a Pencil, so the only evidence
   * is the `pointerType` of the events actually arriving. Starts false, because
   * a finger is the safe assumption until a pen proves otherwise.
   */
  readonly isStylus = computed(() => this.stylusActive());

  /**
   * True where a *fingertip* is the pointer — coarse, and not a stylus.
   *
   * This is the sizing question, and it is the narrower one:
   * {@link isCoarsePointer} is about what the device can do (no hover, no
   * modifier keys), this is about how accurately the user can aim right now. A
   * Pencil is more precise than a mouse and wants the cursor-sized interface
   * back. Consumed through the `--touch-*` tokens in `styles/base/_touch.scss`
   * rather than read directly — a component's own stylesheet cannot see an
   * `<html>` class, but it inherits a custom property set on one.
   */
  readonly isFingertip = computed(() => this.coarseMatches() && !this.stylusActive());

  /**
   * True where panels floating over the 3D scene should default to folded —
   * because the viewport is narrow, or because there is no cursor to dismiss
   * them with. Implied by {@link isHandheld}.
   */
  readonly isCompact = computed(() => this.compactMatches());

  constructor() {
    this.watch(HANDHELD_MEDIA_QUERY, this.handheldMatches, HANDHELD_CLASS);
    this.watch(COARSE_POINTER_MEDIA_QUERY, this.coarseMatches, COARSE_POINTER_CLASS);
    this.watch(COMPACT_MEDIA_QUERY, this.compactMatches);
    this.watchPointerType();
  }

  /**
   * Keep {@link stylusActive} — and `<html>.is-stylus` — tracking the pointer
   * the user is actually holding.
   *
   * Both `pointerover` and `pointerdown`, because a hover-capable Pencil
   * announces itself on approach and the interface should already be sized by
   * the time it lands; on an iPad without pen hover the first tap does it.
   * Capture phase and passive, so nothing a component does to the event can
   * hide the pointer from this, and listening costs no scroll performance.
   */
  private watchPointerType(): void {
    const view = this.document.defaultView;
    if (!view) {
      return;
    }
    const note = (event: PointerEvent): void => {
      const now = event.timeStamp || Date.now();
      if (event.pointerType === 'pen') {
        this.lastPenMs = now;
        this.setStylus(true);
        return;
      }
      if (event.pointerType === 'touch' && now - this.lastPenMs < STYLUS_HOLD_MS) {
        return;
      }
      this.setStylus(false);
    };
    for (const type of ['pointerover', 'pointerdown'] as const) {
      view.addEventListener(type, note, { capture: true, passive: true });
    }
  }

  private setStylus(active: boolean): void {
    if (this.stylusActive() === active) {
      return;
    }
    this.stylusActive.set(active);
    this.syncClass(STYLUS_CLASS, active);
  }

  /** Keep one signal — and optionally one `<html>` class — tracking one query. */
  private watch(query: string, target: WritableSignal<boolean>, className?: string): void {
    const list = this.query(query);
    if (list) {
      this.lists.push(list);
      list.addEventListener('change', (event) => {
        target.set(event.matches);
        if (className) {
          this.syncClass(className, event.matches);
        }
      });
    }
    if (className) {
      this.syncClass(className, target());
    }
  }

  private query(query: string): MediaQueryList | null {
    const view = this.document.defaultView;
    return view?.matchMedia ? view.matchMedia(query) : null;
  }

  private syncClass(className: string, active: boolean): void {
    this.document.documentElement.classList.toggle(className, active);
  }
}
