import {
  ChangeDetectionStrategy,
  Component,
  EventEmitter,
  effect,
  input,
  signal,
  untracked,
} from '@angular/core';
import { Slider } from '../../../ui/slider/slider';
import { IconButton } from '../../../shared/icon-button/icon-button';
import { TooltipDirective } from '../../../shared/tooltip/tooltip.directive';
import type { FieldDef } from '../../models/field-def';
import type { FieldWidget } from '../../widgets/base-field';

const MIN_DENSITY = 0;
const MAX_DENSITY = 100;

/**
 * Custom widget for `infill_density`.
 *
 * The schema represents infill density as a fraction 0.0–1.0, but the
 * WebSocket API expects a percentage (0–100). This widget displays and
 * edits the value as a percentage while keeping the emitted value in the
 * schema's native fraction form (0.0–1.0).
 *
 * Renders a range slider alongside a read-only numeric readout so the
 * user has both tactile control and a precise value in view.
 */
@Component({
  selector: 'se-infill-density-slider',
  standalone: true,
  imports: [IconButton, TooltipDirective, Slider],
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
    `,
  ],
  template: `
    <label [for]="field().key">
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
    <nexus-slider
      [value]="displayPercent()"
      [min]="MIN"
      [max]="MAX"
      [step]="1"
      unit="%"
      [label]="field().title ?? field().key"
      (valueChange)="onSliderInput($event)"
    ></nexus-slider>
  `,
})
export class InfillDensitySlider implements FieldWidget {
  readonly field = input.required<FieldDef>();
  readonly value = input<unknown>(undefined);
  readonly valueChange = new EventEmitter<unknown>();

  protected readonly MIN = MIN_DENSITY;
  protected readonly MAX = MAX_DENSITY;

  /** Current value expressed as an integer percentage (0–100). */
  protected readonly displayPercent = signal<number>(20);

  constructor() {
    effect(() => {
      const raw = this.value(); // tracked
      untracked(() => {
        if (raw !== undefined && raw !== null) {
          const num = Number(raw);
          // Accept both fraction (0–1) and percent (0–100) gracefully
          const pct = num <= 1 ? Math.round(num * MAX_DENSITY) : Math.round(num);
          this.displayPercent.set(Math.max(MIN_DENSITY, Math.min(MAX_DENSITY, pct)));
        }
      });
    });
  }

  protected onSliderInput(pctRaw: number): void {
    const pct = Math.round(pctRaw);
    this.displayPercent.set(pct);
    // Emit as fraction so callers receive the same units as other fields
    this.valueChange.emit(pct / MAX_DENSITY);
  }
}
