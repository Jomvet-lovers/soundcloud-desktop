import { useQuery, useQueryClient } from '@tanstack/react-query';
import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';
import { CopyLinkButton } from '../components/ui/CopyLinkButton';
import { api } from '../lib/api';
import { preloadTrack } from '../lib/audio';
import { art, dateFormatted, dur, durLong, fc } from '../lib/formatters';
import { useInfiniteScroll, usePlaylist, usePlaylistTracks } from '../lib/hooks';
import {
  Calendar,
  Clock,
  Heart,
  headphones9,
  heart9,
  ListMusic,
  Loader2,
  musicIcon12,
  pauseBlack22,
  pauseCurrent16,
  pauseWhite12,
  playBlack22,
  playCurrent16,
  playWhite12,
  Shuffle,
} from '../lib/icons';
import { useTrackPlay } from '../lib/useTrackPlay';
import { type Track, usePlayerStore } from '../stores/player';

/* ── Playlist Like Button ─────────────────────────────────── */

const PlaylistLikeBtn = React.memo(
  ({ playlistUrn, count }: { playlistUrn: string; count?: number }) => {
    const { data: likeStatus } = useQuery({
      queryKey: ['likes', 'playlist', playlistUrn],
      queryFn: () => api<{ liked: boolean }>(`/likes/playlists/${encodeURIComponent(playlistUrn)}`),
      staleTime: 1000 * 60 * 5,
    });

    const [liked, setLiked] = useState(false);
    const [localCount, setLocalCount] = useState(count ?? 0);
    const qc = useQueryClient();

    useEffect(() => {
      if (likeStatus) setLiked(likeStatus.liked);
    }, [likeStatus]);
    useEffect(() => {
      setLocalCount(count ?? 0);
    }, [count]);

    const toggle = async () => {
      const next = !liked;
      setLiked(next);
      setLocalCount((c) => c + (next ? 1 : -1));
      try {
        await api(`/likes/playlists/${encodeURIComponent(playlistUrn)}`, {
          method: next ? 'POST' : 'DELETE',
        });
        setTimeout(() => {
          qc.invalidateQueries({ queryKey: ['likes', 'playlist', playlistUrn] });
          qc.invalidateQueries({ queryKey: ['me', 'likes', 'playlists'] });
        }, 3000);
      } catch {
        setLiked(!next);
        setLocalCount((c) => c + (next ? -1 : 1));
      }
    };

    return (
      <button
        type="button"
        onClick={toggle}
        className={`inline-flex items-center gap-2 px-4 py-2.5 rounded-xl text-sm font-medium transition-all duration-200 ease-[var(--ease-apple)] cursor-pointer ${
          liked
            ? 'bg-accent/15 text-accent border border-accent/20 shadow-[0_0_20px_rgba(255,85,0,0.1)]'
            : 'glass hover:bg-white/[0.05] text-white/60 hover:text-white/80'
        }`}
      >
        <Heart size={16} fill={liked ? 'currentColor' : 'none'} />
        <span className="tabular-nums">{fc(localCount)}</span>
      </button>
    );
  },
);

/* ── Track Row ────────────────────────────────────────────── */

const TrackRow = React.memo(
  function TrackRow({ track, index, queue }: { track: Track; index: number; queue: Track[] }) {
    const navigate = useNavigate();
    const { isThis, isThisPlaying, togglePlay } = useTrackPlay(track, queue);
    const cover = art(track.artwork_url, 't200x200');

    return (
      <div
        className={`group flex items-center gap-3.5 px-4 py-3 rounded-xl transition-all duration-200 ease-[var(--ease-apple)] ${
          isThis ? 'bg-accent/[0.05] ring-1 ring-accent/15' : 'hover:bg-white/[0.03]'
        }`}
      >
        {/* Index / play */}
        <div
          className="w-8 h-8 flex items-center justify-center shrink-0 cursor-pointer"
          onClick={togglePlay}
          onMouseEnter={() => preloadTrack(track.urn)}
        >
          {isThisPlaying ? (
            <div className="w-7 h-7 rounded-full bg-accent flex items-center justify-center shadow-[0_0_12px_var(--color-accent-glow)]">
              {pauseWhite12}
            </div>
          ) : (
            <>
              <span className="text-[12px] text-white/25 tabular-nums font-medium group-hover:hidden">
                {index + 1}
              </span>
              <div className="hidden group-hover:flex w-7 h-7 rounded-full bg-white/10 items-center justify-center">
                {playWhite12}
              </div>
            </>
          )}
        </div>

        {/* Artwork */}
        <div className="relative w-10 h-10 rounded-lg overflow-hidden shrink-0 ring-1 ring-white/[0.06]">
          {cover ? (
            <img src={cover} alt="" className="w-full h-full object-cover" />
          ) : (
            <div className="w-full h-full flex items-center justify-center bg-white/[0.03]">
              {musicIcon12}
            </div>
          )}
        </div>

        {/* Info */}
        <div className="flex-1 min-w-0">
          <p
            className={`text-[13px] font-medium truncate cursor-pointer transition-colors duration-150 ${
              isThis ? 'text-accent' : 'text-white/85 hover:text-white'
            }`}
            onClick={() => navigate(`/track/${encodeURIComponent(track.urn)}`)}
          >
            {track.title}
          </p>
          <p
            className="text-[11px] text-white/30 truncate mt-0.5 cursor-pointer hover:text-white/50 transition-colors duration-150"
            onClick={() => navigate(`/user/${encodeURIComponent(track.user.urn)}`)}
          >
            {track.user.username}
          </p>
        </div>

        {/* Stats */}
        <div className="hidden sm:flex items-center gap-3 shrink-0">
          {track.playback_count != null && (
            <span className="text-[10px] text-white/20 tabular-nums flex items-center gap-0.5">
              {headphones9}
              {fc(track.playback_count)}
            </span>
          )}
          {(track.favoritings_count ?? track.likes_count) != null && (
            <span className="text-[10px] text-white/20 tabular-nums flex items-center gap-0.5">
              {heart9}
              {fc(track.favoritings_count ?? track.likes_count)}
            </span>
          )}
        </div>

        {/* Duration */}
        <span className="text-[11px] text-white/25 tabular-nums font-medium shrink-0 w-10 text-right">
          {dur(track.duration)}
        </span>
      </div>
    );
  },
  (prev, next) => prev.track.urn === next.track.urn && prev.index === next.index,
);

/* ── Main: PlaylistPage ──────────────────────────────────── */

export const PlaylistPage = React.memo(() => {
  const { urn } = useParams<{ urn: string }>();
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { data: playlist, isLoading: playlistLoading } = usePlaylist(urn);
  const {
    tracks: playlistTracks,
    isLoading: tracksLoading,
    hasNextPage,
    isFetchingNextPage,
    fetchNextPage,
  } = usePlaylistTracks(urn);

  const isLoading = playlistLoading || tracksLoading;

  const tracks: Track[] = React.useMemo(() => {
    if (isLoading || !playlist) return [];
    return playlistTracks.length > 0 ? playlistTracks : (playlist.tracks ?? []);
  }, [isLoading, playlist, playlistTracks]);

  const trackUrns = React.useMemo(() => new Set(tracks.map((t) => t.urn)), [tracks]);
  const isPlayingFromThis = usePlayerStore(
    (s) => s.isPlaying && s.currentTrack != null && trackUrns.has(s.currentTrack.urn),
  );
  const isPausedFromThis = usePlayerStore(
    (s) => !s.isPlaying && s.currentTrack != null && trackUrns.has(s.currentTrack.urn),
  );

  const scrollRef = useInfiniteScroll(hasNextPage ?? false, isFetchingNextPage, fetchNextPage);

  if (isLoading || !playlist) {
    return (
      <div className="h-full flex items-center justify-center">
        <Loader2 size={24} className="text-white/15 animate-spin" />
      </div>
    );
  }
  const cover = art(playlist.artwork_url, 't500x500') ?? art(tracks[0]?.artwork_url, 't500x500');

  const handlePlayAll = () => {
    if (tracks.length === 0) return;
    const { play, pause, resume } = usePlayerStore.getState();
    if (isPlayingFromThis) {
      pause();
    } else if (isPausedFromThis) {
      resume();
    } else {
      play(tracks[0], tracks);
    }
  };

  const handleShuffle = () => {
    if (tracks.length === 0) return;
    const { play } = usePlayerStore.getState();
    const shuffled = [...tracks].sort(() => Math.random() - 0.5);
    play(shuffled[0], shuffled);
  };

  return (
    <div className="p-6 pb-4 space-y-7 animate-fade-in-up">
      {/* ── Hero ─────────────────────────────────────── */}
      <section className="relative rounded-3xl overflow-hidden glass-featured">
        {cover && (
          <div className="absolute inset-0 pointer-events-none">
            <img
              src={cover}
              alt=""
              className="w-full h-full object-cover scale-[1.5] blur-[100px] opacity-25 saturate-150"
            />
            <div className="absolute inset-0 bg-gradient-to-r from-[rgb(8,8,10)]/80 via-[rgb(8,8,10)]/60 to-[rgb(8,8,10)]/80" />
          </div>
        )}

        <div className="relative flex items-center gap-7 p-7">
          {/* Artwork */}
          <div
            className="relative w-[200px] h-[200px] rounded-2xl overflow-hidden shrink-0 shadow-2xl ring-1 ring-white/[0.1] cursor-pointer group/cover"
            onClick={handlePlayAll}
          >
            {cover ? (
              <img
                src={cover}
                alt={playlist.title}
                className="w-full h-full object-cover transition-transform duration-500 ease-[var(--ease-apple)] group-hover/cover:scale-[1.04]"
              />
            ) : (
              <div className="w-full h-full flex items-center justify-center bg-gradient-to-br from-white/[0.04] to-white/[0.01]">
                <ListMusic size={48} className="text-white/15" />
              </div>
            )}

            {/* Play overlay */}
            <div
              className={`absolute inset-0 flex items-center justify-center transition-all duration-300 ${
                isPlayingFromThis
                  ? 'bg-black/30 opacity-100'
                  : 'bg-black/0 opacity-0 group-hover/cover:bg-black/30 group-hover/cover:opacity-100'
              }`}
            >
              <div
                className={`w-14 h-14 rounded-full flex items-center justify-center shadow-xl transition-all duration-300 ease-[var(--ease-apple)] ${
                  isPlayingFromThis
                    ? 'bg-white scale-100'
                    : 'bg-white/90 scale-75 group-hover/cover:scale-100'
                }`}
              >
                {isPlayingFromThis ? pauseBlack22 : playBlack22}
              </div>
            </div>

            {/* Track count pill */}
            <div className="absolute bottom-2.5 right-2.5 flex items-center gap-1 text-[10px] font-medium bg-black/50 backdrop-blur-md text-white/70 px-2 py-1 rounded-full">
              <ListMusic size={10} />
              {tracks.length}
            </div>
          </div>

          {/* Info */}
          <div className="flex-1 min-w-0 py-2">
            <span className="inline-block text-[10px] font-semibold px-2.5 py-1 rounded-full bg-white/[0.06] text-white/40 border border-white/[0.06] mb-3 uppercase tracking-wider">
              {playlist.playlist_type || 'Playlist'}
            </span>

            <h1 className="text-2xl font-bold text-white/95 leading-tight mb-2 line-clamp-2">
              {playlist.title}
            </h1>

            {/* Artist */}
            <div
              className="flex items-center gap-2.5 mb-5 cursor-pointer group/artist"
              onClick={() => navigate(`/user/${encodeURIComponent(playlist.user.urn)}`)}
            >
              {playlist.user.avatar_url && (
                <img
                  src={art(playlist.user.avatar_url, 'small') ?? ''}
                  alt=""
                  className="w-6 h-6 rounded-full ring-1 ring-white/[0.08] group-hover/artist:ring-white/[0.15] transition-all duration-150"
                />
              )}
              <span className="text-[14px] text-white/50 group-hover/artist:text-white/70 transition-colors">
                {playlist.user.username}
              </span>
            </div>

            {/* Action buttons */}
            <div className="flex items-center gap-2.5 flex-wrap">
              <button
                type="button"
                onClick={handlePlayAll}
                className={`inline-flex items-center gap-2 px-5 py-2.5 rounded-xl text-sm font-medium transition-all duration-200 ease-[var(--ease-apple)] cursor-pointer shadow-[0_0_20px_var(--color-accent-glow)] ${
                  isPlayingFromThis
                    ? 'bg-white text-black hover:bg-white/90'
                    : 'bg-accent text-white hover:bg-accent-hover active:scale-[0.97]'
                }`}
              >
                {isPlayingFromThis ? pauseCurrent16 : playCurrent16}
                {t('playlist.playAll')}
              </button>

              <button
                type="button"
                onClick={handleShuffle}
                className="inline-flex items-center gap-2 px-4 py-2.5 rounded-xl text-sm font-medium glass hover:bg-white/[0.05] text-white/60 hover:text-white/80 transition-all duration-200 ease-[var(--ease-apple)] cursor-pointer"
              >
                <Shuffle size={16} />
                {t('playlist.shuffle')}
              </button>
              <PlaylistLikeBtn playlistUrn={playlist.urn} count={playlist.likes_count} />
              <CopyLinkButton url={playlist.permalink_url} />
            </div>
          </div>
        </div>
      </section>

      {/* ── Stats bar ────────────────────────────────── */}
      <section className="flex items-center gap-5 px-1 flex-wrap">
        <div className="flex items-center gap-1.5 text-[12px] text-white/30">
          <ListMusic size={13} className="text-white/20" />
          <span className="tabular-nums font-medium">{tracks.length}</span>
          <span className="text-white/15">{t('search.tracks').toLowerCase()}</span>
        </div>
        {playlist.likes_count != null && (
          <div className="flex items-center gap-1.5 text-[12px] text-white/30">
            <Heart size={13} className="text-white/20" />
            <span className="tabular-nums font-medium">{fc(playlist.likes_count)}</span>
            <span className="text-white/15">{t('track.likes')}</span>
          </div>
        )}
        <div className="flex items-center gap-1.5 text-[12px] text-white/25 ml-auto">
          <Clock size={12} />
          <span className="tabular-nums">{durLong(playlist.duration)}</span>
        </div>
        <div className="flex items-center gap-1.5 text-[12px] text-white/20">
          <Calendar size={12} />
          <span>{dateFormatted(playlist.created_at)}</span>
        </div>
      </section>

      {/* ── Description ──────────────────────────────── */}
      {playlist.description && (
        <section className="glass rounded-2xl p-5">
          <p className="text-[13px] text-white/45 leading-relaxed whitespace-pre-wrap break-words">
            {playlist.description}
          </p>
        </section>
      )}

      {/* ── Track list ───────────────────────────────── */}
      <section>
        {tracks.length === 0 ? (
          <div className="text-center py-12">
            <ListMusic size={32} className="text-white/10 mx-auto mb-3" />
            <p className="text-[13px] text-white/20">{t('playlist.noTracks')}</p>
          </div>
        ) : (
          <div className="space-y-0.5">
            {/* Header */}
            <div className="flex items-center gap-3.5 px-4 py-2 text-[10px] text-white/20 uppercase tracking-wider font-medium">
              <span className="w-8 text-center">#</span>
              <span className="w-10" />
              <span className="flex-1">Title</span>
              <span className="hidden sm:block w-[100px]" />
              <span className="w-10 text-right">
                <Clock size={10} className="inline" />
              </span>
            </div>
            <div className="h-px bg-white/[0.04] mx-4 mb-1" />

            {tracks.map((track, i) => (
              <TrackRow key={track.urn} track={track} index={i} queue={tracks} />
            ))}
            {hasNextPage && (
              <div ref={scrollRef} className="flex justify-center py-4">
                {isFetchingNextPage && <Loader2 size={20} className="animate-spin text-white/30" />}
              </div>
            )}
          </div>
        )}
      </section>
    </div>
  );
});
