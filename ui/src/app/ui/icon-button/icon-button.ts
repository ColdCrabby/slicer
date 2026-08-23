import { ChangeDetectionStrategy, Component, input } from '@angular/core';

export type IconButtonVariant = 'ghost' | 'solid' | 'outline';
export type IconButtonSize = 'sm' | 'md' | 'lg';

/**
 * Square, icon-only button applied to a native `<button>`/`<a>`. Consumers
 * project a `<nexus-icon>` and MUST supply an `aria-label`.
 *
 * ```html
 * <button nexusIconButton aria-label="Collapse" (click)="collapse()">
 *   <nexus-icon name="nav-arrow-left" />
 * </button>
 * ```
 */
@Component({
  selector: 'button[nexusIconButton], a[nexusIconButton]',
  template: '<ng-content />',
  styleUrl: './icon-button.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    class: 'nexus-icon-button',
    '[attr.data-variant]': 'variant()',
    '[attr.data-size]': 'size()',
    '[class.is-active]': 'active()',
  },
})
export class IconButton {
  readonly variant = input<IconButtonVariant>('ghost');
  readonly size = input<IconButtonSize>('md');
  readonly active = input(false);
}
