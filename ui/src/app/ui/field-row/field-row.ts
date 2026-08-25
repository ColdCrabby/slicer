import { ChangeDetectionStrategy, Component, input } from '@angular/core';
import { Icon } from '../../shared/icon/icon';
import { TooltipDirective } from '../../shared/tooltip/tooltip.directive';

/**
 * One labelled row in a settings / wizard form: a title (+ optional info
 * tooltip) on the left, and a projected control on the right. Keeps form
 * templates flat and consistent without per-row boilerplate.
 */
@Component({
  selector: 'nexus-field-row',
  standalone: true,
  imports: [Icon, TooltipDirective],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="field-row" [class.field-row--stacked]="stacked()">
      <div class="field-row__label">
        <span class="field-row__title">{{ label() }}</span>
        @if (description(); as d) {
          <button type="button" class="field-row__info" [tooltip]="d" aria-label="More information">
            <nexus-icon name="info-circle" />
          </button>
        }
      </div>
      <div class="field-row__control">
        <ng-content />
      </div>
    </div>
  `,
  styles: [
    `
      :host {
        display: block;
      }
      .field-row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-lg);
        padding: var(--spacing-sm) 0;
      }
      .field-row--stacked {
        flex-direction: column;
        align-items: stretch;
        gap: var(--spacing-sm);
      }
      .field-row + .field-row {
        border-top: 1px solid var(--color-border-light);
      }
      .field-row__label {
        display: flex;
        align-items: center;
        gap: 5px;
        min-width: 0;
      }
      .field-row__title {
        font-size: var(--font-size-md);
        color: var(--color-text-primary);
      }
      .field-row__info {
        flex: none;
        display: inline-grid;
        place-items: center;
        width: 16px;
        height: 16px;
        padding: 0;
        border: none;
        background: transparent;
        color: var(--color-text-tertiary);
        cursor: help;
        --icon-size: 15px;
        transition: color var(--duration-fast) var(--ease-standard);
      }
      .field-row__info:hover {
        color: var(--color-text-secondary);
      }
      .field-row__info:focus-visible {
        outline: none;
        color: var(--accent);
        box-shadow: 0 0 0 2px var(--color-focus-ring);
        border-radius: 50%;
      }
      .field-row__control {
        flex: none;
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
      }
      .field-row--stacked .field-row__control {
        flex: 1;
      }
    `,
  ],
})
export class FieldRow {
  readonly label = input.required<string>();
  readonly description = input('');
  /** When true the control drops to its own full-width line below the label. */
  readonly stacked = input(false);
}
