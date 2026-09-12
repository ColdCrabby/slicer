import { ChangeDetectionStrategy, Component, EventEmitter, computed, input } from '@angular/core';
import { TooltipDirective } from '@coldcrabby/ui';
import { IconButton } from '../../../shared/icon-button/icon-button';
import type { FieldDef } from '../../models/field-def';
import type { FieldWidget } from '../base-field';

/**
 * Common values per parameter, for the datalist below.
 *
 * Filament types are the materials a desktop FDM machine actually sees; the
 * engine treats the field as free text (a blend, or a vendor's own name, has to
 * remain typable), so this is a shortcut and not a whitelist.
 */
const SUGGESTIONS: Record<string, readonly string[]> = {
  // `filament_type` is deliberately absent: it has a dropdown of its own,
  // because its value is machine-read from the G-code header.
  bed_type: ['Textured PEI', 'Smooth PEI', 'Cool Plate', 'Engineering Plate', 'High Temp Plate'],
};

/**
 * A plain text parameter.
 *
 * The widget resolver used to fall through to the number field for anything it
 * did not recognise, so every free-text parameter — a filament's name, a
 * printer's vendor, a bed-mesh profile — rendered as a number spinner with
 * increment arrows beside it. A string is not a quantity, and a control that
 * offers to increment one tells the user something untrue about the value.
 *
 * `SUGGESTIONS` drive a native `<datalist>`: the engine accepts any string for
 * these, so the list offers the common answers without refusing the rest. That
 * is the honest control for a value that is conventional but not constrained —
 * unlike an enum, which the schema would have declared if it meant to limit it.
 */
@Component({
  selector: 'se-text-field',
  standalone: true,
  imports: [IconButton, TooltipDirective],
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

      input {
        width: 100%;
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
    <input
      [id]="field().key"
      type="text"
      [value]="text()"
      [attr.list]="suggestions().length ? field().key + '-suggestions' : null"
      [attr.placeholder]="field().default ?? null"
      autocomplete="off"
      spellcheck="false"
      (input)="valueChange.emit($any($event.target).value)"
    />
    @if (suggestions().length) {
      <datalist [id]="field().key + '-suggestions'">
        @for (option of suggestions(); track option) {
          <option [value]="option"></option>
        }
      </datalist>
    }
  `,
})
export class TextField implements FieldWidget {
  readonly field = input.required<FieldDef>();
  readonly value = input<unknown>(undefined);
  readonly valueChange = new EventEmitter<unknown>();

  protected readonly text = computed(() => {
    const v = this.value();
    return v === null || v === undefined ? String(this.field().default ?? '') : String(v);
  });

  /** Conventional answers offered for keys that have them; never a restriction. */
  protected readonly suggestions = computed(() => SUGGESTIONS[this.field().key] ?? []);
}
