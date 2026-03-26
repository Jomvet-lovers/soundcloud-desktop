import { useQuery, useQueryClient } from '@tanstack/react-query';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import React, { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useShallow } from 'zustand/shallow';
import { api } from '../../lib/api';
import { getCurrentTime, handlePrev, seek } from '../../lib/audio';
import { ago, art, durLong } from '../../lib/formatters';
import { type Comment, invalidateAllLikesCache, useTrackComments } from '../../lib/hooks';
import {
  Heart,
  ListPlus,
  Loader2,
  MicVocal,
  MessageCircle,
  pauseBlack18,
  playBlack18,
  repeat1Icon16,
  repeatIcon16,
  SkipBack,
  SkipForward,
  shuffleIcon16,
  X,
} from '../../lib/icons';
import { optimisticToggleLike, useLiked } from '../../lib/likes';
import type { LyricLine } from '../../lib/lyrics';
import { searchLyrics } from '../../lib/lyrics';
import { useLyricsStore } from '../../stores/lyrics';
import { type Track, usePlayerStore } from '../../stores/player';
import { ProgressSlider, ProgressTime } from '../layout/NowPlayingBar';
import { AddToPlaylistDialog } from './AddToPlaylistDialog';

/* ── Color extraction ──────────────────────────────────────── */

function extractColor(src: string): Promise<[number, number, number]> {
  return new Promise((resolve) => {
    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.onload = () => {
      try {
        const c = document.createElement('canvas');
        c.width = 10;
        c.height = 10;
        const ctx = c.getContext('2d')!;
        ctx.drawImage(img, 0, 0, 10, 10);
        const d = ctx.getImageData(0, 0, 10, 10).data;
        let r = 0,
          g = 0,
          b = 0;
        const n = d.length / 4;
        for (let i = 0; i < d.length; i += 4) {
          r += d[i];
          g += d[i + 1];
          b += d[i + 2];
        }
        resolve([Math.round(r / n), Math.round(g / n), Math.round(b / n)]);
      } catch {
        resolve([255, 85, 0]);
      }
    };
    img.onerror = () => resolve([255, 85, 0]);
    img.src = src;
  });
}

/* ── Shared: dynamic background ───────────────────────────── */

const FullscreenBackground = React.memo(
  ({ artworkSrc, color }: { artworkSrc: string | null; color: [number, number, number] }) => {
    const [r, g, b] = color;
    return (
      <div
        className="absolute inset-0 pointer-events-none"
        style={{ contain: 'strict', transform: 'translateZ(0)' }}
      >
        {artworkSrc && (
          <img
            src={artworkSrc}
            alt=""
            className="w-full h-full object-cover scale-[1.4] blur-[100px] opacity-25 saturate-[1.5]"
          />
        )}
        <div
          className="absolute inset-0"
          style={{
            background: `
              radial-gradient(ellipse at 25% 50%, rgba(${r},${g},${b},0.2) 0%, transparent 60%),
              radial-gradient(ellipse at 75% 70%, rgba(${r},${g},${b},0.12) 0%, transparent 50%)
            `,
          }}
        />
      </div>
    );
  },
);

/* ── Shared: like button (for fullscreen panels) ──────────── */

const FullscreenLikeButton = React.memo(({ track }: { track: Track }) => {
  const liked = useLiked(track.urn);
  const qc = useQueryClient();

  const toggle = async () => {
    const next = !liked;
    optimisticToggleLike(qc, track, next);
    invalidateAllLikesCache();
    try {
      await api(`/likes/tracks/${encodeURIComponent(track.urn)}`, {
        method: next ? 'POST' : 'DELETE',
      });
    } catch {
      optimisticToggleLike(qc, track, !next);
    }
  };

  return (
    <button
      type="button"
      onClick={toggle}
      className={`w-11 h-11 rounded-full flex items-center justify-center transition-all duration-200 cursor-pointer hover:bg-white/[0.06] ${
        liked ? 'text-accent' : 'text-white/30 hover:text-white/60'
      }`}
    >
      <Heart size={20} fill={liked ? 'currentColor' : 'none'} />
    </button>
  );
});

/* ── Shared: transport controls + like ────────────────────── */

const Controls = React.memo(({ track }: { track: Track }) => {
  const { isPlaying, next, repeat, shuffle, togglePlay, toggleRepeat, toggleShuffle } =
    usePlayerStore(
      useShallow((s) => ({
        isPlaying: s.isPlaying,
        next: s.next,
        repeat: s.repeat,
        shuffle: s.shuffle,
        togglePlay: s.togglePlay,
        toggleRepeat: s.toggleRepeat,
        toggleShuffle: s.toggleShuffle,
      })),
    );

  const ctrl =
    'w-10 h-10 rounded-full flex items-center justify-center transition-all duration-150 cursor-pointer hover:bg-white/[0.06]';
  const small =
    'w-9 h-9 rounded-full flex items-center justify-center transition-all duration-150 cursor-pointer hover:bg-white/[0.06]';

  return (
    <div className="flex items-center justify-center gap-3">
      <FullscreenLikeButton track={track} />
      <button
        type="button"
        onClick={toggleShuffle}
        className={`${small} ${shuffle ? 'text-accent' : 'text-white/35 hover:text-white/60'}`}
      >
        {shuffleIcon16}
      </button>
      <button
        type="button"
        onClick={handlePrev}
        className={`${ctrl} text-white/60 hover:text-white`}
      >
        <SkipBack size={20} fill="currentColor" />
      </button>
      <button
        type="button"
        onClick={togglePlay}
        className="w-14 h-14 rounded-full bg-white flex items-center justify-center hover:scale-105 active:scale-95 transition-all duration-200 cursor-pointer shadow-lg"
      >
        {isPlaying ? pauseBlack18 : playBlack18}
      </button>
      <button type="button" onClick={next} className={`${ctrl} text-white/60 hover:text-white`}>
        <SkipForward size={20} fill="currentColor" />
      </button>
      <button
        type="button"
        onClick={toggleRepeat}
        className={`${small} ${repeat !== 'off' ? 'text-accent' : 'text-white/35 hover:text-white/60'}`}
      >
        {repeat === 'one' ? repeat1Icon16 : repeatIcon16}
      </button>
      <AddToPlaylistDialog trackUrns={[track.urn]}>
        <button type="button" className={`${small} text-white/30 hover:text-white/60`}>
          <ListPlus size={20} />
        </button>
      </AddToPlaylistDialog>
    </div>
  );
});

/* ── Shared: artwork + info + slider + controls column ────── */

const TrackColumn = React.memo(({ track, maxArt }: { track: Track; maxArt?: string }) => {
  const artwork500 = art(track.artwork_url, 't500x500');
  const artwork200 = art(track.artwork_url, 't200x200');
  const [loaded, setLoaded] = useState(false);

  // Reset loaded state when track changes
  const prevUrlRef = useRef(track.artwork_url);
  if (prevUrlRef.current !== track.artwork_url) {
    prevUrlRef.current = track.artwork_url;
    setLoaded(false);
  }

  return (
    <div className="flex flex-col items-center justify-center gap-5 px-12">
      <div
        className={`w-full ${maxArt ?? 'max-w-[360px]'} aspect-square rounded-2xl overflow-hidden shadow-2xl shadow-black/60 ring-1 ring-white/[0.08] relative`}
      >
        {artwork500 ? (
          <>
            {/* Low-res preview */}
            <img
              src={artwork200 || artwork500}
              alt=""
              className={`absolute inset-0 w-full h-full object-cover transition-opacity duration-500 ${
                loaded ? 'opacity-0' : 'opacity-100'
              }`}
              decoding="async"
            />
            {/* High-res */}
            <img
              src={artwork500}
              alt=""
              onLoad={() => setLoaded(true)}
              className={`absolute inset-0 w-full h-full object-cover transition-opacity duration-500 ${
                loaded ? 'opacity-100' : 'opacity-0'
              }`}
              decoding="async"
            />
          </>
        ) : (
          <div className="w-full h-full bg-gradient-to-br from-white/[0.06] to-white/[0.02] flex items-center justify-center">
            <MicVocal size={48} className="text-white/10" />
          </div>
        )}
      </div>

      <div className={`w-full ${maxArt ?? 'max-w-[360px]'} text-center space-y-1`}>
        <p className="text-[18px] font-bold text-white/95 truncate">{track.title}</p>
        <p className="text-[14px] text-white/40 truncate">{track.user.username}</p>
      </div>

      <div className={`w-full ${maxArt ?? 'max-w-[360px]'}`}>
        <ProgressSlider />
        <div className="flex justify-center mt-1">
          <ProgressTime />
        </div>
      </div>

      <Controls track={track} />
    </div>
  );
});

/* ── Shared: color hook ───────────────────────────────────── */

function useArtworkColor(artworkUrl: string | null) {
  const colorRef = useRef<[number, number, number]>([255, 85, 0]);
  const prevArtRef = useRef<string | null>(null);

  useEffect(() => {
    const src = art(artworkUrl, 't200x200');
    if (!src || src === prevArtRef.current) return;
    prevArtRef.current = src;
    extractColor(src).then((c) => {
      colorRef.current = c;
    });
  }, [artworkUrl]);

  return colorRef;
}

/* ── Synced Lyrics — CSS data-state + DOM scroll, 0 re-renders */

const SyncedLyrics = React.memo(({ lines }: { lines: LyricLine[] }) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const activeRef = useRef(-1);
  const linesRef = useRef(lines);
  const lineElsRef = useRef<HTMLElement[]>([]);
  linesRef.current = lines;

  // biome-ignore lint/correctness/useExhaustiveDependencies: lines triggers DOM re-cache
  useEffect(() => {
    const container = containerRef.current;
    if (container) {
      lineElsRef.current = Array.from(container.querySelectorAll<HTMLElement>('.lyric-line'));
    }
    activeRef.current = -1;

    void invoke('audio_set_lyrics_timeline', {
      lines: lines.map((line) => ({ timeSecs: line.time })),
    });

    const unlistenPromise = listen<number | null>('lyrics:active_line', (event) => {
      const lineEls = lineElsRef.current;
      if (!container || lineEls.length === 0) return;

      const idx = typeof event.payload === 'number' ? event.payload : -1;
      if (idx === activeRef.current) return;

      const prev = activeRef.current;
      activeRef.current = idx;

      if (prev >= 0 && prev < lineEls.length) {
        lineEls[prev].dataset.state = prev < idx ? 'past' : '';
      }

      if (idx >= 0 && idx < lineEls.length) {
        lineEls[idx].dataset.state = 'active';
        const el = lineEls[idx];
        const top = el.offsetTop - container.clientHeight / 2 + el.clientHeight / 2;
        container.scrollTo({ top, behavior: 'smooth' });
      }

      if (prev !== -1 && idx !== -1) {
        const lo = Math.min(prev, idx);
        const hi = Math.max(prev, idx);
        for (let i = lo; i <= hi; i++) {
          if (i === idx || i === prev) continue;
          const state = i < idx ? 'past' : '';
          if (lineEls[i].dataset.state !== state) lineEls[i].dataset.state = state;
        }
      }
    });

    return () => {
      void invoke('audio_clear_lyrics_timeline');
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, [lines]);

  return (
    <div
      ref={containerRef}
      className="flex-1 overflow-y-auto scrollbar-hide px-12 py-16 relative"
      style={{
        maskImage: 'linear-gradient(transparent 0%, black 10%, black 90%, transparent 100%)',
      }}
    >
      {lines.map((line, i) => (
        <div key={`${line.time}-${i}`} className="lyric-line" onClick={() => seek(line.time)}>
          {line.text}
        </div>
      ))}
      <div className="h-[40vh]" />
    </div>
  );
});

/* ── Plain Lyrics ──────────────────────────────────────────── */

const PlainLyrics = React.memo(({ text }: { text: string }) => (
  <div
    className="flex-1 overflow-y-auto scrollbar-hide px-12 py-16"
    style={{ maskImage: 'linear-gradient(transparent 0%, black 10%, black 90%, transparent 100%)' }}
  >
    <div className="text-[18px] text-white/60 font-medium whitespace-pre-wrap leading-loose">
      {text}
    </div>
  </div>
));

const PanelTabButton = React.memo(
  ({
    active,
    children,
    onClick,
  }: {
    active: boolean;
    children: React.ReactNode;
    onClick: () => void;
  }) => (
    <button
      type="button"
      onClick={onClick}
      className={`px-3.5 py-2 rounded-xl text-[12px] font-medium transition-all duration-200 cursor-pointer ${
        active
          ? 'bg-white/[0.08] text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.04)]'
          : 'text-white/35 hover:text-white/70 hover:bg-white/[0.04]'
      }`}
    >
      {children}
    </button>
  ),
);

const TimedCommentCard = React.memo(
  ({
    comment,
    state,
    onSeek,
  }: {
    comment: Comment;
    state: 'past' | 'active' | 'future';
    onSeek: (seconds: number) => void;
  }) => {
    const avatar = art(comment.user.avatar_url, 'small');
    const commentTime = comment.timestamp != null ? comment.timestamp / 1000 : 0;

    return (
      <button
        type="button"
        onClick={() => onSeek(commentTime)}
        className={`w-full text-left rounded-2xl border px-4 py-3 transition-all duration-300 cursor-pointer ${
          state === 'active'
            ? 'bg-gradient-to-br from-white/[0.14] to-white/[0.08] border-white/14 ring-1 ring-accent/25 shadow-[0_16px_36px_rgba(0,0,0,0.26)]'
            : state === 'past'
              ? 'bg-white/[0.025] border-white/[0.035] hover:bg-white/[0.04]'
              : 'bg-white/[0.045] border-white/[0.05] hover:bg-white/[0.06]'
        }`}
      >
        <div className="flex items-start gap-3">
          <img
            src={avatar ?? ''}
            alt=""
            className="w-9 h-9 rounded-full shrink-0 ring-1 ring-white/[0.08]"
          />
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <span className="text-[12px] font-medium text-white/78 truncate">
                {comment.user.username}
              </span>
              <span
                className={`text-[10px] tabular-nums shrink-0 ${
                  state === 'active' ? 'text-accent' : 'text-white/30'
                }`}
              >
                {durLong(comment.timestamp ?? 0)}
              </span>
              <span className="text-[10px] text-white/18 ml-auto shrink-0">
                {ago(comment.created_at)}
              </span>
            </div>
            <p className="mt-1 text-[13px] leading-relaxed text-white/58 break-words">
              {comment.body}
            </p>
          </div>
        </div>
      </button>
    );
  },
);

const TimedCommentsRail = React.memo(({ trackUrn }: { trackUrn: string }) => {
  const { t } = useTranslation();
  const containerRef = useRef<HTMLDivElement>(null);
  const itemRefs = useRef(new Map<number, HTMLDivElement>());
  const [activeIndex, setActiveIndex] = useState(-1);
  const { comments, fetchNextPage, hasNextPage, isFetchingNextPage, isLoading } =
    useTrackComments(trackUrn);

  const timedComments = useMemo(
    () =>
      [...comments]
        .filter((comment) => comment.timestamp != null && comment.body.trim())
        .sort((a, b) => (a.timestamp ?? 0) - (b.timestamp ?? 0)),
    [comments],
  );

  useEffect(() => {
    if (!hasNextPage || isFetchingNextPage) return;
    void fetchNextPage();
  }, [fetchNextPage, hasNextPage, isFetchingNextPage]);

  useEffect(() => {
    const getIndexForTime = (timeMs: number) => {
      if (timedComments.length === 0) return -1;

      let lo = 0;
      let hi = timedComments.length - 1;
      let best = -1;

      while (lo <= hi) {
        const mid = (lo + hi) >> 1;
        const timestamp = timedComments[mid].timestamp ?? 0;
        if (timestamp <= timeMs) {
          best = mid;
          lo = mid + 1;
        } else {
          hi = mid - 1;
        }
      }

      return best;
    };

    const syncActiveIndex = () => {
      const nextIndex = getIndexForTime(Math.max(0, getCurrentTime()) * 1000);
      setActiveIndex((prev) => (prev === nextIndex ? prev : nextIndex));
    };

    syncActiveIndex();
    const id = window.setInterval(syncActiveIndex, 250);
    return () => window.clearInterval(id);
  }, [timedComments]);

  const focusIndex = activeIndex >= 0 ? activeIndex : timedComments.length > 0 ? 0 : -1;

  useEffect(() => {
    if (focusIndex < 0) return;
    const container = containerRef.current;
    const item = itemRefs.current.get(timedComments[focusIndex]?.id ?? -1);
    if (!container || !item) return;

    const top = item.offsetTop - container.clientHeight / 2 + item.clientHeight / 2;
    container.scrollTo({ top: Math.max(0, top), behavior: 'smooth' });
  }, [focusIndex, timedComments]);

  if (isLoading && timedComments.length === 0) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center gap-3">
        <Loader2 size={24} className="animate-spin text-white/15" />
        <p className="text-[13px] text-white/25">{t('track.comments')}</p>
      </div>
    );
  }

  if (timedComments.length === 0) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center gap-4 px-12 text-center">
        <MessageCircle size={40} className="text-white/[0.06]" />
        <p className="text-[15px] text-white/30 font-medium">{t('track.noComments')}</p>
      </div>
    );
  }

  return (
    <div className="flex-1 min-h-0 px-8 pb-8">
      <div
        ref={containerRef}
        className="h-full overflow-y-auto scrollbar-hide px-4 py-[32vh] space-y-3 relative"
        style={{
          maskImage: 'linear-gradient(transparent 0%, black 12%, black 88%, transparent 100%)',
        }}
      >
        {timedComments.map((comment, index) => {
          const state = index < activeIndex ? 'past' : index === activeIndex ? 'active' : 'future';
          const distance = Math.abs(index - focusIndex);
          const scale = Math.max(0.9, 1 - distance * 0.035);
          const opacity =
            state === 'active' ? 1 : Math.max(0.28, distance === 0 ? 0.94 : 1 - distance * 0.15);

          return (
            <div
              key={comment.id}
              ref={(node) => {
                if (node) itemRefs.current.set(comment.id, node);
                else itemRefs.current.delete(comment.id);
              }}
              className="relative"
              style={{
                transform: `scale(${scale}) translateZ(0)`,
                opacity,
                filter: distance >= 4 ? 'blur(1.5px)' : distance >= 2 ? 'blur(0.6px)' : 'none',
              }}
            >
              <TimedCommentCard
                comment={comment}
                state={state}
                onSeek={(seconds) => seek(seconds)}
              />
            </div>
          );
        })}
      </div>
    </div>
  );
});

/* ── Lyrics Panel (fullscreen, 50/50) ─────────────────────── */

export const LyricsPanel = React.memo(() => {
  const open = useLyricsStore((s) => s.open);
  const close = useLyricsStore((s) => s.close);
  const tab = useLyricsStore((s) => s.tab);
  const setTab = useLyricsStore((s) => s.setTab);
  const rightPanelOpen = useLyricsStore((s) => s.rightPanelOpen);
  const setRightPanelOpen = useLyricsStore((s) => s.setRightPanelOpen);
  const toggleRightPanel = useLyricsStore((s) => s.toggleRightPanel);
  const track = usePlayerStore((s) => s.currentTrack);
  const { t } = useTranslation();
  const colorRef = useArtworkColor(track?.artwork_url ?? null);

  const { data: lyrics, isLoading } = useQuery({
    queryKey: ['lyrics', track?.user.username, track?.title],
    queryFn: () => searchLyrics(track!.user.username, track!.title),
    enabled: open && !!track,
    staleTime: Number.POSITIVE_INFINITY,
    retry: 1,
  });

  useEffect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') close();
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [open, close]);

  if (!open || !track) return null;

  const artwork200 = art(track.artwork_url, 't200x200');

  return (
    <div className="fixed inset-0 z-[60] flex flex-col overflow-hidden animate-fade-in-up bg-[#08080a]">
      <FullscreenBackground artworkSrc={artwork200} color={colorRef.current} />

      <div
        className={`relative z-10 px-6 pt-5 pb-2 ${rightPanelOpen ? 'flex items-center justify-between gap-4' : 'flex items-center justify-center gap-4'}`}
        data-tauri-drag-region
      >
        <div className="inline-flex items-center gap-1.5 rounded-2xl border border-white/[0.05] bg-white/[0.03] p-1">
          <PanelTabButton
            active={rightPanelOpen && tab === 'lyrics'}
            onClick={() => {
              setTab('lyrics');
              setRightPanelOpen(true);
            }}
          >
            {t('track.lyrics')}
          </PanelTabButton>
          <PanelTabButton
            active={rightPanelOpen && tab === 'comments'}
            onClick={() => {
              setTab('comments');
              setRightPanelOpen(true);
            }}
          >
            {t('track.comments')}
          </PanelTabButton>
          <PanelTabButton active={!rightPanelOpen} onClick={toggleRightPanel}>
            {rightPanelOpen ? t('track.hidePanel') : t('track.showPanel')}
          </PanelTabButton>
        </div>
        <button
          type="button"
          onClick={close}
          className="w-9 h-9 rounded-full flex items-center justify-center text-white/25 hover:text-white/70 hover:bg-white/[0.08] transition-all duration-200 cursor-pointer"
        >
          <X size={18} />
        </button>
      </div>

      {rightPanelOpen ? (
        <div
          className="relative z-10 grid grid-cols-2 flex-1 min-h-0"
          style={{ isolation: 'isolate' }}
        >
          <TrackColumn track={track} />

          <div className="absolute left-1/2 top-0 bottom-0 w-px bg-white/[0.04]" />

          <div className="min-h-0 flex flex-col">
            {tab === 'comments' ? (
              <TimedCommentsRail trackUrn={track.urn} />
            ) : isLoading ? (
              <div className="flex-1 flex flex-col items-center justify-center gap-3">
                <Loader2 size={24} className="animate-spin text-white/15" />
                <p className="text-[13px] text-white/25">{t('track.lyricsLoading')}</p>
              </div>
            ) : lyrics?.synced ? (
              <SyncedLyrics lines={lyrics.synced} />
            ) : lyrics?.plain ? (
              <PlainLyrics text={lyrics.plain} />
            ) : (
              <div className="flex-1 flex flex-col items-center justify-center gap-4 px-12 text-center">
                <MicVocal size={40} className="text-white/[0.06]" />
                <p className="text-[15px] text-white/30 font-medium">{t('track.lyricsNotFound')}</p>
                <p className="text-[12px] text-white/15 leading-relaxed max-w-[300px]">
                  {t('track.lyricsNotFoundHint')}
                </p>
              </div>
            )}
          </div>
        </div>
      ) : (
        <div
          className="relative z-10 flex-1 flex items-center justify-center min-h-0"
          style={{ isolation: 'isolate' }}
        >
          <TrackColumn track={track} maxArt="max-w-[420px]" />
        </div>
      )}
    </div>
  );
});
