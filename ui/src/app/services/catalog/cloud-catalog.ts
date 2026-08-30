import { Injectable, InjectionToken, computed, inject, signal } from '@angular/core';
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

/**
 * Turn a catalog entry into a fresh, fully-owned local copy: new id, marked
 * `user`, with `basedOn` pointing back at the source entry for lineage.
 */
export function toUserCopy<T extends ProfileMeta>(entry: T, name?: string): T {
  return {
    ...structuredClone(entry),
    id: uid(),
    source: 'user',
    based_on: entry.source === 'catalog' ? entry.id : entry.based_on,
    name: name ?? entry.name,
  };
}

/**
 * Cloud-only base dataset access.
 *
 * The catalog is never persisted locally — it is fetched on demand and cached
 * in memory for the session. Importing an entry (via {@link toUserCopy}) is the
 * only path that writes to local storage, and it always produces a `user` copy
 * so the offline install keeps working even when the cloud is gone.
 */
@Injectable({ providedIn: 'root' })
export class CloudCatalog {
  private readonly source = inject(CATALOG_SOURCE);

  private readonly _printers = signal<PrinterProfile[]>([]);
  private readonly _filaments = signal<FilamentProfile[]>([]);
  private readonly _profiles = signal<PrintProfile[]>([]);
  private readonly _status = signal<CatalogStatus>('idle');
  private readonly _query = signal('');

  readonly printers = this._printers.asReadonly();
  readonly filaments = this._filaments.asReadonly();
  readonly profiles = this._profiles.asReadonly();
  readonly status = this._status.asReadonly();
  /** The query the currently-shown results were fetched for (empty = browse). */
  readonly query = this._query.asReadonly();
  readonly available = computed(() => this._status() === 'ready');
  readonly loading = computed(() => this._status() === 'loading');

  /** Monotonic token so a slow in-flight fetch can't overwrite a newer one. */
  private requestSeq = 0;

  /**
   * Fetch all catalog categories for `query` (empty = browse everything).
   * Skips a redundant fetch when the same query is already loaded, unless
   * `force`. Out-of-order responses are dropped so the latest query always wins.
   */
  async load(force = false, query = ''): Promise<void> {
    const q = query.trim();
    if (
      !force &&
      q === this._query() &&
      (this._status() === 'ready' || this._status() === 'loading')
    ) {
      return;
    }
    const seq = ++this.requestSeq;
    this._query.set(q);
    this._status.set('loading');
    try {
      const [printers, filaments, profiles] = await Promise.all([
        this.source.printers(q),
        this.source.filaments(q),
        this.source.profiles(q),
      ]);
      if (seq !== this.requestSeq) {
        return;
      }
      this._printers.set(printers);
      this._filaments.set(filaments);
      this._profiles.set(profiles);
      this._status.set('ready');
    } catch {
      if (seq !== this.requestSeq) {
        return;
      }
      this._status.set('unavailable');
    }
  }

  /** Re-fetch the catalog filtered by a fuzzy `query`. Always hits the source. */
  search(query: string): Promise<void> {
    return this.load(true, query);
  }
}
