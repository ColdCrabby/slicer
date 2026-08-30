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
  production: true,
  apiUrl: `${httpProtocol}//${window.location.host}/api`,
  wsUrl: `${wsProtocol}//${window.location.host}/ws`,
  runtimeMode: 'web',
  catalogApiUrl: 'https://cloud-presets.onrender.com',
};
