import React, { useEffect, useRef } from 'react';
import { getCurrentTime, subscribe } from '../../lib/audio';
import { art, durLong } from '../../lib/formatters';
import type { Comment } from '../../lib/hooks';
import { useAuthStore } from '../../stores/auth';
import type { Track } from '../../stores/player';
import { LiveWaveform } from '../music/soundwave/waveform';
import type { TrackAura } from './useTrackAura';
import { FloatingComments, WaveVoices } from './WaveVoices';

/** The floor of the room: the live waveform, recolored to the track's own hue
 *  (scoped --color-accent), with voices plotted on it and a time ruler. */
export const RoomFloor = React.memo(function RoomFloor({
  track,
  isCurrent,
  comments,
  aura,
  onSeek,
  commentAt,
  onCommentPosition,
}: {
  track: Track;
  isCurrent: boolean;
  comments: Comment[];
  aura: TrackAura;
  onSeek: (seconds: number) => void;
  /** Pending comment position (ms) picked on the lower lane, if any. */
  commentAt: number | null;
  onCommentPosition: (positionMs: number) => void;
}) {
  const elapsedRef = useRef<HTMLSpanElement>(null);
  const myUser = useAuthStore((s) => s.user);

  useEffect(() => {
    if (!isCurrent) {
      if (elapsedRef.current) elapsedRef.current.textContent = '0:00';
      return;
    }
    const paint = () => {
      if (elapsedRef.current)
        elapsedRef.current.textContent = durLong(Math.floor(getCurrentTime() * 1000));
    };
    paint();
    return subscribe(paint);
  }, [isCurrent]);

  const durationMs = track.full_duration ?? track.duration;
  const playableFrac = durationMs > 0 ? Math.min(1, track.duration / durationMs) : 1;
  const previewTail = track.access === 'preview' && playableFrac < 0.995 ? 1 - playableFrac : 0;

  return (
    <div
      className="relative"
      style={
        {
          '--color-accent': aura.accent,
          '--color-accent-glow': aura.accentGlow,
        } as React.CSSProperties
      }
    >
      <div className="relative w-full">
        <LiveWaveform track={track} isCurrent={isCurrent} onCommentPosition={onCommentPosition} />
        {previewTail > 0 && (
          <div
            className="absolute inset-y-0 right-0 pointer-events-none rounded-r-lg bg-gradient-to-r from-transparent via-black/55 via-40% to-black/70"
            style={{
              width: `${previewTail * 100}%`,
            }}
            title={'Preview only'}
          />
        )}
        {/* Comment layers share the bars' box (right gutter excluded) so a dot
            or pin sits exactly where its timestamp lies on the wave. */}
        <div className="pointer-events-none absolute inset-y-0 left-0 right-10 z-10">
          <WaveVoices
            comments={comments}
            durationMs={durationMs}
            isCurrent={isCurrent}
            onSeek={onSeek}
          />
          <FloatingComments comments={comments} durationMs={durationMs} isCurrent={isCurrent} />
        </div>
        {commentAt != null && durationMs > 0 && (
          <div className="pointer-events-none absolute inset-y-0 left-0 right-10">
            <div
              className="absolute top-[75%] -translate-x-1/2 -translate-y-1/2"
              style={{
                left: `${Math.min(1, Math.max(0, commentAt / durationMs)) * 100}%`,
              }}
            >
              {myUser?.avatar_url ? (
                <img
                  src={art(myUser.avatar_url, 'small') ?? ''}
                  alt=""
                  className="w-6 h-6 rounded-full object-cover"
                />
              ) : (
                <span className="w-6 h-6 rounded-full bg-white/15 flex items-center justify-center text-[10px] font-semibold text-white/70">
                  {(myUser?.username || '?').slice(0, 1).toUpperCase()}
                </span>
              )}
            </div>
          </div>
        )}
        <div className="pointer-events-none absolute right-0 top-0 bottom-0 flex flex-col items-end text-[11px] tabular-nums text-white/45">
          <div className="flex flex-1 items-end">
            <span ref={elapsedRef}>0:00</span>
          </div>
          <div className="flex flex-1 items-start">
            <span>{durLong(track.duration)}</span>
          </div>
        </div>
        {previewTail > 0 && (
          <span className="pointer-events-none absolute bottom-1 left-1 text-[9px] text-white/25">
            {'Preview only'}
          </span>
        )}
      </div>
    </div>
  );
});
