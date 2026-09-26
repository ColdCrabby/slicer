import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import { NavigationProgress } from '../../services/navigation-progress';
import { Icon, TooltipDirective } from '@coldcrabby/ui';
import { SettingsNav } from '../../services/settings-nav';
import { ActiveSelection } from '../../services/profiles/active-selection';
import { PrintersStore } from '../../services/profiles/printers-store';
import { FilamentsStore } from '../../services/profiles/filaments-store';
import { PrintProfilesStore } from '../../services/profiles/print-profiles-store';
import { LabelsStore } from '../../services/profiles/labels-store';
import { SETTINGS_GROUPS } from './settings-sections';
import { SettingsSearchBox } from './settings-search-box';
import { SettingsNavFooter } from './settings-nav-footer';

/**
 * Settings area frame: a section sidebar on the left, routed content right.
 *
 * The sidebar is built to be read at a glance, not just clicked through:
 *
 * - **Grouped.** App preferences, the profile library, and — apart at the foot —
 *   the release notes and the resets. Nine flat rows gave no hint which were
 *   about this device and which about the printers you own.
 * - **Live.** Each library page shows how many you have, and the three that
 *   slice show which one is the default — the answer to "what will this print
 *   with?" without opening anything.
 * - **Searchable.** One box finds a page, an app preference, one of your
 *   profiles, or any slicing parameter the editors show, and lands on it.
 *
 * Its width and its fold are shared with the profile editors through
 * {@link SettingsNav}: on a window the width of an iPad held sideways it folds
 * to icons by itself while an editor needs the room for its outline.
 */
@Component({
  selector: 'nexus-settings-shell',
  imports: [
    RouterLink,
    RouterLinkActive,
    RouterOutlet,
    Icon,
    TooltipDirective,
    SettingsSearchBox,
    SettingsNavFooter,
  ],
  templateUrl: './settings-shell.html',
  styleUrl: './settings-shell.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SettingsShell {
  protected readonly nav = inject(SettingsNav);
  private readonly navigation = inject(NavigationProgress);
  private readonly active = inject(ActiveSelection);
  private readonly printers = inject(PrintersStore);
  private readonly filaments = inject(FilamentsStore);
  private readonly processes = inject(PrintProfilesStore);
  private readonly labels = inject(LabelsStore);

  protected readonly groups = SETTINGS_GROUPS;

  /** How many of each thing the library holds, for the count beside its page. */
  protected countFor(path: string): number | null {
    switch (path) {
      case 'printers':
        return this.printers.count();
      case 'filaments':
        return this.filaments.count();
      case 'profiles':
        return this.processes.count();
      case 'labels':
        return this.labels.items().length;
      default:
        return null;
    }
  }

  /** The default for each page that slices, named under its label. */
  protected summaryFor(path: string): string | null {
    switch (path) {
      case 'printers':
        return this.active.printer()?.name ?? null;
      case 'filaments':
        return this.active.filament()?.name ?? null;
      case 'profiles':
        return this.active.profile()?.name ?? null;
      default:
        return null;
    }
  }

  /** The fold button's words — which also explain a list that folded itself. */
  protected readonly foldLabel = computed(() =>
    !this.nav.collapsed()
      ? 'Collapse section list'
      : this.nav.foldedForRoom()
        ? 'Expand section list (folded to make room for the outline)'
        : 'Expand section list',
  );

  /**
   * Whether this section is the one currently being loaded.
   *
   * Every settings section is its own lazy chunk, so the first visit to one can
   * involve a fetch. `path` is relative to `/settings`, matching the template's
   * `routerLink`.
   */
  protected isPending(path: string): boolean {
    return this.navigation.isPendingUnder(`/settings/${path}`);
  }
}
