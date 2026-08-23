import { ChangeDetectionStrategy, Component, computed, input, output } from '@angular/core';

/**
 * Design-system numeric input with stepper buttons and an optional unit
 * suffix. Wraps a native `<input type="number">` (keyboard + a11y intact),
 * hides the ugly native spinners, and clamps to `min`/`max`. Controlled: the
 * parent owns `value` and updates it from `valueChange`.
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
        (click)="nudge(-1)"
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
        (click)="nudge(1)"
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
  readonly valueChange = output<number>();

  // Decimal places implied by the step, so nudging 0.2 by 0.02 stays clean.
  protected readonly decimals = computed(() => {
    const s = String(this.step());
    const dot = s.indexOf('.');
    return dot === -1 ? 0 : s.length - dot - 1;
  });

  protected nudge(dir: number): void {
    if (this.disabled()) return;
    this.commit(this.value() + dir * this.step());
  }

  protected onChange(raw: string): void {
    const next = Number(raw);
    this.commit(Number.isFinite(next) ? next : this.value());
  }

  private commit(next: number): void {
    const clamped = Math.min(this.max(), Math.max(this.min(), next));
    const rounded = Number(clamped.toFixed(this.decimals()));
    if (rounded !== this.value()) this.valueChange.emit(rounded);
  }
}
