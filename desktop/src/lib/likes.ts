import type { QueryClient } from '@tanstack/react-query';
import { useSyncExternalStore } from 'react';
import { useAuthStore } from '../stores/auth';
import type { Track } from '../stores/player';

interface TrackListResponse {
  collection: Track[];
  next_href: string | null;
}

/* ── Global liked URNs store ─────────────────────────────── */

const _likedUrns = new Map<string, boolean>();
const _listeners = new Set<() => void>();

function notify() {
  for (const l of _listeners) l();
}

/** Sync liked URNs from loaded liked tracks (called on every useLikedTracks data change) */
export function initLikedUrns(tracks: Track[]) {
  let changed = false;
  for (const t of tracks) {
    if (!_likedUrns.has(t.urn)) {
      _likedUrns.set(t.urn, true);
      changed = true;
    }
  }
  if (changed) notify();
}

/** Set like status for a track URN */
export function setLikedUrn(urn: string, liked: boolean) {
  if (liked) {
    _likedUrns.set(urn, true);
  } else {
    _likedUrns.delete(urn);
  }
  notify();
}

/** Check if a track URN is liked */
export function isUrnLiked(urn: string): boolean {
  return _likedUrns.has(urn);
}

/** React hook — subscribes to like status for a specific URN */
export function useLiked(urn: string): boolean {
  return useSyncExternalStore(
    (cb) => {
      _listeners.add(cb);
      return () => _listeners.delete(cb);
    },
    () => _likedUrns.has(urn),
  );
}

/* ── Optimistic toggle (TanStack Query cache) ───────────── */

export function optimisticToggleLike(qc: QueryClient, track: Track, nowLiked: boolean) {
  // Update global liked URNs
  setLikedUrn(track.urn, nowLiked);

  // Update favorites count in auth store
  const { user } = useAuthStore.getState();
  if (user) {
    useAuthStore.setState({
      user: { ...user, public_favorites_count: user.public_favorites_count + (nowLiked ? 1 : -1) },
    });
  }

  // Update all liked tracks infinite queries
  qc.setQueriesData<{ pages: TrackListResponse[]; pageParams: unknown[] }>(
    { queryKey: ['me', 'likes', 'tracks'] },
    (old) => {
      if (!old?.pages) return old;
      if (nowLiked) {
        const pages = [...old.pages];
        pages[0] = {
          ...pages[0],
          collection: [track, ...pages[0].collection.filter((t) => t.urn !== track.urn)],
        };
        return { ...old, pages };
      }
      return {
        ...old,
        pages: old.pages.map((page) => ({
          ...page,
          collection: page.collection.filter((t) => t.urn !== track.urn),
        })),
      };
    },
  );

  // Update single track query
  qc.setQueryData<Track>(['track', track.urn], (old) => {
    if (!old) return old;
    return { ...old, user_favorite: nowLiked };
  });

  // Delayed refetch for single track (eventual consistency).
  // Liked tracks list is NOT invalidated — the optimistic cache update above
  // is already correct, and SC API is eventually consistent so early refetch
  // would overwrite optimistic data with stale results.
  setTimeout(() => {
    qc.invalidateQueries({ queryKey: ['track', track.urn], exact: true });
  }, 5000);
}
