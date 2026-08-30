import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { KeyboardShortcuts } from '../../services/keyboard-shortcuts/keyboard-shortcuts';
import { SectionHeader } from '../../ui/section-header/section-header';

interface ShortcutRow {
  actionId: string;
  displayText: string;
  displayDescription: string;
}

interface ShortcutGroup {
  title: string;
  rows: ShortcutRow[];
}

@Component({
  selector: 'nexus-settings-shortcuts',
  imports: [SectionHeader],
  templateUrl: './shortcuts.html',
  styleUrl: './shortcuts.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ShortcutsSettings {
  private readonly shortcuts = inject(KeyboardShortcuts);

  protected readonly groups: ShortcutGroup[] = this.buildGroups();

  private buildGroups(): ShortcutGroup[] {
    const byId = new Map(this.shortcuts.getAll().map((row) => [row.actionId, row]));
    const pick = (ids: string[]): ShortcutRow[] =>
      ids.map((id) => byId.get(id)).filter((row): row is ShortcutRow => row !== undefined);

    const alt = this.shortcuts.isMac ? '⌥' : 'Alt';

    return [
      { title: 'Editing', rows: pick(['undo', 'redo', 'redo-alt', 'place-objects']) },
      {
        title: 'Object mode',
        rows: pick([
          'object-mode-translate',
          'object-mode-rotate',
          'object-mode-scale',
          'object-mode-pull-to-floor',
        ]),
      },
      { title: 'View', rows: pick(['toggle-gravity', 'toggle-view-mode', 'toggle-projection']) },
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
