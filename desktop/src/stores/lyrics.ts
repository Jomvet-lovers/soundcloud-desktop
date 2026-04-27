import { create } from 'zustand';

export type LyricsPanelTab = 'lyrics' | 'comments';

export const LYRICS_SPLIT_MIN = 0.32;
export const LYRICS_SPLIT_MAX = 0.68;
export const LYRICS_SPLIT_DEFAULT = 0.5;
export const LYRICS_SPLIT_KEYBOARD_STEP = 0.03;

export function clampLyricsSplit(value: number): number {
  return Math.max(LYRICS_SPLIT_MIN, Math.min(LYRICS_SPLIT_MAX, value));
}

interface OpenLyricsPanelOptions {
  tab?: LyricsPanelTab;
  rightPanelOpen?: boolean;
}

interface LyricsUIState {
  open: boolean;
  tab: LyricsPanelTab;
  rightPanelOpen: boolean;
  splitRatio: number;
  toggle: () => void;
  openPanel: (options?: OpenLyricsPanelOptions | LyricsPanelTab) => void;
  setTab: (tab: LyricsPanelTab) => void;
  setRightPanelOpen: (open: boolean) => void;
  toggleRightPanel: () => void;
  setSplitRatio: (ratio: number) => void;
  close: () => void;
}

export const useLyricsStore = create<LyricsUIState>()((set) => ({
  open: false,
  tab: 'lyrics',
  rightPanelOpen: true,
  splitRatio: LYRICS_SPLIT_DEFAULT,
  toggle: () => set((s) => ({ open: !s.open })),
  openPanel: (options) =>
    set((s) => {
      const normalized = typeof options === 'string' ? { tab: options } : (options ?? {});
      return {
        open: true,
        tab: normalized.tab ?? s.tab,
        rightPanelOpen: normalized.rightPanelOpen ?? s.rightPanelOpen,
      };
    }),
  setTab: (tab) => set({ tab }),
  setRightPanelOpen: (rightPanelOpen) => set({ rightPanelOpen }),
  toggleRightPanel: () => set((s) => ({ rightPanelOpen: !s.rightPanelOpen })),
  setSplitRatio: (ratio) => set({ splitRatio: clampLyricsSplit(ratio) }),
  close: () => set({ open: false }),
}));
