import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import type { Label } from '../../models/label.model';
import { Icon } from '@coldcrabby/ui';

/**
 * A single label rendered as a subtle, GitHub-style tinted pill. The fill,
 * text, and border are all derived from the label's hue via `color-mix`, so a
 * label never renders as a loud solid block — `light`-toned labels are more
 * transparent than `dark` ones, and both stay quieter than the app's accent.
 *
 * The recipe lives in CSS (keyed off `--label-color` + `data-tone`) so it stays
 * theme-aware: text is mixed toward `--color-text-primary`, staying legible in
 * both light and dark app themes.
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
      [attr.data-tone]="label().tone"
      [style.--label-color]="label().color"
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
        --tint-bg: 20%;
        --tint-text: 72%;
        --tint-border: 45%;
        display: inline-flex;
        align-items: center;
        gap: 4px;
        max-width: 100%;
        padding: 0 8px;
        height: 20px;
        border-radius: var(--radius-sm);
        font-size: var(--font-size-xs);
        font-weight: var(--font-weight-medium);
        line-height: 1;
        white-space: nowrap;
        background: color-mix(in oklab, var(--label-color) var(--tint-bg), transparent);
        color: color-mix(in oklab, var(--label-color) var(--tint-text), var(--color-text-primary));
        border: 1px solid color-mix(in oklab, var(--label-color) var(--tint-border), transparent);
      }
      .chip[data-tone='light'] {
        --tint-bg: 10%;
        --tint-text: 50%;
        --tint-border: 26%;
      }
      .chip--sm {
        height: 18px;
        padding: 0 6px;
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
        background: color-mix(in oklab, currentColor 18%, transparent);
        color: inherit;
        cursor: pointer;
        --icon-size: 10px;

        &:hover {
          background: color-mix(in oklab, currentColor 34%, transparent);
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
}
