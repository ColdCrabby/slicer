import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  computed,
  effect,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { Router, RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import { NavigationProgress } from '../../services/navigation-progress';
import { SAVE_DEBOUNCE_MS } from '../../services/profiles/engine-write-through';
import { ProfileSync, type ProfileSyncStatus } from '../../services/profiles/profile-sync';
import { Icon, TooltipDirective } from '@coldcrabby/ui';
import { SettingsNav } from '../../services/settings-nav';
import { ActiveSelection } from '../../services/profiles/active-selection';
import { PrintersStore } from '../../services/profiles/printers-store';
import { FilamentsStore } from '../../services/profiles/filaments-store';
import { PrintProfilesStore } from '../../services/profiles/print-profiles-store';
import { LabelsStore } from '../../services/profiles/labels-store';
import { KeyboardShortcuts } from '../../services/keyboard-shortcuts/keyboard-shortcuts';
import { Viewport } from '../../services/viewport';
import { SETTINGS_GROUPS } from './settings-sections';
import {
  preferenceEntries,
  profileEntries,
  searchSettings,
  sectionEntries,
  settingEntries,
  type SearchEntry,
} from './settings-search';
import { focusConfigureTarget } from './configure-scroll';
import { storageNote } from './prefs/storage-note';

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
  imports: [RouterLink, RouterLinkActive, RouterOutlet, Icon, TooltipDirective],
  templateUrl: './settings-shell.html',
  styleUrl: './settings-shell.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SettingsShell {
  protected readonly nav = inject(SettingsNav);
  private readonly profileSync = inject(ProfileSync);
  private readonly navigation = inject(NavigationProgress);
  private readonly router = inject(Router);
  private readonly active = inject(ActiveSelection);
  private readonly printers = inject(PrintersStore);
  private readonly filaments = inject(FilamentsStore);
  private readonly processes = inject(PrintProfilesStore);
  private readonly labels = inject(LabelsStore);
  private readonly shortcuts = inject(KeyboardShortcuts);
  private readonly viewport = inject(Viewport);

  protected readonly groups = SETTINGS_GROUPS;

  /** Where the library lives in this runtime — the footer's one line. */
  protected readonly storage = storageNote();

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

  // --- Search ------------------------------------------------------------

  protected readonly query = signal('');
  protected readonly activeIndex = signal(0);
  private readonly searchInput = viewChild<ElementRef<HTMLInputElement>>('searchInput');

  /** Pages and preferences — small, and known without loading anything. */
  private readonly fixedEntries: readonly SearchEntry[] = [
    ...sectionEntries(),
    ...preferenceEntries(),
  ];

  /**
   * The slicing parameters, loaded the first time someone reaches for the
   * search. The schema they come from is what the profile editors parse, so it
   * shares their chunk instead of joining the one every Settings page loads.
   */
  private readonly settingIndex = signal<readonly SearchEntry[]>([]);
  private settingIndexRequested = false;

  private readonly entries = computed<SearchEntry[]>(() => [
    ...this.fixedEntries,
    ...profileEntries({
      printer: this.printers.items(),
      filament: this.filaments.items(),
      process: this.processes.items(),
    }),
    ...this.settingIndex(),
  ]);

  protected readonly results = computed(() => searchSettings(this.entries(), this.query()));

  protected readonly activeOptionId = computed(() =>
    this.query() && this.results().length ? `settings-result-${this.activeIndex()}` : null,
  );

  protected readonly searchPlaceholder = computed(() =>
    this.viewport.isHandheld()
      ? 'Search settings'
      : `Search settings (${this.shortcuts.shortcutFor('focus-settings-search')})`,
  );

  /** Fetch the parameter index; idempotent. */
  protected warmSearch(): void {
    if (this.settingIndexRequested) {
      return;
    }
    this.settingIndexRequested = true;
    void import('../../components/profiles/profile-param-groups').then((m) =>
      this.settingIndex.set(settingEntries(m.EDITOR_PARAM_GROUPS)),
    );
  }

  protected setQuery(event: Event): void {
    this.query.set((event.target as HTMLInputElement).value);
    this.activeIndex.set(0);
    this.warmSearch();
  }

  protected clearQuery(): void {
    this.query.set('');
    this.searchInput()?.nativeElement.focus();
  }

  /** Arrow keys walk the results, Enter opens one, Escape clears the box. */
  protected onSearchKey(event: KeyboardEvent): void {
    const count = this.results().length;
    switch (event.key) {
      case 'ArrowDown':
        if (count) {
          event.preventDefault();
          this.activeIndex.set((this.activeIndex() + 1) % count);
        }
        break;
      case 'ArrowUp':
        if (count) {
          event.preventDefault();
          this.activeIndex.set((this.activeIndex() - 1 + count) % count);
        }
        break;
      case 'Enter': {
        const hit = this.results()[this.activeIndex()];
        if (hit) {
          event.preventDefault();
          this.open(hit);
        }
        break;
      }
      case 'Escape':
        if (this.query()) {
          event.preventDefault();
          this.query.set('');
        }
        break;
    }
  }

  /**
   * Go to a result and land on it.
   *
   * A preference is landed on here rather than through the URL: the page may
   * already be the one on screen, and then there is no new page to read a
   * fragment. The profile editors take their `id` and `focus` from the query
   * string, which they follow for as long as they are open.
   */
  protected open(entry: SearchEntry): void {
    this.query.set('');
    void this.router.navigate([entry.path], { queryParams: entry.queryParams ?? {} }).then(() => {
      if (entry.target) {
        focusConfigureTarget(entry.target);
      }
    });
  }

  /** `$mod+f` on a page with no outline of its own. */
  focusSearch(): void {
    if (this.nav.collapsed()) {
      return;
    }
    this.searchInput()?.nativeElement.focus({ preventScroll: true });
    this.searchInput()?.nativeElement.select();
  }

  // --- Sync --------------------------------------------------------------

  /** Aggregated profile-library sync status; `idle` renders nothing. */
  protected readonly syncStatus = this.profileSync.status;

  /**
   * Whether to show the indicator. Delayed by the save debounce so a quick save
   * (settled within the debounce window) never flashes it; hidden immediately
   * once sync goes idle.
   */
  protected readonly syncVisible = signal(false);

  /**
   * The status the indicator displays. Held at the last active value while
   * fading out so the label doesn't blank mid-animation.
   */
  private readonly shownStatus = signal<ProfileSyncStatus>('idle');

  /** Short, non-alarming label for the shown sync status. */
  protected readonly syncLabel = computed(() => {
    switch (this.shownStatus()) {
      case 'loading':
        return 'Loading…';
      case 'saving':
        return 'Saving…';
      case 'error':
        return "Couldn't save";
      default:
        return '';
    }
  });

  /** True when the shown status is an error, for the danger styling. */
  protected readonly syncIsError = computed(() => this.shownStatus() === 'error');

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

  constructor() {
    effect((onCleanup) => {
      const status = this.syncStatus();
      if (status === 'idle') {
        this.syncVisible.set(false);
        return;
      }
      this.shownStatus.set(status);
      const timer = setTimeout(() => this.syncVisible.set(true), SAVE_DEBOUNCE_MS);
      onCleanup(() => clearTimeout(timer));
    });

    // `$mod+f` searches Settings — unless a profile editor's outline is up, in
    // which case it filters the outline, and hands the key back when it goes.
    this.shortcuts.settingsSearchRef = this;
    inject(DestroyRef).onDestroy(() => {
      if (this.shortcuts.settingsSearchRef === this) {
        this.shortcuts.settingsSearchRef = null;
      }
    });
  }
}
