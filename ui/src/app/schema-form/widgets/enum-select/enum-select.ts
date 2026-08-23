import { ChangeDetectionStrategy, Component, EventEmitter, computed, input } from '@angular/core';
import { Select } from '../../../ui/select/select';
import type { SelectOption } from '../../../ui/select/select';
import { IconButton } from '../../../shared/icon-button/icon-button';
import { TooltipDirective } from '../../../shared/tooltip/tooltip.directive';
import type { FieldDef } from '../../models/field-def';
import type { FieldWidget } from '../base-field';

/**
 * Dropdown widget for enum fields with more than 3 options. Renders the
 * design-system `nexus-select`, mapping each enum variant to an option whose
 * secondary line is the variant description.
 */
@Component({
  selector: 'se-enum-select',
  standalone: true,
  imports: [IconButton, TooltipDirective, Select],
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
    <nexus-select
      [options]="options()"
      [value]="stringValue()"
      (valueChange)="valueChange.emit($event)"
    ></nexus-select>
  `,
})
export class EnumSelect implements FieldWidget {
  readonly field = input.required<FieldDef>();
  readonly value = input<unknown>(undefined);
  readonly valueChange = new EventEmitter<unknown>();

  protected readonly options = computed<SelectOption[]>(() =>
    (this.field().enumOptions ?? []).map((o) => ({
      value: o.value,
      label: o.value,
      description: o.description,
    })),
  );

  protected readonly stringValue = computed(() => {
    const v = this.value() ?? this.field().default;
    return v == null ? null : String(v);
  });
}
