import { ChangeDetectionStrategy, Component, EventEmitter, computed, input } from '@angular/core';
import { CodeEditor } from '../../../components/code-editor/code-editor';
import type { FieldDef } from '../../models/field-def';
import type { FieldWidget } from '../../widgets/base-field';

/**
 * Custom widget for multiline G-code string fields (schema `x-widget: "gcode"`,
 * e.g. `start_gcode`, `end_gcode`, `layer_gcode`, `start_filament_gcode`,
 * `end_filament_gcode`). The generic string widget falls through to a number
 * input, so this swaps in the Monaco-backed {@link CodeEditor} with G-code
 * syntax highlighting, matching the dedicated blocks in the printer/filament
 * profile editors.
 *
 * The label sits above a full-width editor rather than beside it — a code block
 * needs the horizontal room a right-aligned control can't give.
 */
@Component({
  selector: 'se-gcode-field',
  standalone: true,
  imports: [CodeEditor],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <label class="gcode-field__label" [for]="field().key">{{ field().title ?? field().key }}</label>
    @if (descriptionText(); as d) {
      <p class="gcode-field__desc">{{ d }}</p>
    }
    <nexus-code-editor
      class="gcode-field__editor"
      language="gcode"
      [content]="stringValue()"
      (contentChange)="valueChange.emit($event)"
    ></nexus-code-editor>
  `,
  styles: [
    `
      :host {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
      }
      .gcode-field__label {
        font-size: var(--font-size-md, 12px);
        font-weight: 500;
        color: var(--color-text-secondary);
        user-select: none;
      }
      .gcode-field__desc {
        margin: 0;
        font-size: var(--font-size-xs);
        line-height: 1.5;
        color: var(--color-text-tertiary);
        white-space: pre-line;
      }
      .gcode-field__editor {
        height: 160px;
        border: 1px solid var(--color-border-light);
        border-radius: var(--radius-sm, 6px);
        overflow: hidden;
      }
    `,
  ],
})
export class GcodeField implements FieldWidget {
  readonly field = input.required<FieldDef>();
  readonly value = input<unknown>(undefined);
  readonly valueChange = new EventEmitter<unknown>();

  protected readonly stringValue = computed(() => {
    const raw = this.value() ?? this.field().default;
    return typeof raw === 'string' ? raw : '';
  });

  /** Schema description with the engine's lightweight Markdown stripped. */
  protected readonly descriptionText = computed(() =>
    (this.field().description ?? '').replace(/\*\*/g, '').replace(/`/g, '').trim(),
  );
}
