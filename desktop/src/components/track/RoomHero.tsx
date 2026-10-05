import React from 'react';
import { useNavigate } from 'react-router-dom';
import { ago } from '../../lib/formatters';
import type { Comment } from '../../lib/hooks';
import { Pause, Play } from '../../lib/icons';
import {
  getArtistDisplay,
  getArtistLinkItems,
  getDisplayTitle,
  getParticipants,
} from '../../lib/track-display';
import type { Track } from '../../stores/player';
import { ArtistNameLinks } from '../music/ArtistNameLinks';
import { TrackStatusBadges } from '../music/TrackStatusBadges';
import { ArtistLinks } from './ArtistLinks';
import { CommentForm } from './comments';
import { RoomFloor } from './RoomFloor';
import { TrackActionRail } from './TrackActionRail';
import type { TrackAura } from './useTrackAura';

const KIND_TONE: Record<string, string> = {
  original: 'bg-emerald-500/15 text-emerald-300/90',
  demo: 'bg-sky-500/15 text-sky-300/90',
  alt: 'bg-violet-500/15 text-violet-300/90',
  reupload: 'bg-amber-500/12 text-amber-300/80',
  cover: 'bg-fuchsia-500/15 text-fuchsia-300/90',
};

const KIND_LABEL: Record<string, string> = {
  original: 'original',
  demo: 'demo',
  alt: 'alt',
  reupload: 're-upload',
  cover: 'cover',
};

const Dot = () => <span className="text-white/20">{'·'}</span>;

/** The hero's left column, laid out like SoundCloud's: play control inline
 *  with the title + meta, the waveform under it with the comment avatars on
 *  its edge, and the action row (like / comment / utilities) below. The
 *  artwork lives in the page's right column. */
export const RoomHero = React.memo(function RoomHero({
  track,
  aura,
  isThis,
  isThisPlaying,
  isOwner,
  comments,
  commentAt,
  onPlay,
  onSeek,
  onCommentPosition,
  onCommentCommitted,
}: {
  track: Track;
  aura: TrackAura;
  isThis: boolean;
  isThisPlaying: boolean;
  isOwner: boolean;
  comments: Comment[];
  commentAt: number | null;
  onPlay: () => void;
  onSeek: (seconds: number) => void;
  onCommentPosition: (positionMs: number) => void;
  onCommentCommitted: () => void;
}) {
  const navigate = useNavigate();

  const title = getDisplayTitle(track) || 'Untitled';
  const ad = getArtistDisplay(track);
  const participants = getParticipants(track, ['remixer', 'producer']);
  const artistLinks = getArtistLinkItems(track);
  const mainLink = artistLinks.slice(0, 1);
  const featLinks = artistLinks.slice(1);
  const year = track.release_year ?? track.enrichment?.album?.year;

  const titleStyle = aura.hasGenre
    ? {
        background: aura.aura.nameGradient,
        WebkitBackgroundClip: 'text',
        backgroundClip: 'text',
        WebkitTextFillColor: 'transparent',
        filter: 'drop-shadow(0 6px 22px rgba(0,0,0,0.5))',
      }
    : { color: 'rgba(255,255,255,0.96)', textShadow: '0 6px 22px rgba(0,0,0,0.5)' };

  return (
    <section className="relative" style={{ isolation: 'isolate' }}>
      <div className="flex items-center gap-5">
        <button
          type="button"
          onClick={onPlay}
          aria-label={isThisPlaying ? 'Pause' : 'Play'}
          className="w-[68px] h-[68px] shrink-0 rounded-full border border-white/[0.18] hover:border-white/[0.4] flex items-center justify-center text-white/90 hover:text-white transition-colors cursor-pointer"
        >
          {isThisPlaying ? (
            <Pause size={24} fill="currentColor" strokeWidth={0} />
          ) : (
            <Play size={24} fill="currentColor" strokeWidth={0} className="ml-1" />
          )}
        </button>

        <div className="flex-1 min-w-0">
          <h1
            className="text-3xl md:text-4xl xl:text-5xl font-black leading-[1.02] tracking-tighter break-words"
            style={titleStyle}
          >
            {title}
          </h1>

          <div className="mt-2.5 flex flex-wrap items-center gap-x-2 gap-y-1.5 text-[12.5px]">
            <ArtistNameLinks
              items={mainLink}
              linkClassName="font-semibold text-white/85 hover:text-white cursor-pointer transition-colors"
            />
            {ad.isEnriched && ad.verified && (
              <span
                className="text-[11px] text-emerald-400/80"
                title={`Verified artist (confidence ${(ad.confidence ?? 0).toFixed(2)})`}
              >
                ✓
              </span>
            )}
            {ad.isEnriched && !ad.verified && (
              <span
                className="text-[11px] text-amber-400/70"
                title={`Parsed from title, not externally verified (confidence ${(ad.confidence ?? 0).toFixed(2)})`}
              >
                ?
              </span>
            )}
            {ad.uploadKind && (
              <span
                className={`px-1.5 py-0.5 rounded-md text-[9px] tracking-wider font-semibold ${
                  KIND_TONE[ad.uploadKind] ?? 'bg-white/[0.06] text-white/50'
                }`}
              >
                {KIND_LABEL[ad.uploadKind] ?? ad.uploadKind}
              </span>
            )}
            {track.created_at && (
              <>
                <Dot />
                <span className="text-white/40">{ago(track.created_at)}</span>
              </>
            )}
            {track.genre && (
              <>
                <Dot />
                <span
                  className="text-[11px] font-semibold text-white/55"
                  style={{ color: aura.hasGenre ? aura.accent : undefined }}
                >
                  {track.genre}
                </span>
              </>
            )}
            {year && (
              <>
                <Dot />
                <span className="text-[11px] font-semibold text-white/40 tabular-nums">{year}</span>
              </>
            )}
            <TrackStatusBadges meta={track._scd_meta} />
          </div>

          {(featLinks.length > 0 || participants || ad.uploader) && (
            <div className="mt-1.5 text-[12px] text-white/40 flex flex-wrap items-center gap-x-1.5 gap-y-0.5">
              {featLinks.length > 0 && (
                <span>
                  {'feat.'}{' '}
                  <ArtistNameLinks
                    items={featLinks}
                    linkClassName="cursor-pointer text-white/55 hover:text-white/85 transition-colors"
                  />
                </span>
              )}
              {participants?.remixers && participants.remixers.length > 0 && (
                <span>
                  {featLinks.length > 0 && '· '}
                  <ArtistLinks artists={participants.remixers} /> {'Remix'}
                </span>
              )}
              {participants?.producers && participants.producers.length > 0 && (
                <span>
                  {(featLinks.length > 0 || participants.remixers.length > 0) && '· '}
                  {'prod.'} <ArtistLinks artists={participants.producers} />
                </span>
              )}
              {(featLinks.length > 0 || participants) && ad.uploader && <span>·</span>}
              {ad.uploader && (
                <span>
                  uploaded by{' '}
                  <span
                    className="text-white/55 hover:text-white/80 cursor-pointer transition-colors"
                    onClick={() => navigate(`/user/${encodeURIComponent(track.user.urn)}`)}
                  >
                    {ad.uploader}
                  </span>
                </span>
              )}
            </div>
          )}
        </div>
      </div>

      <div className="mt-6 pt-5 border-t border-white/[0.07]">
        <RoomFloor
          track={track}
          isCurrent={isThis}
          comments={comments}
          aura={aura}
          onSeek={onSeek}
          commentAt={commentAt}
          onCommentPosition={onCommentPosition}
        />
      </div>

      <div className="mt-5">
        <TrackActionRail track={track} isOwner={isOwner}>
          <CommentForm
            trackUrn={track.urn}
            isCurrent={isThis}
            accent={aura.accent}
            pendingAt={commentAt}
            onCommitted={onCommentCommitted}
          />
        </TrackActionRail>
      </div>
    </section>
  );
});
