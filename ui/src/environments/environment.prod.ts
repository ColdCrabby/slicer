type Environment = {
  production: boolean;
  apiUrl: string;
  wsUrl: string;
  runtimeMode: 'native' | 'cloud' | 'web';
  catalogApiUrl: string;
};

export const environment: Environment = {
  production: true,
  apiUrl: '/api',
  wsUrl: 'ws://' + window.location.host,
  runtimeMode: 'cloud',
  catalogApiUrl: 'https://cloud-presets.onrender.com',
};
