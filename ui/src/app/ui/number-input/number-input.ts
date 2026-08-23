import {
  afterNextRender,
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  ElementRef,
  inject,
  input,
  output,
} from '@angular/core';

/**
 * Design-system numeric input with stepper buttons and an optional unit
 * suffix. Wraps a native `<input type="number">` (keyboard + a11y intact),
 * hides the ugly native spinners, and clamps to `min`/`max`. Controlled: the
 * parent owns `value` and updates it from `valueChange`.
 *
 * Adjusting the value:
 * - Stepper buttons, `ArrowUp`/`ArrowDown`, or the mouse wheel change the value
 *   by `step` and emit `valueChange` immediately (no blur required).
 * - **Shift** makes the step coarse (×10); **Alt/⌥** makes it fine (×0.1).
 * - The wheel adjusts while the field is focused; pass `wheelHover` to also
 *   adjust on hover (use only where the field is not inside a scroll area).
 *
 * ```html
 * <nexus-number-input [value]="layer()" (valueChange)="layer.set($event)"
 *   [min]="0.04" [max]="0.6" [step]="0.02" unit="mm" />
 * ```
 */
@Component({
  selector: 'nexus-number-input',
  standalone: true,
  changeDetection: ChangeDetectionStrategy.OnPush,
  styleUrl: './number-input.scss',
  template: `
    <div class="field" [class.is-disabled]="disabled()">
      <button
        type="button"
        class="step"
        tabindex="-1"
        aria-label="Decrease"
        [disabled]="disabled() || value() <= min()"
        (click)="nudge(-1, $event)"
      >
        <span class="glyph">&minus;</span>
      </button>
      <input
        class="input"
        type="number"
        inputmode="decimal"
        [min]="min()"
        [max]="max()"
        [step]="step()"
        [value]="value()"
        [disabled]="disabled()"
        [attr.aria-label]="label() || null"
        (keydown)="onKeydown($event)"
        (change)="onChange($any($event.target).value)"
      />
      @if (unit()) {
        <span class="unit">{{ unit() }}</span>
      }
      <button
        type="button"
        class="step"
        tabindex="-1"
        aria-label="Increase"
        [disabled]="disabled() || value() >= max()"
        (click)="nudge(1, $event)"
      >
        <span class="glyph">+</span>
      </button>
    </div>
  `,
})
export class NumberInput {
  readonly value = input(0);
  readonly min = input(Number.NEGATIVE_INFINITY);
  readonly max = input(Number.POSITIVE_INFINITY);
  readonly step = input(1);
  readonly disabled = input(false);
  readonly unit = input('');
  readonly label = input('');
  /**
   * Decimal places kept when committing a value. Defaults to the precision
   * implied by `step` (so a `step` of `0.02` keeps two decimals). Set this to
   * decouple typed precision from the stepper increment — e.g. `step=1` for a
   * 1 mm nudge while still accepting `12.34` from the keyboard.
   */
  readonly precision = input<number | null>(null);
  /**
   * When true the mouse wheel adjusts the value on hover, not just while the
   * field is focused. Enable only where the field is not inside a scroll
   * container (e.g. a floating panel), so wheeling never hijacks page scroll.
   */
  readonly wheelHover = input(false);
  readonly valueChange = output<number>();

  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);

  // Decimal places implied by the step, so nudging 0.2 by 0.02 stays clean.
  protected readonly decimals = computed(() => {
    const override = this.precision();
    if (override !== null) {
      return override;
    }
    return decimalsOf(this.step());
  });

  constructor() {
    // Wheel must be a non-passive listener so preventDefault can stop page
    // scroll; Angular template `(wheel)` bindings cannot opt out of passive.
    const destroyRef = inject(DestroyRef);
    afterNextRender(() => {
      const el = this.host.nativeElement;
      el.addEventListener('wheel', this.onWheel, { passive: false });
      destroyRef.onDestroy(() => el.removeEventListener('wheel', this.onWheel));
    });
  }

  protected nudge(dir: number, event?: MouseEvent | KeyboardEvent | WheelEvent): void {
    if (this.disabled()) return;
    const effStep = this.step() * stepFactor(event);
    this.commit(this.value() + dir * effStep, effStep);
  }

  protected onKeydown(event: KeyboardEvent): void {
    if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') {
      return;
    }
    // Route arrows through our commit so the modifier step + instant emit
    // apply (native arrow-stepping only fires `change` on blur).
    event.preventDefault();
    this.nudge(event.key === 'ArrowUp' ? 1 : -1, event);
  }

  protected onChange(raw: string): void {
    const next = Number(raw);
    this.commit(Number.isFinite(next) ? next : this.value());
  }

  private readonly onWheel = (event: WheelEvent): void => {
    if (this.disabled() || event.deltaY === 0) {
      return;
    }
    const focused = this.host.nativeElement.contains(document.activeElement);
    if (!focused && !this.wheelHover()) {
      return;
    }
    event.preventDefault();
    this.nudge(event.deltaY < 0 ? 1 : -1, event);
  };

  private commit(next: number, effStep = this.step()): void {
    // Keep enough decimals for the effective step so a fine (×0.1) nudge is
    // never rounded away, while never dropping the field's base precision.
    const dec = Math.max(this.decimals(), decimalsOf(effStep));
    const clamped = Math.min(this.max(), Math.max(this.min(), next));
    const rounded = Number(clamped.toFixed(dec));
    if (rounded !== this.value()) this.valueChange.emit(rounded);
  }
}

/** Number of decimal places in a numeric literal (e.g. `0.02` → 2). */
function decimalsOf(n: number): number {
  const s = String(n);
  const dot = s.indexOf('.');
  return dot === -1 ? 0 : s.length - dot - 1;
}

/** Modifier-driven step multiplier: Shift = coarse ×10, Alt/⌥ = fine ×0.1. */
function stepFactor(event?: { shiftKey?: boolean; altKey?: boolean }): number {
  if (event?.shiftKey) return 10;
  if (event?.altKey) return 0.1;
  return 1;
}
