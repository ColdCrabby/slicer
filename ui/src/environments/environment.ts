const BACKEND_PORT = 5201;

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
  catalogApiUrl: 'https://cloud-presets.onrender.com',
};
