import { ChangeDetectionStrategy, Component, input } from '@angular/core';

/**
 * One labelled row in a settings / wizard form: a title (+ optional
 * description) on the left, and a projected control on the right. Keeps form
 * templates flat and consistent without per-row boilerplate.
 */
@Component({
  selector: 'nexus-field-row',
  standalone: true,
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="field-row" [class.field-row--stacked]="stacked()">
      <div class="field-row__label">
        <span class="field-row__title">{{ label() }}</span>
        @if (description()) {
          <span class="field-row__desc">{{ description() }}</span>
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
        padding: var(--spacing-md) 0;
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
        flex-direction: column;
        gap: 2px;
        min-width: 0;
      }
      .field-row__title {
        font-size: var(--font-size-md);
        color: var(--color-text-primary);
      }
      .field-row__desc {
        font-size: var(--font-size-xs);
        color: var(--color-text-tertiary);
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
