import { Injector } from '@angular/core';
import { getPreset, searchPresets, type PresetSummary } from '../../../generated/catalog-client';
import {
  FILAMENT_MATERIALS,
  makeFilament,
  type FilamentMaterial,
  type FilamentProfile,
} from '../../models/filament.model';
import { makePrinter, type PrinterProfile } from '../../models/printer.model';
import { makePrintProfile, type PrintProfile } from '../../models/print-profile.model';
import { type CatalogPage, type CatalogSource, CATALOG_SPEC_KEY } from './cloud-catalog';

/**
 * Page size for browsing/searching. Deliberately well under the OpenAPI
 * `limit` ceiling (100): a picker shows one page at a time (see
 * {@link CloudCatalog.loadMorePrinters} and friends), so the first paint only
 * has to wait on this many rows rather than the whole category.
 */
const PAGE_LIMIT = 30;

type PresetType = NonNullable<PresetSummary['type']>;

/**
 * The real {@link CatalogSource}: the Cold Crabby Preset Cloud.
 *
 * List methods fetch **one page at a time** (an empty query browses; a
 * non-empty one searches) rather than walking the cursor to exhaustion, so
 * opening a picker never blocks on \u2014 or holds in memory \u2014 the whole category.
 * Every summary is widened into the profile shape the existing wizards
 * consume: the {@link makePrinter}/{@link makeFilament}/{@link makePrintProfile}
 * factories supply valid defaults for the structured fields a summary cannot
 * carry, and the summary's own fields (name, vendor, model/material) overwrite
 * them. The result is tagged `source: 'catalog'` with an `import_url` back to
 * the preset's canonical detail URL for lineage.
 *
 * `*Detail` calls `GET /v1/presets/{id}` for the full preset and overlays its
 * real `params` onto the already-widened summary, which is what turns a
 * browsed entry into something actually worth importing.
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

  async printers(query?: string, cursor?: string): Promise<CatalogPage<PrinterProfile>> {
    const page = await this.fetchPage('printer', query, cursor);
    return {
      items: page.results.map((s) =>
        makePrinter({
          id: s.id,
          name: s.name,
          vendor: s.vendor,
          model: s.model ?? 'Generic',
          source: 'catalog',
          import_url: this.detailUrl(s.id),
          [CATALOG_SPEC_KEY]: s.spec,
        }),
      ),
      nextCursor: page.nextCursor,
    };
  }

  async filaments(query?: string, cursor?: string): Promise<CatalogPage<FilamentProfile>> {
    const page = await this.fetchPage('filament', query, cursor);
    return {
      items: page.results.map((s) =>
        makeFilament({
          id: s.id,
          name: s.name,
          vendor: s.vendor,
          material: coerceMaterial(s.material),
          source: 'catalog',
          import_url: this.detailUrl(s.id),
          [CATALOG_SPEC_KEY]: s.spec,
        }),
      ),
      nextCursor: page.nextCursor,
    };
  }

  async profiles(query?: string, cursor?: string): Promise<CatalogPage<PrintProfile>> {
    const page = await this.fetchPage('process', query, cursor);
    return {
      items: page.results.map((s) =>
        makePrintProfile({
          id: s.id,
          name: s.name,
          source: 'catalog',
          import_url: this.detailUrl(s.id),
          [CATALOG_SPEC_KEY]: s.spec,
        }),
      ),
      nextCursor: page.nextCursor,
    };
  }

  async printerDetail(base: PrinterProfile): Promise<PrinterProfile> {
    const detail = await this.fetchDetail(base.id);
    return {
      ...base,
      name: detail.name,
      vendor: detail.vendor,
      import_url: detail.import_url,
      params: { ...(base.params as Record<string, unknown>), ...detail.params },
    };
  }

  async filamentDetail(base: FilamentProfile): Promise<FilamentProfile> {
    const detail = await this.fetchDetail(base.id);
    return {
      ...base,
      name: detail.name,
      vendor: detail.vendor,
      import_url: detail.import_url,
      params: { ...(base.params as Record<string, unknown>), ...detail.params },
    };
  }

  async profileDetail(base: PrintProfile): Promise<PrintProfile> {
    const detail = await this.fetchDetail(base.id);
    return {
      ...base,
      name: detail.name,
      import_url: detail.import_url,
      params: { ...(base.params as Record<string, unknown>), ...detail.params },
    };
  }

  /** Canonical detail URL for a preset, recorded as import provenance. */
  private detailUrl(id: string): string {
    return `${this.baseUrl.replace(/\/$/, '')}/v1/presets/${encodeURIComponent(id)}`;
  }

  /**
   * Fetch one page of one preset type. A non-empty `query` is passed to the
   * server as `q` for fuzzy, ranked search; an empty one browses.
   */
  private async fetchPage(
    type: PresetType,
    query: string | undefined,
    cursor: string | undefined,
  ): Promise<{ results: PresetSummary[]; nextCursor?: string }> {
    const q = query?.trim() || undefined;
    const { data, error } = await searchPresets({
      query: { type, q, limit: PAGE_LIMIT, cursor },
      injector: this.injector,
    });
    if (error) {
      throw new Error(`Catalog search failed for "${type}".`);
    }
    return { results: data?.results ?? [], nextCursor: data?.next_cursor };
  }

  /** Fetch the full preset behind one catalog id via `GET /v1/presets/{id}`. */
  private async fetchDetail(
    id: string,
  ): Promise<{
    name: string;
    vendor: string;
    import_url: string;
    params: Record<string, unknown>;
  }> {
    const { data, error } = await getPreset({ path: { id }, injector: this.injector });
    if (error || !data) {
      throw new Error(`Catalog detail failed for preset "${id}".`);
    }
    return {
      name: data.name,
      vendor: data.vendor,
      import_url: data.import_url,
      params: data.params,
    };
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
