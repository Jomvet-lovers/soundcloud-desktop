import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';
import { tauriStorage } from '../lib/tauri-storage';

/**
 * `db` — поиск в локальной базе SCD (быстрее, ограничен зеркалом).
 * `sc` — fan-out в SoundCloud API (медленнее, видит всё).
 */
export type SearchSource = 'db' | 'sc';

/**
 * `text` — лексический поиск (название/артист + строчки лирики) с тизером «по вайбу».
 * `vibe` — чисто семантический поиск по вайбу (борда + атмосфера под выдачу).
 */
export type SearchMode = 'text' | 'vibe';

/** Result ordering on the Search page (SoundCloud search ignores `sort`). */
export type SearchSort = 'relevance' | 'plays' | 'newest' | 'likes';

interface SearchPrefsState {
  source: SearchSource;
  setSource: (s: SearchSource) => void;
  mode: SearchMode;
  setMode: (m: SearchMode) => void;
  sort: SearchSort;
  setSort: (s: SearchSort) => void;
}

export const useSearchPrefsStore = create<SearchPrefsState>()(
  persist(
    (set) => ({
      source: 'db',
      setSource: (source) => set({ source }),
      mode: 'text',
      setMode: (mode) => set({ mode }),
      sort: 'relevance',
      setSort: (sort) => set({ sort }),
    }),
    {
      name: 'sc-search-prefs',
      storage: createJSONStorage(() => tauriStorage),
      // v0 saved the active result tab (`tab`). The tab is view state on the
      // Search page now, not a preference — drop the stale key on load so an
      // old "playlists" choice can't stick as the default.
      version: 1,
      migrate: (persisted) => {
        const prefs = Object.fromEntries(
          Object.entries((persisted ?? {}) as Record<string, unknown>).filter(
            ([key]) => key !== 'tab',
          ),
        );
        return prefs as unknown as SearchPrefsState;
      },
    },
  ),
);
