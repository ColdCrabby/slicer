import {
  afterNextRender,
  Component,
  computed,
  DestroyRef,
  DOCUMENT,
  ElementRef,
  HostListener,
  inject,
  Renderer2,
  signal,
  viewChild,
} from '@angular/core';
import { Icon } from '@coldcrabby/ui';

const STORAGE_WIDTH_KEY = 'nexus.sidebar.width';
const STORAGE_COLLAPSED_KEY = 'nexus.sidebar.collapsed';
const DEFAULT_WIDTH = 280;
const MIN_WIDTH = 180;
const MAX_WIDTH = 480;

// Hover-intent delays so a collapsed sidebar only opens/closes deliberately.
const HOVER_OPEN_DELAY_MS = 180;
const HOVER_CLOSE_DELAY_MS = 240;

@Component({
  selector: 'nexus-sidebar',
  standalone: true,
  imports: [Icon],
  templateUrl: './sidebar.component.html',
  styleUrl: './sidebar.component.scss',
  host: {
    '(mouseenter)': 'onMouseEnter()',
    '(mouseleave)': 'onMouseLeave()',
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

  /** Docked (false) reserves layout space; collapsed (true) floats as an overlay. */
  protected readonly collapsed = signal(this.readCollapsed());
  /** A deliberate tap/click peek that persists until dismissed (scrim/Escape/toggle). */
  protected readonly overlayOpen = signal(false);
  /** An ephemeral hover preview (pointer-capable devices only); closes on leave. */
  protected readonly hoverPreview = signal(false);
  protected readonly isDragging = signal(false);

  /** Whether the content has been scrolled far enough to offer a "scroll to top". */
  protected readonly showScrollTop = signal(false);

  private readonly scrollContainer = viewChild<ElementRef<HTMLElement>>('scrollContainer');

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
  private readonly destroyRef = inject(DestroyRef);

  constructor() {
    afterNextRender(() => {
      this.applyCssWidth(this.readWidth());
    });

    this.destroyRef.onDestroy(() => {
      this.clearHoverTimers();
      for (const fn of this.dragCleanup) {
        fn();
      }
    });
  }

  /** Reveal the panel (used by the settings-search shortcut). Pins it open. */
  expand(): void {
    this.clearHoverTimers();
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
    this.collapsed.set(next);
    this.clearHoverTimers();
    this.overlayOpen.set(false);
    this.hoverPreview.set(false);
    this.saveCollapsed(next);
  }

  /** Hover-intent open: only after a short, deliberate hover (pointer devices). */
  protected onMouseEnter(): void {
    if (!this.supportsHover || !this.collapsed() || this.overlayOpen()) {
      return;
    }
    this.clearCloseTimer();
    if (this.hoverPreview() || this.hoverOpenTimer !== null) {
      return;
    }
    this.hoverOpenTimer = setTimeout(() => {
      this.hoverOpenTimer = null;
      this.hoverPreview.set(true);
    }, HOVER_OPEN_DELAY_MS);
  }

  /** Hover-intent close: a brief grace period so the panel doesn't flicker. */
  protected onMouseLeave(): void {
    if (!this.supportsHover) {
      return;
    }
    this.clearOpenTimer();
    if (!this.hoverPreview()) {
      return;
    }
    this.clearCloseTimer();
    this.hoverCloseTimer = setTimeout(() => {
      this.hoverCloseTimer = null;
      this.hoverPreview.set(false);
    }, HOVER_CLOSE_DELAY_MS);
  }

  /** Reveal-hint tap: open a persistent overlay peek (the primary touch gesture). */
  protected onOpenPeek(event: MouseEvent): void {
    event.stopPropagation();
    if (!this.collapsed()) {
      return;
    }
    this.clearHoverTimers();
    this.hoverPreview.set(false);
    this.overlayOpen.set(true);
  }

  /** Tap/click the scrim behind an overlay peek to dismiss it (stays hidden). */
  protected dismissOverlay(): void {
    this.clearHoverTimers();
    this.overlayOpen.set(false);
    this.hoverPreview.set(false);
  }

  @HostListener('document:keydown.escape')
  protected onEscape(): void {
    if (!this.collapsed()) {
      return;
    }
    this.clearHoverTimers();
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
