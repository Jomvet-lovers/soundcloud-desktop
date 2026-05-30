import {create} from 'zustand';

/**
 * The live search query, shared between the global header search field and the
 * Search page. Transient (not persisted) — it's the in-flight query, not a pref.
 * The header writes it from anywhere; the Search page reads + debounces it.
 */
interface SearchQueryState {
    q: string;
    setQ: (q: string) => void;
}

export const useSearchQueryStore = create<SearchQueryState>((set) => ({
    q: '',
    setQ: (q) => set({q}),
}));
