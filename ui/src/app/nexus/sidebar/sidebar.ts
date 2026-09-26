import {
  afterNextRender,
  Component,
  computed,
  DestroyRef,
  DOCUMENT,
  effect,
  ElementRef,
  HostListener,
  inject,
  Renderer2,
  signal,
  viewChild,
} from '@angular/core';
import { Icon } from '@coldcrabby/ui';
import { Panel } from '../../ui/panel/panel';
import { KeyboardShortcuts } from '../../services/keyboard-shortcuts/keyboard-shortcuts';
import { Viewport } from '../../services/viewport';

const STORAGE_WIDTH_KEY = 'nexus.sidebar.width';
const STORAGE_COLLAPSED_KEY = 'nexus.sidebar.collapsed';
const DEFAULT_WIDTH = 280;
/**
 * Narrow enough to be worth dragging to, wide enough that the panel's own lead
 * row still fits across.
 *
 * That row is a label filter on the left and the plate's undo and sync on the
 * right. A label chip cannot wrap when it is the only one, so below this the
 * chip simply grew out of the filter and sat underneath the buttons — at the
 * old 180 it overhung by some 50px. This leaves the filter its icon plus a chip
 * of ordinary length; a single very long label can still outgrow it.
 */
const MIN_WIDTH = 240;
const MAX_WIDTH = 480;

// Hover-intent delays so a collapsed sidebar only opens/closes deliberately.
const HOVER_OPEN_DELAY_MS = 180;
const HOVER_CLOSE_DELAY_MS = 240;
// How far past the panel's edge the pointer must travel before a peek closes.
// Generous enough that the panel sliding in under a stationary pointer never
// reads as "the pointer left".
const HOVER_LEAVE_GRACE_PX = 32;
// How close to the scene's edge a pointer must rest to arm a peek. Wide enough
// to hit without aiming — the reveal handle only hints at this band, it is not
// the target; the hover delay and the held-button check are what keep a pass
// across the edge from opening it.
const EDGE_ARM_PX = 56;
/**
 * The scene's own controls, which the edge band never opens the drawer from:
 * everything floating on the plate, and any control at all.
 */
const SCENE_CHROME = '.shell-actions-layer > *, button, a, input, select, [role="button"]';

@Component({
  selector: 'nexus-sidebar',
  standalone: true,
  imports: [Icon, Panel],
  templateUrl: './sidebar.component.html',
  styleUrl: './sidebar.component.scss',
  host: {
    '[class.is-collapsed]': 'collapsed()',
    '[class.is-expanded]': 'isExpanded()',
    '[class.is-edge-armed]': 'edgeArmed()',
    '[class.is-dragging]': 'isDragging()',
    '[class.panels-unsettled]': '!settled()',
  },
})
export class Sidebar {
  private readonly el = inject(ElementRef<HTMLElement>);
  private readonly renderer = inject(Renderer2);
  private readonly document = inject(DOCUMENT);
  private readonly viewport = inject(Viewport);

  /** The user's docked/hidden preference, honoured wherever there is room. */
  private readonly dockedPreference = signal(this.readCollapsed());
  /**
   * Docked (false) keeps the panel open over the scene, which pads its content
   * clear of it; collapsed (true) hides it until peeked.
   *
   * A phone is never wide enough to dock: 280px of settings beside a 390px
   * screen leaves no scene to settle them against. The stored preference is
   * kept rather than overwritten, so the same browser docks again the moment it
   * is wide enough.
   */
  protected readonly collapsed = computed(
    () => this.viewport.isHandheld() || this.dockedPreference(),
  );
  /** A deliberate tap/click peek that persists until dismissed (scrim/Escape/toggle). */
  protected readonly overlayOpen = signal(false);
  /** An ephemeral hover preview (pointer-capable devices only); closes on leave. */
  protected readonly hoverPreview = signal(false);
  protected readonly isDragging = signal(false);
  /** Past first render; see `.panels-unsettled` in styles/components/_panels.scss. */
  protected readonly settled = signal(false);

  /** Whether the content has been scrolled far enough to offer a "scroll to top". */
  protected readonly showScrollTop = signal(false);

  private readonly scrollContainer = viewChild<ElementRef<HTMLElement>>('scrollContainer');
  private readonly panel = viewChild<ElementRef<HTMLElement>>('panel');

  // Only arm hover-intent on devices that truly hover. iOS/iPadOS emit synthetic
  // mouse events on tap with no matching `mouseleave`, which would otherwise leave
  // a preview stuck open — the exact "sidebar won't close" bug on touch.
  private readonly supportsHover =
    typeof window !== 'undefined' && !!window.matchMedia?.('(hover: hover)').matches;

  protected readonly isExpanded = computed(
    () => !this.collapsed() || this.overlayOpen() || this.hoverPreview(),
  );
  /** The pointer is resting in the edge band, and a peek is about to open. */
  protected readonly edgeArmed = signal(false);
  /** A tap/click peek, which is the kind that needs dismissing (hover ones close themselves). */
  protected readonly isPinnedPeek = computed(() => this.collapsed() && this.overlayOpen());

  private dragStartX = 0;
  private dragStartWidth = 0;
  private dragCleanup: (() => void)[] = [];
  private hoverOpenTimer: ReturnType<typeof setTimeout> | null = null;
  private hoverCloseTimer: ReturnType<typeof setTimeout> | null = null;
  /** Tears down the document pointer listeners that close a hover peek. */
  private pointerWatch: (() => void) | null = null;
  private readonly destroyRef = inject(DestroyRef);

  constructor() {
    afterNextRender(() => {
      this.applyCssWidth(this.readWidth());
      // A beat after the stored width and docked state are in, so neither is
      // animated into place on the way in.
      setTimeout(() => this.settled.set(true), 60);
    });

    this.armEdgeHover();
    this.armOutsideDismiss();

    const shortcuts = inject(KeyboardShortcuts);
    const ref = { toggle: () => this.toggle() };
    shortcuts.printSettingsRef = ref;
    this.destroyRef.onDestroy(() => {
      if (shortcuts.printSettingsRef === ref) {
        shortcuts.printSettingsRef = null;
      }
    });

    this.destroyRef.onDestroy(() => {
      this.clearHoverTimers();
      this.stopPointerWatch();
      for (const fn of this.dragCleanup) {
        fn();
      }
    });
  }

  /** Reveal the panel (used by the settings-search shortcut). Pins it open. */
  expand(): void {
    this.clearHoverTimers();
    this.stopPointerWatch();
    if (this.collapsed()) {
      this.hoverPreview.set(false);
      this.overlayOpen.set(true);
    }
  }

  /**
   * The keyboard's toggle: dock or hide where there is room to dock, and open
   * or close the drawer on a phone, where there is not.
   */
  toggle(): void {
    if (this.viewport.isHandheld()) {
      if (this.isExpanded()) {
        this.dismissOverlay();
      } else {
        this.expand();
      }
      return;
    }
    const next = !this.collapsed();
    this.dockedPreference.set(next);
    this.dismissOverlay();
    this.saveCollapsed(next);
  }

  /** Track scroll depth so the floating "scroll to top" affordance can appear. */
  protected onContentScroll(event: Event): void {
    const top = (event.target as HTMLElement).scrollTop;
    this.showScrollTop.set(top > 240);
  }

  /** Smoothly return the content to the top — quick access to the preset controls. */
  scrollToTop(): void {
    this.scrollContainer()?.nativeElement.scrollTo({ top: 0, behavior: 'smooth' });
  }

  /**
   * Dock ⇄ hide toggle. Fully deterministic on every platform: it flips the
   * persisted docked state and clears any transient peek, so tapping it always
   * does exactly what it says — no reliance on a follow-up `mouseleave`.
   */
  protected onCollapseToggle(event: MouseEvent): void {
    event.stopPropagation();
    this.toggle();
  }

  /**
   * Hover-intent open, armed by the pointer's position rather than by any
   * element.
   *
   * An invisible strip along the edge would have to be `pointer-events: auto`
   * to receive `mouseenter` — so that strip lands squarely on the leftmost
   * slice of the 3D scene, for its whole height, swallowing camera drags, click-to-select and (because the
   * sidebar is a *sibling* of `<main>`) file drops. Reading `clientX` instead
   * costs one comparison per move and lays nothing over the plate.
   *
   * Registered only while the panel is collapsed and hidden, so a docked or
   * open sidebar carries no listener at all.
   */
  private armEdgeHover(): void {
    effect((onCleanup) => {
      if (!this.supportsHover || !this.collapsed() || this.isExpanded()) {
        return;
      }
      const onMove = (event: PointerEvent): void => {
        // A held button means a gesture is already in flight. Orbiting the plate
        // and dragging past the rail is not a request for the settings panel —
        // and having it slide out mid-drag is the one way the peek can cover
        // the very thing the hand is working on.
        if (event.buttons !== 0) {
          this.clearOpenTimer();
          return;
        }
        // The band is the scene's own edge, bounded by the scene's height: the
        // titlebar and the nav rail sit in the same column of pixels and have
        // nothing to do with the settings drawer.
        const rect = this.el.nativeElement.getBoundingClientRect();
        const atEdge =
          event.clientX >= rect.left &&
          event.clientX <= rect.left + EDGE_ARM_PX &&
          event.clientY >= rect.top &&
          event.clientY <= rect.bottom;
        // Only over the plate itself. The toolbar, the objects list and the
        // tool cards float inside the same band, and a pointer on its way to
        // one of their buttons is not asking for the settings — opening the
        // drawer then puts it over the very button being reached for.
        const overChrome =
          event.target instanceof Element &&
          event.target.closest(SCENE_CHROME) !== null &&
          event.target.closest('.sidebar-reveal-hint') === null;
        if (!atEdge || overChrome) {
          this.clearOpenTimer();
          return;
        }
        if (this.hoverOpenTimer !== null) {
          return;
        }
        this.edgeArmed.set(true);
        this.hoverOpenTimer = setTimeout(() => {
          this.hoverOpenTimer = null;
          this.edgeArmed.set(false);
          this.hoverPreview.set(true);
          this.watchPointerForLeave();
        }, HOVER_OPEN_DELAY_MS);
      };
      this.document.addEventListener('pointermove', onMove);
      onCleanup(() => {
        this.document.removeEventListener('pointermove', onMove);
        this.clearOpenTimer();
      });
    });
  }

  /**
   * Close a tap/click peek when the next press lands somewhere else — without
   * consuming that press.
   *
   * The obvious implementation is a transparent full-screen button, and it is
   * the wrong one: invisible or not, it is still a button over the entire
   * window, so the press that dismissed the drawer never reached the plate
   * behind it and every select, move or camera drag after a peek cost two
   * gestures. A passive listener closes the panel and lets the same press land
   * on whatever it was aimed at.
   *
   * Two things count as inside. The host covers the panel, its dock nub and
   * the reveal handle. The floating container covers the popovers the panel's own
   * selects and tooltips open, which the floating service renders at body level
   * and which are therefore "outside" by DOM position while being the panel by
   * every other measure.
   *
   * Capture phase, because the G-code inspector stops `pointerdown` from
   * bubbling; without it, reaching for the legend would leave the drawer open
   * over the plate.
   */
  private armOutsideDismiss(): void {
    effect((onCleanup) => {
      if (!this.isPinnedPeek()) {
        return;
      }
      const onDown = (event: PointerEvent): void => {
        const target = event.target;
        if (!(target instanceof Node) || this.el.nativeElement.contains(target)) {
          return;
        }
        if (target instanceof Element && target.closest('.nexus-floating-container') !== null) {
          return;
        }
        this.dismissOverlay();
      };
      this.document.addEventListener('pointerdown', onDown, { capture: true });
      onCleanup(() => this.document.removeEventListener('pointerdown', onDown, { capture: true }));
    });
  }

  /**
   * Close a hover peek by where the pointer actually *is*, not by a `mouseleave`.
   *
   * The panel mounts, unmounts and slides under a stationary pointer, and every
   * one of those emits enter/leave pairs that say nothing about intent — which
   * is how the peek used to oscillate. Measuring the distance from the panel's
   * own edge is immune to all of it: the panel is either under the pointer or
   * it is not, however it got there.
   *
   * The edge is taken from the layout box rather than the animated rect, so a
   * panel still turning in is judged by where it is going, not where it is.
   */
  private watchPointerForLeave(): void {
    if (this.pointerWatch) {
      return;
    }
    const onMove = (event: PointerEvent): void => {
      const panel = this.panel()?.nativeElement;
      if (!panel) {
        return;
      }
      const edge =
        this.el.nativeElement.getBoundingClientRect().left + panel.offsetLeft + panel.offsetWidth;
      if (event.clientX <= edge + HOVER_LEAVE_GRACE_PX) {
        this.clearCloseTimer();
        return;
      }
      if (this.hoverCloseTimer !== null) {
        return;
      }
      this.hoverCloseTimer = setTimeout(() => {
        this.hoverCloseTimer = null;
        this.hoverPreview.set(false);
        this.stopPointerWatch();
      }, HOVER_CLOSE_DELAY_MS);
    };
    // Leaving the window entirely is an unambiguous "done with it" — but so is
    // an element being unmounted under the pointer, and both arrive as a
    // `pointerout` with no `relatedTarget`. The tab and the edge strip are both
    // unmounted the instant the peek opens, so taking that at face value would
    // close the panel the moment it opened: the very oscillation this watcher
    // exists to end. A removed node is no longer connected; a window-leave
    // target still is.
    const onOut = (event: PointerEvent): void => {
      const target = event.target as Node | null;
      if (event.relatedTarget !== null || (target !== null && !target.isConnected)) {
        return;
      }
      this.hoverPreview.set(false);
      this.stopPointerWatch();
    };
    this.document.addEventListener('pointermove', onMove);
    this.document.addEventListener('pointerout', onOut);
    this.pointerWatch = () => {
      this.document.removeEventListener('pointermove', onMove);
      this.document.removeEventListener('pointerout', onOut);
    };
  }

  private stopPointerWatch(): void {
    this.clearCloseTimer();
    this.pointerWatch?.();
    this.pointerWatch = null;
  }

  /** Reveal-hint tap: open a persistent overlay peek (the primary touch gesture). */
  protected onOpenPeek(event: MouseEvent): void {
    event.stopPropagation();
    if (!this.collapsed()) {
      return;
    }
    this.clearHoverTimers();
    this.stopPointerWatch();
    this.hoverPreview.set(false);
    this.overlayOpen.set(true);
  }

  /** The drawer's own close button — a tap outside works too, but is not obvious. */
  protected onDone(): void {
    this.dismissOverlay();
  }

  /** Put a peek away, whichever kind it was, and stop anything that could reopen it. */
  private dismissOverlay(): void {
    this.clearHoverTimers();
    this.stopPointerWatch();
    this.overlayOpen.set(false);
    this.hoverPreview.set(false);
  }

  @HostListener('document:keydown.escape')
  protected onEscape(): void {
    if (!this.collapsed()) {
      return;
    }
    this.dismissOverlay();
  }

  private clearOpenTimer(): void {
    this.edgeArmed.set(false);
    if (this.hoverOpenTimer !== null) {
      clearTimeout(this.hoverOpenTimer);
      this.hoverOpenTimer = null;
    }
  }

  private clearCloseTimer(): void {
    if (this.hoverCloseTimer !== null) {
      clearTimeout(this.hoverCloseTimer);
      this.hoverCloseTimer = null;
    }
  }

  private clearHoverTimers(): void {
    this.clearOpenTimer();
    this.clearCloseTimer();
  }

  protected onResizeStart(event: MouseEvent): void {
    if (this.collapsed()) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    this.startResize(event.clientX);

    let rafId: number | null = null;
    let latestX = event.clientX;

    const onMove = (e: MouseEvent): void => {
      latestX = e.clientX;
      if (rafId !== null) {
        return;
      }
      rafId = requestAnimationFrame(() => {
        rafId = null;
        const delta = latestX - this.dragStartX;
        const width = Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, this.dragStartWidth + delta));
        this.applyCssWidth(width);
      });
    };

    const onUp = (): void => {
      if (rafId !== null) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }
      this.document.removeEventListener('mousemove', onMove);
      this.document.removeEventListener('mouseup', onUp);
      this.isDragging.set(false);
      this.saveWidth(this.panelWidth());
    };

    this.document.addEventListener('mousemove', onMove);
    this.document.addEventListener('mouseup', onUp);
    this.dragCleanup.push(
      () => this.document.removeEventListener('mousemove', onMove),
      () => this.document.removeEventListener('mouseup', onUp),
    );
  }

  protected onResizeTouchStart(event: TouchEvent): void {
    if (this.collapsed()) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    const touch = event.touches[0];
    this.startResize(touch.clientX);

    let rafId: number | null = null;
    let latestX = touch.clientX;

    const onMove = (e: TouchEvent): void => {
      latestX = e.touches[0].clientX;
      if (rafId !== null) {
        return;
      }
      rafId = requestAnimationFrame(() => {
        rafId = null;
        const delta = latestX - this.dragStartX;
        const width = Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, this.dragStartWidth + delta));
        this.applyCssWidth(width);
      });
    };

    const onEnd = (): void => {
      if (rafId !== null) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }
      this.document.removeEventListener('touchmove', onMove);
      this.document.removeEventListener('touchend', onEnd);
      this.document.removeEventListener('touchcancel', onEnd);
      this.isDragging.set(false);
      this.saveWidth(this.panelWidth());
    };

    this.document.addEventListener('touchmove', onMove, { passive: false });
    this.document.addEventListener('touchend', onEnd);
    this.document.addEventListener('touchcancel', onEnd);
    this.dragCleanup.push(
      () => this.document.removeEventListener('touchmove', onMove),
      () => this.document.removeEventListener('touchend', onEnd),
      () => this.document.removeEventListener('touchcancel', onEnd),
    );
  }

  private startResize(clientX: number): void {
    this.isDragging.set(true);
    this.dragStartX = clientX;
    this.dragStartWidth = this.panelWidth();
  }

  private panelWidth(): number {
    return this.panel()?.nativeElement.offsetWidth ?? DEFAULT_WIDTH;
  }

  /**
   * Written on the parent, not the host: the scene beside the sidebar pads its
   * content by the same number while the panel is docked, and a custom property
   * on the host would be invisible to its sibling.
   */
  private applyCssWidth(width: number): void {
    const target = this.el.nativeElement.parentElement ?? this.el.nativeElement;
    target.style.setProperty('--sidebar-w', `${width}px`);
  }

  private readWidth(): number {
    try {
      const stored = localStorage.getItem(STORAGE_WIDTH_KEY);
      if (stored) {
        const parsed = parseInt(stored, 10);
        if (!Number.isNaN(parsed)) {
          return Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, parsed));
        }
      }
    } catch {
      // storage unavailable
    }
    return DEFAULT_WIDTH;
  }

  private saveWidth(width: number): void {
    try {
      localStorage.setItem(STORAGE_WIDTH_KEY, String(width));
    } catch {
      // storage unavailable
    }
  }

  private readCollapsed(): boolean {
    try {
      return localStorage.getItem(STORAGE_COLLAPSED_KEY) === 'true';
    } catch {
      return false;
    }
  }

  private saveCollapsed(value: boolean): void {
    try {
      localStorage.setItem(STORAGE_COLLAPSED_KEY, String(value));
    } catch {
      // storage unavailable
    }
  }
}
