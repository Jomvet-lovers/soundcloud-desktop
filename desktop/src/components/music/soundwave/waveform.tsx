import React, { useEffect, useMemo, useRef } from 'react';
import { getCurrentTime, getDuration, seek, subscribe } from '../../../lib/audio';
import { type PerfMode, usePerfMode } from '../../../lib/perf';
import { useTrackWaveform } from '../../../lib/waveform';
import type { Track } from '../../../stores/player';

const BAR_COUNT = 380;

/** Rendered bars per mode (drawn twice: muted + accent layer). */
function barsForMode(mode: PerfMode): number {
  if (mode === 'light') return 150;
  if (mode === 'medium') return 260;
  return BAR_COUNT;
}

/** Map SC waveform samples to bar heights (0..1), ONE sample per bar.
 *  The official wave is a straight decimation of the sample array: picking
 *  the bucket peak instead flattens alternating samples into a level top,
 *  which is why the spikes disappeared. */
function downsample(samples: number[], height: number, count: number): number[] {
  if (!samples.length) return new Array(count).fill(0.25);
  const out = new Array<number>(count);
  for (let i = 0; i < count; i++) {
    const idx = Math.min(samples.length - 1, Math.floor((i * samples.length) / count));
    out[i] = Math.min(1, samples[idx] / height);
  }
  return out;
}

/** Decorative fallback pattern used during load / when SC has no waveform. */
function fallbackBars(count: number): number[] {
  const arr = new Array<number>(count);
  for (let i = 0; i < count; i++) {
    const x = i / count;
    const base = 0.35 + 0.28 * Math.sin(x * Math.PI * 2);
    const detail = 0.18 * Math.sin(x * Math.PI * 14 + 1.3);
    arr[i] = Math.max(0.06, Math.min(0.88, base + detail));
  }
  return arr;
}

interface Props {
  /** Track whose waveform to render; null → idle/fallback pattern. */
  track: Track | null;
  /** Whether `track` is the one currently loaded in the audio engine. */
  isCurrent: boolean;
  /** Click on the dimmed lower lane: pick where to comment (no seek). */
  onCommentPosition?: (positionMs: number) => void;
}

/**
 * Progress-bearing waveform. Bars are drawn twice (muted + accent); the accent
 * layer is clipped by `--sw-progress` which we update via DOM refs on each
 * audio tick — no React re-renders while the track plays.
 */
export const LiveWaveform = React.memo(
  function LiveWaveform({ track, isCurrent, onCommentPosition }: Props) {
    const { data: samples } = useTrackWaveform(track);
    const { mode } = usePerfMode();
    const barCount = barsForMode(mode);

    const bars = useMemo(() => {
      if (!samples) return fallbackBars(barCount);
      return downsample(samples.values, samples.height, barCount);
    }, [samples, barCount]);

    const rootRef = useRef<HTMLDivElement>(null);
    const hintRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
      if (!isCurrent) {
        if (rootRef.current) rootRef.current.style.setProperty('--sw-progress', '0%');
        if (hintRef.current) hintRef.current.style.left = '0%';
        return;
      }
      const paint = () => {
        const t = getCurrentTime();
        const d = getDuration();
        const pct = d > 0 ? Math.min(100, Math.max(0, (t / d) * 100)) : 0;
        if (rootRef.current) rootRef.current.style.setProperty('--sw-progress', `${pct}%`);
        if (hintRef.current) hintRef.current.style.left = `${pct}%`;
      };
      paint();
      return subscribe(paint);
    }, [isCurrent]);

    const handleBarClick = (e: React.MouseEvent<HTMLDivElement>) => {
      if (!isCurrent) return;
      const rect = e.currentTarget.getBoundingClientRect();
      const pct = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
      const d = getDuration();
      if (d > 0) seek(pct * d);
    };

    /** Lower lane click: pin where to comment — the playhead stays put. */
    const handleCommentClick = (e: React.MouseEvent<HTMLDivElement>) => {
      if (!onCommentPosition) return;
      e.stopPropagation();
      const rect = e.currentTarget.getBoundingClientRect();
      const pct = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
      const d = getDuration() || (track?.duration ?? 0) / 1000;
      if (d > 0) onCommentPosition(Math.round(pct * d * 1000));
    };

    return (
      <div
        ref={rootRef}
        className={`sw-bars relative w-full h-[104px] ${isCurrent ? 'cursor-pointer' : 'cursor-default'}`}
        onClick={handleBarClick}
      >
        <div className="sw-layer-muted absolute inset-y-0 left-0 right-10 flex items-center gap-px">
          {bars.map((v, i) => (
            <div key={i} className="sw-bar flex-1" style={{ height: `${v * 100}%` }} />
          ))}
        </div>
        <div className="sw-layer-accent absolute inset-y-0 left-0 right-10 flex items-center gap-px">
          {bars.map((v, i) => (
            <div key={i} className="sw-bar flex-1" style={{ height: `${v * 100}%` }} />
          ))}
        </div>
        {/* Lower lane: dims the mirrored half; clicking pins a comment spot
            there without seeking. */}
        <div
          className="absolute left-0 right-0 top-1/2 bottom-0 cursor-crosshair"
          style={{ background: 'rgba(0,0,0,0.45)' }}
          onClick={handleCommentClick}
        />
        {/* Center separator line, like the official wave (contrastText @ 50%). */}
        <div
          className="pointer-events-none absolute left-0 right-10 top-1/2 -translate-y-1/2 h-px"
          style={{ background: 'rgba(255,255,255,0.45)' }}
        />
        {isCurrent && (
          <div
            ref={hintRef}
            className="absolute top-0 bottom-0 w-px pointer-events-none"
            style={{
              left: '0%',
              transform: 'translateX(-50%)',
              background: 'rgba(255,255,255,0.9)',
            }}
          />
        )}
      </div>
    );
  },
  (prev, next) =>
    prev.track?.urn === next.track?.urn &&
    prev.isCurrent === next.isCurrent &&
    prev.onCommentPosition === next.onCommentPosition,
);
