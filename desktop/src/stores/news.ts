import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';
import { tauriStorage } from '../lib/tauri-storage';

interface NewsState {
  /** IDs permanently dismissed by user */
  permanentlyDismissed: string[];
  /** IDs dismissed for this session only */
  sessionDismissed: string[];
  dismissForever: (id: string) => void;
  dismissOnce: (id: string) => void;
  isDismissed: (id: string) => boolean;
}

export const useNewsStore = create<NewsState>()(
  persist(
    (set, get) => ({
      permanentlyDismissed: [],
      sessionDismissed: [],

      dismissForever: (id) =>
        set((s) => ({
          permanentlyDismissed: [...s.permanentlyDismissed, id],
          sessionDismissed: s.sessionDismissed.filter((d) => d !== id),
        })),

      dismissOnce: (id) =>
        set((s) => ({
          sessionDismissed: [...s.sessionDismissed, id],
        })),

      isDismissed: (id) => {
        const s = get();
        return s.permanentlyDismissed.includes(id) || s.sessionDismissed.includes(id);
      },
    }),
    {
      name: 'news',
      storage: createJSONStorage(() => tauriStorage),
      partialize: (s) => ({ permanentlyDismissed: s.permanentlyDismissed }),
    },
  ),
);
