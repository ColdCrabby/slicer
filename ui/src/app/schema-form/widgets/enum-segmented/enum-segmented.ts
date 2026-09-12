import { ChangeDetectionStrategy, Component, EventEmitter, computed, input } from '@angular/core';
import { Segmented, type SegmentOption, TooltipDirective } from '@coldcrabby/ui';
import { IconButton } from '../../../shared/icon-button/icon-button';
import type { FieldDef } from '../../models/field-def';
import type { FieldWidget } from '../base-field';

/**
 * The default control for a short enum: a recessed track showing every choice
 * at once, with the selection raised as a pill.
 *
 * This is what a small, plainly-named choice looks like — `Draft · Normal ·
 * High Quality`, `Light · Dark · Transparent`. Nothing has to be opened to see
 * the alternatives, and the row stays one line tall, which is what keeps a
 * settings group scannable. Each variant's description rides along as a
 * tooltip; a field whose options genuinely need that text on screen asks for
 * `x-widget = "cards"` instead.
 *
 * The control stacks itself vertically when the sidebar is too narrow for the
 * labels, so long variant names stay readable rather than being clipped.
 */
@Component({
  selector: 'se-enum-segmented',
  standalone: true,
  imports: [IconButton, TooltipDirective, Segmented],
  changeDetection: ChangeDetectionStrategy.OnPush,
  styles: [
    `
      :host {
        display: flex;
        flex-direction: column;
        gap: 6px;
      }

      .legend {
        display: flex;
        align-items: center;
        gap: 4px;
        font-size: 12px;
        font-weight: 500;
        color: var(--color-text-secondary);
        user-select: none;
      }
    `,
  ],
  template: `
    <span class="legend field-label">
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
    </span>
    <nexus-segmented
      [options]="options()"
      [value]="stringValue()"
      [label]="field().title ?? field().key"
      (valueChange)="valueChange.emit($event)"
    ></nexus-segmented>
  `,
})
export class EnumSegmented implements FieldWidget {
  readonly field = input.required<FieldDef>();
  readonly value = input<unknown>(undefined);
  readonly valueChange = new EventEmitter<unknown>();

  protected readonly options = computed<SegmentOption[]>(() =>
    (this.field().enumOptions ?? []).map((o) => ({
      value: o.value,
      label: o.label,
      description: o.description,
    })),
  );

  protected readonly stringValue = computed(() => {
    const v = this.value() ?? this.field().default;
    return v == null ? null : String(v);
  });
}
