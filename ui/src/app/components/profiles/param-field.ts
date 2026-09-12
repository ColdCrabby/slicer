import { ChangeDetectionStrategy, Component, computed, input, output } from '@angular/core';
import type { FieldDef } from '../../schema-form/models/field-def';
import { controlFor } from '../../schema-form/models/field-control';
import { unitForField } from '../../schema-form/models/field-units';
import { filamentTypeOptions } from '../../schema-form/custom-widgets/filament-type-field/filament-types';
import { noticeForField } from '../../schema-form/field-exceptions/field-exceptions';
import { FieldNoticeView } from '../../schema-form/field-notice/field-notice';
import { GcodeField } from '../../schema-form/custom-widgets/gcode-field/gcode-field';
import {
  ColorPicker,
  NumberInput,
  RadioGroup,
  type RadioOption,
  Segmented,
  type SegmentOption,
  Select,
  type SelectOption,
  Slider,
  Switch,
} from '@coldcrabby/ui';
import { FieldShell } from './field-shell';

/**
 * One schema-driven parameter row for the print-profile editor: the label and
 * its control on top, with the field's **description rendered inline below** —
 * the editor has the vertical room, so the guidance is always visible instead
 * of hidden behind a cramped info tooltip.
 *
 * Dumb by design: it knows nothing about profiles or the store. Give it a
 * parsed {@link FieldDef} and the current value; it emits the edited value.
 *
 * **The control comes from `controlFor`, the same function the slice sidebar
 * asks.** That is the whole point of routing through it: a parameter the
 * sidebar shows as option cards is option cards here too, and one shown as a
 * segmented control is not quietly a dropdown on this page. This component
 * decides only how each kind is *dressed* for a full-width editor row — the
 * sidebar's `field-registry` does the same for a 320 px panel.
 *
 * A thin wrapper around {@link FieldShell}: the row markup and styling live
 * there so the row rhythm is defined once. Three kinds opt out of the
 * beside-the-label layout — `gcode` renders the shared {@link GcodeField} full
 * width, and `cards` and `segmented` stack under their label. A segmented
 * control squeezed into the right-hand column collapses into a vertical list
 * of its own; given the row it stays the single track it is meant to be, which
 * is also how the hand-built editors lay one out.
 *
 * It also renders the field's {@link noticeForField} exception, sharing one
 * registry with the slice sidebar so a caution cannot exist in one surface and
 * not the other. That matters most for **cross-contract** cautions: a filament
 * setting that depends on the machine (chamber temperature needing a chamber
 * heater) can only be flagged here, because this editor is the one place the
 * user sets it.
 */
@Component({
  selector: 'nexus-param-field',
  standalone: true,
  imports: [
    ColorPicker,
    NumberInput,
    RadioGroup,
    Segmented,
    Select,
    Slider,
    Switch,
    FieldShell,
    GcodeField,
    FieldNoticeView,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (kind() === 'gcode') {
      <se-gcode-field
        [field]="field()"
        [value]="value()"
        (valueChange)="valueChange.emit($event)"
      />
    } @else {
      <nexus-field-shell
        [title]="field().title ?? field().key"
        [description]="field().description ?? ''"
        [stacked]="isStacked()"
      >
        @switch (kind()) {
          @case ('cards') {
            <nexus-radio-group
              [options]="radioOptions()"
              [value]="stringValue()"
              [label]="field().title ?? field().key"
              (valueChange)="valueChange.emit($event)"
            />
          }
          @case ('segmented') {
            <nexus-segmented
              [options]="segmentOptions()"
              [value]="stringValue()"
              [label]="field().title ?? field().key"
              (valueChange)="valueChange.emit($event)"
            />
          }
          @case ('select') {
            <nexus-select
              [options]="selectOptions()"
              [value]="stringValue()"
              (valueChange)="valueChange.emit($event)"
            />
          }
          @case ('filament-type') {
            <nexus-select
              [options]="materialOptions()"
              [value]="stringValue()"
              (valueChange)="valueChange.emit($event)"
            />
          }
          @case ('switch') {
            <nexus-switch [checked]="boolValue()" (checkedChange)="valueChange.emit($event)" />
          }
          @case ('slider') {
            <nexus-slider
              [value]="displayed()"
              [min]="min()"
              [max]="max()"
              [step]="step()"
              [unit]="unit()"
              [label]="field().title ?? field().key"
              (valueChange)="onNumberChange($event)"
            />
          }
          @case ('color') {
            <nexus-color-picker
              [value]="colorValue()"
              [ariaLabel]="field().title ?? field().key"
              (valueChange)="valueChange.emit($event)"
            />
          }
          @case ('text') {
            <input
              class="param-field-text"
              type="text"
              [value]="stringValue()"
              [attr.aria-label]="field().title ?? field().key"
              (input)="onTextInput($event)"
            />
          }
          @default {
            <nexus-number-input
              [value]="displayed()"
              [min]="min()"
              [max]="max()"
              [step]="step()"
              [unit]="unit()"
              (valueChange)="onNumberChange($event)"
            />
          }
        }
      </nexus-field-shell>
    }
    <se-field-notice class="param-field-notice" [notice]="notice()" />
  `,
  styles: [
    `
      :host {
        display: block;
      }
      /* The between-row divider depends on adjacency of *these* hosts — the
         nested shell hosts aren't siblings — so it stays at this level. */
      :host + :host {
        border-top: 1px solid var(--color-border-light);
      }

      .param-field-notice {
        margin-bottom: var(--spacing-sm);
      }

      .param-field-text {
        min-width: 0;
        padding: var(--spacing-sm) var(--spacing-md);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-md);
        background: var(--color-surface);
        color: var(--color-text-primary);
        font: inherit;
        font-size: var(--font-size-sm);
      }
    `,
  ],
})
export class ParamField {
  /** Parsed schema field definition (label, type, enum options, bounds). */
  readonly field = input.required<FieldDef>();
  /** Current value from the profile's `params` bag. */
  readonly value = input<unknown>(undefined);
  /**
   * Other values in scope for cross-field notices. The profile editors pass the
   * profile's own params **merged with the active printer's**, so a filament
   * setting can ask about the machine it will run on.
   */
  readonly siblings = input<Readonly<Record<string, unknown>>>({});
  /** Emits the edited value (number, boolean, or enum string). */
  readonly valueChange = output<unknown>();

  /** Which control to render — the shared taxonomy, not a local guess. */
  protected readonly kind = computed(() => controlFor(this.field()));

  /** Controls that need the full row rather than the right-hand column. */
  protected readonly isStacked = computed(
    () => this.kind() === 'cards' || this.kind() === 'segmented',
  );

  protected readonly selectOptions = computed<SelectOption[]>(() =>
    (this.field().enumOptions ?? []).map((o) => ({ value: o.value, label: o.label })),
  );

  protected readonly segmentOptions = computed<SegmentOption[]>(() =>
    (this.field().enumOptions ?? []).map((o) => ({
      value: o.value,
      label: o.label,
      description: o.description,
    })),
  );

  protected readonly radioOptions = computed<RadioOption[]>(() =>
    (this.field().enumOptions ?? []).map((o) => ({
      value: o.value,
      label: o.label,
      description: o.description,
    })),
  );

  /**
   * Material names: the conventional list plus whatever the profile already
   * holds, so a vendor's `PLA Matte` survives the dropdown being opened.
   */
  protected readonly materialOptions = computed<SelectOption[]>(() =>
    filamentTypeOptions(this.stringValue()).map((o) => ({ value: o.value, label: o.label })),
  );

  /**
   * The value to display: the profile's own value when set, otherwise the
   * field's schema default. A profile's `params` is a *sparse* overlay, so an
   * absent key resolves to the engine default at slice time — showing that
   * default (rather than a misleading `0`) keeps the editor honest.
   */
  private readonly resolved = computed(() => this.value() ?? this.field().default);

  /**
   * Field-specific caution to render with the control, if any. Evaluated
   * against the **resolved** value (schema default when the sparse `params`
   * bag omits the key) so the notice reflects what will actually be sliced.
   */
  protected readonly notice = computed(() =>
    noticeForField(this.field(), this.resolved(), this.siblings()),
  );

  protected readonly stringValue = computed(() => {
    const v = this.resolved();
    return v == null ? '' : String(v);
  });

  protected readonly colorValue = computed(() => this.stringValue() || '#000000');

  protected readonly boolValue = computed(() => Boolean(this.resolved()));

  private readonly numeric = computed(() => {
    const v = this.resolved();
    return typeof v === 'number' ? v : Number(v ?? 0);
  });

  /**
   * Unit, step and scale from `x-unit` / `x-step` — the same lookup the sidebar
   * uses. Without it this editor showed `fan_speed` as a bare `1`, where the
   * panel two clicks away read `100 %` for the same stored value.
   */
  private readonly resolvedUnit = computed(() => unitForField(this.field()));
  protected readonly unit = computed(() => this.resolvedUnit().unit);
  protected readonly step = computed(() => this.resolvedUnit().step);
  private readonly scale = computed(() => this.resolvedUnit().scale ?? 1);

  /** A fraction is shown as a percentage; everything else as stored. */
  protected readonly displayed = computed(() => {
    const value = this.numeric() * this.scale();
    // A fraction times 100 lands on values like 24.999999999999996.
    return this.scale() === 1 ? value : Math.round(value * 1e6) / 1e6;
  });

  /** Convert back before emitting: the stored form is what the engine reads. */
  protected onNumberChange(shown: number): void {
    const factor = this.scale();
    this.valueChange.emit(factor === 1 ? shown : Math.round((shown / factor) * 1e6) / 1e6);
  }

  protected onTextInput(event: Event): void {
    this.valueChange.emit((event.target as HTMLInputElement).value);
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
