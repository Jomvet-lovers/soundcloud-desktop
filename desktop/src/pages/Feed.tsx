import { useQueryClient } from '@tanstack/react-query';
import { useCallback, useMemo } from 'react';
import { PlaylistCard } from '../components/music/PlaylistCard';
import { TrackCard } from '../components/music/TrackCard';
import { ago } from '../lib/formatters';
import { type FeedItem, type Playlist, useFeed } from '../lib/hooks';
import { AudioLines, Loader2, RefreshCw, Repeat2 } from '../lib/icons';
import type { Track } from '../stores/player';

/** Playlists arrive as the same `FeedOrigin` shape as tracks; the card wants the
 *  full `Playlist`. SoundCloud labels sets with `set_type` (`album`, `ep`, …),
 *  which the card renders as its type badge — the type is our own addition
 *  because `FeedOrigin` only guarantees the track-ish half of the fields. */
function asPlaylist(item: FeedItem): Playlist {
  const origin = item.origin;
  return {
    ...origin,
    playlist_type: origin.playlist_type ?? origin.set_type ?? 'Playlist',
  } as Playlist;
}

/** One tile. The context line above the artwork is present on every cell —
 *  otherwise reposted tiles would start lower than plain ones and the grid
 *  rows would stagger. */
function FeedTile({ item, queue }: { item: FeedItem; queue: Track[] }) {
  const by = item.type.endsWith('-repost') ? (item.user?.username ?? null) : null;
  const when = ago(item.created_at);

  return (
    <div className="min-w-0">
      <p className="mb-1.5 h-4 flex items-center gap-1 text-[10px] text-white/30">
        {by && <Repeat2 size={10} className="shrink-0" />}
        <span className="truncate">
          {by ? `${by} reposted · ` : ''}
          {when}
        </span>
      </p>
      {item.kind === 'playlist' ? (
        <PlaylistCard playlist={asPlaylist(item)} showPlayback />
      ) : (
        <TrackCard track={item.origin} queue={queue} />
      )}
    </div>
  );
}

/** Feed — what the people you follow uploaded and reposted, newest first.
 *  SoundCloud's own stream, laid out like Home: a flat card grid, not a list. */
export function Feed() {
  const feed = useFeed();
  const qc = useQueryClient();

  const refetch = useCallback(() => {
    void qc.invalidateQueries({ queryKey: ['me', 'feed'] });
  }, [qc]);

  // Everything playable on the page, so playing one tile continues through the
  // feed the way Home's grid does. The feed loads in full, so this is the whole
  // list rather than a page of it.
  const queue = useMemo(
    () => feed.items.filter((it) => it.kind === 'track').map((it) => it.origin as Track),
    [feed.items],
  );

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
        <div className="flex justify-center py-20">
          <Loader2 size={32} className="animate-spin text-white/20" />
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
        <div className="mt-5 grid grid-cols-3 gap-2.5 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6 xl:grid-cols-7">
          {feed.items.map((item) => (
            <FeedTile key={item.id} item={item} queue={queue} />
          ))}
        </div>
      )}
    </div>
  );
}
