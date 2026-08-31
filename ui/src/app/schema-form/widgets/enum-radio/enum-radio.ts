import { ChangeDetectionStrategy, Component, EventEmitter, computed, input } from '@angular/core';
import { RadioGroup, type RadioOption, TooltipDirective } from '@coldcrabby/ui';
import { IconButton } from '../../../shared/icon-button/icon-button';
import type { FieldDef } from '../../models/field-def';
import type { FieldWidget } from '../base-field';

/**
 * Radio-group widget for enum fields with 3 or fewer options. Renders the
 * design-system `nexus-radio-group` as selectable option cards, each showing
 * the variant description so the user can tell the options apart at a glance.
 */
@Component({
  selector: 'se-enum-radio',
  standalone: true,
  imports: [IconButton, TooltipDirective, RadioGroup],
  changeDetection: ChangeDetectionStrategy.OnPush,
  styles: [
    `
      :host {
        display: flex;
        flex-direction: column;
        gap: 6px;
      }

      .legend {
        display: flex;
        align-items: center;
        gap: 4px;
        font-size: 12px;
        font-weight: 500;
        color: var(--color-text-secondary);
        user-select: none;
      }
    `,
  ],
  template: `
    <span class="legend">
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
    </span>
    <nexus-radio-group
      [options]="options()"
      [value]="stringValue()"
      [label]="field().title ?? field().key"
      (valueChange)="valueChange.emit($event)"
    ></nexus-radio-group>
  `,
})
export class EnumRadio implements FieldWidget {
  readonly field = input.required<FieldDef>();
  readonly value = input<unknown>(undefined);
  readonly valueChange = new EventEmitter<unknown>();

  protected readonly options = computed<RadioOption[]>(() =>
    (this.field().enumOptions ?? []).map((o) => ({
      value: o.value,
      label: o.label,
      description: o.description,
    })),
  );

  protected readonly stringValue = computed(() => {
    const v = this.value() ?? this.field().default;
    return v == null ? null : String(v);
  });
}
