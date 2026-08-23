import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';

export type SwitchSize = 'sm' | 'md';

/**
 * Design-system toggle switch. A controlled, presentational primitive: the
 * parent owns the `checked` state and updates it in response to
 * `checkedChange`. Renders as a native `<button role="switch">` so keyboard
 * (Space/Enter) and screen readers work without extra wiring.
 *
 * ```html
 * <nexus-switch [checked]="spiral()" (checkedChange)="spiral.set($event)" label="Spiral vase" />
 * ```
 */
@Component({
  selector: 'nexus-switch',
  standalone: true,
  changeDetection: ChangeDetectionStrategy.OnPush,
  styleUrl: './switch.scss',
  host: { '[attr.data-size]': 'size()' },
  template: `
    <button
      type="button"
      role="switch"
      class="switch"
      [attr.aria-checked]="checked()"
      [attr.aria-label]="label() || null"
      [disabled]="disabled()"
      (click)="toggle()"
    >
      <span class="track">
        <span class="thumb"></span>
      </span>
    </button>
  `,
})
export class Switch {
  readonly checked = input(false);
  readonly disabled = input(false);
  readonly size = input<SwitchSize>('md');
  readonly label = input('');
  readonly checkedChange = output<boolean>();

  protected toggle(): void {
    if (this.disabled()) return;
    this.checkedChange.emit(!this.checked());
  }
}
