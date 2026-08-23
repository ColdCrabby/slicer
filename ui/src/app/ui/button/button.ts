import { ChangeDetectionStrategy, Component, input } from '@angular/core';

export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'subtle' | 'danger';
export type ButtonSize = 'sm' | 'md' | 'lg';

/**
 * Design-system button applied to a native `<button>` or `<a>` element so it
 * stays accessible and composes with `routerLink`. Colour comes from the accent
 * tokens, so it follows the OS accent automatically.
 *
 * ```html
 * <button nexusButton variant="primary" size="lg">Slice</button>
 * <a nexusButton variant="ghost" routerLink="/settings">Settings</a>
 * ```
 */
@Component({
  selector: 'button[nexusButton], a[nexusButton]',
  template: '<ng-content />',
  styleUrl: './button.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    class: 'nexus-button',
    '[attr.data-variant]': 'variant()',
    '[attr.data-size]': 'size()',
    '[class.is-block]': 'block()',
  },
})
export class Button {
  readonly variant = input<ButtonVariant>('secondary');
  readonly size = input<ButtonSize>('md');
  readonly block = input(false);
}
