import * as Slider from '@radix-ui/react-slider';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import {
  Heart,
  ListMusic,
  Pause,
  Play,
  Repeat,
  Repeat1,
  Shuffle,
  SkipBack,
  SkipForward,
  Volume1,
  Volume2,
  VolumeX,
} from 'lucide-react';
import React, { useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useShallow } from 'zustand/shallow';
import { api } from '../../lib/api';
import { art } from '../../lib/cdn';
import { useCdnUrl } from '../../lib/useCdnUrl';
import { type Track, usePlayerStore } from '../../stores/player';
import { ScdnImg } from '../ui/ScdnImg';

function formatTime(seconds: number) {
  if (!seconds || !Number.isFinite(seconds)) return '0:00';
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, '0')}`;
}

/* ── Progress Slider (full-width, Radix) ──────────────────────── */
const ProgressSlider = React.memo(() => {
  const progress = usePlayerStore((s) => s.progress);
  const duration = usePlayerStore((s) => s.duration);
  const seek = usePlayerStore((s) => s.seek);

  const [dragging, setDragging] = useState(false);
  const [dragValue, setDragValue] = useState(0);

  const displayValue = dragging ? dragValue : progress;

  return (
    <Slider.Root
      className="relative flex items-center w-full h-5 cursor-pointer group select-none touch-none"
      value={[displayValue]}
      max={duration || 1}
      step={0.1}
      onValueChange={([v]) => {
        setDragValue(v);
        if (!dragging) setDragging(true);
      }}
      onValueCommit={([v]) => {
        seek(v);
        setDragging(false);
      }}
    >
      <Slider.Track className="relative h-[3px] grow rounded-full bg-white/[0.08] group-hover:h-[5px] transition-all duration-150">
        <Slider.Range className="absolute h-full rounded-full bg-accent" />
      </Slider.Track>
      <Slider.Thumb className="block w-3 h-3 rounded-full bg-accent shadow-[0_0_10px_var(--color-accent-glow)] scale-0 opacity-0 group-hover:scale-100 group-hover:opacity-100 data-[dragging]:w-4 data-[dragging]:h-4 data-[dragging]:scale-100 data-[dragging]:opacity-100 data-[dragging]:shadow-[0_0_12px_var(--color-accent-glow)] transition-all duration-150 outline-none" />
    </Slider.Root>
  );
});

/* ── Volume Slider (0-200%, extra zone after 100%) ──────────────── */
const VolumeSlider = React.memo(({ className = '' }: { className?: string }) => {
  const volume = usePlayerStore((s) => s.volume);
  const setVolume = usePlayerStore((s) => s.setVolume);

  const isOver100 = volume > 100;

  return (
    <Slider.Root
      className={`relative flex items-center h-5 cursor-pointer group select-none touch-none ${className}`}
      value={[volume]}
      max={200}
      step={1}
      onValueChange={([v]) => setVolume(v)}
      onWheel={(e) => {
        e.preventDefault();
        setVolume(Math.max(0, Math.min(200, volume + (e.deltaY < 0 ? 1 : -1))));
      }}
    >
      <Slider.Track className="relative h-[3px] grow rounded-full bg-white/[0.08] group-hover:h-[4px] transition-all duration-150">
        <Slider.Range
          className={`absolute h-full rounded-full ${isOver100 ? 'bg-amber-400/80' : 'bg-white/60'}`}
        />
        {/* 100% tick mark */}
        <div className="absolute top-0 h-full w-px bg-white/20" style={{ left: '50%' }} />
      </Slider.Track>
      <Slider.Thumb
        className={`block w-2.5 h-2.5 rounded-full transition-all duration-150 outline-none scale-0 opacity-0 group-hover:scale-100 group-hover:opacity-100 ${isOver100 ? 'bg-amber-400' : 'bg-white'}`}
      />
    </Slider.Root>
  );
});

/* ── Control button ──────────────────────────────────────────────── */
function ControlBtn({
  onClick,
  active = false,
  children,
  size = 'default',
}: {
  onClick: () => void;
  active?: boolean;
  children: React.ReactNode;
  size?: 'default' | 'sm';
}) {
  const s = size === 'sm' ? 'w-9 h-9' : 'w-10 h-10';
  return (
    <button
      type="button"
      onClick={onClick}
      className={`${s} rounded-full flex items-center justify-center transition-all duration-150 ease-[var(--ease-apple)] cursor-pointer hover:bg-white/[0.04] ${
        active ? 'text-accent' : 'text-white/40 hover:text-white/70'
      }`}
    >
      {children}
    </button>
  );
}

const ControlVolumeBtn = React.memo(({ size = 'default' }: { size?: 'default' | 'sm' }) => {
  const { setVolume, volume } = usePlayerStore(
    useShallow((s) => ({
      setVolume: s.setVolume,
      volume: s.volume,
    })),
  );
  const s = size === 'sm' ? 'w-9 h-9' : 'w-10 h-10';
  return (
    <button
      type="button"
      onClick={() => setVolume(volume > 0 ? 0 : 50)}
      className={`${s} rounded-full flex items-center justify-center transition-all duration-150 ease-[var(--ease-apple)] cursor-pointer hover:bg-white/[0.04] ${
        volume === 0 ? 'text-accent' : 'text-white/40 hover:text-white/70'
      }`}
    >
      {volume === 0 ? (
        <VolumeX size={16} />
      ) : volume < 50 ? (
        <Volume1 size={16} />
      ) : (
        <Volume2 size={16} />
      )}
    </button>
  );
});

const ProgressTime = React.memo(() => {
  const { progress, duration } = usePlayerStore(
    useShallow((s) => ({
      progress: Math.floor(s.progress),
      duration: Math.floor(s.duration),
    })),
  );

  return (
    <div className="flex items-center gap-1.5">
      <span className="text-[11px] text-white/50 tabular-nums font-medium">
        {formatTime(progress)}
      </span>
      <span className="text-[11px] text-white/20">/</span>
      <span className="text-[11px] text-white/30 tabular-nums font-medium">
        {formatTime(duration)}
      </span>
    </div>
  );
});

/* ── Like button ─────────────────────────────────────────────────── */
function LikeButton({ trackUrn }: { trackUrn: string }) {
  const qc = useQueryClient();

  // Fetch actual user_favorite state from API
  const { data: trackData } = useQuery({
    queryKey: ['track', trackUrn],
    queryFn: () => api<Track>(`/tracks/${encodeURIComponent(trackUrn)}`),
    enabled: !!trackUrn,
    staleTime: 30_000,
  });

  const [liked, setLiked] = useState<boolean | null>(null);
  const prevUrn = useRef(trackUrn);

  // Reset override when track changes
  if (prevUrn.current !== trackUrn) {
    prevUrn.current = trackUrn;
    setLiked(null);
  }

  const isLiked = liked ?? trackData?.user_favorite ?? false;

  const toggle = async () => {
    const next = !isLiked;
    setLiked(next);
    try {
      await api(`/likes/tracks/${encodeURIComponent(trackUrn)}`, {
        method: next ? 'POST' : 'DELETE',
      });
      qc.invalidateQueries({ queryKey: ['track', trackUrn], exact: true });
      qc.invalidateQueries({ queryKey: ['track', trackUrn, 'favoriters'] });
    } catch {
      setLiked(!next);
    }
  };

  return (
    <button
      type="button"
      onClick={toggle}
      className={`w-9 h-9 rounded-full flex items-center justify-center shrink-0 transition-all duration-200 cursor-pointer hover:bg-white/[0.04] ${
        isLiked ? 'text-accent' : 'text-white/30 hover:text-white/60'
      }`}
    >
      <Heart size={16} fill={isLiked ? 'currentColor' : 'none'} />
    </button>
  );
}

/* ── NowPlayingBar ───────────────────────────────────────────────── */
export const NowPlayingBar = React.memo(
  ({ onQueueToggle, queueOpen }: { onQueueToggle: () => void; queueOpen: boolean }) => {
    const navigate = useNavigate();
    const {
      currentTrack,
      isPlaying,
      volume,
      shuffle,
      repeat,
      togglePlay,
      next,
      prev,
      toggleShuffle,
      toggleRepeat,
    } = usePlayerStore(
      useShallow((s) => ({
        currentTrack: s.currentTrack,
        isPlaying: s.isPlaying,
        volume: s.volume,
        shuffle: s.shuffle,
        repeat: s.repeat,
        togglePlay: s.togglePlay,
        next: s.next,
        prev: s.prev,
        toggleShuffle: s.toggleShuffle,
        toggleRepeat: s.toggleRepeat,
      })),
    );

    const rawArtwork = art(currentTrack?.artwork_url, 't200x200');
    const artwork = useCdnUrl(rawArtwork);

    return (
      <div className="shrink-0 relative">
        {/* Glow from artwork */}
        {artwork && (
          <div
            className="absolute inset-0 opacity-[0.05] blur-3xl pointer-events-none"
            style={{
              backgroundImage: `url(${artwork})`,
              backgroundSize: 'cover',
              backgroundPosition: 'center',
            }}
          />
        )}

        {/* Progress bar — full width on top */}
        <ProgressSlider />

        <div className="h-[76px] flex items-center px-5 gap-3 relative">
          {/* ── Left: track info ── */}
          <div className="flex items-center gap-3.5 w-[280px] min-w-0">
            {currentTrack ? (
              <>
                <div
                  className="w-14 h-14 rounded-[10px] shrink-0 overflow-hidden cursor-pointer shadow-xl shadow-black/40 ring-1 ring-white/[0.06] hover:ring-white/[0.12] transition-all duration-200"
                  onClick={() => navigate(`/track/${encodeURIComponent(currentTrack.urn)}`)}
                >
                  {artwork ? (
                    <ScdnImg src={artwork} alt="" className="w-full h-full object-cover" />
                  ) : (
                    <div className="w-full h-full bg-white/[0.04]" />
                  )}
                </div>
                <div className="min-w-0 flex-1">
                  <p
                    className="text-[13px] text-white/90 truncate font-medium cursor-pointer hover:text-white leading-tight transition-colors"
                    onClick={() => navigate(`/track/${encodeURIComponent(currentTrack.urn)}`)}
                  >
                    {currentTrack.title}
                  </p>
                  <p
                    className="text-[11px] text-white/35 truncate mt-1 cursor-pointer hover:text-white/55 transition-colors"
                    onClick={() => navigate(`/user/${encodeURIComponent(currentTrack.user.urn)}`)}
                  >
                    {currentTrack.user.username}
                  </p>
                </div>
                <LikeButton trackUrn={currentTrack.urn} />
              </>
            ) : (
              <p className="text-[13px] text-white/15">Not playing</p>
            )}
          </div>

          {/* ── Center: controls ── */}
          <div className="flex-1 flex flex-col items-center gap-0.5">
            <div className="flex items-center gap-0.5">
              <ControlBtn onClick={toggleShuffle} active={shuffle} size="sm">
                <Shuffle size={16} />
              </ControlBtn>
              <ControlBtn onClick={prev}>
                <SkipBack size={20} fill="currentColor" />
              </ControlBtn>

              {/* Play/pause */}
              <button
                type="button"
                onClick={togglePlay}
                className="w-12 h-12 rounded-full bg-white/90 flex items-center justify-center text-black hover:bg-white hover:scale-105 active:scale-95 transition-all duration-200 ease-[var(--ease-apple)] cursor-pointer mx-1.5"
              >
                {isPlaying ? (
                  <Pause size={20} fill="black" strokeWidth={0} />
                ) : (
                  <Play size={20} fill="black" strokeWidth={0} className="ml-0.5" />
                )}
              </button>

              <ControlBtn onClick={next}>
                <SkipForward size={20} fill="currentColor" />
              </ControlBtn>
              <ControlBtn onClick={toggleRepeat} active={repeat !== 'off'} size="sm">
                {repeat === 'one' ? <Repeat1 size={16} /> : <Repeat size={16} />}
              </ControlBtn>
            </div>

            {/* Time */}
            <ProgressTime />
          </div>

          {/* ── Right: volume + queue ── */}
          <div className="flex items-center gap-0.5 w-[220px] justify-end">
            <ControlBtn onClick={onQueueToggle} active={queueOpen} size="sm">
              <ListMusic size={16} />
            </ControlBtn>
            <ControlVolumeBtn size="sm" />
            <VolumeSlider className="w-[100px]" />
            <span
              className={`text-[10px] tabular-nums w-[34px] text-right shrink-0 ${volume > 100 ? 'text-amber-400/70' : 'text-white/30'}`}
            >
              {volume}%
            </span>
          </div>
        </div>
      </div>
    );
  },
);
