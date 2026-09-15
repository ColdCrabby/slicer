import { ChangeDetectionStrategy, Component, EventEmitter, computed, input } from '@angular/core';
import { NumberInput, TooltipDirective } from '@coldcrabby/ui';
import { IconButton } from '../../../shared/icon-button/icon-button';
import type { UnitOption } from '../../models/field-units';
import type { FieldDef } from '../../models/field-def';
import { UnitToggle } from '../../unit-toggle/unit-toggle';
import type { FieldWidget } from '../base-field';

/** The two shapes a `RelativeSpeed` value takes on the wire. */
type ParsedRelativeSpeed =
  { kind: 'absolute'; mmS: number } | { kind: 'percent'; fraction: number };

const MODES: readonly UnitOption[] = [
  { id: 'mm_s', label: 'mm/s' },
  { id: 'percent', label: '%' },
];

/**
 * Read a `RelativeSpeed`'s wire form: a bare number is absolute mm/s, a
 * string ending in `%` is a fraction of the source field. Mirrors
 * `RelativeSpeed`'s own `Serialize`/`Deserialize` in `src/settings/relative_speed.rs`.
 */
function parse(raw: unknown, fallback: unknown): ParsedRelativeSpeed {
  const value = raw === null || raw === undefined || raw === '' ? fallback : raw;
  if (typeof value === 'string') {
    const trimmed = value.trim();
    if (trimmed.endsWith('%')) {
      const n = Number(trimmed.slice(0, -1));
      return { kind: 'percent', fraction: (Number.isFinite(n) ? n : 0) / 100 };
    }
    const n = Number(trimmed);
    return { kind: 'absolute', mmS: Number.isFinite(n) ? n : 0 };
  }
  const n = Number(value ?? 0);
  return { kind: 'absolute', mmS: Number.isFinite(n) ? n : 0 };
}

/** Round to a couple of decimals — enough precision, no float noise. */
function round(n: number): number {
  return Math.round(n * 100) / 100;
}

/**
 * A speed given either as a literal mm/s value or as a percentage of another
 * field named by the schema's `x-relative-to` extension (e.g.
 * `overhang_2_4_speed`, a fraction of `perimeter_speed`).
 *
 * The mode shown is whichever shape the stored value currently is — there is
 * no separate "display unit" state to drift out of sync with it. Pressing the
 * toggle converts the value into the other shape against the source field's
 * *current* value and emits that, so what's on screen never silently means
 * something else after a click; it just stops tracking the source from that
 * point, exactly like typing a literal number always did.
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

  protected readonly modes = MODES;

  private readonly parsed = computed(() => parse(this.value(), this.field().default));

  /** The source field's current speed, in mm/s — `0` when it isn't set yet. */
  private readonly source = computed(() => {
    const key = this.field().relativeTo;
    const raw = key ? this.siblings()[key] : undefined;
    const n = Number(raw);
    return Number.isFinite(n) ? n : 0;
  });

  protected readonly mode = computed(() => (this.parsed().kind === 'percent' ? 'percent' : 'mm_s'));
  protected readonly unit = computed(() => (this.mode() === 'percent' ? '%' : 'mm/s'));
  protected readonly step = computed(() => (this.mode() === 'percent' ? 5 : 5));

  /** The number shown in whichever mode the stored value is currently in. */
  protected readonly displayed = computed(() => {
    const p = this.parsed();
    return p.kind === 'percent' ? round(p.fraction * 100) : p.mmS;
  });

  protected onValueChange(shown: number): void {
    this.valueChange.emit(this.mode() === 'percent' ? `${shown}%` : shown);
  }

  /** Convert the current value into the other shape and emit it. */
  protected onToggle(): void {
    const p = this.parsed();
    const source = this.source();
    if (p.kind === 'percent') {
      this.valueChange.emit(round(p.fraction * source));
    } else {
      this.valueChange.emit(`${round(source > 0 ? (p.mmS / source) * 100 : 0)}%`);
    }
  }
}
