import { useQueryClient } from '@tanstack/react-query';
import { useCallback } from 'react';
import { FeedRow } from '../components/feed/FeedRow';
import { useFeed } from '../lib/hooks';
import { AudioLines, Loader2, RefreshCw } from '../lib/icons';

/** Feed — what the people you follow uploaded and reposted, newest first.
 *  SoundCloud's own stream (`/stream`), tracks and playlists in one list,
 *  extended with "Load more". */
export function Feed() {
  const feed = useFeed();
  const qc = useQueryClient();

  const refetch = useCallback(() => {
    void qc.invalidateQueries({ queryKey: ['me', 'feed'] });
  }, [qc]);

  return (
    <div className="px-5 py-6 md:px-8">
      <div className="flex items-center justify-between gap-4">
        <h1 className="text-[24px] font-semibold tracking-tight text-white/92">{'Feed'}</h1>
        <button
          type="button"
          onClick={refetch}
          disabled={feed.isFetching}
          title={'Refresh'}
          className="w-8 h-8 rounded-lg flex items-center justify-center text-white/40 hover:text-white/80 hover:bg-white/[0.06] transition-colors cursor-pointer disabled:opacity-40"
        >
          <RefreshCw size={14} className={feed.isFetching ? 'animate-spin' : ''} />
        </button>
      </div>

      {feed.isLoading ? (
        <div className="mt-6 flex justify-center py-16">
          <Loader2 size={28} className="animate-spin text-white/15" />
        </div>
      ) : feed.items.length === 0 ? (
        <div className="mt-6 flex flex-col items-center gap-3 text-center py-14 px-6">
          <span className="w-14 h-14 rounded-2xl bg-white/[0.04] flex items-center justify-center text-white/25">
            <AudioLines size={24} />
          </span>
          <p className="text-white/40 text-sm max-w-[300px]">
            {'Nothing new from who you follow — check back later'}
          </p>
        </div>
      ) : (
        <>
          <div className="mt-5 flex flex-col gap-1">
            {feed.items.map((item) => (
              <FeedRow key={item.id} item={item} />
            ))}
          </div>

          {feed.hasNextPage && (
            <div className="mt-4 flex justify-center">
              <button
                type="button"
                onClick={() => feed.fetchNextPage()}
                disabled={feed.isFetchingNextPage}
                className="px-4 py-2 rounded-xl bg-white/[0.05] hover:bg-white/[0.08] text-[13px] text-white/60 hover:text-white/90 font-medium transition-colors cursor-pointer disabled:opacity-40"
              >
                {feed.isFetchingNextPage ? 'Loading…' : 'Load more'}
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}
