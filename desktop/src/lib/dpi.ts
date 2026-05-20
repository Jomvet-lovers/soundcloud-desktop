import { invoke } from '@tauri-apps/api/core';
import { useSettingsStore } from '../stores/settings';

function push(enabled: boolean) {
  invoke('dpi_set_enabled', { enabled }).catch(console.error);
}

export function initDpiSync() {
  push(useSettingsStore.getState().dpiBypass);
  useSettingsStore.subscribe((s, prev) => {
    if (s.dpiBypass !== prev.dpiBypass) push(s.dpiBypass);
  });
}

export function getDpiStrategy(): Promise<string> {
  return invoke<string>('dpi_strategy');
}
