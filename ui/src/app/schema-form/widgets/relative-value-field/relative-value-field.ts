import {
  ChangeDetectionStrategy,
  Component,
  EventEmitter,
  computed,
  input,
  signal,
} from '@angular/core';
import { NumberInput, TooltipDirective } from '@coldcrabby/ui';
import { IconButton } from '../../../shared/icon-button/icon-button';
import type { FieldDef } from '../../models/field-def';
import {
  parseRelativeValue,
  relativeModes,
  relativeScale,
  relativeStep,
  relativeUnit,
  roundRelative,
} from '../../models/relative-value';
import { UnitToggle } from '../../unit-toggle/unit-toggle';
import type { FieldWidget } from '../base-field';

/**
 * A value given either as a literal number or as a percentage of another field,
 * named by the schema's `x-relative-to` or `x-derived-from` extension — an
 * overhang band as a fraction of `perimeter_speed`, a bead width as a fraction
 * of `nozzle_diameter_mm`.
 *
 * The control cycles that field's absolute readings and then `%`, entirely
 * local to *this* field, unlike an ordinary number field's unit toggle (which
 * flips the shared `UnitPreference` and so moves every speed field on the page
 * at once). That global behaviour doesn't fit here: `%` is a genuinely
 * different *stored* shape, only this field's own, so making the
 * mm/s↔mm/min leg global while `%` stayed local would mean the same button
 * sometimes moves the whole page and sometimes just one control — the confusing
 * half of "pick one". Converting is what stops the number on screen from
 * silently meaning something else after a click.
 *
 * How many stops it has comes from the field's own unit: a speed has two
 * absolute readings and a width has one, so a width cycles two states rather
 * than three. Offering a width `mm/min` would be offering a speed for a
 * distance.
 *
 * The slice sidebar's own widget — the profile editor renders the same
 * control inline (its `FieldShell` already carries the label), sharing the
 * parsing in `models/relative-value.ts` rather than this component.
 */
@Component({
  selector: 'se-relative-value-field',
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

      .control ::ng-deep .unit {
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
    <div class="control">
      <nexus-number-input
        [value]="displayed()"
        [min]="0"
        [step]="step()"
        [unit]="unit()"
        [label]="field().title ?? field().key"
        (valueChange)="onValueChange($event)"
      ></nexus-number-input>
      <se-unit-toggle [current]="mode()" [options]="modes()" (cycle)="onToggle()" />
    </div>
  `,
})
export class RelativeValueField implements FieldWidget {
  readonly field = input.required<FieldDef>();
  readonly value = input<unknown>(undefined);
  readonly siblings = input<Readonly<Record<string, unknown>>>({});
  readonly valueChange = new EventEmitter<unknown>();

  protected readonly modes = computed(() => relativeModes(this.field().unit));

  private readonly parsed = computed(() => parseRelativeValue(this.value(), this.field().default));

  /** Which absolute reading to show, while the value isn't `%`. Local to this
   * field — see the class doc for why it isn't the shared preference. */
  private readonly absoluteUnit = signal<string | null>(null);

  /** The absolute mode this field starts and returns to. */
  private readonly baseMode = computed(() => this.modes()[0].id);

  /** The source field's current value — `0` when it isn't set yet. */
  private readonly source = computed(() => {
    const key = this.field().relativeTo;
    const raw = key ? this.siblings()[key] : undefined;
    const n = Number(raw);
    return Number.isFinite(n) ? n : 0;
  });

  /** Which of the three stops the control is on right now. */
  protected readonly mode = computed(() =>
    this.parsed().kind === 'percent' ? 'percent' : (this.absoluteUnit() ?? this.baseMode()),
  );
  protected readonly unit = computed(() => relativeUnit(this.mode(), this.field().unit));
  protected readonly step = computed(() => relativeStep(this.mode(), this.field().unit));

  /** The number shown in whichever mode the stored value is currently in. */
  protected readonly displayed = computed(() => {
    const p = this.parsed();
    if (p.kind === 'percent') return roundRelative(p.fraction * 100);
    return roundRelative(p.value * relativeScale(this.mode()));
  });

  protected onValueChange(shown: number): void {
    const m = this.mode();
    if (m === 'percent') {
      this.valueChange.emit(`${shown}%`);
      return;
    }
    this.valueChange.emit(roundRelative(shown / relativeScale(m)));
  }

  /** Advance to the next stop, converting the value as needed. */
  protected onToggle(): void {
    const modes = this.modes();
    const index = modes.findIndex((option) => option.id === this.mode());
    const next = modes[(index + 1) % modes.length].id;

    // An absolute-to-absolute step is a pure display flip: the stored number is
    // the same distance or speed either way.
    if (next !== 'percent' && this.mode() !== 'percent') {
      this.absoluteUnit.set(next);
      return;
    }

    const source = this.source();
    const p = this.parsed();
    if (next === 'percent') {
      const absolute = p.kind === 'absolute' ? p.value : 0;
      this.valueChange.emit(`${roundRelative(source > 0 ? (absolute / source) * 100 : 0)}%`);
      return;
    }
    // percent → back to the field's base absolute reading
    const fraction = p.kind === 'percent' ? p.fraction : 0;
    this.absoluteUnit.set(next);
    this.valueChange.emit(roundRelative(fraction * source));
  }
}
