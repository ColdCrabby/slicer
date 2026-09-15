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
  parseRelativeSpeed,
  relativeSpeedScale,
  relativeSpeedStep,
  relativeSpeedUnit,
  RELATIVE_SPEED_MODES,
  roundRelative,
} from '../../models/relative-speed';
import { UnitToggle } from '../../unit-toggle/unit-toggle';
import type { FieldWidget } from '../base-field';

/**
 * A speed given either as a literal mm/s value or as a percentage of another
 * field named by the schema's `x-relative-to` extension (e.g.
 * `overhang_2_4_speed`, a fraction of `perimeter_speed`).
 *
 * The control cycles three states — `mm/s`, `mm/min`, `%` — entirely local to
 * *this* field, unlike an ordinary number field's unit toggle (which flips the
 * shared `UnitPreference` and so moves every speed field on the page at once).
 * That global behaviour doesn't fit here: `%` is a genuinely different
 * *stored* shape, only this field's own, so making the mm/s↔mm/min leg global
 * while `%` stayed local would mean the same button sometimes moves the whole
 * page and sometimes just one control — the confusing half of "pick one".
 * Converting is what stops the number on screen from silently meaning
 * something else after a click: leaving `%` re-enters at mm/s, exactly like
 * typing a literal number always did.
 *
 * The slice sidebar's own widget — the profile editor renders the same
 * control inline (its `FieldShell` already carries the label), sharing the
 * parsing in `models/relative-speed.ts` rather than this component.
 */
@Component({
  selector: 'se-relative-speed-field',
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
      <se-unit-toggle [current]="mode()" [options]="modes" (cycle)="onToggle()" />
    </div>
  `,
})
export class RelativeSpeedField implements FieldWidget {
  readonly field = input.required<FieldDef>();
  readonly value = input<unknown>(undefined);
  readonly siblings = input<Readonly<Record<string, unknown>>>({});
  readonly valueChange = new EventEmitter<unknown>();

  protected readonly modes = RELATIVE_SPEED_MODES;

  private readonly parsed = computed(() => parseRelativeSpeed(this.value(), this.field().default));

  /** Which absolute unit to read/show in, while the value isn't `%`. Local to
   * this field — see the class doc for why it isn't the shared preference. */
  private readonly absoluteUnit = signal<'mm_s' | 'mm_min'>('mm_s');

  /** The source field's current speed, in mm/s — `0` when it isn't set yet. */
  private readonly source = computed(() => {
    const key = this.field().relativeTo;
    const raw = key ? this.siblings()[key] : undefined;
    const n = Number(raw);
    return Number.isFinite(n) ? n : 0;
  });

  /** Which of the three stops the control is on right now. */
  protected readonly mode = computed(() =>
    this.parsed().kind === 'percent' ? 'percent' : this.absoluteUnit(),
  );
  protected readonly unit = computed(() => relativeSpeedUnit(this.mode()));
  protected readonly step = computed(() => relativeSpeedStep(this.mode()));

  /** The number shown in whichever mode the stored value is currently in. */
  protected readonly displayed = computed(() => {
    const p = this.parsed();
    if (p.kind === 'percent') return roundRelative(p.fraction * 100);
    return roundRelative(p.mmS * relativeSpeedScale(this.mode()));
  });

  protected onValueChange(shown: number): void {
    const m = this.mode();
    if (m === 'percent') {
      this.valueChange.emit(`${shown}%`);
      return;
    }
    this.valueChange.emit(roundRelative(shown / relativeSpeedScale(m)));
  }

  /** Advance to the next of the three stops, converting the value as needed. */
  protected onToggle(): void {
    const m = this.mode();
    if (m === 'mm_s') {
      this.absoluteUnit.set('mm_min'); // pure display flip, value unchanged
      return;
    }
    const source = this.source();
    if (m === 'mm_min') {
      const p = this.parsed(); // still 'absolute' here
      const mmS = p.kind === 'absolute' ? p.mmS : 0;
      this.valueChange.emit(`${roundRelative(source > 0 ? (mmS / source) * 100 : 0)}%`);
      return;
    }
    // percent → back to mm/s
    const p = this.parsed();
    const fraction = p.kind === 'percent' ? p.fraction : 0;
    this.absoluteUnit.set('mm_s');
    this.valueChange.emit(roundRelative(fraction * source));
  }
}
