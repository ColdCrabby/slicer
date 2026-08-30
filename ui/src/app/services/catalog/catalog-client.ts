import {
  type EnvironmentProviders,
  makeEnvironmentProviders,
  provideAppInitializer,
} from '@angular/core';
import { client } from '../../../generated/catalog-client/client.gen';
import { provideHeyApiClient } from '../../../generated/catalog-client/client/client.gen';

/**
 * Wire up the generated Cold Crabby Preset Cloud client for Angular.
 *
 * The client is the hey-api **Angular** client, so it issues requests through
 * Angular's `HttpClient` (and its interceptors) rather than a bare `fetch`.
 * Two things must be set once at startup:
 *
 * - `provideHeyApiClient(client)` injects the app's `HttpClient` into the
 *   client (it runs in an injection context; `RemoteCatalogSource` calls the
 *   SDK later, outside one, and relies on this).
 * - the **base URL**, because the client is generated from the *remote* OpenAPI
 *   document whose host is `raw.githubusercontent.com` — useless for requests.
 *   `environment.catalogApiUrl` supplies the real deployed/local API origin.
 */
export function provideCatalogClient(baseUrl: string): EnvironmentProviders {
  return makeEnvironmentProviders([
    provideHeyApiClient(client),
    provideAppInitializer(() => {
      client.setConfig({ baseUrl });
    }),
  ]);
}
