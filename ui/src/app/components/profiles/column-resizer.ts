import { ChangeDetectionStrategy, Component, ElementRef, inject, signal } from '@angular/core';
import { BrowserStorage } from '../../services/browser-storage';

const WIDTH_KEY = 'manage.listWidth';

/** What the column may be dragged between, in px. */
const MIN_WIDTH = 240;
const MAX_WIDTH = 560;

/** Keyboard nudge per arrow press, in px. */
const STEP = 16;

/**
 * The drag handle between a manage page's list and its editor.
 *
 * Writes `--mgr-list-w` on the surrounding `.mgr__body`, which is the first
 * grid track, and remembers it. Those lists hold long names — a vendor profile
 * called "Voron 2.4 350 · ABS · 0.6 nozzle" is not a 280px string — and the
 * column that shows them was a fixed range nobody could argue with.
 *
 * Deliberately not a splitter component with slots: the grid already owns the
 * layout, so the handle only has to write one number into it.
 */
@Component({
  selector: 'nexus-column-resizer',
  standalone: true,
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<span class="grip" aria-hidden="true"></span>`,
  styleUrl: './column-resizer.scss',
  host: {
    role: 'separator',
    'aria-orientation': 'vertical',
    'aria-label': 'Resize the list column',
    tabindex: '0',
    '[class.is-dragging]': 'dragging()',
    '(pointerdown)': 'onPointerDown($event)',
    '(keydown)': 'onKeyDown($event)',
  },
})
export class ColumnResizer {
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly storage = inject(BrowserStorage);

  protected readonly dragging = signal(false);

  constructor() {
    const stored = this.storage.getJson<number>(WIDTH_KEY, 'local');
    if (stored !== null) {
      // Applied on construction rather than after render: the grid should come
      // up at the user's width, not snap to it a frame later.
      queueMicrotask(() => this.apply(stored));
    }
  }

  private get body(): HTMLElement | null {
    return this.host.nativeElement.closest<HTMLElement>('.mgr__body');
  }

  protected onPointerDown(event: PointerEvent): void {
    const body = this.body;
    if (!body || event.button !== 0) {
      return;
    }
    event.preventDefault();
    this.host.nativeElement.setPointerCapture(event.pointerId);
    this.dragging.set(true);

    const left = body.getBoundingClientRect().left;
    const move = (e: PointerEvent) => this.apply(e.clientX - left);
    const up = () => {
      this.dragging.set(false);
      this.persist();
      this.host.nativeElement.removeEventListener('pointermove', move);
      this.host.nativeElement.removeEventListener('pointerup', up);
      this.host.nativeElement.removeEventListener('pointercancel', up);
    };
    // Listeners on the captured element, so a fast drag that outruns the
    // pointer keeps resizing instead of dropping the gesture over the editor.
    this.host.nativeElement.addEventListener('pointermove', move);
    this.host.nativeElement.addEventListener('pointerup', up);
    this.host.nativeElement.addEventListener('pointercancel', up);
  }

  /** Arrow keys move it too — a pointer drag is not the only way to aim. */
  protected onKeyDown(event: KeyboardEvent): void {
    const delta = event.key === 'ArrowLeft' ? -STEP : event.key === 'ArrowRight' ? STEP : 0;
    if (delta === 0) {
      return;
    }
    event.preventDefault();
    const list = this.host.nativeElement.closest<HTMLElement>('.mgr__list');
    this.apply((list?.getBoundingClientRect().width ?? MIN_WIDTH) + delta);
    this.persist();
  }

  private apply(width: number): void {
    const clamped = Math.round(Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, width)));
    this.body?.style.setProperty('--mgr-list-w', `${clamped}px`);
  }

  /** Persist at the end of a gesture, not on every frame of it. */
  private persist(): void {
    const value = this.body?.style.getPropertyValue('--mgr-list-w');
    const width = value ? Number.parseInt(value, 10) : Number.NaN;
    if (Number.isFinite(width)) {
      this.storage.writeJson(WIDTH_KEY, width, 'local');
    }
  }
}
