import {create} from 'zustand';
import {createJSONStorage, persist} from 'zustand/middleware';
import {tauriStorage} from '../lib/tauri-storage';

interface NewsState {
  /** IDs dismissed by user — never shown again */
  dismissed: string[];
  dismiss: (id: string) => void;
  isDismissed: (id: string) => boolean;
}

export const useNewsStore = create<NewsState>()(
  persist(
    (set, get) => ({
      dismissed: [],

      dismiss: (id) =>
        set((s) => (s.dismissed.includes(id) ? s : { dismissed: [...s.dismissed, id] })),

      isDismissed: (id) => get().dismissed.includes(id),
    }),
    {
      name: 'news',
      storage: createJSONStorage(() => tauriStorage),
      version: 1,
      // v0 split dismissal into permanent/session — carry permanent ones over
      migrate: (persisted, version) => {
        if (version < 1) {
          const legacy = (persisted as { permanentlyDismissed?: unknown })?.permanentlyDismissed;
          if (Array.isArray(legacy)) return { dismissed: legacy as string[] };
        }
        return persisted as NewsState;
      },
      partialize: (s) => ({ dismissed: s.dismissed }),
    },
  ),
);
