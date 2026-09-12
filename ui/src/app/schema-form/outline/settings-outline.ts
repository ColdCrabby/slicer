import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import { Icon } from '@coldcrabby/ui';
import type { OutlineEntry, OutlineSection } from '../models/outline';
import type { Tier } from '../models/relevance';

/** A jump request for a single setting. */
export interface OutlineFieldJump {
  group: string;
  key: string;
}

/**
 * The settings outline — every section and every setting name, at once.
 *
 * Presentational: it renders the table of contents it is handed and emits where
 * the user wants to go. Revealing tiers, expanding the accordion and scrolling
 * are the form's job, because they are changes to the form's own state.
 */
@Component({
  selector: 'se-settings-outline',
  standalone: true,
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './settings-outline.html',
  styleUrl: './settings-outline.scss',
})
export class SettingsOutline {
  readonly sections = input.required<readonly OutlineSection[]>();

  /** Section the form was scrolled to when the outline opened, if any. */
  readonly currentSection = input<string | null>(null);

  /** What the user was filtering by, so the empty state can quote it. */
  readonly query = input('');

  readonly jumpSection = output<string>();
  readonly jumpField = output<OutlineFieldJump>();

  /**
   * The tier a run of rows begins at, or `null` mid-run.
   *
   * Entries arrive ordered everyday → advanced → expert, so each tier is one
   * contiguous run and one label above it says everything a badge on all
   * fourteen rows would have said — fourteen times, in a column two hundred
   * rows long.
   */
  protected startsTier(entries: readonly OutlineEntry[], index: number): Tier | null {
    const tier = entries[index].tier;
    if (tier === 'everyday') {
      return null;
    }
    return index === 0 || entries[index - 1].tier !== tier ? tier : null;
  }

  /** Label for the divider that says how deep the form keeps what follows. */
  protected tierLabel(tier: Tier): string {
    return tier === 'advanced' ? 'Advanced' : 'Expert';
  }
}
