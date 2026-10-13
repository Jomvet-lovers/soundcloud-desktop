import { type MouseEvent, memo } from 'react';
import { useNavigate } from 'react-router-dom';
import { preloadTrack } from '../../lib/audio';
import { ago, art, dur, fc } from '../../lib/formatters';
import type { FeedItem } from '../../lib/hooks';
import {
  headphones11,
  heart11,
  ListMusic,
  Music,
  pauseWhite14,
  playWhite14,
  Repeat2,
} from '../../lib/icons';
import { useTrackPlay } from '../../lib/useTrackPlay';
import type { Track } from '../../stores/player';
import { usePlayerStore } from '../../stores/player';
import { playlistMenuHandler, trackMenuHandler } from '../../stores/track-menu';
import { LikeButton } from '../music/LikeButton';
import { TrackTitleArtist } from '../music/TrackTitleArtist';

/** `track-repost` / `playlist-repost` rows carry the reposter at item level;
 *  a plain upload has the uploader there instead, which the row already shows. */
function reposterName(item: FeedItem): string | null {
  if (!item.type.endsWith('-repost')) return null;
  return item.user?.username ?? null;
}

/** The rows this feed item can play: a track is its own queue, a playlist uses
 *  the inline `tracks` SC's `/stream` sends. Stub entries (`{id}` only, no
 *  title) can't be played, so they are not queued. */
function playableTracks(item: FeedItem): Track[] {
  const tracks = item.kind === 'playlist' ? (item.origin.tracks ?? []) : [item.origin];
  return tracks.filter((t) => t.urn && t.title);
}

/** Play/pause affordance filling the cover. */
function CoverPlay({ track, queue }: { track: Track; queue: Track[] }) {
  const { isThisPlaying, togglePlay } = useTrackPlay(track, queue);
  const isPlayingFromQueue = usePlayerStore(
    (s) =>
      s.isPlaying && s.currentTrack != null && queue.some((t) => t.urn === s.currentTrack?.urn),
  );

  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation();
        togglePlay();
      }}
      onMouseEnter={() => preloadTrack(track.urn)}
      title={isPlayingFromQueue ? 'Pause' : 'Play'}
      className={`absolute inset-0 flex items-center justify-center bg-black/45 transition-opacity cursor-pointer ${
        isThisPlaying ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'
      }`}
    >
      <span className="w-7 h-7 rounded-full bg-white/15 flex items-center justify-center">
        {isPlayingFromQueue ? pauseWhite14 : playWhite14}
      </span>
    </button>
  );
}

/** Count columns shared by both row kinds — the same slots LibraryTrackRow
 *  uses, so the pages read alike. */
function Counters({ plays, likes }: { plays?: number | null; likes?: number | null }) {
  return (
    <div className="hidden sm:flex items-center gap-4 shrink-0 pr-4">
      {plays != null && (
        <span className="text-[11px] text-white/30 tabular-nums flex items-center gap-1.5 w-16">
          {headphones11}
          {fc(plays)}
        </span>
      )}
      {likes != null && (
        <span className="text-[11px] text-white/30 tabular-nums flex items-center gap-1.5 w-14">
          {heart11}
          {fc(likes)}
        </span>
      )}
    </div>
  );
}

/** One feed row. Tracks get the track row treatment (title links to the track
 *  page, like button, artist links); playlists get their title + track count
 *  and link to the playlist page. Reposts prepend a "X reposted" line. */
export const FeedRow = memo(function FeedRow({ item }: { item: FeedItem }) {
  const navigate = useNavigate();
  const isPlaylist = item.kind === 'playlist';
  const origin = item.origin;
  const queue = playableTracks(item);
  const by = reposterName(item);
  const when = ago(item.created_at);
  const cover = art(origin.artwork_url, 't200x200');

  const open = (e: MouseEvent) => {
    e.stopPropagation();
    const page = isPlaylist ? 'playlist' : 'track';
    navigate(`/${page}/${encodeURIComponent(origin.urn)}`);
  };

  return (
    <div>
      {by && (
        <div className="flex items-center gap-1.5 px-4 pb-1 text-[11px] text-white/30">
          <Repeat2 size={12} className="shrink-0" />
          <span className="truncate">{by} reposted</span>
          {when && <span className="shrink-0 tabular-nums">{when}</span>}
        </div>
      )}
      <div
        className="group flex items-center gap-4 px-4 py-3 rounded-md transition-colors duration-150 hover:bg-white/[0.04]"
        onContextMenu={
          isPlaylist ? playlistMenuHandler({ urn: origin.urn }) : trackMenuHandler(origin)
        }
      >
        {/* A block element: an inline <span> ignores w/h/overflow, which pushed
            the artwork out of its ring and skewed every row against the
            playlist ones. */}
        <div className="relative w-10 h-10 rounded-lg overflow-hidden shrink-0 ring-1 ring-white/[0.08]">
          {cover ? (
            <img src={cover} alt="" decoding="async" className="w-full h-full object-cover" />
          ) : (
            <div className="w-full h-full flex items-center justify-center bg-white/[0.04]">
              {isPlaylist ? (
                <ListMusic size={14} className="text-white/25" />
              ) : (
                <Music size={14} className="text-white/25" />
              )}
            </div>
          )}
          {queue[0] && <CoverPlay track={queue[0]} queue={queue} />}
        </div>

        {isPlaylist ? (
          <button type="button" onClick={open} className="min-w-0 flex-1 text-left">
            {/* Fixed height: a playlist row must line up with the two-line track
                rows regardless of how its own fields fall out. */}
            <span className="block text-[14px] font-medium leading-5 truncate text-white/90 hover:text-white transition-colors duration-150">
              {origin.title}
            </span>
            <span className="block text-[12px] leading-4 truncate mt-0.5 text-white/40">
              {origin.user?.username ? `${origin.user.username} · ` : ''}
              {origin.track_count != null ? `${fc(origin.track_count)} tracks` : 'Playlist'}
            </span>
          </button>
        ) : (
          <TrackTitleArtist track={origin} size="md" />
        )}

        {!isPlaylist && <LikeButton track={origin} />}

        {isPlaylist ? (
          <Counters likes={origin.likes_count} />
        ) : (
          <Counters
            plays={origin.playback_count}
            likes={origin.favoritings_count ?? origin.likes_count}
          />
        )}

        {!by && (
          <span className="text-[11px] text-white/30 tabular-nums shrink-0 w-10 text-right">
            {when}
          </span>
        )}
        {origin.duration > 0 && (
          <span className="text-[12px] text-white/30 tabular-nums font-medium shrink-0 w-12 text-right">
            {dur(origin.duration)}
          </span>
        )}
      </div>
    </div>
  );
});
