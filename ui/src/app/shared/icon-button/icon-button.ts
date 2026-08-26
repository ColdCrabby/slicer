import { ChangeDetectionStrategy, Component, input } from '@angular/core';
import { Icon } from '../icon/icon';

/**
 * A minimal icon-only button backed by an Iconoir SVG.
 *
 * Usage:
 * ```html
 * <nexus-icon-button icon="trash" label="Delete item" size="sm" />
 * ```
 *
 * Pair with `[tooltip]` from `TooltipDirective` at the call site for
 * accessible hover/focus tooltips, or supply a descriptive `label` for
 * screen-reader `aria-label` alone.
 */
@Component({
  selector: 'nexus-icon-button',
  standalone: true,
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <button type="button" class="icon-btn" [attr.aria-label]="label()">
      <nexus-icon
        [name]="icon()"
        [variant]="variant()"
        [style.--icon-stroke-width]="strokeWidth()"
      />
    </button>
  `,
  styles: [
    `
      :host {
        display: inline-flex;
      }

      .icon-btn {
        display: inline-grid;
        place-items: center;
        width: 24px;
        height: 24px;
        border: none;
        padding: 0;
        background: transparent;
        color: var(--color-text-tertiary);
        cursor: pointer;
        border-radius: var(--radius-sm);
        flex-shrink: 0;
        transition:
          color var(--transition-fast),
          background-color var(--transition-fast);

        nexus-icon {
          --icon-size: 18px;
        }

        &:hover {
          color: var(--color-text-secondary);
          background: var(--color-surface-hover);
        }

        &:focus-visible {
          outline: 2px solid var(--color-focus-ring);
          outline-offset: 1px;
        }
      }
    `,
  ],
})
export class IconButton {
  /** Iconoir icon name (matches the SVG filename in `/assets/icons/`). */
  readonly icon = input.required<string>();

  /** Accessible label used as `aria-label` on the inner button. */
  readonly label = input<string>('');

  /** Icon style variant. Defaults to `regular` (outlined). */
  readonly variant = input<'regular' | 'solid'>('regular');

  /** SVG stroke-width override. Leave undefined to use the icon's built-in value. */
  readonly strokeWidth = input<number | string | undefined>(undefined);
}
