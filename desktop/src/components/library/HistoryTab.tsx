import React from 'react';
import { useNavigate } from 'react-router-dom';
import { art } from '../../lib/formatters';
import { useHistory, useInfiniteScroll } from '../../lib/hooks';
import { Loader2, Music, playWhite14 } from '../../lib/icons';
import { usePlayerStore } from '../../stores/player';
import { VirtualList } from '../ui/VirtualList';
import { historyEntryToTrack, historyTrackUrn } from './history-utils';

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
          rowHeight={68}
          overscan={10}
          className="flex flex-col"
          disabled={entries.length < 60}
          getItemKey={(entry) => entry.id}
          renderItem={(entry) => (
            <div className="group flex items-center gap-4 px-4 py-3 rounded-2xl hover:bg-white/[0.04] transition-all duration-300">
              <button
                type="button"
                className="relative w-11 h-11 rounded-xl overflow-hidden shrink-0 ring-1 ring-white/[0.08] shadow-md cursor-pointer"
                onClick={() => {
                  const tracks = entries.map(historyEntryToTrack);
                  play(historyEntryToTrack(entry), tracks);
                }}
              >
                {entry.artworkUrl ? (
                  <img
                    src={art(entry.artworkUrl, 't200x200') ?? ''}
                    alt=""
                    className="w-full h-full object-cover"
                    decoding="async"
                  />
                ) : (
                  <div className="w-full h-full flex items-center justify-center bg-white/[0.04]">
                    <Music size={14} className="text-white/20" />
                  </div>
                )}
                <div className="absolute inset-0 flex items-center justify-center bg-black/40 opacity-0 group-hover:opacity-100 transition-opacity text-white">
                  {playWhite14}
                </div>
              </button>

              <div className="flex-1 min-w-0 flex flex-col justify-center">
                <p
                  className="text-[14px] font-medium truncate text-white/90 hover:text-white cursor-pointer transition-colors"
                  onClick={() =>
                    navigate(`/track/${encodeURIComponent(historyTrackUrn(entry.scTrackId))}`)
                  }
                >
                  {entry.title}
                </p>
                <p
                  className={`text-[12px] text-white/40 truncate mt-0.5${entry.artistUrn ? ' hover:text-white/60 cursor-pointer transition-colors' : ''}`}
                  onClick={() =>
                    entry.artistUrn && navigate(`/user/${encodeURIComponent(entry.artistUrn)}`)
                  }
                >
                  {entry.artistName}
                </p>
              </div>

              <span className="text-[11px] text-white/20 tabular-nums shrink-0">
                {new Date(entry.playedAt).toLocaleTimeString([], {
                  hour: '2-digit',
                  minute: '2-digit',
                })}
              </span>
            </div>
          )}
        />
      ) : (
        <div className="py-20 text-center text-white/20">{'No listening history'}</div>
      )}

      <div ref={sentinelRef} className="h-12 flex items-center justify-center mt-4">
        {historyQuery.isFetchingNextPage && (
          <Loader2 size={20} className="text-white/15 animate-spin" />
        )}
      </div>
    </div>
  );
});
