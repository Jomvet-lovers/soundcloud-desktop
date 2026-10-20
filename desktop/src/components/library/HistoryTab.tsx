import React from 'react';
import { useNavigate } from 'react-router-dom';
import { art, dur } from '../../lib/formatters';
import { useHistory, useInfiniteScroll } from '../../lib/hooks';
import { Loader2, Music, playWhite14 } from '../../lib/icons';
import { usePlayerStore } from '../../stores/player';
import { trackMenuHandler } from '../../stores/track-menu';
import { VirtualList } from '../ui/VirtualList';
import { historyEntryToTrack } from './history-utils';

export const HistoryTab = React.memo(function HistoryTab() {
  const navigate = useNavigate();
  const play = usePlayerStore((s) => s.play);
  const historyQuery = useHistory();
  const { entries, isLoading } = historyQuery;
  const sentinelRef = useInfiniteScroll(
    !!historyQuery.hasNextPage,
    !!historyQuery.isFetchingNextPage,
    historyQuery.fetchNextPage,
  );

  return (
    <div className="min-h-[400px]">
      {isLoading ? (
        <div className="flex justify-center py-20">
          <Loader2 size={32} className="animate-spin text-white/20" />
        </div>
      ) : entries.length > 0 ? (
        <VirtualList
          items={entries}
          rowHeight={56}
          overscan={10}
          className="flex flex-col"
          disabled={entries.length < 60}
          getItemKey={(entry) => entry.id}
          renderItem={(entry) => {
            const track = historyEntryToTrack(entry);
            return (
              <div
                className="group flex h-14 items-center gap-3 border-b border-white/[0.04] px-4 transition-colors hover:bg-white/[0.04]"
                onContextMenu={trackMenuHandler(track)}
                title={new Date(entry.playedAt).toLocaleString()}
              >
                <button
                  type="button"
                  className="relative h-10 w-10 shrink-0 cursor-pointer overflow-hidden rounded-md ring-1 ring-white/[0.08]"
                  onClick={() => play(track, entries.map(historyEntryToTrack))}
                >
                  {entry.artworkUrl ? (
                    <img
                      src={art(entry.artworkUrl, 't200x200') ?? ''}
                      alt=""
                      className="h-full w-full object-cover"
                      decoding="async"
                    />
                  ) : (
                    <div className="flex h-full w-full items-center justify-center bg-white/[0.04]">
                      <Music size={14} className="text-white/20" />
                    </div>
                  )}
                  <div className="absolute inset-0 flex items-center justify-center bg-black/40 text-white opacity-0 transition-opacity group-hover:opacity-100">
                    {playWhite14}
                  </div>
                </button>

                <div className="flex min-w-0 flex-1 flex-col justify-center">
                  <p
                    className="cursor-pointer truncate text-[14px] font-medium text-white/90 transition-colors hover:text-white"
                    onClick={() => navigate(`/track/${encodeURIComponent(track.urn)}`)}
                  >
                    {entry.title}
                  </p>
                  <p
                    className={`mt-0.5 truncate text-[12px] text-white/40${
                      entry.artistUrn ? ' cursor-pointer transition-colors hover:text-white/60' : ''
                    }`}
                    onClick={() =>
                      entry.artistUrn && navigate(`/user/${encodeURIComponent(entry.artistUrn)}`)
                    }
                  >
                    {entry.artistName}
                  </p>
                </div>

                <span className="shrink-0 text-[12px] font-medium tabular-nums text-white/30">
                  {dur(entry.duration)}
                </span>
              </div>
            );
          }}
        />
      ) : (
        <div className="py-20 text-center text-white/20">{'No listening history'}</div>
      )}

      <div ref={sentinelRef} className="mt-4 flex h-12 items-center justify-center">
        {historyQuery.isFetchingNextPage && (
          <Loader2 size={20} className="animate-spin text-white/15" />
        )}
      </div>
    </div>
  );
});
