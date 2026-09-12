import { ChangeDetectionStrategy, Component, EventEmitter, computed, input } from '@angular/core';
import { NumberInput, TooltipDirective } from '@coldcrabby/ui';
import { IconButton } from '../../../shared/icon-button/icon-button';
import type { FieldDef } from '../../models/field-def';
import { unitForField } from '../../models/field-units';
import type { FieldWidget } from '../base-field';

@Component({
  selector: 'se-number-field',
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
    <label class="field-label" [for]="field().key">
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
      [value]="displayed()"
      [min]="min()"
      [max]="max()"
      [step]="step()"
      [unit]="unit()"
      [label]="field().title ?? field().key"
      (valueChange)="onValueChange($event)"
    ></nexus-number-input>
  `,
})
export class NumberField implements FieldWidget {
  readonly field = input.required<FieldDef>();
  readonly value = input<unknown>(undefined);
  readonly valueChange = new EventEmitter<unknown>();

  protected readonly numeric = computed(() => {
    const v = this.value();
    if (v === null || v === undefined || v === '') return Number(this.field().default ?? 0);
    return Number(v);
  });
  /** Unit + step derived from the parameter's name — see `field-units.ts`. */
  private readonly resolvedUnit = computed(() => unitForField(this.field()));
  protected readonly unit = computed(() => this.resolvedUnit().unit);
  protected readonly step = computed(() => this.resolvedUnit().step);
  /** Factor between what the engine stores and what the control shows. */
  private readonly scale = computed(() => this.resolvedUnit().scale ?? 1);

  /**
   * The number the user sees. A fraction is shown as a percentage, so
   * `fan_speed: 1.0` reads as `100 %` rather than as `1 %`.
   */
  protected readonly displayed = computed(() => {
    const value = this.numeric() * this.scale();
    // A fraction times 100 lands on values like 24.999999999999996.
    return this.scale() === 1 ? value : Math.round(value * 1e6) / 1e6;
  });

  /** Convert back before emitting: the stored form is what the engine reads. */
  protected onValueChange(shown: number): void {
    const factor = this.scale();
    this.valueChange.emit(factor === 1 ? shown : Math.round((shown / factor) * 1e6) / 1e6);
  }

  // Bounds are stated in the stored scale, so they are converted with the value.
  protected readonly min = computed(() => {
    const min = this.field().minimum;
    return min === undefined ? Number.NEGATIVE_INFINITY : min * this.scale();
  });
  protected readonly max = computed(() => {
    const max = this.field().maximum;
    return max === undefined ? Number.POSITIVE_INFINITY : max * this.scale();
  });
}
