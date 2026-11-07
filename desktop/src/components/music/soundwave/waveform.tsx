import React, { useEffect, useRef } from 'react';
import { getCurrentTime, getDuration, seek, subscribe } from '../../../lib/audio';
import { usePerfMode } from '../../../lib/perf';
import { useTrackWaveform } from '../../../lib/waveform';
import type { Track } from '../../../stores/player';
import { useSettingsStore } from '../../../stores/settings';

/** The right gutter holds the time readout, so the bars stop short of it —
 *  clicks and the playhead must map onto the same width the bars occupy. */
const WAVE_GUTTER_PX = 40;

/** Where `clientX` sits across the bar area (0..1), gutter excluded. */
function wavePct(el: HTMLElement | null, clientX: number): number {
  if (!el) return 0;
  const rect = el.getBoundingClientRect();
  const width = Math.max(1, rect.width - WAVE_GUTTER_PX);
  return Math.min(1, Math.max(0, (clientX - rect.left) / width));
}

/** Decorative fallback pattern used during load / when SC has no waveform. */
function fallbackBars(count: number): number[] {
  const arr = new Array<number>(count);
  for (let i = 0; i < count; i++) {
    const x = i / count;
    const base = 0.35 + 0.28 * Math.sin(x * Math.PI * 2);
    const detail = 0.18 * Math.sin(x * Math.PI * 14 + 1.3);
    arr[i] = Math.max(0.22, Math.min(0.95, base + detail));
  }
  return arr;
}

/** Built once — the loading pattern never changes. */
const FALLBACK_BARS = fallbackBars(160);

/**
 * Paint the wave at the sample resolution instead of a handful of fixed bars.
 * SoundCloud's own waveform is one column per sample (1800 of them) drawn at
 * the raw value; averaging them into a few hundred buckets smooths away the
 * transients that give the wave its shape. We draw one device-pixel column per
 * output pixel, taking the peak of the samples that fall into that column, so
 * a 1800-sample wave keeps its detail even in a ~1000 px wide player.
 */
function paintWave(
  canvas: HTMLCanvasElement,
  samples: number[] | null,
  sampleHeight: number,
  fallback: number[],
  color: string,
  dpr: number,
): void {
  const rect = canvas.getBoundingClientRect();
  const cssW = Math.max(1, Math.round(rect.width));
  const cssH = Math.max(1, Math.round(rect.height));
  const w = Math.max(1, Math.round(cssW * dpr));
  // SoundCloud's waveform is 280 px tall for 140 sample levels (2 px per
  // level). At 104 CSS px our canvas is only ~208 device px tall on a 2x
  // screen (1.5 px per level), so rounding bar heights merges neighbouring
  // levels and flattens loud tracks. Keep at least 280 rows of headroom.
  const h = Math.max(1, Math.round(cssH * dpr), 280);
  if (canvas.width !== w) canvas.width = w;
  if (canvas.height !== h) canvas.height = h;

  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  ctx.clearRect(0, 0, w, h);
  ctx.fillStyle = color;

  const real = !!(samples && samples.length > 0);
  const values = real ? (samples as number[]) : fallback;
  const scale = real ? Math.max(1, sampleHeight) : 1;
  const n = values.length;
  const mid = h / 2;

  // SoundCloud's track page draws 2 CSS px bars with a 1 px gap, one sample per
  // bar taken at the bar's position, and runs the peak-normalised value through
  // a fixed cubic. The curve was fitted against its own canvas over 24 tracks
  // (two independent sets): h ≈ 1.034v³ − 1.162v² + 1.081v, error ~0.7 px on a
  // 100 px wave for every track in both sets.
  const pitch = Math.max(3, Math.round(3 * dpr));
  const barW = Math.max(2, Math.round(2 * dpr));
  for (let x = 0; x < w; x += pitch) {
    const index = Math.min(n - 1, Math.floor((x * n) / w));
    const v = values[index] / scale;
    const value = real ? 1.0339 * v * v * v - 1.1618 * v * v + 1.0814 * v : v;
    const barH = value > 0 ? value * h : 0;
    if (barH > 0) {
      ctx.fillRect(x, mid - barH / 2, Math.min(barW, w - x), barH);
    }
  }
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
 * Progress-bearing waveform. The wave is painted twice (muted + accent); the
 * accent layer is clipped by `--sw-progress`, which we update via DOM refs on
 * each audio tick — no React re-renders while the track plays.
 */
export const LiveWaveform = React.memo(
  function LiveWaveform({ track, isCurrent, onCommentPosition }: Props) {
    const { data: samples, isLoading } = useTrackWaveform(track);
    const { mode } = usePerfMode();
    const accentColor = useSettingsStore((s) => s.accentColor);
    // The muted lane ink comes from CSS vars; repaint when the mode flips.
    const themeMode = useSettingsStore((s) => s.themeMode);
    const dprCap = mode === 'light' ? 1 : 2;
    // The wave spans the track's full duration (the SoundCloud waveform axis),
    // not the engine's loaded-track duration — which may belong to another track.
    const waveSpanMs = track?.full_duration ?? track?.duration ?? 0;

    const rootRef = useRef<HTMLDivElement>(null);
    const mutedRef = useRef<HTMLCanvasElement>(null);
    const accentRef = useRef<HTMLCanvasElement>(null);
    const hintRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
      // Muted-lane ink is read from CSS (canvas cannot resolve var()), so a
      // theme flip has to repaint even though the value is not touched here.
      void themeMode;
      const paint = () => {
        const dpr = Math.min(window.devicePixelRatio || 1, dprCap);
        const muted = mutedRef.current;
        const accent = accentRef.current;
        if (muted) {
          // Muted-lane ink lives in CSS (`--sw-muted`) so the light theme can
          // flip it to ink; canvas cannot resolve var(), so read it here.
          const mutedInk = getComputedStyle(muted).getPropertyValue('--sw-muted').trim();
          paintWave(
            muted,
            samples?.values ?? null,
            samples?.height ?? 1,
            FALLBACK_BARS,
            mutedInk || 'color-mix(in srgb, var(--ov-ink) 22%, transparent)',
            dpr,
          );
        }
        if (accent) {
          const scoped = getComputedStyle(accent).getPropertyValue('--color-accent').trim();
          paintWave(
            accent,
            samples?.values ?? null,
            samples?.height ?? 1,
            FALLBACK_BARS,
            scoped || accentColor,
            dpr,
          );
        }
      };
      // Window drags fire ResizeObserver far faster than the screen refreshes;
      // coalesce them into one repaint per frame. The bitmap is only rebuilt
      // when its size actually changed, so this is cheap.
      let frame = 0;
      const schedule = () => {
        if (frame) return;
        frame = requestAnimationFrame(() => {
          frame = 0;
          paint();
        });
      };
      paint();
      const ro = new ResizeObserver(schedule);
      if (rootRef.current) ro.observe(rootRef.current);
      window.addEventListener('resize', schedule);
      return () => {
        if (frame) cancelAnimationFrame(frame);
        ro.disconnect();
        window.removeEventListener('resize', schedule);
      };
    }, [samples, dprCap, accentColor, themeMode]);

    useEffect(() => {
      const span = waveSpanMs / 1000;
      if (!isCurrent) {
        if (rootRef.current) rootRef.current.style.setProperty('--sw-progress', '0%');
        if (hintRef.current) hintRef.current.style.left = '0%';
        return;
      }
      const tick = () => {
        const t = getCurrentTime();
        // The bars span the track's full duration, so the playhead rides that
        // too — on a preview it stops where the playable part ends instead of
        // sweeping the whole wave.
        const d = span > 0 ? span : getDuration();
        const pct = d > 0 ? Math.min(100, Math.max(0, (t / d) * 100)) : 0;
        if (rootRef.current) rootRef.current.style.setProperty('--sw-progress', `${pct}%`);
        if (hintRef.current) hintRef.current.style.left = `${pct}%`;
      };
      tick();
      return subscribe(tick);
    }, [isCurrent, waveSpanMs]);

    const handleBarClick = (e: React.MouseEvent<HTMLDivElement>) => {
      if (!isCurrent) return;
      const pct = wavePct(rootRef.current, e.clientX);
      const span = waveSpanMs / 1000;
      const engineD = getDuration();
      // Wave position → seconds on the wave axis, clamped to what the engine
      // can actually play (a preview streams only its first stretch).
      const target =
        span > 0
          ? Math.min(pct * span, engineD > 0 ? engineD : Number.POSITIVE_INFINITY)
          : pct * engineD;
      if (target > 0) seek(target);
    };

    /** Lower lane click: pin where to comment — the playhead stays put. The
     *  position rides the same time axis the comment dots use (the track's full
     *  duration), never whatever the engine currently has loaded. */
    const handleCommentClick = (e: React.MouseEvent<HTMLDivElement>) => {
      if (!onCommentPosition) return;
      e.stopPropagation();
      const d = waveSpanMs / 1000;
      if (d > 0) {
        onCommentPosition(Math.round(wavePct(rootRef.current, e.clientX) * d * 1000));
      }
    };

    return (
      <div
        data-ui="waveform"
        ref={rootRef}
        className={`sw-bars relative w-full h-[104px] ${isCurrent ? 'cursor-pointer' : 'cursor-default'}`}
        onClick={handleBarClick}
      >
        <div className="sw-layer-muted absolute inset-y-0 left-0 right-10">
          <canvas ref={mutedRef} className="block h-full w-full" />
        </div>
        <div className="sw-layer-accent absolute inset-y-0 left-0 right-10">
          <canvas ref={accentRef} className="sw-wave-canvas block h-full w-full" />
        </div>
        {isLoading && (
          <div
            className="sw-shimmer absolute inset-y-0 left-0 right-10 pointer-events-none"
            aria-hidden
            style={{
              background:
                'linear-gradient(90deg, transparent 0%, color-mix(in srgb, var(--ov-ink) 7%, transparent) 50%, transparent 100%)',
              backgroundSize: '200% 100%',
            }}
          />
        )}
        {/* Lower lane: the mirrored half is dimmed towards the page colour, as
            on SoundCloud's own wave; clicking pins a comment spot there without
            seeking. Stops at the gutter like the bars do. */}
        <div
          className="absolute left-0 right-10 top-1/2 bottom-0 cursor-crosshair"
          style={{ background: 'var(--sw-lane-dim)' }}
          onClick={handleCommentClick}
        />
        {/* Center separator line, like the official wave: 2 px of 25% ink. */}
        <div
          className="pointer-events-none absolute left-0 right-10 top-1/2 -translate-y-1/2 h-0.5"
          style={{ background: 'color-mix(in srgb, var(--ov-ink) 25%, transparent)' }}
        />
        {isCurrent && (
          <div className="pointer-events-none absolute inset-y-0 left-0 right-10">
            <div
              ref={hintRef}
              className="absolute top-0 bottom-0 w-[2px]"
              style={{
                left: '0%',
                transform: 'translateX(-50%)',
                background: 'color-mix(in srgb, var(--ov-ink) 50%, transparent)',
              }}
            />
          </div>
        )}
      </div>
    );
  },
  (prev, next) =>
    prev.track?.urn === next.track?.urn &&
    prev.isCurrent === next.isCurrent &&
    prev.onCommentPosition === next.onCommentPosition,
);
