import { create } from 'zustand';

interface LyricsUIState {
  open: boolean;
  toggle: () => void;
  openPanel: () => void;
  close: () => void;
}

export const useLyricsStore = create<LyricsUIState>()((set) => ({
  open: false,
  toggle: () => set((s) => ({ open: !s.open })),
  openPanel: () => set({ open: true }),
  close: () => set({ open: false }),
}));
