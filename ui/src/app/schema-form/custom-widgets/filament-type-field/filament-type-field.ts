import { ChangeDetectionStrategy, Component, EventEmitter, computed, input } from '@angular/core';
import { Select, type SelectOption, TooltipDirective } from '@coldcrabby/ui';
import { IconButton } from '../../../shared/icon-button/icon-button';
import type { FieldDef } from '../../models/field-def';
import type { FieldWidget } from '../../widgets/base-field';
import { filamentTypeOptions } from './filament-types';

/**
 * Material picker for `filament_type`.
 *
 * Keeps whatever the active profile already holds even when it is not one of
 * the standard names: a vendor profile carrying `PLA+` or `PLA Matte` must not
 * be silently rewritten just because the user opened the dropdown. That value
 * is offered as an extra option, marked as coming from the profile, and stays
 * selectable — the list constrains what a user can *newly* type, not what the
 * library is allowed to contain.
 */
@Component({
  selector: 'se-filament-type-field',
  standalone: true,
  imports: [IconButton, TooltipDirective, Select],
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
    <nexus-select
      [options]="options()"
      [value]="current()"
      (valueChange)="valueChange.emit($event)"
    ></nexus-select>
  `,
})
export class FilamentTypeField implements FieldWidget {
  readonly field = input.required<FieldDef>();
  readonly value = input<unknown>(undefined);
  readonly valueChange = new EventEmitter<unknown>();

  protected readonly current = computed(() => {
    const raw = this.value();
    const text = raw === null || raw === undefined ? '' : String(raw).trim();
    return text || String(this.field().default ?? 'PLA');
  });

  protected readonly options = computed<SelectOption[]>(() => filamentTypeOptions(this.current()));
}
