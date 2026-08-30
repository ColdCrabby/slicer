const BACKEND_PORT = 5201;
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
  apiUrl: `${httpProtocol}//${host}:${BACKEND_PORT}/api`,
  wsUrl: `${wsProtocol}//${host}:${BACKEND_PORT}/ws`,
  runtimeMode: 'cloud',
  catalogApiUrl: `${httpProtocol}//${host}:${CATALOG_PORT}`,
};
