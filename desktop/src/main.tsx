import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import { setCacheServerPort } from './lib/constants';
import './i18n';
import './lib/audio';
import './lib/discord';
import './lib/tray';
import './index.css';

if (import.meta.env.DEV) {
  const script = document.createElement('script');
  script.src = 'https://unpkg.com/react-scan/dist/auto.global.js';
  script.crossOrigin = 'anonymous';
  document.head.appendChild(script);
}

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 1000 * 60 * 5,
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
});

const waitForController = () =>
  new Promise<void>((resolve) =>
    navigator.serviceWorker.addEventListener('controllerchange', () => resolve(), { once: true }),
  );

async function registerServiceWorker(port: number) {
  if (!('serviceWorker' in navigator)) return;

  await navigator.serviceWorker.register(`/sw.js?port=${port}`);

  if (!navigator.serviceWorker.controller) {
    await waitForController();
  }
}

async function bootstrap() {
  const port = await invoke<number>('get_cache_server_port');
  setCacheServerPort(port);

  await registerServiceWorker(port);

  ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <QueryClientProvider client={queryClient}>
        <App />
      </QueryClientProvider>
    </React.StrictMode>,
  );
}

void bootstrap();
