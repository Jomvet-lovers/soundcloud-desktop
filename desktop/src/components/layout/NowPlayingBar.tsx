import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
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
import { EqualizerPanel } from '../music/EqualizerPanel';
import { LikeButton } from '../music/LikeButton';
import { SoundTuningPopover } from '../music/SoundTuningPopover';

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
  return (
    <div className="min-w-0">
      <Link
        to={`/track/${encodeURIComponent(track.urn)}`}
        className="block truncate text-[13px] font-medium text-white/92 hover:underline"
      >
        {displayTitle}
      </Link>
      <p className="truncate text-[11px] text-white/45">{artistDisplay.primary}</p>
    </div>
  );
}

const iconBtn =
  'flex size-8 items-center justify-center rounded-full text-white/55 transition-colors hover:bg-white/[0.06] hover:text-white/90 disabled:cursor-default disabled:opacity-30 disabled:hover:bg-transparent';

/** Docked, Spotify-style now-playing bar. */
export const NowPlayingBar = React.memo(function NowPlayingBar({
  onQueueToggle,
  queueOpen,
}: {
  onQueueToggle: () => void;
  queueOpen: boolean;
}) {
  const { t } = useTranslation();
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

  return (
    <footer className="flex h-[72px] flex-none items-center gap-4 border-t border-white/[0.08] bg-[#0b0b0e] px-4">
      {/* Left: track info */}
      <div className="flex min-w-0 flex-1 items-center gap-3">
        {currentTrack ? (
          <>
            <div className="size-12 flex-none overflow-hidden rounded-md bg-white/[0.06]">
              {artwork ? <img src={artwork} alt="" className="size-full object-cover" /> : null}
            </div>
            <TrackMeta track={currentTrack} />
            <LikeButton track={currentTrack} />
          </>
        ) : (
          <p className="text-[12px] text-white/35">{t('player.notPlaying')}</p>
        )}
      </div>

      {/* Center: controls + progress */}
      <div className="flex w-[42%] max-w-[640px] min-w-0 flex-col items-center gap-1.5">
        <div className="flex items-center gap-2">
          <button
            type="button"
            className={`${iconBtn} ${shuffle ? 'text-accent' : ''}`}
            onClick={toggleShuffle}
            title={t('player.shuffle')}
          >
            {shuffleIcon16}
          </button>
          <button
            type="button"
            className={iconBtn}
            onClick={handlePrev}
            title={t('player.prevTrack')}
          >
            {skipBack20}
          </button>
          <button
            type="button"
            disabled={!currentTrack}
            onClick={() => (isPlaying ? pause() : resume())}
            className="flex size-9 items-center justify-center rounded-full bg-white text-black transition-transform hover:scale-105 disabled:opacity-30"
            title={isPlaying ? t('player.pause') : t('player.play')}
          >
            {isPlaying ? pauseBlack20 : playBlack20}
          </button>
          <button
            type="button"
            disabled={!currentTrack}
            className={iconBtn}
            onClick={next}
            title={t('player.nextTrack')}
          >
            {skipForward20}
          </button>
          <button
            type="button"
            className={`${iconBtn} ${repeat !== 'off' ? 'text-accent' : ''}`}
            onClick={toggleRepeat}
            title={t('player.repeat')}
          >
            {repeat === 'one' ? repeat1Icon16 : repeatIcon16}
          </button>
        </div>

        <div className="flex w-full items-center gap-2">
          <span className="w-10 text-right font-mono text-[10.5px] tabular-nums text-white/40">
            {formatTime(time)}
          </span>
          <input
            type="range"
            min={0}
            max={Math.max(duration, 0)}
            step={1}
            value={Math.min(time, duration || 0)}
            disabled={!currentTrack || duration <= 0}
            onChange={(e) => seek(Number(e.target.value))}
            className="h-1 w-full cursor-pointer disabled:cursor-default"
            style={{ accentColor: 'var(--color-accent)' }}
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
          <button type="button" className={iconBtn} title={t('eq.title')}>
            {slidersHorizontal16}
          </button>
        </EqualizerPanel>
        <button
          type="button"
          className={`${iconBtn} ${queueOpen ? 'text-accent' : ''}`}
          onClick={onQueueToggle}
          title={t('player.queue')}
        >
          {listMusic16}
        </button>
        <button
          type="button"
          className={iconBtn}
          onClick={() => setVolume(volume > 0 ? 0 : volumeBeforeMute)}
          title={t('player.mute')}
        >
          {volume === 0 ? volumeXIcon16 : volume < 50 ? volume1Icon16 : volume2Icon16}
        </button>
        <input
          type="range"
          min={0}
          max={100}
          step={1}
          value={Math.min(volume, 100)}
          onChange={(e) => setVolume(Number(e.target.value))}
          className="h-1 w-24 cursor-pointer"
          style={{ accentColor: 'var(--color-accent)' }}
        />
      </div>
    </footer>
  );
});
