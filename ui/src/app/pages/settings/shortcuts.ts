import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { KeyboardShortcuts } from '../../services/keyboard-shortcuts/keyboard-shortcuts';

interface ShortcutRow {
  actionId: string;
  displayText: string;
  displayDescription: string;
}

interface ShortcutGroup {
  title: string;
  rows: ShortcutRow[];
}

/**
 * Every keyboard shortcut, grouped by where it applies.
 *
 * A reference rather than a page of its own: it lives at the foot of Settings →
 * Controls, beside the trackpad and touch preferences, because "how do I drive
 * this" is one question whichever hand is answering it.
 */
@Component({
  selector: 'nexus-shortcut-reference',
  templateUrl: './shortcuts.html',
  styleUrl: './shortcuts.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ShortcutReference {
  private readonly shortcuts = inject(KeyboardShortcuts);

  protected readonly groups: ShortcutGroup[] = this.buildGroups();

  private buildGroups(): ShortcutGroup[] {
    const byId = new Map(this.shortcuts.getAll().map((row) => [row.actionId, row]));
    const pick = (ids: string[]): ShortcutRow[] =>
      ids.map((id) => byId.get(id)).filter((row): row is ShortcutRow => row !== undefined);

    const alt = this.shortcuts.isMac ? '⌥' : 'Alt';

    return [
      {
        title: 'Editing',
        rows: pick([
          'undo',
          'redo',
          'redo-alt',
          'slice',
          'place-objects',
          'select-all',
          'duplicate-selected',
          'remove-selected',
          'remove-selected-alt',
        ]),
      },
      {
        title: 'Object mode',
        rows: [
          ...pick([
            'object-mode-translate',
            'object-mode-rotate',
            'object-mode-scale',
            'object-mode-pull-to-floor',
            'object-mode-paint',
            'leave-tool',
            'brush-quick-adjust',
          ]),
          // Eight bindings, one idea: listed once rather than per arrow.
          {
            actionId: 'nudge',
            displayText: '← ↑ → ↓',
            displayDescription: `Nudge the selection 1 mm, as seen from the camera (Shift 10 mm, ${alt} 0.1 mm)`,
          },
        ],
      },
      {
        title: 'View',
        rows: pick([
          'zoom-to-selection',
          'toggle-gravity',
          'toggle-view-mode',
          'toggle-projection',
        ]),
      },
      {
        title: 'Number fields',
        rows: [
          {
            actionId: 'numfield-scroll',
            displayText: 'Scroll',
            displayDescription:
              'Adjust a number field by one step (hover in the transform panel, or focus elsewhere)',
          },
          {
            actionId: 'numfield-arrows',
            displayText: '↑ / ↓',
            displayDescription: 'Step a focused number field up or down',
          },
          {
            actionId: 'numfield-coarse',
            displayText: 'Shift',
            displayDescription: 'Hold while scrolling or stepping for a coarse ×10 step',
          },
          {
            actionId: 'numfield-fine',
            displayText: alt,
            displayDescription: 'Hold while scrolling or stepping for a fine ×0.1 step',
          },
        ],
      },
      {
        title: 'G-code viewer',
        rows: pick([
          'gcode-next-extrusion',
          'gcode-prev-extrusion',
          'gcode-next-layer',
          'gcode-prev-layer',
        ]),
      },
      { title: 'Search', rows: pick(['focus-settings-search']) },
    ].filter((group) => group.rows.length > 0);
  }
}
