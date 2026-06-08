import {QueryClientProvider} from '@tanstack/react-query';
import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import {ErrorBoundary} from './components/ErrorBoundary';
import {changeAppLanguage} from './i18n';
import {initAuthBridge} from './lib/auth-session';
import {setupCacheMaintenance} from './lib/cache';
import {setServerPorts} from './lib/constants';
import {trackedInvoke as invoke, setupUiWatchdog} from './lib/diagnostics';
import {queryClient} from './lib/query-client';
import './index.css';
import {useSettingsStore} from './stores/settings';

// Sync language from persisted settings → i18n after tauriStorage rehydration
useSettingsStore.persist.onFinishHydration((state) => {
  if (state.language) {
    void changeAppLanguage(state.language);
  }
});

if (import.meta.env.DEV) {
  const script = document.createElement('script');
  script.src = 'https://unpkg.com/react-scan/dist/auto.global.js';
  script.crossOrigin = 'anonymous';
  document.head.appendChild(script);
}

function scheduleAfterFirstPaint(task: () => void) {
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      if ('requestIdleCallback' in window) {
        window.requestIdleCallback(() => task(), { timeout: 1500 });
      } else {
        setTimeout(task, 1);
      }
    });
  });
}

function startDeferredRuntime() {
  scheduleAfterFirstPaint(() => {
    setupUiWatchdog();
    setupCacheMaintenance();
    void import('./lib/scproxy');
    void import('./lib/tray');
    void import('./lib/audio');
    void import('./lib/queue-autopilot');
    void import('./lib/discord');
  });
}

async function fixWebviewScale() {
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    const { getCurrentWebview } = await import('@tauri-apps/api/webview');
    const monitorScale = await getCurrentWindow().scaleFactor();
    const webviewDpr = window.devicePixelRatio;
    if (monitorScale > 1 && webviewDpr < monitorScale * 0.8) {
      await getCurrentWebview().setZoom(monitorScale / webviewDpr);
    }
  } catch {}
}

async function bootstrap() {
  await fixWebviewScale();
  await useSettingsStore.persist.rehydrate();

  const settings = useSettingsStore.getState();
  await changeAppLanguage(settings.language);

  const [staticPort, proxyPort] = await invoke<[number, number]>('get_server_ports');
  setServerPorts(staticPort, proxyPort);

    // Seed the Rust-owned session into the frontend mirror + subscribe to
    // auth:changed before the first render so the shell/login gate is correct.
    await initAuthBridge();

  ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <QueryClientProvider client={queryClient}>
          <ErrorBoundary fullscreen>
              <App/>
          </ErrorBoundary>
      </QueryClientProvider>
    </React.StrictMode>,
  );

  void startDeferredRuntime();
}

void bootstrap();
