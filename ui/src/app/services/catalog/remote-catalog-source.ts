import { Injector } from '@angular/core';
import { searchPresets, type PresetSummary } from '../../../generated/catalog-client';
import {
  FILAMENT_MATERIALS,
  makeFilament,
  type FilamentMaterial,
  type FilamentProfile,
} from '../../models/filament.model';
import { makePrinter, type PrinterProfile } from '../../models/printer.model';
import { makePrintProfile, type PrintProfile } from '../../models/print-profile.model';
import { type CatalogSource, CATALOG_SPEC_KEY } from './cloud-catalog';

/** Hard ceiling per the OpenAPI `limit` bound. */
const PAGE_LIMIT = 100;
/** Safety cap on pages walked, so a broken cursor can never loop forever. */
const MAX_PAGES = 100;

type PresetType = NonNullable<PresetSummary['type']>;

/**
 * The real {@link CatalogSource}: the Cold Crabby Preset Cloud.
 *
 * The served API only exposes fuzzy **search** returning *summaries* — there is
 * no endpoint yet for a full preset body — so each category is fetched by
 * *browsing* (an empty query, filtered by `type`) and paging through the cursor
 * until the catalog is exhausted. Every summary is widened into the profile
 * shape the existing wizards consume: the {@link makePrinter}/{@link makeFilament}/
 * {@link makePrintProfile} factories supply valid defaults for the structured
 * fields the summary cannot carry, and the summary's own fields (name, vendor,
 * model/material) overwrite them. The result is tagged `source: 'catalog'` with
 * an `import_url` back to the preset's canonical detail URL for lineage.
 *
 * Any transport or HTTP error rejects, which {@link CloudCatalog} turns into its
 * `unavailable` state — the app then falls back to "create from scratch".
 */
export class RemoteCatalogSource implements CatalogSource {
  /**
   * `injector` is passed to every request so the hey-api Angular client can
   * lazily resolve `HttpClient` from DI. The SDK is called here from async
   * methods — outside any injection context — so without it the client would
   * throw `NG0203` on the first request.
   */
  constructor(
    private readonly baseUrl: string,
    private readonly injector: Injector,
  ) {}

  async printers(query?: string): Promise<PrinterProfile[]> {
    const summaries = await this.browse('printer', query);
    return summaries.map((s) =>
      makePrinter({
        id: s.id,
        name: s.name,
        vendor: s.vendor,
        model: s.model ?? 'Generic',
        source: 'catalog',
        import_url: this.detailUrl(s.id),
        [CATALOG_SPEC_KEY]: s.spec,
      }),
    );
  }

  async filaments(query?: string): Promise<FilamentProfile[]> {
    const summaries = await this.browse('filament', query);
    return summaries.map((s) =>
      makeFilament({
        id: s.id,
        name: s.name,
        vendor: s.vendor,
        material: coerceMaterial(s.material),
        source: 'catalog',
        import_url: this.detailUrl(s.id),
        [CATALOG_SPEC_KEY]: s.spec,
      }),
    );
  }

  async profiles(query?: string): Promise<PrintProfile[]> {
    const summaries = await this.browse('process', query);
    return summaries.map((s) =>
      makePrintProfile({
        id: s.id,
        name: s.name,
        source: 'catalog',
        import_url: this.detailUrl(s.id),
        [CATALOG_SPEC_KEY]: s.spec,
      }),
    );
  }

  /** Canonical detail URL for a preset, recorded as import provenance. */
  private detailUrl(id: string): string {
    return `${this.baseUrl.replace(/\/$/, '')}/v1/presets/${encodeURIComponent(id)}`;
  }

  /**
   * Walk every page of one preset type, returning all summaries. A non-empty
   * `query` is passed to the server as `q` for fuzzy, ranked search; an empty
   * one browses the whole category.
   */
  private async browse(type: PresetType, query?: string): Promise<PresetSummary[]> {
    const q = query?.trim() || undefined;
    const all: PresetSummary[] = [];
    let cursor: string | undefined;
    for (let page = 0; page < MAX_PAGES; page++) {
      const { data, error } = await searchPresets({
        query: { type, q, limit: PAGE_LIMIT, cursor },
        injector: this.injector,
      });
      if (error) {
        throw new Error(`Catalog search failed for "${type}".`);
      }
      if (!data) {
        break;
      }
      if (data.results) {
        all.push(...data.results);
      }
      cursor = data.next_cursor;
      if (!cursor) {
        break;
      }
    }
    return all;
  }
}

/**
 * Coerce the summary's free-form material string to a known
 * {@link FilamentMaterial}, defaulting to `PLA` when absent or unrecognised.
 */
function coerceMaterial(material: string | undefined): FilamentMaterial {
  if (!material) {
    return 'PLA';
  }
  const normalized = material.trim().toUpperCase();
  return FILAMENT_MATERIALS.find((m) => m.toUpperCase() === normalized) ?? 'PLA';
}
