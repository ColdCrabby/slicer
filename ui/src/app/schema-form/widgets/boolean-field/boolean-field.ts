import { ChangeDetectionStrategy, Component, EventEmitter, input } from '@angular/core';
import { Switch, TooltipDirective } from '@coldcrabby/ui';
import { IconButton } from '../../../shared/icon-button/icon-button';
import type { FieldDef } from '../../models/field-def';
import type { FieldWidget } from '../base-field';

@Component({
  selector: 'se-boolean-field',
  standalone: true,
  imports: [IconButton, TooltipDirective, Switch],
  changeDetection: ChangeDetectionStrategy.OnPush,
  styles: [
    `
      :host {
        display: block;
      }

      .bool-row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-md);
        min-height: 24px;
      }

      .bool-label {
        display: flex;
        align-items: center;
        gap: 4px;
        font-size: 12px;
        font-weight: 500;
        color: var(--color-text-secondary);
        cursor: pointer;
        user-select: none;
      }
    `,
  ],
  template: `
    <div class="bool-row">
      <span class="bool-label" (click)="toggle()">
        {{ field().title ?? field().key }}
        @if (field().description) {
          <nexus-icon-button
            icon="help-circle"
            label="More info"
            [tooltip]="field().description!"
            [tooltipMode]="'block'"
            [tooltipClickToggle]="true"
            (click)="$event.stopPropagation()"
          />
        }
      </span>
      <nexus-switch
        [checked]="!!value()"
        [label]="field().title ?? field().key"
        (checkedChange)="valueChange.emit($event)"
      ></nexus-switch>
    </div>
  `,
})
export class BooleanField implements FieldWidget {
  readonly field = input.required<FieldDef>();
  readonly value = input<unknown>(undefined);
  readonly valueChange = new EventEmitter<unknown>();

  protected toggle(): void {
    this.valueChange.emit(!this.value());
  }
}
