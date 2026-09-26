import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { TooltipDirective } from '@coldcrabby/ui';
import { IconButton } from '../../../shared/icon-button/icon-button';
import { prefAnchor, prefById } from './pref-registry';

/**
 * One app preference: its name and a line of what it does on the left, its
 * control on the right, and the longer explanation behind a help mark.
 *
 * The words come from {@link PREFS} by id, so the page states only *which*
 * preference a row is and supplies the control; the Settings search indexes the
 * same entry, and lands on the row by the anchor this component stamps.
 *
 * **The help mark is the same block tooltip every settings field uses**, and
 * opens on tap as well as hover, so an iPad reaches it too. Expanding in place
 * pushed the rows below it down.
 *
 * The control wraps under the text when the row is too narrow for both — a
 * segmented control squeezed beside a label reads worse than one on its own
 * line.
 */
@Component({
  selector: 'nexus-pref-row',
  standalone: true,
  imports: [IconButton, TooltipDirective],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './pref-row.html',
  styleUrl: './pref-row.scss',
  host: {
    '[id]': 'anchor()',
  },
})
export class PrefRow {
  /** Which preference this row is, by its {@link PREFS} id. */
  readonly pref = input.required<string>();
  /**
   * A live line to show instead of the registry's — what Automatic is doing
   * right now, say. Empty keeps the registry's.
   */
  readonly hint = input('');

  protected readonly def = computed(() => prefById(this.pref()));
  protected readonly anchor = computed(() => prefAnchor(this.pref()));
  protected readonly line = computed(() => this.hint() || this.def().hint || '');
}
