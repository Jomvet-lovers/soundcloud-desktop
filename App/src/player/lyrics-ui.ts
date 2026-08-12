import { useSyncExternalStore } from 'react';

/** UI-состояние экрана лирики (донор `stores/lyrics.ts`). Модульный стор: тоглится
 *  из плеера (LyricsBtn/PillTrack) и читается оверлеем в AppShell без провайдера. */
export type LyricsTab = 'lyrics' | 'comments';

export const LYRICS_SPLIT_MIN = 0.32;
export const LYRICS_SPLIT_MAX = 0.68;
export const LYRICS_SPLIT_DEFAULT = 0.5;
export const LYRICS_SPLIT_STEP = 0.03;

export const clampSplit = (v: number): number => Math.max(LYRICS_SPLIT_MIN, Math.min(LYRICS_SPLIT_MAX, v));

interface OpenOptions {
  tab?: LyricsTab;
  rightPanelOpen?: boolean;
}

interface LyricsUi {
  open: boolean;
  tab: LyricsTab;
  rightPanelOpen: boolean;
  splitRatio: number;
  visualizer: boolean;
}

let state: LyricsUi = { open: false, tab: 'lyrics', rightPanelOpen: true, splitRatio: LYRICS_SPLIT_DEFAULT, visualizer: false };
const listeners = new Set<() => void>();

function set(patch: Partial<LyricsUi>): void {
  state = { ...state, ...patch };
  for (const l of listeners) l();
}

export const lyricsUi = {
  open: (opts?: OpenOptions) =>
    set({ open: true, tab: opts?.tab ?? state.tab, rightPanelOpen: opts?.rightPanelOpen ?? state.rightPanelOpen }),
  close: () => set({ open: false }),
  toggle: () => set({ open: !state.open }),
  setTab: (tab: LyricsTab) => set({ tab }),
  setRightPanelOpen: (rightPanelOpen: boolean) => set({ rightPanelOpen }),
  toggleRightPanel: () => set({ rightPanelOpen: !state.rightPanelOpen }),
  setSplitRatio: (r: number) => set({ splitRatio: clampSplit(r) }),
  toggleVisualizer: () => set({ visualizer: !state.visualizer }),
};

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => {
    listeners.delete(l);
  };
};

export const useLyricsUi = (): LyricsUi => useSyncExternalStore(subscribe, () => state, () => state);
