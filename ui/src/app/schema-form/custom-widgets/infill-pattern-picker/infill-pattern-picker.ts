import { ChangeDetectionStrategy, Component, EventEmitter, computed, input } from '@angular/core';
import { Segmented, type SegmentOption, TooltipDirective } from '@coldcrabby/ui';
import { IconButton } from '../../../shared/icon-button/icon-button';
import type { FieldDef } from '../../models/field-def';
import type { FieldWidget } from '../../widgets/base-field';

const PATTERNS: SegmentOption[] = [
  {
    value: 'Rectilinear',
    label: 'Lines',
    description: 'Parallel lines alternating direction per layer (default, fastest).',
  },
  {
    value: 'Grid',
    label: 'Grid',
    description: 'Perpendicular lines forming a grid pattern (stronger).',
  },
  {
    value: 'Honeycomb',
    label: 'Hex',
    description: 'Hexagonal cells (good strength-to-weight ratio).',
  },
  {
    value: 'Gyroid',
    label: 'Gyroid',
    description: '3D mathematical pattern (experimental, best strength).',
  },
  {
    value: 'TpmsD',
    label: 'TPMS-D',
    description: 'Triply Periodic Minimal Surface – Diamond (organic, isotropic).',
  },
];

/**
 * Custom widget for `infill_pattern`.
 *
 * Renders the enum options as a `nexus-segmented` control; each segment's
 * description shows as an inline tooltip so the abbreviated labels stay
 * discoverable.
 */
@Component({
  selector: 'se-infill-pattern-picker',
  standalone: true,
  imports: [Segmented, IconButton, TooltipDirective],
  changeDetection: ChangeDetectionStrategy.OnPush,
  styles: [
    `
      :host {
        display: flex;
        flex-direction: column;
        gap: 6px;
      }

      .field-label {
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
    <span class="field-label">
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
      [options]="patterns"
      [value]="stringValue()"
      [label]="field().title ?? field().key"
      (valueChange)="valueChange.emit($event)"
    ></nexus-segmented>
  `,
})
export class InfillPatternPicker implements FieldWidget {
  readonly field = input.required<FieldDef>();
  readonly value = input<unknown>(undefined);
  readonly valueChange = new EventEmitter<unknown>();

  readonly patterns = PATTERNS;

  protected readonly stringValue = computed(() => {
    const v = this.value() ?? this.field().default ?? PATTERNS[0].value;
    return v == null ? null : String(v);
  });
}
