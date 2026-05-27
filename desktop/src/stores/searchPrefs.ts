import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';
import { tauriStorage } from '../lib/tauri-storage';

/**
 * `db` — поиск в локальной базе SCD (быстрее, ограничен зеркалом).
 * `sc` — fan-out в SoundCloud API (медленнее, видит всё).
 */
export type SearchSource = 'db' | 'sc';

interface SearchPrefsState {
  source: SearchSource;
  setSource: (s: SearchSource) => void;
}

export const useSearchPrefsStore = create<SearchPrefsState>()(
  persist(
    (set) => ({
      source: 'db',
      setSource: (source) => set({ source }),
    }),
    {
      name: 'sc-search-prefs',
      storage: createJSONStorage(() => tauriStorage),
    },
  ),
);
