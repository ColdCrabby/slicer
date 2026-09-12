import { ChangeDetectionStrategy, Component, computed, input, output } from '@angular/core';
import { TooltipDirective } from '@coldcrabby/ui';
import type { UnitOption } from '../models/field-units';

/**
 * The unit suffix of a numeric field, when the unit is a choice — press it to
 * read the same value in the next unit the family offers.
 *
 * It sits beside the number input rather than inside it because the design
 * system's in-field suffix is static text. That turns out to be the honest
 * arrangement: a unit the user can press should not look identical to one they
 * cannot, and a chip that lights up under the pointer says so without a label
 * or an icon explaining itself.
 *
 * Purely presentational. It renders the unit it is given and emits the next
 * one; which fields share a unit, and where that choice is remembered, is the
 * business of `UnitPreference`.
 */
@Component({
  selector: 'se-unit-toggle',
  standalone: true,
  imports: [TooltipDirective],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <button
      type="button"
      class="unit-toggle"
      [attr.aria-label]="'Unit: ' + label() + '. Switch to ' + nextLabel() + '.'"
      [tooltip]="'Show in ' + nextLabel()"
      (click)="cycle.emit()"
    >
      {{ label() }}
    </button>
  `,
  styles: [
    `
      :host {
        display: inline-flex;
        flex: none;
      }

      .unit-toggle {
        display: inline-flex;
        align-items: center;
        height: 34px;
        padding: 0 var(--spacing-xs);
        border: 1px solid transparent;
        border-radius: var(--radius-md);
        background: transparent;
        color: var(--color-text-tertiary);
        font: inherit;
        font-size: var(--font-size-xs);
        white-space: nowrap;
        cursor: pointer;
        transition:
          background-color var(--duration-fast) var(--ease-standard),
          border-color var(--duration-fast) var(--ease-standard),
          color var(--duration-fast) var(--ease-standard);
      }

      .unit-toggle:hover {
        border-color: var(--color-border);
        background: var(--color-surface-hover);
        color: var(--color-text-primary);
      }

      .unit-toggle:active {
        background: var(--accent-soft);
        color: var(--accent);
      }

      .unit-toggle:focus-visible {
        outline: 2px solid var(--accent);
        outline-offset: 1px;
      }
    `,
  ],
})
export class UnitToggle {
  /** Id of the unit currently on screen. */
  readonly current = input.required<string>();
  /** Every unit the family offers, in the order this control cycles them. */
  readonly options = input.required<readonly UnitOption[]>();
  /** The user pressed it; the owner advances the preference. */
  readonly cycle = output<void>();

  private readonly index = computed(() => {
    const at = this.options().findIndex((o) => o.id === this.current());
    return at === -1 ? 0 : at;
  });

  protected readonly label = computed(() => this.options()[this.index()]?.label ?? '');

  /** What the next press will switch to — the whole content of the tooltip. */
  protected readonly nextLabel = computed(() => {
    const options = this.options();
    return options[(this.index() + 1) % options.length]?.label ?? this.label();
  });
}
