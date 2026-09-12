import {
  ChangeDetectionStrategy,
  Component,
  EventEmitter,
  computed,
  inject,
  input,
} from '@angular/core';
import { NumberInput, TooltipDirective } from '@coldcrabby/ui';
import { IconButton } from '../../../shared/icon-button/icon-button';
import { UnitPreference } from '../../../services/unit-preference';
import type { FieldDef } from '../../models/field-def';
import { displayUnitOf, toDisplay, toStored, unitForField } from '../../models/field-units';
import { UnitToggle } from '../../unit-toggle/unit-toggle';
import type { FieldWidget } from '../base-field';

@Component({
  selector: 'se-number-field',
  standalone: true,
  imports: [IconButton, TooltipDirective, NumberInput, UnitToggle],
  changeDetection: ChangeDetectionStrategy.OnPush,
  styles: [
    `
      :host {
        display: flex;
        flex-direction: column;
        gap: 6px;
      }

      .control {
        position: relative;
        display: flex;
        align-items: center;
      }

      /*
       * The design-system suffix still renders — it is what reserves the right
       * width and shrinks the input — but the toggle is drawn over it. Hiding
       * it rather than dropping it is what keeps a switchable field the exact
       * same shape as a fixed-unit one.
       */
      .control.has-unit-toggle ::ng-deep .unit {
        visibility: hidden;
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
    <div class="control" [class.has-unit-toggle]="switchable()">
      <nexus-number-input
        [value]="displayed()"
        [min]="min()"
        [max]="max()"
        [step]="step()"
        [unit]="unit()"
        [label]="field().title ?? field().key"
        (valueChange)="onValueChange($event)"
      ></nexus-number-input>
      @if (switchable(); as family) {
        <se-unit-toggle
          [current]="currentUnit()"
          [options]="resolvedUnit().options ?? []"
          (cycle)="units.cycle(family)"
        />
      }
    </div>
  `,
})
export class NumberField implements FieldWidget {
  readonly field = input.required<FieldDef>();
  readonly value = input<unknown>(undefined);
  readonly valueChange = new EventEmitter<unknown>();

  /**
   * The stored value as a number, rounded when the engine's type is integral.
   *
   * One widget covers both: a `u32` field and an `f64` field differ in exactly
   * this rounding and in their step, which `unitForField` already floors at 1
   * for an integer. They were two near-identical components until the split
   * had to be re-derived in the registry — which meant the control taxonomy
   * gave an answer and then something downstream second-guessed it.
   */
  protected readonly numeric = computed(() => {
    const v = this.value();
    const raw = v === null || v === undefined || v === '' ? this.field().default : v;
    const n = Number(raw ?? 0);
    return this.field().type === 'integer' ? Math.round(n) : n;
  });
  /** Which unit each convertible family is read in — shared across the app. */
  protected readonly units = inject(UnitPreference);
  /** Unit, step and conversion for this field — see `field-units.ts`. */
  protected readonly resolvedUnit = computed(() =>
    unitForField(this.field(), this.units.display()),
  );
  /** The family whose unit the user may switch here, if any. */
  protected readonly switchable = computed(() => this.resolvedUnit().family);
  protected readonly currentUnit = computed(() => {
    const family = this.switchable();
    return family ? displayUnitOf(family, this.units.display()) : '';
  });
  protected readonly unit = computed(() => this.resolvedUnit().unit);
  protected readonly step = computed(() => this.resolvedUnit().step);
  /** Factor between what the engine stores and what the control shows. */
  private readonly scale = computed(() => this.resolvedUnit().scale);

  /**
   * The number the user sees. A fraction is shown as a percentage, so
   * `fan_speed: 1.0` reads as `100 %` rather than as `1 %`, and a travel speed
   * the engine holds as `9000` mm/min reads as `150 mm/s`.
   */
  protected readonly displayed = computed(() => toDisplay(this.numeric(), this.scale()));

  /** Convert back before emitting: the stored form is what the engine reads. */
  protected onValueChange(shown: number): void {
    this.valueChange.emit(toStored(shown, this.scale()));
  }

  // Bounds are stated in the stored scale, so they are converted with the value.
  protected readonly min = computed(() => {
    const min = this.field().minimum;
    return min === undefined ? Number.NEGATIVE_INFINITY : toDisplay(min, this.scale());
  });
  protected readonly max = computed(() => {
    const max = this.field().maximum;
    return max === undefined ? Number.POSITIVE_INFINITY : toDisplay(max, this.scale());
  });
}
