import { ChangeDetectionStrategy, Component, computed, input, output, signal } from '@angular/core';

/**
 * Design-system dual-thumb range slider. Two overlapping native
 * `<input type="range">` elements (keyboard + a11y intact) draw a shared groove
 * with an amber fill between the thumbs. Controlled: the parent owns `low`/`high`
 * and updates them from `rangeChange`, which emits a clamped `[low, high]` pair
 * (the thumbs never cross).
 *
 * ```html
 * <nexus-range-slider [low]="lo()" [high]="hi()" unit="°C"
 *   (rangeChange)="lo.set($event[0]); hi.set($event[1])" />
 * ```
 */
@Component({
  selector: 'nexus-range-slider',
  standalone: true,
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './range-slider.html',
  styleUrl: './range-slider.scss',
})
export class RangeSlider {
  readonly min = input(0);
  readonly max = input(100);
  readonly step = input(1);
  readonly low = input(0);
  readonly high = input(100);
  readonly disabled = input(false);
  readonly unit = input('');
  readonly label = input('');
  readonly showValue = input(true);
  readonly rangeChange = output<[number, number]>();

  protected readonly activeThumb = signal<'low' | 'high' | null>(null);

  protected readonly lowPercent = computed(() => this.pct(this.low()));
  protected readonly highPercent = computed(() => this.pct(this.high()));

  // Keep the low thumb reachable when both handles pile up at the maximum,
  // otherwise the high thumb (last in the DOM) sits on top and traps it.
  protected readonly lowOnTop = computed(
    () => this.activeThumb() === 'low' || this.low() >= this.max(),
  );

  private pct(v: number): number {
    const span = this.max() - this.min();
    if (span <= 0) return 0;
    return Math.max(0, Math.min(100, ((v - this.min()) / span) * 100));
  }

  protected onLowInput(el: HTMLInputElement): void {
    // Clamp against the high thumb and write the value straight back to the
    // native input — when the clamp is a no-op the bound value() doesn't change,
    // so Angular won't reset the thumb and it would visually drift past high.
    const clamped = Math.min(Number(el.value), this.high());
    el.value = String(clamped);
    if (clamped !== this.low()) this.rangeChange.emit([clamped, this.high()]);
  }

  protected onHighInput(el: HTMLInputElement): void {
    const clamped = Math.max(Number(el.value), this.low());
    el.value = String(clamped);
    if (clamped !== this.high()) this.rangeChange.emit([this.low(), clamped]);
  }
}
