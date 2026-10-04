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

/** Active tab on the Search page — kept across navigation (back button). */
export type SearchTab = 'tracks' | 'users' | 'playlists' | 'albums';

/** Result ordering on the Search page (SoundCloud search ignores `sort`). */
export type SearchSort = 'relevance' | 'plays' | 'newest' | 'likes';

interface SearchPrefsState {
  source: SearchSource;
  setSource: (s: SearchSource) => void;
  mode: SearchMode;
  setMode: (m: SearchMode) => void;
  tab: SearchTab;
  setTab: (t: SearchTab) => void;
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
      tab: 'tracks',
      setTab: (tab) => set({ tab }),
      sort: 'relevance',
      setSort: (sort) => set({ sort }),
    }),
    {
      name: 'sc-search-prefs',
      storage: createJSONStorage(() => tauriStorage),
    },
  ),
);
