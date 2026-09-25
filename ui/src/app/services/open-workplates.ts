import { Injectable, computed, effect, inject, untracked } from '@angular/core';
import { toSignal } from '@angular/core/rxjs-interop';
import { NavigationEnd, Router } from '@angular/router';
import { filter, map, startWith } from 'rxjs';
import { BrowserStorage } from './browser-storage';
import { SlicerFile } from './slicer-file';

const STORAGE_KEY = 'workplate.open-tabs';

/** Routes that mean "a plate the user is starting, which has no id yet". */
const DRAFT_ROUTES = new Set(['/', '/slice/new']);

/** One open tab: a workplate's `request_uuid` plus the filename it was last seen with. */
export interface OpenWorkplateTab {
  uuid: string;
  filename: string | null;
}

/**
 * Which workplates are open as tabs in the titlebar.
 *
 * This is the list and nothing else. Making a tab the plate on screen is
 * {@link WorkplateSession}'s job, reached through the `/slice/:requestUuid`
 * route, exactly as clicking a browser tab is a navigation and not a
 * re-implementation of the page.
 *
 * The list lives in `localStorage` and is read through {@link BrowserStorage},
 * so a second window of the same slicer sees a tab opened in the first rather
 * than the two quietly overwriting each other's list.
 */
@Injectable({ providedIn: 'root' })
export class OpenWorkplates {
  private readonly storage = inject(BrowserStorage);
  private readonly router = inject(Router);
  private readonly slicerFile = inject(SlicerFile);

  /** Raw stored list; `BrowserStorage` keeps it in step across windows. */
  private readonly stored = this.storage.get(STORAGE_KEY, 'local');

  /** Open workplate tabs, in the order they were opened. */
  readonly tabs = computed<readonly OpenWorkplateTab[]>(() => parse(this.stored()));

  /** The `:requestUuid` of the route currently on screen, or `null` off `/slice`. */
  readonly activeUuid = toSignal(
    this.router.events.pipe(
      filter((event) => event instanceof NavigationEnd),
      startWith(null),
      map(() => this.#uuidFromUrl(this.router.url)),
    ),
    { initialValue: this.#uuidFromUrl(this.router.url) },
  );

  /**
   * True while the user is on a plate that has no `request_uuid` yet — the
   * dashboard they land on after pressing `+`, or the empty-plate drop screen.
   *
   * Without it the strip renders a tablist with nothing selected while the user
   * is demonstrably somewhere; the draft tab is replaced by a real one the
   * moment a model lands.
   */
  readonly isNewPlate = toSignal(
    this.router.events.pipe(
      filter((event) => event instanceof NavigationEnd),
      startWith(null),
      map(() => this.#isDraftUrl(this.router.url)),
    ),
    { initialValue: this.#isDraftUrl(this.router.url) },
  );

  constructor() {
    // A plate becomes a tab the moment it's loaded — upload, history entry, or
    // deep link — mirroring how a browser tab opens the moment its page loads,
    // not only when the user explicitly asks for one.
    //
    // `open()` reads the tab list, so calling it straight from the effect would
    // make that list one of the effect's own dependencies — and closing a tab
    // writes it. The effect would re-run, still see the closed plate in
    // `requestUuid()`, and immediately re-open the tab it had just removed,
    // which is why the close button did nothing for the plate on screen.
    // `untracked` keeps the trigger to "which plate is loaded", as intended.
    effect(() => {
      const uuid = this.slicerFile.requestUuid();
      const filename = this.slicerFile.sourceFilename();
      if (uuid) {
        untracked(() => this.open(uuid, filename));
      }
    });
  }

  /** Add (or update the remembered filename of) a tab. */
  open(uuid: string, filename?: string | null): void {
    const tabs = this.tabs();
    const existing = tabs.find((tab) => tab.uuid === uuid);
    if (existing) {
      if (filename && !existing.filename) {
        this.#persist(tabs.map((tab) => (tab.uuid === uuid ? { ...tab, filename } : tab)));
      }
      return;
    }
    this.#persist([...tabs, { uuid, filename: filename ?? null }]);
  }

  /**
   * Close a tab. If it was the one on screen, navigates to its neighbour — or
   * to the dashboard when it was the last one open.
   *
   * Closing does not discard the plate: its saved setup and its model files
   * stay exactly where reopening it from the history list would look for them.
   */
  close(uuid: string): void {
    const tabs = this.tabs();
    const index = tabs.findIndex((tab) => tab.uuid === uuid);
    if (index === -1) {
      return;
    }
    this.#persist(tabs.filter((tab) => tab.uuid !== uuid));

    if (this.activeUuid() !== uuid) {
      return;
    }
    const remaining = this.tabs();
    const fallback = remaining[index] ?? remaining[index - 1];
    void this.router.navigate(fallback ? ['/slice', fallback.uuid] : ['/']);
  }

  /**
   * Close every tab, or every tab but `keep`.
   *
   * One call rather than a loop of {@link close}: each of those would navigate,
   * and the route only settles asynchronously — so a loop ended up on a plate it
   * had already closed, and then dutifully reopened it.
   */
  closeAllExcept(keep?: string): void {
    const remaining = keep ? this.tabs().filter((tab) => tab.uuid === keep) : [];
    this.#persist(remaining);

    const active = this.activeUuid();
    if (!active || remaining.some((tab) => tab.uuid === active)) {
      return;
    }
    void this.router.navigate(remaining[0] ? ['/slice', remaining[0].uuid] : ['/']);
  }

  /**
   * Close every tab after `uuid`. If the workplate on screen was one of them, the
   * tab the request came from takes over — it is the one the user is pointing at.
   */
  closeToTheRightOf(uuid: string): void {
    const tabs = this.tabs();
    const index = tabs.findIndex((tab) => tab.uuid === uuid);
    if (index === -1) {
      return;
    }
    const remaining = tabs.slice(0, index + 1);
    this.#persist(remaining);

    const active = this.activeUuid();
    if (!active || remaining.some((tab) => tab.uuid === active)) {
      return;
    }
    void this.router.navigate(['/slice', uuid]);
  }

  #persist(tabs: readonly OpenWorkplateTab[]): void {
    this.storage.writeJson(STORAGE_KEY, tabs, 'local');
  }

  /** True when `url` is a route the user works on a plate with no id yet. */
  #isDraftUrl(url: string): boolean {
    return DRAFT_ROUTES.has(url.split(/[?#]/)[0]);
  }

  /** Extract the workplate UUID from a `/slice/:requestUuid` URL. */
  #uuidFromUrl(url: string): string | null {
    const match = url.split(/[?#]/)[0].match(/^\/slice\/([^/]+)$/);
    if (!match) {
      return null;
    }
    const segment = decodeURIComponent(match[1]);
    return segment === 'new' ? null : segment;
  }
}

/** Read the stored list defensively — a corrupt entry must not cost the tabs. */
function parse(raw: string | null): readonly OpenWorkplateTab[] {
  if (!raw) {
    return [];
  }
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) {
      return [];
    }
    return parsed
      .filter((tab): tab is OpenWorkplateTab => typeof (tab as OpenWorkplateTab)?.uuid === 'string')
      .map((tab) => ({ uuid: tab.uuid, filename: tab.filename ?? null }));
  } catch {
    return [];
  }
}
