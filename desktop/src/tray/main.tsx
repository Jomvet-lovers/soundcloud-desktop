import React from 'react';
import ReactDOM from 'react-dom/client';
import {changeAppLanguage} from '../i18n';
import {applyAccentVars, applyBgVars, applyPerfMode} from '../lib/apply-theme';
import {setupVisibilityGate} from '../lib/perf';
import {useSettingsStore} from '../stores/settings';
import '../index.css';
import './tray.css';
import {MiniPlayer} from './MiniPlayer';

/** Mirror the main window's accent/perf theming into this separate webview context. */
function applyTheme() {
    const s = useSettingsStore.getState();
    applyAccentVars(s.accentColor);
    applyBgVars(s.bgPrimary);
    applyPerfMode(s.perfMode);
}

async function bootstrap() {
    await useSettingsStore.persist.rehydrate();
    applyTheme();
    await changeAppLanguage(useSettingsStore.getState().language);

    ReactDOM.createRoot(document.getElementById('tray-root')!).render(
        <React.StrictMode>
            <MiniPlayer/>
        </React.StrictMode>,
    );

    setupVisibilityGate();

    // Re-pick theme/language changes made in the main window each time the popover re-shows
    // (separate store instance — it only reads the shared on-disk state on demand).
    document.addEventListener('visibilitychange', () => {
        if (document.visibilityState !== 'visible') return;
        void Promise.resolve(useSettingsStore.persist.rehydrate()).then(applyTheme);
    });
}

void bootstrap();
