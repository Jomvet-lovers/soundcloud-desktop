import React, { useEffect, useRef, useState } from 'react';
import { Link } from 'react-router-dom';
import { useShallow } from 'zustand/shallow';
import { getCurrentTime, getDuration, handlePrev, seek, subscribe } from '../../lib/audio';
import { art, formatTime } from '../../lib/formatters';
import {
  listMusic16,
  pauseBlack20,
  playBlack20,
  repeat1Icon16,
  repeatIcon16,
  shuffleIcon16,
  skipBack20,
  skipForward20,
  slidersHorizontal16,
  volume1Icon16,
  volume2Icon16,
  volumeXIcon16,
} from '../../lib/icons';
import { useArtistDisplay, useDisplayTitle } from '../../lib/track-display';
import { type Track, usePlayerStore } from '../../stores/player';
import { trackMenuHandler, userMenuHandler } from '../../stores/track-menu';
import { EqualizerPanel } from '../music/EqualizerPanel';
import { LikeButton } from '../music/LikeButton';
import { SoundTuningPopover } from '../music/SoundTuningPopover';
import { RangeSlider } from '../ui/RangeSlider';

/** Position/duration clock, refreshed on audio events + a slow interval. */
function useAudioClock() {
  const [state, setState] = useState({ time: 0, duration: 0 });
  useEffect(() => {
    const update = () => {
      const time = getCurrentTime();
      const duration = getDuration();
      setState((prev) =>
        Math.abs(prev.time - time) < 0.2 && Math.abs(prev.duration - duration) < 0.2
          ? prev
          : { time, duration },
      );
    };
    const unsub = subscribe(update);
    const id = setInterval(update, 500);
    update();
    return () => {
      unsub();
      clearInterval(id);
    };
  }, []);
  return state;
}

function TrackMeta({ track }: { track: Track }) {
  const displayTitle = useDisplayTitle(track);
  const artistDisplay = useArtistDisplay(track);
  const artistUrn = track.user?.urn;
  return (
    <div className="min-w-0">
      <Link
        to={`/track/${encodeURIComponent(track.urn)}`}
        className="block truncate text-[13px] font-medium text-white/92 hover:underline"
      >
        {displayTitle}
      </Link>
      {artistUrn ? (
        <Link
          to={`/user/${encodeURIComponent(artistUrn)}`}
          onContextMenu={userMenuHandler({
            target: `/user/${encodeURIComponent(artistUrn)}`,
            permalink: track.user?.permalink_url,
          })}
          className="block truncate text-[11px] text-white/45 hover:text-white/70 hover:underline"
        >
          {artistDisplay.primary}
        </Link>
      ) : (
        <p className="truncate text-[11px] text-white/45">{artistDisplay.primary}</p>
      )}
    </div>
  );
}

const iconBtn =
  'flex size-8 items-center justify-center rounded-full transition-colors hover:bg-white/[0.06] disabled:cursor-default disabled:opacity-30 disabled:hover:bg-transparent';
const iconBtnIdle = `${iconBtn} text-white/55 hover:text-white/90`;
const iconBtnOn = `${iconBtn} text-accent hover:text-accent-hover`;

/** Docked, Spotify-style now-playing bar. */
export const NowPlayingBar = React.memo(function NowPlayingBar({
  onQueueToggle,
  queueOpen,
}: {
  onQueueToggle: () => void;
  queueOpen: boolean;
}) {
  const {
    currentTrack,
    isPlaying,
    shuffle,
    repeat,
    pause,
    resume,
    next,
    toggleShuffle,
    toggleRepeat,
    volume,
    volumeBeforeMute,
    setVolume,
  } = usePlayerStore(
    useShallow((s) => ({
      currentTrack: s.currentTrack,
      isPlaying: s.isPlaying,
      shuffle: s.shuffle,
      repeat: s.repeat,
      pause: s.pause,
      resume: s.resume,
      next: s.next,
      toggleShuffle: s.toggleShuffle,
      toggleRepeat: s.toggleRepeat,
      volume: s.volume,
      volumeBeforeMute: s.volumeBeforeMute,
      setVolume: s.setVolume,
    })),
  );
  const { time, duration } = useAudioClock();
  const artwork = art(currentTrack?.artwork_url, 't200x200');
  const repeatTitle =
    repeat === 'off' ? 'Repeat off' : repeat === 'one' ? 'Repeat one' : 'Repeat all';
  const barRef = useRef<HTMLElement>(null);

  // Wheel over the bar steps the volume: 1 point on the 0..100 scale per tick.
  // Chromium (WebView2) reports ~100px per mouse tick and Firefox reports 3
  // lines per tick, so 100 normalized pixels are one step; trackpads send many
  // small deltas that add up.
  useEffect(() => {
    const el = barRef.current;
    if (!el) return;
    const PIXELS_PER_STEP = 100;
    const LINE_PX = 33;
    let pending = 0;
    const onWheel = (event: WheelEvent) => {
      // deltaMode 1 = lines, 2 = pages (Firefox); normalize to pixels.
      const unit = event.deltaMode === 1 ? LINE_PX : event.deltaMode === 2 ? 400 : 1;
      pending += event.deltaY * unit;
      event.preventDefault();
      while (Math.abs(pending) >= PIXELS_PER_STEP) {
        const direction = pending > 0 ? -1 : 1;
        pending -= pending > 0 ? PIXELS_PER_STEP : -PIXELS_PER_STEP;
        const { volume, setVolume } = usePlayerStore.getState();
        const next = Math.max(0, Math.min(100, volume + direction));
        if (next !== volume) setVolume(next);
      }
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, []);

  return (
    <footer
      ref={barRef}
      className="flex h-[72px] flex-none items-center gap-4 border-t border-white/[0.08] bg-[#0b0b0e] px-4"
    >
      {/* Left: track info */}
      <div
        key={currentTrack?.urn ?? 'none'}
        className="flex min-w-0 flex-1 items-center gap-3 animate-swap-in"
        onContextMenu={currentTrack ? trackMenuHandler(currentTrack) : undefined}
      >
        {currentTrack ? (
          <>
            <Link
              to={`/track/${encodeURIComponent(currentTrack.urn)}`}
              className="size-12 flex-none overflow-hidden rounded-md bg-white/[0.06] cursor-pointer transition-opacity hover:opacity-80"
            >
              {artwork ? <img src={artwork} alt="" className="size-full object-cover" /> : null}
            </Link>
            <TrackMeta track={currentTrack} />
            <LikeButton track={currentTrack} variant="bar" />
          </>
        ) : (
          <p className="text-[12px] text-white/35">{'Not playing'}</p>
        )}
      </div>

      {/* Center: controls + progress */}
      <div className="flex w-[42%] max-w-[640px] min-w-0 flex-col items-center gap-1.5">
        <div className="flex items-center gap-2">
          <button
            type="button"
            className={shuffle ? iconBtnOn : iconBtnIdle}
            onClick={toggleShuffle}
            title={'Shuffle'}
          >
            {shuffleIcon16}
          </button>
          <button
            type="button"
            className={iconBtnIdle}
            onClick={handlePrev}
            title={'Previous track'}
          >
            {skipBack20}
          </button>
          <button
            type="button"
            disabled={!currentTrack}
            onClick={() => (isPlaying ? pause() : resume())}
            className="flex size-9 items-center justify-center rounded-full bg-white text-black transition-transform hover:scale-105 disabled:opacity-30"
            title={isPlaying ? 'Pause' : 'Play'}
          >
            <span
              key={isPlaying ? 'pause' : 'play'}
              className="animate-icon-pop flex items-center justify-center"
            >
              {isPlaying ? pauseBlack20 : playBlack20}
            </span>
          </button>
          <button
            type="button"
            disabled={!currentTrack}
            className={iconBtnIdle}
            onClick={next}
            title={'Next track'}
          >
            {skipForward20}
          </button>
          <button
            type="button"
            className={repeat !== 'off' ? iconBtnOn : iconBtnIdle}
            onClick={toggleRepeat}
            title={repeatTitle}
          >
            {repeat === 'one' ? repeat1Icon16 : repeatIcon16}
          </button>
        </div>

        <div className="flex w-full items-center gap-2">
          <span className="w-10 text-right font-mono text-[10.5px] tabular-nums text-white/40">
            {formatTime(time)}
          </span>
          <RangeSlider
            ariaLabel={'Seek'}
            min={0}
            max={Math.max(duration, 0)}
            step={1}
            value={Math.min(time, duration || 0)}
            disabled={!currentTrack || duration <= 0}
            onChange={(v) => seek(v)}
            className="w-full"
          />
          <span className="w-10 font-mono text-[10.5px] tabular-nums text-white/40">
            {formatTime(duration)}
          </span>
        </div>
      </div>

      {/* Right: tuning, queue, EQ, volume */}
      <div className="flex flex-1 items-center justify-end gap-2">
        <SoundTuningPopover />
        <EqualizerPanel>
          <button type="button" className={iconBtnIdle} title={'Equalizer'}>
            {slidersHorizontal16}
          </button>
        </EqualizerPanel>
        <button
          type="button"
          className={queueOpen ? iconBtnOn : iconBtnIdle}
          onClick={onQueueToggle}
          title={'Queue'}
        >
          {listMusic16}
        </button>
        <button
          type="button"
          className={iconBtnIdle}
          onClick={() => setVolume(volume > 0 ? 0 : volumeBeforeMute)}
          title={'Mute / Unmute'}
        >
          {volume === 0 ? volumeXIcon16 : volume < 50 ? volume1Icon16 : volume2Icon16}
        </button>
        <RangeSlider
          ariaLabel={'Volume'}
          min={0}
          max={100}
          step={1}
          value={Math.min(volume, 100)}
          onChange={(v) => setVolume(v)}
          className="w-24"
        />
      </div>
    </footer>
  );
});
