import { client } from '../../../generated/catalog-client/client.gen';

/**
 * Point the generated Cold Crabby Preset Cloud client at an API base URL.
 *
 * The client is generated from the *remote* OpenAPI document, whose host is
 * `raw.githubusercontent.com` — useless for real requests — so the app must set
 * the deployed API origin once at startup. Mirrors cloud-presets'
 * `configureCloudPresetsClient`: one `setConfig` call, no per-request wiring.
 */
export function configureCatalogClient(baseUrl: string): void {
  client.setConfig({ baseUrl });
}
