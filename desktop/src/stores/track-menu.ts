import type { MouseEvent } from 'react';
import { create } from 'zustand';
import type { Track } from './player';

/** An artist/profile link target for the right-click menu. */
export interface UserMenuTarget {
  /** Route to open (e.g. `/user/…` or `/artist/…`). */
  target: string;
  /** Public profile URL for "Copy link", when known. */
  permalink?: string | null;
}

/** One app-wide right-click menu: track actions, or profile actions. */
interface TrackMenuState {
  kind: 'track' | 'user' | null;
  track: Track | null;
  user: UserMenuTarget | null;
  x: number;
  y: number;
  openTrack: (track: Track, x: number, y: number) => void;
  openUser: (user: UserMenuTarget, x: number, y: number) => void;
  close: () => void;
}

export const useTrackMenuStore = create<TrackMenuState>()((set) => ({
  kind: null,
  track: null,
  user: null,
  x: 0,
  y: 0,
  openTrack: (track, x, y) => set({ kind: 'track', track, user: null, x, y }),
  openUser: (user, x, y) => set({ kind: 'user', track: null, user, x, y }),
  close: () => set({ kind: null, track: null, user: null }),
}));

/** Attach to any track surface: opens the track menu at the pointer. */
export function trackMenuHandler(track: Track) {
  return (e: MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    useTrackMenuStore.getState().openTrack(track, e.clientX, e.clientY);
  };
}

/** Attach to artist/profile links: opens the profile menu at the pointer. */
export function userMenuHandler(user: UserMenuTarget) {
  return (e: MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    useTrackMenuStore.getState().openUser(user, e.clientX, e.clientY);
  };
}
