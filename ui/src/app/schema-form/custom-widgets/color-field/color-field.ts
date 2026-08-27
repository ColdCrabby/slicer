import { ChangeDetectionStrategy, Component, EventEmitter, computed, input } from '@angular/core';
import { ColorPicker } from '../../../ui/color-picker/color-picker';
import { IconButton } from '../../../shared/icon-button/icon-button';
import { TooltipDirective } from '../../../shared/tooltip/tooltip.directive';
import type { FieldDef } from '../../models/field-def';
import type { FieldWidget } from '../../widgets/base-field';

/**
 * Custom widget for `#rrggbb` colour string fields (e.g. the thumbnail's custom
 * model colour). The generic string widget would render a bare text input, so
 * this swaps in the design-system {@link ColorPicker} with a swatch trigger and
 * hex popover instead.
 */
@Component({
  selector: 'se-color-field',
  standalone: true,
  imports: [ColorPicker, IconButton, TooltipDirective],
  changeDetection: ChangeDetectionStrategy.OnPush,
  styles: [
    `
      :host {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
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
    <nexus-color-picker
      [value]="hexValue()"
      [ariaLabel]="(field().title ?? field().key) + ' colour'"
      (valueChange)="valueChange.emit($event)"
    />
  `,
})
export class ColorField implements FieldWidget {
  readonly field = input.required<FieldDef>();
  readonly value = input<unknown>(undefined);
  readonly valueChange = new EventEmitter<unknown>();

  protected readonly hexValue = computed(() => {
    const raw = this.value() ?? this.field().default;
    return typeof raw === 'string' && /^#[0-9a-fA-F]{6}$/.test(raw) ? raw : '#e0912f';
  });
}
