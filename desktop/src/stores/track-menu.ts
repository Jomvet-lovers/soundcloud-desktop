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

/** A playlist target for the right-click menu (sidebar pins, …). */
export interface PlaylistMenuTarget {
  urn: string;
  /** Public URL when known; otherwise it is fetched on demand. */
  permalink?: string | null;
}

/** One app-wide right-click menu: track actions, profile actions, or playlist
 *  actions. */
interface TrackMenuState {
  kind: 'track' | 'user' | 'playlist' | null;
  track: Track | null;
  user: UserMenuTarget | null;
  playlist: PlaylistMenuTarget | null;
  x: number;
  y: number;
  openTrack: (track: Track, x: number, y: number) => void;
  openUser: (user: UserMenuTarget, x: number, y: number) => void;
  openPlaylist: (playlist: PlaylistMenuTarget, x: number, y: number) => void;
  close: () => void;
}

export const useTrackMenuStore = create<TrackMenuState>()((set) => ({
  kind: null,
  track: null,
  user: null,
  playlist: null,
  x: 0,
  y: 0,
  openTrack: (track, x, y) => set({ kind: 'track', track, user: null, playlist: null, x, y }),
  openUser: (user, x, y) => set({ kind: 'user', track: null, user, playlist: null, x, y }),
  openPlaylist: (playlist, x, y) =>
    set({ kind: 'playlist', track: null, user: null, playlist, x, y }),
  close: () => set({ kind: null, track: null, user: null, playlist: null }),
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

/** Attach to playlist surfaces (sidebar pins): opens the playlist menu. */
export function playlistMenuHandler(playlist: PlaylistMenuTarget) {
  return (e: MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    useTrackMenuStore.getState().openPlaylist(playlist, e.clientX, e.clientY);
  };
}
