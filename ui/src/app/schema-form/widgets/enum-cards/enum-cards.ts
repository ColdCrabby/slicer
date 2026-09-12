import { ChangeDetectionStrategy, Component, EventEmitter, computed, input } from '@angular/core';
import { RadioGroup, type RadioOption, TooltipDirective } from '@coldcrabby/ui';
import { IconButton } from '../../../shared/icon-button/icon-button';
import type { FieldDef } from '../../models/field-def';
import type { FieldWidget } from '../base-field';

/**
 * Option-card widget for an enum whose branches work differently enough that
 * the names alone will not separate them — `Classic` vs `Arachne`, `All at
 * Once` vs `One at a Time`. Renders the design-system `nexus-radio-group` as
 * selectable cards, each carrying the variant's description on its own line.
 *
 * It is the **tallest** control the form has, so it is never chosen by option
 * count: a field opts in with `x-widget = "cards"` and everything else with
 * three or fewer choices stays a segmented control. `ControlKind` in
 * `models/field-control.ts` carries the whole set and the rule separating them.
 */
@Component({
  selector: 'se-enum-cards',
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
    <span class="legend field-label">
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
export class EnumCards implements FieldWidget {
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
