import type { QueryClient } from '@tanstack/react-query';
import type { Track } from '../stores/player';

interface TrackListResponse {
  collection: Track[];
  next_href: string | null;
}

export function optimisticToggleLike(qc: QueryClient, track: Track, nowLiked: boolean) {
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

  // Delayed refetch for eventual consistency
  setTimeout(() => {
    qc.invalidateQueries({ queryKey: ['me', 'likes', 'tracks'] });
    qc.invalidateQueries({ queryKey: ['track', track.urn], exact: true });
  }, 3000);
}
