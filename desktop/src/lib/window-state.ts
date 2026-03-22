import { LogicalSize, getCurrentWindow } from '@tauri-apps/api/window';
import { useSettingsStore } from '../stores/settings';

let initialized = false;
let saveTimer: number | null = null;

async function persistCurrentWindowState() {
  const appWindow = getCurrentWindow();
  const maximized = await appWindow.isMaximized();

  if (maximized) {
    useSettingsStore.getState().setWindowState({ maximized: true });
    return;
  }

  const scaleFactor = await appWindow.scaleFactor();
  const logicalSize = (await appWindow.innerSize()).toLogical(scaleFactor);

  useSettingsStore.getState().setWindowState({
    width: Math.round(logicalSize.width),
    height: Math.round(logicalSize.height),
    maximized: false,
  });
}

export async function applySavedWindowState() {
  const { windowWidth, windowHeight, windowMaximized } = useSettingsStore.getState();
  const appWindow = getCurrentWindow();

  await appWindow.setSize(new LogicalSize(windowWidth, windowHeight));

  if (windowMaximized) {
    await appWindow.maximize();
  }
}

export async function setupWindowStatePersistence() {
  if (initialized) return;
  initialized = true;

  const appWindow = getCurrentWindow();
  const scheduleSave = () => {
    if (saveTimer != null) {
      window.clearTimeout(saveTimer);
    }

    saveTimer = window.setTimeout(() => {
      void persistCurrentWindowState().catch((error) => {
        console.warn('[window-state] failed to persist window state', error);
      });
    }, 150);
  };

  await appWindow.onResized(scheduleSave);
}
