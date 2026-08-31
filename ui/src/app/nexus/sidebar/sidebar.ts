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
import { Viewport } from '../../services/viewport';

const STORAGE_WIDTH_KEY = 'nexus.sidebar.width';
const STORAGE_COLLAPSED_KEY = 'nexus.sidebar.collapsed';
const DEFAULT_WIDTH = 280;
const MIN_WIDTH = 180;
const MAX_WIDTH = 480;

// Hover-intent delays so a collapsed sidebar only opens/closes deliberately.
const HOVER_OPEN_DELAY_MS = 180;
const HOVER_CLOSE_DELAY_MS = 240;
// How far past the panel's edge the pointer must travel before a peek closes.
// Generous enough that the panel sliding in under a stationary pointer never
// reads as "the pointer left".
const HOVER_LEAVE_GRACE_PX = 32;
// How close to the screen edge a pointer must rest to arm a peek.
const EDGE_ARM_PX = 14;

@Component({
  selector: 'nexus-sidebar',
  standalone: true,
  imports: [Icon],
  templateUrl: './sidebar.component.html',
  styleUrl: './sidebar.component.scss',
  host: {
    '[class.is-collapsed]': 'collapsed()',
    '[class.is-expanded]': 'isExpanded()',
    '[class.is-overlay]': 'isOverlay()',
    '[class.is-dragging]': 'isDragging()',
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
   * Docked (false) reserves layout space; collapsed (true) floats as an overlay.
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
  /** Panel is visible but floating over the scene (collapsed + peeking). */
  protected readonly isOverlay = computed(
    () => this.collapsed() && (this.overlayOpen() || this.hoverPreview()),
  );
  /** A tap/click peek that warrants a dismissable scrim (hover previews don't). */
  protected readonly isScrimOpen = computed(() => this.collapsed() && this.overlayOpen());

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
    });

    this.armEdgeHover();

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
    const next = !this.collapsed();
    this.dockedPreference.set(next);
    this.clearHoverTimers();
    this.stopPointerWatch();
    this.overlayOpen.set(false);
    this.hoverPreview.set(false);
    this.saveCollapsed(next);
  }

  /**
   * Hover-intent open, armed by the pointer's position rather than by any
   * element.
   *
   * An invisible strip along the edge would have to be `pointer-events: auto`
   * to receive `mouseenter`, and the sidebar host is zero-width while collapsed
   * — so that strip lands squarely on the leftmost slice of the 3D scene, for
   * its whole height, swallowing camera drags, click-to-select and (because the
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
        const left = this.el.nativeElement.getBoundingClientRect().left;
        const atEdge = event.clientX >= left && event.clientX <= left + EDGE_ARM_PX;
        // The tab overlaps the arming band, and it is a button: aiming at it
        // should arm a click, not a reveal.
        const overTab =
          event.target instanceof Element && event.target.closest('.sidebar-reveal-hint') !== null;
        if (!atEdge || overTab) {
          this.clearOpenTimer();
          return;
        }
        if (this.hoverOpenTimer !== null) {
          return;
        }
        this.hoverOpenTimer = setTimeout(() => {
          this.hoverOpenTimer = null;
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
   * Close a hover peek by where the pointer actually *is*, not by a `mouseleave`.
   *
   * The panel mounts, unmounts and slides under a stationary pointer, and every
   * one of those emits enter/leave pairs that say nothing about intent — which
   * is how the peek used to oscillate. Measuring the distance from the panel's
   * own edge is immune to all of it: the panel is either under the pointer or
   * it is not, however it got there.
   *
   * The edge is taken from the layout width rather than the animated rect, so a
   * panel still sliding in is judged by where it is going, not where it is.
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
      const edge = this.el.nativeElement.getBoundingClientRect().left + panel.offsetWidth;
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

  /** Tap/click the scrim behind an overlay peek to dismiss it (stays hidden). */
  protected dismissOverlay(): void {
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
    this.clearHoverTimers();
    this.stopPointerWatch();
    this.overlayOpen.set(false);
    this.hoverPreview.set(false);
  }

  private clearOpenTimer(): void {
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
      this.saveWidth(this.el.nativeElement.offsetWidth);
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
      this.saveWidth(this.el.nativeElement.offsetWidth);
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
    this.dragStartWidth = this.el.nativeElement.offsetWidth;
  }

  private applyCssWidth(width: number): void {
    this.el.nativeElement.style.setProperty('--sidebar-w', `${width}px`);
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
