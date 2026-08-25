import { ChangeDetectionStrategy, Component, computed, input, output } from '@angular/core';
import type { FieldDef } from '../../schema-form/models/field-def';
import { NumberInput } from '../../ui/number-input/number-input';
import { Select, type SelectOption } from '../../ui/select/select';
import { Switch } from '../../ui/switch/switch';

/**
 * One schema-driven parameter row for the print-profile editor: the label and
 * its control on top, with the field's **description rendered inline below** —
 * the editor has the vertical room, so the guidance is always visible instead
 * of hidden behind a cramped info tooltip.
 *
 * Dumb by design: it knows nothing about profiles or the store. Give it a
 * parsed {@link FieldDef} and the current value; it emits the edited value. The
 * control is chosen from the field's shape — enum → select, boolean → switch,
 * everything else → number input — so any new `SlicingParams` field appears
 * automatically with no per-field wiring.
 */
@Component({
  selector: 'nexus-param-field',
  standalone: true,
  imports: [NumberInput, Select, Switch],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="param-field">
      <div class="param-field__row">
        <span class="param-field__title">{{ field().title ?? field().key }}</span>
        <span class="param-field__control">
          @switch (kind()) {
            @case ('enum') {
              <nexus-select
                [options]="selectOptions()"
                [value]="stringValue()"
                (valueChange)="valueChange.emit($event)"
              />
            }
            @case ('boolean') {
              <nexus-switch
                [checked]="boolValue()"
                (checkedChange)="valueChange.emit($event)"
              />
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
        </span>
      </div>
      @if (descriptionText(); as d) {
        <p class="param-field__desc">{{ d }}</p>
      }
    </div>
  `,
  styles: [
    `
      :host {
        display: block;
      }
      :host + :host .param-field {
        border-top: 1px solid var(--color-border-light);
      }
      .param-field {
        padding: var(--spacing-md) 0;
      }
      .param-field__row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-lg);
      }
      .param-field__title {
        min-width: 0;
        font-size: var(--font-size-md);
        color: var(--color-text-primary);
      }
      .param-field__control {
        flex: none;
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
      }
      .param-field__desc {
        margin: var(--spacing-xs) 0 0;
        max-width: 62ch;
        font-size: var(--font-size-xs);
        line-height: 1.5;
        color: var(--color-text-tertiary);
        white-space: pre-line;
      }
    `,
  ],
})
export class ParamField {
  /** Parsed schema field definition (label, type, enum options, bounds). */
  readonly field = input.required<FieldDef>();
  /** Current value from the profile's `params` bag. */
  readonly value = input<unknown>(undefined);
  /** Emits the edited value (number, boolean, or enum string). */
  readonly valueChange = output<unknown>();

  /** Which control to render, derived purely from the field's shape. */
  protected readonly kind = computed<'enum' | 'boolean' | 'number'>(() => {
    const f = this.field();
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
