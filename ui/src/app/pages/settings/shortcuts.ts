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

    return [
      { title: 'Editing', rows: pick(['undo', 'redo', 'redo-alt', 'auto-orient']) },
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
