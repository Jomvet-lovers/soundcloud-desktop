import { useQuery } from '@tanstack/react-query';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { ROOM_KEYFRAMES } from '../components/track/keyframes';
import { LinerNotes } from '../components/track/LinerNotes';
import { RoomHero } from '../components/track/RoomHero';
import { RoomSleeve } from '../components/track/RoomSleeve';
import { RoomVoices } from '../components/track/RoomVoices';
import { TrackCover } from '../components/track/TrackCover';
import { useTrackAura } from '../components/track/useTrackAura';
import { api } from '../lib/api';
import { seek } from '../lib/audio';
import { art } from '../lib/formatters';
import {
  useInfiniteScroll,
  useRelatedTracks,
  useTrackComments,
  useTrackFavoriters,
} from '../lib/hooks';
import { ChevronLeft, Loader2 } from '../lib/icons';
import { setLikedUrn } from '../lib/likes';
import { useScdMeta } from '../lib/scdMeta';
import { getArtistDisplay, getDisplayTitle } from '../lib/track-display';
import { useAuthStore } from '../stores/auth';
import { type Track, usePlayerStore } from '../stores/player';

function HeroSkeleton() {
  return (
    <div className="relative">
      <div className="flex flex-col lg:flex-row gap-8">
        <div className="w-[180px] h-[180px] md:w-[220px] md:h-[220px] rounded-[2.2rem] skeleton-shimmer shrink-0 self-center lg:self-start" />
        <div className="flex-1 space-y-4 w-full">
          <div className="h-4 w-40 rounded-full skeleton-shimmer" />
          <div className="h-12 w-3/4 rounded-2xl skeleton-shimmer" />
          <div className="h-5 w-48 rounded-full skeleton-shimmer" />
          <div className="h-11 w-64 rounded-full skeleton-shimmer mt-6" />
        </div>
      </div>
      <div className="h-[96px] mt-8 rounded-xl skeleton-shimmer" />
    </div>
  );
}

export const TrackPage = React.memo(function TrackPage() {
  const { urn } = useParams<{ urn: string }>();
  const navigate = useNavigate();

  const {
    data: track,
    isLoading,
    isError,
  } = useQuery({
    queryKey: ['track', urn],
    queryFn: () => api<Track>(`/tracks/${encodeURIComponent(urn!)}`),
    enabled: !!urn,
    staleTime: 30_000,
  });

  const {
    comments,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
    isLoading: commentsLoading,
  } = useTrackComments(urn);
  const commentsSentinel = useInfiniteScroll(hasNextPage, isFetchingNextPage, fetchNextPage);

  const { data: relatedData, isLoading: relatedLoading } = useRelatedTracks(urn, 10);
  const { data: favoritersData } = useTrackFavoriters(urn, 12);

  const relatedRaw = useMemo(() => relatedData?.collection ?? [], [relatedData]);
  const related = useScdMeta(relatedRaw);
  const favoriters = useMemo(() => favoritersData?.collection ?? [], [favoritersData]);

  const trackUrn = track?.urn;
  const isThis = usePlayerStore((s) => !!trackUrn && s.currentTrack?.urn === trackUrn);
  const isThisPlaying = usePlayerStore(
    (s) => !!trackUrn && s.currentTrack?.urn === trackUrn && s.isPlaying,
  );

  const aura = useTrackAura(track?.genre);
  const myUrn = useAuthStore((s) => s.user?.urn);

  // Position (ms) picked on the waveform's lower lane — pins the comment
  // there without moving the playhead. Keyed by URN so it resets per track.
  const [commentPin, setCommentPin] = useState<{ urn: string | undefined; ms: number | null }>({
    urn,
    ms: null,
  });
  const commentAt = commentPin.urn === urn ? commentPin.ms : null;
  const handleCommentPosition = useCallback(
    (positionMs: number) => setCommentPin({ urn, ms: positionMs }),
    [urn],
  );
  const handleCommentCommitted = useCallback(() => setCommentPin({ urn, ms: null }), [urn]);

  useEffect(() => {
    if (track?.user_favorite && track.urn) setLikedUrn(track.urn, true);
  }, [track?.urn, track?.user_favorite]);

  const handlePlay = useCallback(() => {
    if (!track) return;
    const st = usePlayerStore.getState();
    if (st.currentTrack?.urn === track.urn) {
      if (st.isPlaying) st.pause();
      else st.resume();
    } else {
      st.play(track, [track]);
    }
  }, [track]);

  // Jump into the song from a comment: seek when it's already loaded, else
  // start it (and its voices begin to rise as the playhead sweeps).
  const jumpTo = useCallback(
    (seconds: number) => {
      if (!track) return;
      const st = usePlayerStore.getState();
      if (st.currentTrack?.urn === track.urn) seek(seconds);
      else st.play(track, [track]);
    },
    [track],
  );

  if (isLoading || (!track && !isError)) {
    return (
      <div className="relative min-h-full w-full">
        <style>{ROOM_KEYFRAMES}</style>
        <div
          className="relative z-10 max-w-[1320px] mx-auto px-4 md:px-8 pt-5 pb-10"
          style={{ isolation: 'isolate' }}
        >
          <HeroSkeleton />
        </div>
      </div>
    );
  }

  if (!track) {
    return (
      <div className="relative min-h-full w-full flex items-center justify-center">
        <div className="relative z-10 flex flex-col items-center gap-4 text-center px-6">
          <Loader2 size={22} className="text-white/15" />
          <p className="text-white/40 text-sm">{'Failed to load track'}</p>
          <button
            type="button"
            onClick={() => navigate(-1)}
            className="inline-flex items-center gap-1.5 h-9 pl-2.5 pr-4 rounded-full text-[12px] text-white/70 hover:text-white transition-colors cursor-pointer"
            style={{
              background: 'rgba(255,255,255,0.05)',
              border: '0.5px solid rgba(255,255,255,0.1)',
            }}
          >
            <ChevronLeft size={14} />
            {'Back'}
          </button>
        </div>
      </div>
    );
  }

  const isOwner = !!myUrn && track.user?.urn === myUrn;
  const displayTitle = getDisplayTitle(track) || 'Untitled';
  const artistDisplay = getArtistDisplay(track);
  const cover = art(track.artwork_url, 't500x500');

  return (
    <div className="relative min-h-full w-full">
      <style>{ROOM_KEYFRAMES}</style>

      <div
        className="relative z-10 max-w-[1320px] mx-auto px-4 md:px-8 pt-5 pb-10 space-y-6"
        style={{ isolation: 'isolate' }}
      >
        <div className="flex items-center justify-between">
          <button
            type="button"
            onClick={() => navigate(-1)}
            className="inline-flex items-center justify-center w-9 h-9 rounded-full text-white/55 hover:text-white hover:bg-white/[0.06] transition-all duration-200 cursor-pointer"
            aria-label={'Back'}
          >
            <ChevronLeft size={18} />
          </button>
          {aura.hasGenre && (
            <span className="text-[10px] text-white/20">{'A room tuned to this track'}</span>
          )}
        </div>

        <div className="grid grid-cols-1 lg:grid-cols-[minmax(0,1fr)_336px] gap-6 lg:gap-8 items-start">
          {/* Right column: artwork + artist / stats / fans / related.
              `contents` on narrow windows so the artwork can lead the page. */}
          <div className="contents lg:flex lg:flex-col lg:gap-6 lg:col-start-2 lg:row-start-1">
            <div className="order-1 lg:order-none w-full max-w-[320px] self-center lg:self-auto lg:max-w-none">
              <TrackCover
                title={displayTitle}
                coverUrl={cover ?? undefined}
                aura={aura.aura}
                verified={artistDisplay.isEnriched && artistDisplay.verified}
                sizeClassName="w-[260px] h-[260px] lg:w-[336px] lg:h-[336px]"
              />
            </div>
            <div className="order-3 lg:order-none">
              <RoomSleeve
                track={track}
                favoriters={favoriters}
                related={related}
                relatedLoading={relatedLoading}
                aura={aura}
              />
            </div>
          </div>

          {/* Left column: hero, liner notes, comments. */}
          <div className="order-2 lg:order-none lg:col-start-1 lg:row-start-1 min-w-0 space-y-6">
            <RoomHero
              track={track}
              aura={aura}
              isThis={isThis}
              isThisPlaying={isThisPlaying}
              isOwner={isOwner}
              comments={comments}
              commentAt={commentAt}
              onPlay={handlePlay}
              onSeek={jumpTo}
              onCommentPosition={handleCommentPosition}
              onCommentCommitted={handleCommentCommitted}
            />
            <LinerNotes track={track} aura={aura} />
            <RoomVoices
              commentCount={track.comment_count}
              comments={comments}
              loading={commentsLoading}
              fetchingMore={isFetchingNextPage}
              sentinelRef={commentsSentinel}
              aura={aura}
              onSeek={jumpTo}
            />
          </div>
        </div>
      </div>
    </div>
  );
});
