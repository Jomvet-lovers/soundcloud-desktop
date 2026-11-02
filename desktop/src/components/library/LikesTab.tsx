import React, { useEffect, useMemo } from 'react';
import { useInfiniteScroll, useLikedTracks } from '../../lib/hooks';
import { Loader2 } from '../../lib/icons';
import { armLikesContinuation } from '../../lib/queue-continuation';
import { VirtualList } from '../ui/VirtualList';
import { LibraryTrackRow } from './LibraryTrackRow';

export const LikesTab = React.memo(function LikesTab({ filter }: { filter: string }) {
  const likesQuery = useLikedTracks();
  const { tracks: likedTracks, isLoading } = likesQuery;
  const sentinelRef = useInfiniteScroll(
    !!likesQuery.hasNextPage,
    !!likesQuery.isFetchingNextPage,
    likesQuery.fetchNextPage,
  );

  // Auto-fetch remaining pages when filtering
  useEffect(() => {
    if (filter && likesQuery.hasNextPage && !likesQuery.isFetchingNextPage) {
      likesQuery.fetchNextPage();
    }
  }, [filter, likesQuery.hasNextPage, likesQuery.isFetchingNextPage]);

  const filtered = useMemo(() => {
    if (!filter) return likedTracks;
    const q = filter.toLowerCase();
    return likedTracks.filter(
      (tr) => tr.title.toLowerCase().includes(q) || tr.user.username.toLowerCase().includes(q),
    );
  }, [likedTracks, filter]);

  // Starting a track from likes → install the layer: it plays ALL likes through
  // to the end, fetching pages as the queue drains, and hands control to the
  // wave once they run out (lib/queue-autopilot.ts). Under an active filter the
  // queue already equals the whole match (pages pulled above), so no source is
  // needed (play() reset the previous one anyway) → straight to the wave.
  // filterRef keeps the callback stable for memo'd rows while still seeing the
  // fresh filter.
  const filterRef = React.useRef(filter);
  filterRef.current = filter;
  const onLikePlay = React.useCallback(() => {
    if (!filterRef.current) armLikesContinuation();
  }, []);

  return (
    <div className="min-h-[400px]">
      <div className="flex flex-col gap-1">
        {isLoading ? (
          <div className="flex justify-center py-20">
            <Loader2 size={32} className="animate-spin text-white/20" />
          </div>
        ) : filtered.length > 0 ? (
          <VirtualList
            items={filtered}
            rowHeight={64}
            overscan={8}
            className="flex flex-col gap-1"
            disabled={filtered.length < 40}
            getItemKey={(track) => track.urn}
            prefetchUrn={(track) => track.urn}
            renderItem={(track, i) => (
              <LibraryTrackRow track={track} index={i} queue={filtered} onPlay={onLikePlay} />
            )}
          />
        ) : (
          <div className="py-20 text-center text-white/20">
            {filter && likesQuery.hasNextPage
              ? 'Loading...'
              : filter
                ? 'No matches'
                : 'No liked tracks yet'}
          </div>
        )}
      </div>
      {!filter ? (
        <div ref={sentinelRef} className="h-12 flex items-center justify-center mt-4">
          {likesQuery.isFetchingNextPage && (
            <Loader2 size={20} className="text-white/15 animate-spin" />
          )}
        </div>
      ) : likesQuery.isFetchingNextPage ? (
        <div className="h-12 flex items-center justify-center mt-4">
          <Loader2 size={20} className="text-white/15 animate-spin" />
        </div>
      ) : null}
    </div>
  );
});
