import {create} from 'zustand';
import {createJSONStorage, persist} from 'zustand/middleware';
import {tauriStorage} from '../lib/tauri-storage';

export type AppMode = 'online' | 'offline';

interface AppStatusState {
  navigatorOnline: boolean;
  backendReachable: boolean;
  offlineBypass: boolean;
  setNavigatorOnline: (online: boolean) => void;
  setBackendReachable: (reachable: boolean) => void;
  setOfflineBypass: (value: boolean) => void;
  resetConnectivity: () => void;
}

export const useAppStatusStore = create<AppStatusState>()(
  persist(
    (set) => ({
      navigatorOnline: typeof navigator === 'undefined' ? true : navigator.onLine,
      backendReachable: true,
      offlineBypass: false,
      setNavigatorOnline: (online) => set({ navigatorOnline: online }),
      setBackendReachable: (backendReachable) => set({ backendReachable }),
      setOfflineBypass: (offlineBypass) => set({ offlineBypass }),
      resetConnectivity: () =>
        set({
          navigatorOnline: typeof navigator === 'undefined' ? true : navigator.onLine,
          backendReachable: true,
          offlineBypass: false,
        }),
    }),
    {
      name: 'sc-app-status',
      storage: createJSONStorage(() => tauriStorage),
      partialize: (state) => ({ offlineBypass: state.offlineBypass }),
    },
  ),
);

export const selectAppMode = (s: AppStatusState): AppMode =>
  s.offlineBypass || !s.navigatorOnline || !s.backendReachable ? 'offline' : 'online';

export function useAppMode(): AppMode {
  return useAppStatusStore(selectAppMode);
}

export function getAppMode(): AppMode {
  return selectAppMode(useAppStatusStore.getState());
}

export function isOfflineMode() {
  return getAppMode() !== 'online';
}
