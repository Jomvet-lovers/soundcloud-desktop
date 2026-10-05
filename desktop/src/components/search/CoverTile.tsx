import { Compass, Pause, Play, Sparkles } from 'lucide-react';
import { memo } from 'react';
import { preloadTrack } from '../../lib/audio';
import {
  hardStopHoverPreview,
  PREVIEW_WINDOW_MS,
  startHoverPreview,
  stopHoverPreview,
  useIsPreviewActive,
} from '../../lib/audioPreview';
import { art } from '../../lib/formatters';
import { useArtistDisplay, useDisplayTitle } from '../../lib/track-display';
import { useTrackPlay } from '../../lib/useTrackPlay';
import type { Track } from '../../stores/player';
import type { WallItem } from './utils';

interface CoverTileProps {
  item: WallItem;
  /** Stable thunk → live play queue, resolved lazily so tile memo isn't broken. */
  getQueue: () => Track[];
  onDive?: (track: Track) => void;
}

export const CoverTile = memo(function CoverTile({ item, getQueue, onDive }: CoverTileProps) {
  const { track, kind, matchedLine, hero } = item;
  const displayTitle = useDisplayTitle(track);
  const artistDisplay = useArtistDisplay(track);
  const { isThis, isThisPlaying, togglePlay } = useTrackPlay(track, getQueue);
  const previewing = useIsPreviewActive(track.urn);

  const cover = art(track.artwork_url, hero ? 't500x500' : 't300x300');
  const span = hero ? 'span 2' : 'span 1';

  const enter = () => {
    preloadTrack(track.urn);
    startHoverPreview(track.urn);
  };
  const leave = () => stopHoverPreview();
  const activate = () => {
    hardStopHoverPreview();
    togglePlay();
  };

  return (
    <div
      className={`tg-tile group relative${kind === 'vibe' ? ' tg-vibe' : ''}`}
      style={{
        gridColumn: span,
        gridRow: span,
      }}
    >
      {kind === 'vibe' && (
        <div
          className="absolute inset-0 rounded-2xl pointer-events-none opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          style={{ boxShadow: 'inset 0 0 0 1.5px var(--color-accent)' }}
        />
      )}

      <div className="relative w-full h-full">
        <div
          role="button"
          tabIndex={0}
          onMouseEnter={enter}
          onMouseLeave={leave}
          onClick={activate}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') {
              e.preventDefault();
              activate();
            }
          }}
          className="tg-lift relative block w-full h-full rounded-2xl overflow-hidden cursor-pointer bg-white/[0.03]"
        >
          {cover ? (
            <img
              src={cover}
              alt=""
              loading="lazy"
              decoding="async"
              draggable={false}
              className="absolute inset-0 w-full h-full object-cover transition-transform duration-700 group-hover:scale-[1.08]"
            />
          ) : (
            <div className="absolute inset-0 bg-white/[0.04]" />
          )}

          {/* Lyric hit: the matched line is the signal — a pull-quote over the cover. */}
          {matchedLine ? (
            <div
              className="absolute inset-x-0 bottom-0 px-3 pt-8 pb-3 pointer-events-none"
              style={{ background: 'linear-gradient(to top, rgba(0,0,0,0.82), transparent)' }}
            >
              <p
                className="font-serif italic text-white/95 leading-snug border-l-2 pl-2 line-clamp-3"
                style={{ borderColor: 'var(--color-accent)', fontSize: hero ? '17px' : '12px' }}
              >
                {matchedLine}
              </p>
            </div>
          ) : (
            <div
              className="absolute inset-x-0 bottom-0 px-2.5 pt-8 pb-2.5 opacity-0 group-hover:opacity-100 transition-opacity duration-500 pointer-events-none"
              style={{ background: 'linear-gradient(to top, rgba(0,0,0,0.78), transparent)' }}
            >
              <p
                className={`truncate font-semibold text-white ${hero ? 'text-sm' : 'text-[12px]'}`}
              >
                {displayTitle}
              </p>
              <p className="truncate text-[11px] text-white/55">{artistDisplay.primary}</p>
            </div>
          )}

          {/* AI-vibe glyph (subtle at rest, firms on hover) */}
          {kind === 'vibe' && (
            <div className="absolute top-2 left-2 text-accent/70 group-hover:text-accent transition-colors duration-300">
              <Sparkles size={hero ? 16 : 12} />
            </div>
          )}

          {/* Play affordance */}
          <div
            className={`absolute top-2 right-2 flex items-center justify-center rounded-full transition-all duration-300 ${
              isThisPlaying ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'
            }`}
            style={{
              width: hero ? 40 : 30,
              height: hero ? 40 : 30,
              background: 'rgba(0,0,0,0.62)',
              border: '0.5px solid rgba(255,255,255,0.18)',
            }}
          >
            {isThisPlaying ? (
              <Pause size={hero ? 18 : 14} className="text-white" fill="currentColor" />
            ) : (
              <Play
                size={hero ? 18 : 14}
                className="text-white translate-x-[1px]"
                fill="currentColor"
              />
            )}
          </div>

          {/* Hero rabbit-hole: dive into the vibe of this track */}
          {hero && onDive && (
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                hardStopHoverPreview();
                onDive(track);
              }}
              className="absolute bottom-2 right-2 flex items-center gap-1 h-7 px-2.5 rounded-full text-[11px] text-white/85 opacity-0 group-hover:opacity-100 transition-all duration-300 cursor-pointer"
              style={{
                background: 'rgba(0,0,0,0.62)',
                border: '0.5px solid rgba(255,255,255,0.18)',
              }}
            >
              <Compass size={12} />
              {'Similar'}
            </button>
          )}
        </div>

        {/* Rings live OUTSIDE the hover-scaled .tg-lift so the 15s progress arc
            keeps a fixed, readable size on small tiles instead of inflating. */}
        {isThis && (
          <div
            className="absolute inset-0 rounded-2xl pointer-events-none"
            style={{
              boxShadow: 'inset 0 0 0 2px var(--color-accent)',
            }}
          />
        )}
        {previewing && (
          <svg
            className="absolute inset-0 w-full h-full pointer-events-none"
            viewBox="0 0 100 100"
            preserveAspectRatio="none"
            aria-hidden
          >
            <rect
              x="1.5"
              y="1.5"
              width="97"
              height="97"
              rx="7"
              fill="none"
              stroke="var(--color-accent)"
              strokeWidth="2.6"
              pathLength={1000}
              strokeDasharray={1000}
              style={
                {
                  '--tg-ring-len': '1000',
                  strokeDashoffset: 1000,
                  animation: `tg-ring-sweep ${PREVIEW_WINDOW_MS}ms linear forwards`,
                } as React.CSSProperties
              }
            />
          </svg>
        )}
      </div>
    </div>
  );
});
