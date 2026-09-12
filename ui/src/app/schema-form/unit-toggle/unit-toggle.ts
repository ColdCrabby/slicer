import { ChangeDetectionStrategy, Component, computed, input, output } from '@angular/core';
import { TooltipDirective } from '@coldcrabby/ui';
import type { UnitOption } from '../models/field-units';

/**
 * The unit suffix of a numeric field, when the unit is a choice — press it to
 * read the same value in the next unit the family offers.
 *
 * It sits **inside** the field, exactly where a static unit suffix sits, so a
 * field whose unit happens to be switchable is not a differently-shaped control
 * from one whose unit is fixed. The design system's own suffix is static text,
 * so the owner renders that suffix for its width and hides it, and this lands
 * on top of it: same place, same size, same tone at rest. What marks it as
 * pressable is what happens under a pointer — nothing shouts at a reader who
 * never needs it.
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
      /*
       * Parked over the number input's own unit slot. The offset is the
       * stepper's width plus the slot's right padding — both fixed by the
       * design system — so it lands on the hidden suffix without anyone
       * measuring anything at runtime.
       */
      :host {
        position: absolute;
        right: calc(32px + var(--spacing-sm));
        top: 50%;
        transform: translateY(-50%);
        display: inline-flex;
      }

      .unit-toggle {
        display: inline-flex;
        align-items: center;
        padding: 0;
        border: none;
        background: transparent;
        color: var(--color-text-tertiary);
        font: inherit;
        font-size: var(--font-size-xs);
        line-height: 1;
        white-space: nowrap;
        cursor: pointer;
        /* Dotted, not solid: the affordance of a definition, not of a link. */
        text-decoration: underline dotted transparent;
        text-underline-offset: 3px;
        transition:
          color var(--duration-fast) var(--ease-standard),
          text-decoration-color var(--duration-fast) var(--ease-standard);
      }

      .unit-toggle:hover {
        color: var(--color-text-primary);
        text-decoration-color: currentColor;
      }

      .unit-toggle:active {
        color: var(--accent);
      }

      .unit-toggle:focus-visible {
        outline: 2px solid var(--accent);
        outline-offset: 2px;
        border-radius: var(--radius-sm);
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
