import { ChangeDetectionStrategy, Component, computed, input, output } from '@angular/core';
import { labelTextColor, type Label } from '../../models/label.model';
import { Icon } from '../../shared/icon/icon';

/**
 * A single label rendered as a GitHub-style coloured pill. Text colour is
 * derived from the label colour for legibility. Optionally shows a remove
 * affordance (used inside the label picker).
 */
@Component({
  selector: 'nexus-label-chip',
  standalone: true,
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <span
      class="chip"
      [class.chip--sm]="size() === 'sm'"
      [style.background]="label().color"
      [style.color]="textColor()"
    >
      <span class="chip__name">{{ label().name }}</span>
      @if (removable()) {
        <button
          type="button"
          class="chip__remove"
          [attr.aria-label]="'Remove ' + label().name"
          (click)="remove.emit(); $event.stopPropagation()"
        >
          <nexus-icon name="xmark" />
        </button>
      }
    </span>
  `,
  styles: [
    `
      :host {
        display: inline-flex;
        min-width: 0;
      }
      .chip {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        max-width: 100%;
        padding: 2px 9px;
        border-radius: 999px;
        font-size: var(--font-size-xs);
        font-weight: var(--font-weight-medium);
        line-height: 1.5;
        white-space: nowrap;
      }
      .chip--sm {
        padding: 1px 7px;
        font-size: 10px;
      }
      .chip__name {
        overflow: hidden;
        text-overflow: ellipsis;
      }
      .chip__remove {
        display: inline-grid;
        place-items: center;
        width: 14px;
        height: 14px;
        margin-right: -3px;
        padding: 0;
        border: none;
        border-radius: 50%;
        background: color-mix(in oklab, currentColor 20%, transparent);
        color: inherit;
        cursor: pointer;
        --icon-size: 10px;

        &:hover {
          background: color-mix(in oklab, currentColor 40%, transparent);
        }
      }
    `,
  ],
})
export class LabelChip {
  readonly label = input.required<Label>();
  readonly size = input<'sm' | 'md'>('md');
  readonly removable = input(false);
  readonly remove = output<void>();

  protected readonly textColor = computed(() => labelTextColor(this.label().color));
}
