import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  computed,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { Router } from '@angular/router';
import { Icon } from '@coldcrabby/ui';
import { KeyboardShortcuts } from '../../services/keyboard-shortcuts/keyboard-shortcuts';
import { FilamentsStore } from '../../services/profiles/filaments-store';
import { PrintProfilesStore } from '../../services/profiles/print-profiles-store';
import { PrintersStore } from '../../services/profiles/printers-store';
import { SettingsNav } from '../../services/settings-nav';
import { Viewport } from '../../services/viewport';
import { focusConfigureTarget } from './configure-scroll';
import {
  preferenceEntries,
  profileEntries,
  searchSettings,
  sectionEntries,
  settingEntries,
  type SearchEntry,
} from './settings-search';

/**
 * The Settings sidebar's search: the box, and the results that take the section
 * list's place while it has a query.
 *
 * Results replace the list rather than floating over it, the way the system's
 * own settings search does — the sidebar is where the reader was looking, so it
 * is where the answer appears. The shell hides its list while {@link active}.
 *
 * Owns `$mod+f` on Settings pages that have no outline of their own; a profile
 * editor's outline borrows the key while it is up and hands it back.
 */
@Component({
  selector: 'nexus-settings-search',
  imports: [Icon],
  templateUrl: './settings-search-box.html',
  styleUrl: './settings-search-box.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SettingsSearchBox {
  private readonly router = inject(Router);
  private readonly nav = inject(SettingsNav);
  private readonly printers = inject(PrintersStore);
  private readonly filaments = inject(FilamentsStore);
  private readonly processes = inject(PrintProfilesStore);
  private readonly shortcuts = inject(KeyboardShortcuts);
  private readonly viewport = inject(Viewport);

  protected readonly query = signal('');
  protected readonly activeIndex = signal(0);
  private readonly input = viewChild<ElementRef<HTMLInputElement>>('searchInput');

  /** Whether results are showing in place of the section list. */
  readonly active = computed(() => this.query().trim().length > 0);

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
    this.active() && this.results().length ? `settings-result-${this.activeIndex()}` : null,
  );

  protected readonly placeholder = computed(() =>
    this.viewport.isHandheld()
      ? 'Search settings'
      : `Search settings (${this.shortcuts.shortcutFor('focus-settings-search')})`,
  );

  constructor() {
    this.shortcuts.settingsSearchRef = this;
    inject(DestroyRef).onDestroy(() => {
      if (this.shortcuts.settingsSearchRef === this) {
        this.shortcuts.settingsSearchRef = null;
      }
    });
  }

  /** Fetch the parameter index; idempotent. */
  protected warm(): void {
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
    this.warm();
  }

  protected clear(): void {
    this.query.set('');
    this.input()?.nativeElement.focus();
  }

  /** Arrow keys walk the results, Enter opens one, Escape clears the box. */
  protected onKey(event: KeyboardEvent): void {
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
   * string.
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
    const input = this.input()?.nativeElement;
    if (!input || this.nav.collapsed()) {
      return;
    }
    input.focus({ preventScroll: true });
    input.select();
  }
}
