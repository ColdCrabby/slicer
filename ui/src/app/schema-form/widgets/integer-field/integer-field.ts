import { ChangeDetectionStrategy, Component, EventEmitter, computed, input } from '@angular/core';
import { NumberInput } from '../../../ui/number-input/number-input';
import { IconButton } from '../../../shared/icon-button/icon-button';
import { TooltipDirective } from '../../../shared/tooltip/tooltip.directive';
import type { FieldDef } from '../../models/field-def';
import type { FieldWidget } from '../base-field';

@Component({
  selector: 'se-integer-field',
  standalone: true,
  imports: [IconButton, TooltipDirective, NumberInput],
  changeDetection: ChangeDetectionStrategy.OnPush,
  styles: [
    `
      :host {
        display: flex;
        flex-direction: column;
        gap: 6px;
      }

      label {
        display: flex;
        align-items: center;
        gap: 4px;
        font-size: 12px;
        font-weight: 500;
        color: var(--color-text-secondary);
        user-select: none;
        cursor: default;
      }
    `,
  ],
  template: `
    <label [for]="field().key">
      <span>{{ field().title ?? field().key }}</span>
      @if (field().description) {
        <nexus-icon-button
          icon="help-circle"
          label="More info"
          [tooltip]="field().description!"
          [tooltipMode]="'block'"
          [tooltipClickToggle]="true"
        />
      }
    </label>
    <nexus-number-input
      [value]="numeric()"
      [min]="min()"
      [max]="max()"
      [step]="1"
      [label]="field().title ?? field().key"
      (valueChange)="valueChange.emit($event)"
    ></nexus-number-input>
  `,
})
export class IntegerField implements FieldWidget {
  readonly field = input.required<FieldDef>();
  readonly value = input<unknown>(undefined);
  readonly valueChange = new EventEmitter<unknown>();

  protected readonly numeric = computed(() => {
    const v = this.value();
    if (v === null || v === undefined || v === '')
      return Math.round(Number(this.field().default ?? 0));
    return Math.round(Number(v));
  });
  protected readonly min = computed(() => this.field().minimum ?? Number.NEGATIVE_INFINITY);
  protected readonly max = computed(() => this.field().maximum ?? Number.POSITIVE_INFINITY);
}
