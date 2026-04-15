import { create } from 'zustand';

interface SessionExpiryState {
  sessionExpired: boolean;
  setSessionExpired: (v: boolean) => void;
}

export const useSessionExpiryStore = create<SessionExpiryState>((set) => ({
  sessionExpired: false,
  setSessionExpired: (sessionExpired) => set({ sessionExpired }),
}));
