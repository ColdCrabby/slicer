import { FocusMonitor } from '@angular/cdk/a11y';
import { DestroyRef, Directive, ElementRef, HostListener, inject, input } from '@angular/core';
import type { Subscription } from 'rxjs';
import { FloatingService } from '../floating';
import type { FloatingComponentRef } from '../floating';
import { UserInputModality } from '../input-modality/input-modality';
import { Tooltip } from './tooltip';

const MOUSE_DELAY_MS = 600;
const PEN_HOVER_DELAY_MS = 300;

/**
 * Attaches a positioned tooltip to any host element.
 *
 * Usage: <button [tooltip]="'Reset view'">…</button>
 *
 * Behaviour is driven by the active input modality (via InputModality):
 *
 *   mouse    — show after a short hover delay; hide on mouse-leave.
 *              Keyboard focus/blur events are ignored.
 *
 *   keyboard — show immediately on focus; hide on blur.
 *              Mouse enter/leave events are ignored.
 *
 *   touch    — tooltips are suppressed for finger touches.
 *              However, stylus hover (Apple Pencil, Surface Pen, etc.) is
 *              treated like a mouse hover and shows the tooltip after a
 *              short delay — these devices report `pointerType === 'pen'`
 *              on `pointerenter` while the tip hovers above the screen.
 *
 * Positioning is handled by the shared FloatingService (Floating UI) so the
 * panel flips and shifts to stay on-screen even near viewport edges, and the
 * inline variant renders a pointing arrow.
 */
@Directive({
  selector: '[tooltip]',
})
export class TooltipDirective {
  readonly tooltip = input.required<string>();
  /** Keyboard shortcut hint displayed alongside the tooltip text (e.g. `'Ctrl+Z'`). */
  readonly tooltipShortcut = input<string | undefined>(undefined);
  /** 'inline' — single-line floating label above the host (default).
   *  'block'  — wider markdown-rendered card anchored to the right of the host. */
  readonly tooltipMode = input<'inline' | 'block'>('inline');
  /**
   * When true, clicking the host element toggles the tooltip open/closed.
   * Useful for info icons on touch devices where hover is not practical.
   * The tooltip dismisses on an outside click or Escape.
   */
  readonly tooltipClickToggle = input<boolean>(false);

  private readonly floating = inject(FloatingService);
  private readonly elementRef = inject(ElementRef<HTMLElement>);
  private readonly focusMonitor = inject(FocusMonitor);
  private readonly inputModality = inject(UserInputModality);
  private readonly destroyRef = inject(DestroyRef);

  private floatingRef: FloatingComponentRef<Tooltip> | null = null;
  private showTimeout: ReturnType<typeof setTimeout> | null = null;
  private modalitySub: Subscription | null = null;
  private clickToggleOpen = false;

  constructor() {
    // Hide immediately whenever the user switches input method.
    // This covers e.g. reaching for the mouse while a keyboard tooltip is open,
    // or tabbing away while a hover tooltip is pending.
    this.modalitySub = this.inputModality.modalityChanged$.subscribe(() => {
      if (!this.clickToggleOpen) {
        this.hide();
      }
    });

    // FocusMonitor emits null when focus leaves, or the origin when it arrives.
    // We only act on keyboard-originated focus; mouse clicks that happen to
    // focus an element are ignored here and handled by the hover listeners.
    this.focusMonitor.monitor(this.elementRef).subscribe((origin) => {
      if (origin === null) {
        if (this.inputModality.modality() === 'keyboard') {
          this.hide();
        }
      } else if (origin === 'keyboard') {
        this.show();
      }
    });

    this.destroyRef.onDestroy(() => {
      this.hide();
      this.modalitySub?.unsubscribe();
      this.focusMonitor.stopMonitoring(this.elementRef);
    });
  }

  @HostListener('mouseenter')
  onMouseEnter(): void {
    if (this.inputModality.modality() !== 'mouse') {
      return;
    }
    this.scheduleShow(MOUSE_DELAY_MS);
  }

  @HostListener('mouseleave')
  onMouseLeave(): void {
    if (this.inputModality.modality() !== 'mouse') {
      return;
    }
    if (this.clickToggleOpen) {
      return;
    }
    this.hide();
  }

  /**
   * Stylus hover (Apple Pencil, Surface Pen, …) fires pointer events with
   * `pointerType === 'pen'` while the tip hovers a few millimeters above the
   * screen, before any contact occurs. We treat that exactly like a mouse
   * hover so users with a pencil on a touch device still get tooltips.
   *
   * Finger touches (`pointerType === 'touch'`) and mouse moves are ignored
   * here — they're handled by the modality-aware mouse listeners above.
   */
  @HostListener('pointerenter', ['$event'])
  onPointerEnter(event: PointerEvent): void {
    if (event.pointerType !== 'pen') {
      return;
    }
    this.scheduleShow(PEN_HOVER_DELAY_MS);
  }

  @HostListener('pointerleave', ['$event'])
  onPointerLeave(event: PointerEvent): void {
    if (event.pointerType !== 'pen') {
      return;
    }
    if (this.clickToggleOpen) {
      return;
    }
    this.hide();
  }

  @HostListener('pointercancel', ['$event'])
  onPointerCancel(event: PointerEvent): void {
    if (event.pointerType !== 'pen') {
      return;
    }
    if (this.clickToggleOpen) {
      return;
    }
    this.hide();
  }

  @HostListener('click')
  onClick(): void {
    if (!this.tooltipClickToggle()) {
      return;
    }
    if (this.clickToggleOpen) {
      this.clickToggleOpen = false;
      this.hide();
    } else {
      this.clickToggleOpen = true;
      this.show();
    }
  }

  @HostListener('keydown.escape')
  onEscape(): void {
    this.clickToggleOpen = false;
    this.hide();
  }

  private scheduleShow(delayMs: number): void {
    if (this.showTimeout !== null) {
      clearTimeout(this.showTimeout);
    }
    this.showTimeout = setTimeout(() => this.show(), delayMs);
  }

  private show(): void {
    if (this.floatingRef) {
      return;
    }

    // Nothing to show — skip the empty chip when both text and shortcut are blank
    // (e.g. a segmented option with no description).
    const hasText = this.tooltip().trim().length > 0;
    const hasShortcut = (this.tooltipShortcut() ?? '').trim().length > 0;
    if (!hasText && !hasShortcut) {
      return;
    }

    const isBlock = this.tooltipMode() === 'block';
    const persistent = this.tooltipClickToggle();
    const dismiss = (): void => {
      this.clickToggleOpen = false;
      this.hide();
    };

    this.floatingRef = this.floating.openComponent(Tooltip, {
      reference: this.elementRef.nativeElement,
      arrow: !isBlock,
      interactive: persistent,
      panelClass: isBlock
        ? ['nexus-tooltip-floating', 'nexus-tooltip-floating--block']
        : 'nexus-tooltip-floating',
      options: isBlock
        ? { placement: 'right-start', offset: 8, padding: 8, size: true }
        : { placement: 'bottom', offset: 6, padding: 8 },
      originElement: persistent ? this.elementRef.nativeElement : undefined,
      onOutsidePointer: persistent ? dismiss : undefined,
      onEscape: persistent ? dismiss : undefined,
    });

    this.floatingRef.setInput('text', this.tooltip());
    this.floatingRef.setInput('mode', this.tooltipMode());
    this.floatingRef.setInput('shortcut', this.tooltipShortcut());
    this.floatingRef.setInput('persistent', persistent);
  }

  private hide(): void {
    if (this.showTimeout !== null) {
      clearTimeout(this.showTimeout);
      this.showTimeout = null;
    }

    this.floatingRef?.close();
    this.floatingRef = null;
  }
}
