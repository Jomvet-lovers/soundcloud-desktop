import { create } from 'zustand';

export type AppMode = 'online' | 'offline';

interface AppStatusState {
  navigatorOnline: boolean;
  backendReachable: boolean;
  setNavigatorOnline: (online: boolean) => void;
  setBackendReachable: (reachable: boolean) => void;
  resetConnectivity: () => void;
}

export const useAppStatusStore = create<AppStatusState>((set) => ({
  navigatorOnline: typeof navigator === 'undefined' ? true : navigator.onLine,
  backendReachable: true,
  setNavigatorOnline: (online) => set({ navigatorOnline: online }),
  setBackendReachable: (backendReachable) => set({ backendReachable }),
  resetConnectivity: () =>
    set({
      navigatorOnline: typeof navigator === 'undefined' ? true : navigator.onLine,
      backendReachable: true,
    }),
}));

export function getAppMode(): AppMode {
  const { navigatorOnline, backendReachable } = useAppStatusStore.getState();
  if (!navigatorOnline || !backendReachable) return 'offline';
  return 'online';
}

export function isOfflineMode() {
  return getAppMode() !== 'online';
}
