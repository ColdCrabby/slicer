import { ChangeDetectionStrategy, Component, computed, input, output } from '@angular/core';
import type { FieldDef } from '../../schema-form/models/field-def';
import { noticeForField } from '../../schema-form/field-exceptions/field-exceptions';
import { FieldNoticeView } from '../../schema-form/field-notice/field-notice';
import { GcodeField } from '../../schema-form/custom-widgets/gcode-field/gcode-field';
import { NumberInput } from '../../ui/number-input/number-input';
import { Select, type SelectOption } from '../../ui/select/select';
import { Switch } from '../../ui/switch/switch';
import { FieldShell } from './field-shell';

/**
 * One schema-driven parameter row for the print-profile editor: the label and
 * its control on top, with the field's **description rendered inline below** —
 * the editor has the vertical room, so the guidance is always visible instead
 * of hidden behind a cramped info tooltip.
 *
 * Dumb by design: it knows nothing about profiles or the store. Give it a
 * parsed {@link FieldDef} and the current value; it emits the edited value. The
 * control is chosen from the field's shape — enum → select, boolean → switch,
 * an `x-widget: "gcode"` hint → code editor, everything else → number input —
 * so any new `SlicingParams` field appears automatically with no per-field
 * wiring.
 *
 * A thin wrapper around {@link FieldShell}: this component only picks the
 * control and maps schema → title/description, delegating all row markup and
 * styling to the shell so the row rhythm lives in a single place. The `gcode`
 * widget is the exception — it needs a full-width, stacked layout, so it
 * renders the shared {@link GcodeField} directly (label + editor) rather than
 * the beside-the-label shell.
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
  imports: [NumberInput, Select, Switch, FieldShell, GcodeField, FieldNoticeView],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (kind() === 'gcode') {
      <se-gcode-field
        [field]="field()"
        [value]="value()"
        (valueChange)="valueChange.emit($event)"
      />
    } @else {
      <nexus-field-shell [title]="field().title ?? field().key" [description]="descriptionText()">
        @switch (kind()) {
          @case ('enum') {
            <nexus-select
              [options]="selectOptions()"
              [value]="stringValue()"
              (valueChange)="valueChange.emit($event)"
            />
          }
          @case ('boolean') {
            <nexus-switch [checked]="boolValue()" (checkedChange)="valueChange.emit($event)" />
          }
          @default {
            <nexus-number-input
              [value]="numberValue()"
              [min]="min()"
              [max]="max()"
              [step]="step()"
              (valueChange)="valueChange.emit($event)"
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

  /** Which control to render, derived from the field's `x-widget` hint or shape. */
  protected readonly kind = computed<'enum' | 'boolean' | 'number' | 'gcode'>(() => {
    const f = this.field();
    if (f.widget === 'gcode') {
      return 'gcode';
    }
    if (f.enumOptions?.length) {
      return 'enum';
    }
    return f.type === 'boolean' ? 'boolean' : 'number';
  });

  protected readonly selectOptions = computed<SelectOption[]>(() =>
    (this.field().enumOptions ?? []).map((o) => ({ value: o.value, label: o.label })),
  );

  /**
   * Schema description with the lightweight Markdown the engine emits
   * (`**bold**`, `` `code` ``) stripped, so it reads cleanly as plain helper
   * text. Paragraph breaks are preserved and shown via `white-space: pre-line`.
   */
  protected readonly descriptionText = computed(() =>
    (this.field().description ?? '').replace(/\*\*/g, '').replace(/`/g, '').trim(),
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

  protected readonly boolValue = computed(() => Boolean(this.resolved()));

  protected readonly numberValue = computed(() => {
    const v = this.resolved();
    return typeof v === 'number' ? v : Number(v ?? 0);
  });

  protected readonly min = computed(() => this.field().minimum ?? Number.NEGATIVE_INFINITY);
  protected readonly max = computed(() => this.field().maximum ?? Number.POSITIVE_INFINITY);
  protected readonly step = computed(() => (this.field().type === 'integer' ? 1 : 0.01));
}
