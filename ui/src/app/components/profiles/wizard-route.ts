import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import { Icon } from '@coldcrabby/ui';

/**
 * One way to start a wizard, as a single quiet row.
 *
 * The first screen of every profile wizard asks one question — *where should
 * this start from?* — and the routes are not equal: one is recommended, the
 * others are there for when it does not apply. Rendering all of them expanded
 * put a paragraph, a hint, two headings, a divider, a search field and a
 * catalog error on screen at once, which reads as a form rather than a choice.
 *
 * So a route is a row. `go` routes act on click and move on; `expand` routes
 * unfold whatever they need in place, one at a time, and everything they own —
 * a catalog's search box, its loading and error states — stays folded away
 * until the user asks for that route.
 */
@Component({
  selector: 'nexus-wizard-route',
  standalone: true,
  imports: [Icon],
  templateUrl: './wizard-route.html',
  styleUrl: './wizard-route.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class WizardRoute {
  readonly icon = input.required<string>();
  readonly title = input.required<string>();
  /** One line under the title. Anything longer belongs inside the route. */
  readonly description = input('');
  /** `go` advances the wizard; `expand` unfolds this row's own body. */
  readonly mode = input<'go' | 'expand'>('go');
  /** Whether an `expand` route is currently unfolded. */
  readonly open = input(false);
  /** Marks the route the wizard would pick for you. */
  readonly recommended = input(false);

  /** The row was pressed: navigate, or toggle, as {@link mode} says. */
  readonly activate = output<void>();
}
