// Development environment — same-origin, like production.
//
// The Angular dev server proxies `/api` and `/ws` to the engine (see
// ui/proxy.conf.mjs), so the engine's port is an internal detail:
// scripts/dev.mjs derives it from a seed so several checkouts can run side by
// side. The browser only ever addresses the origin it was loaded from, which
// also means a phone or an iPad on the LAN needs one URL, not two.

// Local cloud-presets API (`pnpm sample-api` in the cloud-presets repo, or its
// unified `pnpm dev` proxied at /v1). Point the catalog at the local instance
// during development instead of the deployed cloud.
const CATALOG_PORT = 8787;

const host = window.location.hostname || 'localhost';
const wsProtocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
const httpProtocol = window.location.protocol === 'https:' ? 'https:' : 'http:';

type Environment = {
  production: boolean;
  apiUrl: string;
  wsUrl: string;
  runtimeMode: 'native' | 'cloud' | 'web';
  catalogApiUrl: string;
};

export const environment: Environment = {
  production: false,
  apiUrl: `${httpProtocol}//${window.location.host}/api`,
  wsUrl: `${wsProtocol}//${window.location.host}/ws`,
  runtimeMode: 'cloud',
  catalogApiUrl: `${httpProtocol}//${host}:${CATALOG_PORT}`,
};
