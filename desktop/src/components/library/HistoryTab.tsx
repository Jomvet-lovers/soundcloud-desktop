import React, { useMemo } from 'react';
import { useHistory, useInfiniteScroll } from '../../lib/hooks';
import { Loader2 } from '../../lib/icons';
import { VirtualList } from '../ui/VirtualList';
import { historyEntryToTrack } from './history-utils';
import { LibraryTrackRow } from './LibraryTrackRow';

export const HistoryTab = React.memo(function HistoryTab() {
  const historyQuery = useHistory();
  const { entries, isLoading } = historyQuery;
  const sentinelRef = useInfiniteScroll(
    !!historyQuery.hasNextPage,
    !!historyQuery.isFetchingNextPage,
    historyQuery.fetchNextPage,
  );

  // History rows use the same row component (and therefore the same geometry,
  // hover actions, duration and context menu) as the local library.
  const tracks = useMemo(() => entries.map(historyEntryToTrack), [entries]);

  return (
    <div className="min-h-[400px]">
      <div className="flex flex-col gap-1">
        {isLoading ? (
          <div className="flex justify-center py-20">
            <Loader2 size={32} className="animate-spin text-white/20" />
          </div>
        ) : entries.length > 0 ? (
          <VirtualList
            items={entries}
            rowHeight={64}
            overscan={8}
            className="flex flex-col gap-1"
            disabled={entries.length < 40}
            getItemKey={(entry) => entry.id}
            renderItem={(_entry, index) => (
              <LibraryTrackRow track={tracks[index]} index={index} queue={tracks} />
            )}
          />
        ) : (
          <div className="py-20 text-center text-white/20">{'No listening history'}</div>
        )}
      </div>

      <div ref={sentinelRef} className="h-12 flex items-center justify-center mt-4">
        {historyQuery.isFetchingNextPage && (
          <Loader2 size={20} className="text-white/15 animate-spin" />
        )}
      </div>
    </div>
  );
});
