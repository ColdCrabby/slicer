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
} from '@angular/core';
import { Icon } from '../../shared/icon/icon';

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
    '[class.is-pinned-open]': 'isPinnedOverlayOpen()',
    '[class.is-dragging]': 'isDragging()',
  },
})
export class Sidebar {
  private readonly el = inject(ElementRef<HTMLElement>);
  private readonly renderer = inject(Renderer2);
  private readonly document = inject(DOCUMENT);

  protected readonly collapsed = signal(this.readCollapsed());
  protected readonly pinnedOpen = signal(false);
  protected readonly hovered = signal(false);
  protected readonly isDragging = signal(false);

  protected readonly isExpanded = computed(
    () => !this.collapsed() || this.pinnedOpen() || this.hovered(),
  );
  protected readonly isPinnedOverlayOpen = computed(() => this.collapsed() && this.pinnedOpen());

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
      this.hovered.set(false);
      this.pinnedOpen.set(true);
    }
  }

  protected onCollapseToggle(event: MouseEvent): void {
    event.stopPropagation();
    const next = !this.collapsed();
    this.collapsed.set(next);
    this.clearHoverTimers();
    this.pinnedOpen.set(false);
    // Unpinning: keep the panel open as an overlay peek under the cursor so it
    // doesn't vanish on click; onMouseLeave auto-hides it once the pointer
    // leaves. Docking: hovered is irrelevant (isExpanded is driven by !collapsed).
    this.hovered.set(next);
    this.saveCollapsed(next);
  }

  /** Hover-intent open: only after a short, deliberate hover. */
  protected onMouseEnter(): void {
    if (!this.collapsed() || this.pinnedOpen()) {
      return;
    }
    this.clearCloseTimer();
    if (this.hovered() || this.hoverOpenTimer !== null) {
      return;
    }
    this.hoverOpenTimer = setTimeout(() => {
      this.hoverOpenTimer = null;
      this.hovered.set(true);
    }, HOVER_OPEN_DELAY_MS);
  }

  /** Hover-intent close: a brief grace period so the panel doesn't flicker. */
  protected onMouseLeave(): void {
    this.clearOpenTimer();
    if (!this.hovered()) {
      return;
    }
    this.clearCloseTimer();
    this.hoverCloseTimer = setTimeout(() => {
      this.hoverCloseTimer = null;
      this.hovered.set(false);
    }, HOVER_CLOSE_DELAY_MS);
  }

  protected onOpenPeek(event: MouseEvent): void {
    event.stopPropagation();
    if (!this.collapsed()) {
      return;
    }
    this.clearHoverTimers();
    this.hovered.set(false);
    this.pinnedOpen.set(true);
  }

  @HostListener('document:keydown.escape')
  protected onEscape(): void {
    if (!this.collapsed()) {
      return;
    }
    this.clearHoverTimers();
    this.pinnedOpen.set(false);
    this.hovered.set(false);
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
