import { Injectable, InjectionToken, computed, inject, signal } from '@angular/core';
import type { FilamentProfile } from '../../models/filament.model';
import type { PrintProfile } from '../../models/print-profile.model';
import type { PrinterProfile } from '../../models/printer.model';
import type { ProfileMeta } from '../../models/profile-source';
import { uid } from '../../models/id';
import { CATALOG_FILAMENTS, CATALOG_PRINTERS, CATALOG_PROFILES } from './catalog-data';

/**
 * The remote data contract. `CloudCatalog` talks only to this interface, so
 * swapping the bundled dataset for a real HTTP backend later is a one-line
 * provider override — no consumer changes. Every method may reject (offline /
 * network error); `CloudCatalog` turns that into an `unavailable` state rather
 * than throwing at the call site.
 */
export interface CatalogSource {
  printers(): Promise<PrinterProfile[]>;
  filaments(): Promise<FilamentProfile[]>;
  profiles(): Promise<PrintProfile[]>;
}

/** Simulated network latency (ms) so the loading state is exercised in dev. */
const SIMULATED_LATENCY_MS = 350;

const delay = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

/** Default source: the bundled vendor dataset shipped in `catalog-data.ts`. */
export class BundledCatalogSource implements CatalogSource {
  async printers(): Promise<PrinterProfile[]> {
    await delay(SIMULATED_LATENCY_MS);
    return CATALOG_PRINTERS;
  }
  async filaments(): Promise<FilamentProfile[]> {
    await delay(SIMULATED_LATENCY_MS);
    return CATALOG_FILAMENTS;
  }
  async profiles(): Promise<PrintProfile[]> {
    await delay(SIMULATED_LATENCY_MS);
    return CATALOG_PROFILES;
  }
}

export const CATALOG_SOURCE = new InjectionToken<CatalogSource>('CATALOG_SOURCE', {
  providedIn: 'root',
  factory: () => new BundledCatalogSource(),
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
    basedOn: entry.source === 'catalog' ? entry.id : entry.basedOn,
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

  readonly printers = this._printers.asReadonly();
  readonly filaments = this._filaments.asReadonly();
  readonly profiles = this._profiles.asReadonly();
  readonly status = this._status.asReadonly();
  readonly available = computed(() => this._status() === 'ready');
  readonly loading = computed(() => this._status() === 'loading');

  /** Fetch all catalog categories. Safe to call repeatedly (in-memory cache). */
  async load(force = false): Promise<void> {
    if (!force && (this._status() === 'ready' || this._status() === 'loading')) {
      return;
    }
    this._status.set('loading');
    try {
      const [printers, filaments, profiles] = await Promise.all([
        this.source.printers(),
        this.source.filaments(),
        this.source.profiles(),
      ]);
      this._printers.set(printers);
      this._filaments.set(filaments);
      this._profiles.set(profiles);
      this._status.set('ready');
    } catch {
      this._status.set('unavailable');
    }
  }
}
