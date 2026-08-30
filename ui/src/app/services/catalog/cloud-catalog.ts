import { Injectable, InjectionToken, type WritableSignal, inject, signal } from '@angular/core';
import type { FilamentProfile } from '../../models/filament.model';
import type { PrintProfile } from '../../models/print-profile.model';
import type { PrinterProfile } from '../../models/printer.model';
import type { ProfileMeta } from '../../models/profile-source';
import { uid } from '../../models/id';

/**
 * The remote data contract. `CloudCatalog` talks only to this interface, so
 * swapping in a real HTTP/WS backend later is a one-line provider override — no
 * consumer changes. Every method may reject (offline / network error);
 * `CloudCatalog` turns that into an `unavailable` state rather than throwing at
 * the call site.
 *
 * Each method takes an optional fuzzy `query`. Search is the **server's** job —
 * the source re-fetches ranked, filtered results for the query rather than the
 * UI filtering a cached page — so the query flows all the way to the API.
 */
export interface CatalogSource {
  printers(query?: string): Promise<PrinterProfile[]>;
  filaments(query?: string): Promise<FilamentProfile[]>;
  profiles(query?: string): Promise<PrintProfile[]>;
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
  printers(_query?: string): Promise<PrinterProfile[]> {
    return Promise.reject(new Error(this.reason));
  }
  filaments(_query?: string): Promise<FilamentProfile[]> {
    return Promise.reject(new Error(this.reason));
  }
  profiles(_query?: string): Promise<PrintProfile[]> {
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

/** Mutable per-category state: the results, their status, and the query. */
interface CategoryState<T> {
  readonly data: WritableSignal<T[]>;
  readonly status: WritableSignal<CatalogStatus>;
  readonly query: WritableSignal<string>;
  /** Monotonic token so a slow fetch can't overwrite a newer one. */
  seq: number;
}

function newCategoryState<T>(): CategoryState<T> {
  return {
    data: signal<T[]>([]),
    status: signal<CatalogStatus>('idle'),
    query: signal(''),
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

  /** Load printers for `query` (empty = browse). Cached unless `force`. */
  loadPrinters(force = false, query = ''): Promise<void> {
    return this.run(this.printersState, (q) => this.source.printers(q), force, query);
  }
  /** Load filaments for `query` (empty = browse). Cached unless `force`. */
  loadFilaments(force = false, query = ''): Promise<void> {
    return this.run(this.filamentsState, (q) => this.source.filaments(q), force, query);
  }
  /** Load processes for `query` (empty = browse). Cached unless `force`. */
  loadProfiles(force = false, query = ''): Promise<void> {
    return this.run(this.processesState, (q) => this.source.profiles(q), force, query);
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

  /**
   * Shared fetch driver for one category. Skips a redundant fetch when the same
   * query is already loaded (unless `force`), and drops out-of-order responses
   * via the per-category sequence token so the latest query always wins.
   */
  private async run<T>(
    state: CategoryState<T>,
    fetcher: (query: string) => Promise<T[]>,
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
      const data = await fetcher(q);
      if (seq !== state.seq) {
        return;
      }
      state.data.set(data);
      state.status.set('ready');
    } catch {
      if (seq !== state.seq) {
        return;
      }
      state.status.set('unavailable');
    }
  }
}
