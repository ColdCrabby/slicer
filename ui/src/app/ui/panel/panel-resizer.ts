import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  afterNextRender,
  inject,
  input,
  signal,
} from '@angular/core';
import { BrowserStorage } from '../../services/browser-storage';

/** Keyboard nudge per arrow press, in px. */
const STEP = 16;

/**
 * The handle between two panels: three dots standing in the gap, dragged to
 * resize the panel before it.
 *
 * It writes one CSS custom property (`property`) on its parent — the layout
 * that owns the panels — and the layout feeds that into its own track. It does
 * not measure or move the panels itself, so it works the same in a grid, a
 * flex row, or anywhere else a width is a variable.
 *
 * Place it next to the panel it sizes — straight after it (`sizes="before"`,
 * the default) or straight before it (`sizes="after"`, for a panel anchored to
 * the right) — and position it over the gap (see `panel-resizer.scss`): it
 * takes no room of its own. With `storageKey` the width is remembered across
 * sessions.
 */
@Component({
  selector: 'nexus-panel-resizer',
  standalone: true,
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<span class="panel-grip" aria-hidden="true"><i></i></span>`,
  styleUrl: './panel-resizer.scss',
  host: {
    class: 'panel-grip-host',
    role: 'separator',
    'aria-orientation': 'vertical',
    '[attr.aria-label]': 'label()',
    '[attr.aria-valuemin]': 'min()',
    '[attr.aria-valuemax]': 'max()',
    tabindex: '0',
    '[class.is-dragging]': 'dragging()',
    '[class.sizes-after]': "sizes() === 'after'",
    '(pointerdown)': 'onPointerDown($event)',
    '(keydown)': 'onKeyDown($event)',
  },
})
export class PanelResizer {
  /** The custom property the width is written to, on the parent element. */
  readonly property = input.required<string>();
  /** What the panel may be dragged between, in px. */
  readonly min = input(240);
  readonly max = input(560);
  /** Where to remember the width; omitted, it lasts only as long as the page. */
  readonly storageKey = input<string>();
  readonly label = input('Resize panel');
  /** Which neighbour is sized: the panel before the handle, or the one after. */
  readonly sizes = input<'before' | 'after'>('before');

  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly storage = inject(BrowserStorage);

  protected readonly dragging = signal(false);

  constructor() {
    // Before the first paint the user sees, so the layout comes up at their
    // width rather than snapping to it a frame later.
    afterNextRender(() => {
      const key = this.storageKey();
      const stored = key ? this.storage.getJson<number>(key, 'local') : null;
      if (stored !== null) {
        this.apply(stored);
      }
    });
  }

  private get layout(): HTMLElement | null {
    return this.host.nativeElement.parentElement;
  }

  /** The panel being sized: the neighbour named by `sizes`. */
  private get panel(): HTMLElement | null {
    const el = this.host.nativeElement;
    return (
      this.sizes() === 'after' ? el.nextElementSibling : el.previousElementSibling
    ) as HTMLElement | null;
  }

  protected onPointerDown(event: PointerEvent): void {
    const panel = this.panel;
    if (!panel || event.button !== 0) {
      return;
    }
    event.preventDefault();
    const el = this.host.nativeElement;
    el.setPointerCapture(event.pointerId);
    this.dragging.set(true);

    // Measured from the panel's far edge, which stays put while the near one
    // follows the pointer.
    const rect = panel.getBoundingClientRect();
    const after = this.sizes() === 'after';
    const move = (e: PointerEvent) =>
      this.apply(after ? rect.right - e.clientX : e.clientX - rect.left);
    const up = () => {
      this.dragging.set(false);
      this.persist();
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      el.removeEventListener('pointercancel', up);
    };
    // On the captured element, so a fast drag that outruns the pointer keeps
    // resizing instead of dropping the gesture over the next panel.
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
    el.addEventListener('pointercancel', up);
  }

  /** Arrow keys move it too — a pointer drag is not the only way to aim. */
  protected onKeyDown(event: KeyboardEvent): void {
    const toward = event.key === 'ArrowLeft' ? -STEP : event.key === 'ArrowRight' ? STEP : 0;
    if (toward === 0) {
      return;
    }
    // Arrows move the handle; a panel after it grows as the handle goes left.
    const delta = this.sizes() === 'after' ? -toward : toward;
    event.preventDefault();
    this.apply((this.panel?.getBoundingClientRect().width ?? this.min()) + delta);
    this.persist();
  }

  private apply(width: number): void {
    const clamped = Math.round(Math.min(this.max(), Math.max(this.min(), width)));
    this.layout?.style.setProperty(this.property(), `${clamped}px`);
    this.host.nativeElement.setAttribute('aria-valuenow', String(clamped));
  }

  /** Persist at the end of a gesture, not on every frame of it. */
  private persist(): void {
    const key = this.storageKey();
    const value = this.layout?.style.getPropertyValue(this.property());
    const width = value ? Number.parseInt(value, 10) : Number.NaN;
    if (key && Number.isFinite(width)) {
      this.storage.writeJson(key, width, 'local');
    }
  }
}
