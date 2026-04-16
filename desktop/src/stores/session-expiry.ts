import { create } from 'zustand';

const REAUTH_COOLDOWN_MS = 5000;

interface SessionExpiryState {
  sessionExpired: boolean;
  reAuthedAt: number;
  setSessionExpired: (v: boolean) => void;
  markReAuthed: () => void;
}

export const useSessionExpiryStore = create<SessionExpiryState>((set, get) => ({
  sessionExpired: false,
  reAuthedAt: 0,
  setSessionExpired: (sessionExpired) => {
    if (sessionExpired && Date.now() - get().reAuthedAt < REAUTH_COOLDOWN_MS) {
      return;
    }
    set({ sessionExpired });
  },
  markReAuthed: () => set({ reAuthedAt: Date.now() }),
}));
