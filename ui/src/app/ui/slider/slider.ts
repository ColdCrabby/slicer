import { ChangeDetectionStrategy, Component, computed, input, output } from '@angular/core';

/**
 * Design-system range slider. Wraps a native `<input type="range">` so drag,
 * keyboard, and accessibility come for free, then paints a custom filled track
 * and thumb. Controlled: the parent owns `value` and updates it from
 * `valueChange`.
 *
 * ```html
 * <nexus-slider [value]="density()" (valueChange)="density.set($event)" unit="%" />
 * ```
 */
@Component({
  selector: 'nexus-slider',
  standalone: true,
  changeDetection: ChangeDetectionStrategy.OnPush,
  styleUrl: './slider.scss',
  template: `
    <input
      type="range"
      class="range"
      [min]="min()"
      [max]="max()"
      [step]="step()"
      [value]="value()"
      [disabled]="disabled()"
      [attr.aria-label]="label() || null"
      [style.--fill]="fillPercent()"
      (input)="onInput($event)"
    />
    @if (showValue()) {
      <output class="readout">{{ value() }}{{ unit() }}</output>
    }
  `,
})
export class Slider {
  readonly value = input(0);
  readonly min = input(0);
  readonly max = input(100);
  readonly step = input(1);
  readonly disabled = input(false);
  readonly unit = input('');
  readonly label = input('');
  readonly showValue = input(true);
  readonly valueChange = output<number>();

  protected readonly fillPercent = computed(() => {
    const span = this.max() - this.min();
    if (span <= 0) return 0;
    const pct = ((this.value() - this.min()) / span) * 100;
    return Math.max(0, Math.min(100, pct));
  });

  protected onInput(event: Event): void {
    this.valueChange.emit(Number((event.target as HTMLInputElement).value));
  }
}
