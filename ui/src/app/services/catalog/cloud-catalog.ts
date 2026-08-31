import {
  Injectable,
  InjectionToken,
  type WritableSignal,
  computed,
  inject,
  signal,
} from '@angular/core';
import type { FilamentProfile } from '../../models/filament.model';
import type { PrintProfile } from '../../models/print-profile.model';
import type { PrinterProfile } from '../../models/printer.model';
import type { ProfileMeta } from '../../models/profile-source';
import { uid } from '../../models/id';

/** One page of catalog results plus the cursor to fetch the next one, if any. */
export interface CatalogPage<T> {
  items: T[];
  nextCursor?: string;
}

/**
 * The remote data contract. `CloudCatalog` talks only to this interface, so
 * swapping in a real HTTP/WS backend later is a one-line provider override — no
 * consumer changes. Every method may reject (offline / network error);
 * `CloudCatalog` turns that into an `unavailable` state rather than throwing at
 * the call site.
 *
 * The list methods take an optional fuzzy `query` and an optional `cursor` from
 * a previous {@link CatalogPage}. Search is the **server's** job — the source
 * re-fetches ranked, filtered results for the query rather than the UI
 * filtering a cached page — and paging is walked **one page at a time** rather
 * than eagerly fetched to exhaustion, so opening a picker never has to wait for
 * (or hold in memory) the whole catalog.
 *
 * `*Detail` fetches the full preset for an already-loaded summary — the only
 * way to obtain real slicing parameters, since list results are summaries. It
 * overlays the fetched fields onto `base` rather than rebuilding a profile from
 * scratch, so identity fields the detail response doesn't carry (e.g. a
 * printer's bed size) keep the summary's best-effort defaults.
 */
export interface CatalogSource {
  printers(query?: string, cursor?: string): Promise<CatalogPage<PrinterProfile>>;
  filaments(query?: string, cursor?: string): Promise<CatalogPage<FilamentProfile>>;
  profiles(query?: string, cursor?: string): Promise<CatalogPage<PrintProfile>>;
  printerDetail(base: PrinterProfile): Promise<PrinterProfile>;
  filamentDetail(base: FilamentProfile): Promise<FilamentProfile>;
  profileDetail(base: PrintProfile): Promise<PrintProfile>;
}

/**
 * Fallback source: **unavailable**. Every lookup rejects, so {@link CloudCatalog}
 * reports the `unavailable` state and the UI offers "create from scratch" while
 * the single builtin default per category keeps the app working offline.
 *
 * This is the injection default and the offline safety net. The running app
 * overrides {@link CATALOG_SOURCE} with `RemoteCatalogSource`, which talks to
 * the real Cold Crabby Preset Cloud (see `app.config.ts`).
 */
export class UnavailableCatalogSource implements CatalogSource {
  private readonly reason = 'Cloud catalog is not connected.';
  printers(): Promise<CatalogPage<PrinterProfile>> {
    return Promise.reject(new Error(this.reason));
  }
  filaments(): Promise<CatalogPage<FilamentProfile>> {
    return Promise.reject(new Error(this.reason));
  }
  profiles(): Promise<CatalogPage<PrintProfile>> {
    return Promise.reject(new Error(this.reason));
  }
  printerDetail(): Promise<PrinterProfile> {
    return Promise.reject(new Error(this.reason));
  }
  filamentDetail(): Promise<FilamentProfile> {
    return Promise.reject(new Error(this.reason));
  }
  profileDetail(): Promise<PrintProfile> {
    return Promise.reject(new Error(this.reason));
  }
}

export const CATALOG_SOURCE = new InjectionToken<CatalogSource>('CATALOG_SOURCE', {
  providedIn: 'root',
  factory: () => new UnavailableCatalogSource(),
});

export type CatalogStatus = 'idle' | 'loading' | 'ready' | 'unavailable';

/** The three catalog categories, each loaded and searched independently. */
export type CatalogCategory = 'printers' | 'filaments' | 'processes';

/**
 * Hidden field carrying the cloud catalog's own human-readable spec string
 * (e.g. "256 × 256 × 256 mm, 0.4 mm nozzle"). The API returns this per summary;
 * the picker shows it as the right-aligned meta line rather than reconstructing
 * one from the profile's *defaulted* structured fields (which a summary can't
 * populate). Set by `RemoteCatalogSource`, read via {@link catalogSpecOf}, and
 * stripped by {@link toUserCopy} so an imported user copy never carries a stale
 * summary string.
 */
export const CATALOG_SPEC_KEY = 'catalog_spec';

/** Read the {@link CATALOG_SPEC_KEY} spec string off a catalog entry, if any. */
export function catalogSpecOf(entry: ProfileMeta): string | undefined {
  const value = entry[CATALOG_SPEC_KEY];
  return typeof value === 'string' && value.trim() ? value : undefined;
}

/**
 * Turn a catalog entry into a fresh, fully-owned local copy: new id, marked
 * `user`, with `basedOn` pointing back at the source entry for lineage.
 */
export function toUserCopy<T extends ProfileMeta>(entry: T, name?: string): T {
  const copy: T = {
    ...structuredClone(entry),
    id: uid(),
    source: 'user',
    based_on: entry.source === 'catalog' ? entry.id : entry.based_on,
    name: name ?? entry.name,
  };
  // The catalog spec describes the *catalog* entry; a user copy has real,
  // editable fields, so the summary string would only be stale junk.
  delete copy[CATALOG_SPEC_KEY];
  return copy;
}

/** Mutable per-category state: the results, their status, the query, and paging. */
interface CategoryState<T> {
  readonly data: WritableSignal<T[]>;
  readonly status: WritableSignal<CatalogStatus>;
  readonly query: WritableSignal<string>;
  /** Cursor for the next page, or `undefined` once the last page was seen. */
  readonly nextCursor: WritableSignal<string | undefined>;
  /** True while a "load more" fetch (not the initial/search load) is in flight. */
  readonly loadingMore: WritableSignal<boolean>;
  /** Monotonic token so a slow fetch can't overwrite a newer one. */
  seq: number;
}

function newCategoryState<T>(): CategoryState<T> {
  return {
    data: signal<T[]>([]),
    status: signal<CatalogStatus>('idle'),
    query: signal(''),
    nextCursor: signal<string | undefined>(undefined),
    loadingMore: signal(false),
    seq: 0,
  };
}

/**
 * Cloud-only base dataset access.
 *
 * The catalog is never persisted locally — it is fetched on demand and cached
 * in memory for the session. Importing an entry (via {@link toUserCopy}) is the
 * only path that writes to local storage, and it always produces a `user` copy
 * so the offline install keeps working even when the cloud is gone.
 *
 * **Each category is loaded independently.** Opening the printer picker fetches
 * only printers — never filaments or processes — and searching one category
 * re-queries only that one. This mirrors how the UI consumes the catalog (one
 * category per wizard / modal) and avoids three requests where one is wanted.
 */
@Injectable({ providedIn: 'root' })
export class CloudCatalog {
  private readonly source = inject(CATALOG_SOURCE);

  private readonly printersState = newCategoryState<PrinterProfile>();
  private readonly filamentsState = newCategoryState<FilamentProfile>();
  private readonly processesState = newCategoryState<PrintProfile>();

  readonly printers = this.printersState.data.asReadonly();
  readonly filaments = this.filamentsState.data.asReadonly();
  readonly profiles = this.processesState.data.asReadonly();

  readonly printersStatus = this.printersState.status.asReadonly();
  readonly filamentsStatus = this.filamentsState.status.asReadonly();
  readonly profilesStatus = this.processesState.status.asReadonly();

  readonly printersQuery = this.printersState.query.asReadonly();
  readonly filamentsQuery = this.filamentsState.query.asReadonly();
  readonly profilesQuery = this.processesState.query.asReadonly();

  /** True when another page of printers is available via {@link loadMorePrinters}. */
  readonly printersHasMore = computed(() => this.printersState.nextCursor() !== undefined);
  /** True when another page of filaments is available via {@link loadMoreFilaments}. */
  readonly filamentsHasMore = computed(() => this.filamentsState.nextCursor() !== undefined);
  /** True when another page of profiles is available via {@link loadMoreProfiles}. */
  readonly profilesHasMore = computed(() => this.processesState.nextCursor() !== undefined);

  readonly printersLoadingMore = this.printersState.loadingMore.asReadonly();
  readonly filamentsLoadingMore = this.filamentsState.loadingMore.asReadonly();
  readonly profilesLoadingMore = this.processesState.loadingMore.asReadonly();

  /** Load the first page of printers for `query` (empty = browse). Cached unless `force`. */
  loadPrinters(force = false, query = ''): Promise<void> {
    return this.run(this.printersState, (q, c) => this.source.printers(q, c), force, query);
  }
  /** Load the first page of filaments for `query` (empty = browse). Cached unless `force`. */
  loadFilaments(force = false, query = ''): Promise<void> {
    return this.run(this.filamentsState, (q, c) => this.source.filaments(q, c), force, query);
  }
  /** Load the first page of processes for `query` (empty = browse). Cached unless `force`. */
  loadProfiles(force = false, query = ''): Promise<void> {
    return this.run(this.processesState, (q, c) => this.source.profiles(q, c), force, query);
  }

  /** Re-fetch printers filtered by a fuzzy `query`. Always hits the source. */
  searchPrinters(query: string): Promise<void> {
    return this.loadPrinters(true, query);
  }
  /** Re-fetch filaments filtered by a fuzzy `query`. Always hits the source. */
  searchFilaments(query: string): Promise<void> {
    return this.loadFilaments(true, query);
  }
  /** Re-fetch processes filtered by a fuzzy `query`. Always hits the source. */
  searchProfiles(query: string): Promise<void> {
    return this.loadProfiles(true, query);
  }

  /** Append the next page of printers, if {@link printersHasMore}. */
  loadMorePrinters(): Promise<void> {
    return this.loadMore(this.printersState, (q, c) => this.source.printers(q, c));
  }
  /** Append the next page of filaments, if {@link filamentsHasMore}. */
  loadMoreFilaments(): Promise<void> {
    return this.loadMore(this.filamentsState, (q, c) => this.source.filaments(q, c));
  }
  /** Append the next page of processes, if {@link profilesHasMore}. */
  loadMoreProfiles(): Promise<void> {
    return this.loadMore(this.processesState, (q, c) => this.source.profiles(q, c));
  }

  /**
   * Fetch the full preset behind a catalog summary and overlay its real
   * `params` onto `base`. Used at import time — list results carry only
   * summaries, so this is the one network round-trip that turns "a name and a
   * spec string" into a profile actually worth importing.
   */
  printerDetail(base: PrinterProfile): Promise<PrinterProfile> {
    return this.source.printerDetail(base);
  }
  filamentDetail(base: FilamentProfile): Promise<FilamentProfile> {
    return this.source.filamentDetail(base);
  }
  profileDetail(base: PrintProfile): Promise<PrintProfile> {
    return this.source.profileDetail(base);
  }

  /**
   * Shared fetch driver for one category's first page. Skips a redundant fetch
   * when the same query is already loaded (unless `force`), and drops
   * out-of-order responses via the per-category sequence token so the latest
   * query always wins.
   */
  private async run<T>(
    state: CategoryState<T>,
    fetcher: (query: string, cursor?: string) => Promise<CatalogPage<T>>,
    force: boolean,
    query: string,
  ): Promise<void> {
    const q = query.trim();
    if (
      !force &&
      q === state.query() &&
      (state.status() === 'ready' || state.status() === 'loading')
    ) {
      return;
    }
    const seq = ++state.seq;
    state.query.set(q);
    state.status.set('loading');
    try {
      const page = await fetcher(q);
      if (seq !== state.seq) {
        return;
      }
      state.data.set(page.items);
      state.nextCursor.set(page.nextCursor);
      state.status.set('ready');
    } catch {
      if (seq !== state.seq) {
        return;
      }
      state.status.set('unavailable');
    }
  }

  /**
   * Append one more page of results for a category, using its cursor from the
   * last {@link run}. A no-op when there is no cursor (last page already seen)
   * or a "load more" fetch is already in flight. Dropped, not retried, on
   * out-of-order or failed responses \u2014 the cursor stays put so the next click
   * on "Load more" simply tries again.
   */
  private async loadMore<T>(
    state: CategoryState<T>,
    fetcher: (query: string, cursor?: string) => Promise<CatalogPage<T>>,
  ): Promise<void> {
    const cursor = state.nextCursor();
    if (!cursor || state.loadingMore()) {
      return;
    }
    const seq = state.seq;
    state.loadingMore.set(true);
    try {
      const page = await fetcher(state.query(), cursor);
      if (seq !== state.seq) {
        return;
      }
      state.data.update((list) => [...list, ...page.items]);
      state.nextCursor.set(page.nextCursor);
    } catch {
      // Leave the cursor as-is: the user can retry via "Load more".
    } finally {
      if (seq === state.seq) {
        state.loadingMore.set(false);
      }
    }
  }
}
