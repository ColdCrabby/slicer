import { ChangeDetectionStrategy, Component, computed, input, signal } from '@angular/core';
import { Icon } from '@coldcrabby/ui';
import { prefAnchor, prefById } from './pref-registry';

/**
 * One app preference: its name and a line of what it does on the left, its
 * control on the right, and the longer explanation one tap away.
 *
 * The words come from {@link PREFS} by id, so the page states only *which*
 * preference a row is and supplies the control; the Settings search indexes the
 * same entry, and lands on the row by the anchor this component stamps.
 *
 * **The ⓘ expands in place rather than showing a tooltip.** A tooltip needs a
 * hover, which an iPad does not have, and the explanations are exactly the
 * sentences that used to sit under every row and turn the page into prose. Tap
 * it and they appear under the line; tap again and they go.
 *
 * The control wraps under the text when the row is too narrow for both — a
 * segmented control squeezed beside a label reads worse than one on its own
 * line.
 */
@Component({
  selector: 'nexus-pref-row',
  standalone: true,
  imports: [Icon],
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
  protected readonly open = signal(false);
}
