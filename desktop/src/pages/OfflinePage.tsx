import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { VirtualList } from '../components/ui/VirtualList';
import { api } from '../lib/api';
import { listCachedUrns } from '../lib/cache';
import { art, dur } from '../lib/formatters';
import { fetchAllLikedTracks } from '../lib/hooks';
import {
  AlertCircle,
  Clock,
  Download,
  Globe,
  Heart,
  Music,
  Play,
  RefreshCw,
  RotateCcw,
} from '../lib/icons';
import { getOfflineLikedTracks, getOfflineTracksByUrns } from '../lib/offline-index';
import { useAppStatusStore } from '../stores/app-status';
import type { Track } from '../stores/player';
import { usePlayerStore } from '../stores/player';

// ─── Types ─────────────────────────────────────────────────

interface OfflineLibraryState {
  cachedTracks: Track[];
  likedTracks: Track[];
  cachedUrns: Set<string>;
}

interface PendingStats {
  pending: number;
  failed: number;
}

const EMPTY_STATE: OfflineLibraryState = {
  cachedTracks: [],
  likedTracks: [],
  cachedUrns: new Set(),
};

const EMPTY_STATS: PendingStats = { pending: 0, failed: 0 };

function buildPlayableQueue(tracks: Track[], cachedUrns: Set<string>) {
  return tracks.filter((track) => cachedUrns.has(track.urn));
}

// ─── Status Badge ──────────────────────────────────────────

const StatusBadge = React.memo(function StatusBadge() {
  const { t } = useTranslation();
  const mode = useAppStatusStore((s) =>
    s.soundcloudBlocked
      ? 'blocked'
      : !s.navigatorOnline || !s.backendReachable
        ? 'offline'
        : 'online',
  );

  const config = {
    blocked: {
      border: 'border-amber-400/20',
      bg: 'bg-amber-400/10',
      text: 'text-amber-200/90',
      glow: 'shadow-[0_0_20px_rgba(251,191,36,0.08)]',
      icon: <AlertCircle size={12} />,
      label: t('offline.blockedBadge'),
    },
    offline: {
      border: 'border-sky-400/20',
      bg: 'bg-sky-400/10',
      text: 'text-sky-100/90',
      glow: 'shadow-[0_0_20px_rgba(56,189,248,0.08)]',
      icon: <Globe size={12} />,
      label: t('offline.offlineBadge'),
    },
    online: {
      border: 'border-emerald-400/20',
      bg: 'bg-emerald-400/10',
      text: 'text-emerald-100/90',
      glow: 'shadow-[0_0_20px_rgba(52,211,153,0.08)]',
      icon: <Download size={12} />,
      label: t('offline.readyBadge'),
    },
  }[mode];

  return (
    <div
      className={`inline-flex items-center gap-2 rounded-full border ${config.border} ${config.bg} ${config.glow} px-3.5 py-1.5 text-[11px] font-semibold uppercase tracking-[0.18em] ${config.text} backdrop-blur-sm`}
    >
      {config.icon}
      {config.label}
    </div>
  );
});

// ─── Pending Actions Badge ─────────────────────────────────

const PendingBadge = React.memo(function PendingBadge({
  stats,
  syncing,
  onSync,
}: {
  stats: PendingStats;
  syncing: boolean;
  onSync: () => void;
}) {
  const { t } = useTranslation();

  if (stats.pending === 0 && stats.failed === 0) return null;

  return (
    <div className="inline-flex items-center gap-2">
      <div className="inline-flex items-center gap-1.5 rounded-full border border-violet-400/16 bg-violet-400/8 px-3 py-1.5 text-[11px] font-semibold text-violet-200/80 shadow-[0_0_16px_rgba(139,92,246,0.06)] backdrop-blur-sm">
        <Clock size={11} />
        {t('offline.pendingCount', { count: stats.pending })}
        {stats.failed > 0 && (
          <span className="ml-1 text-rose-300/80">
            ({t('offline.failedCount', { count: stats.failed })})
          </span>
        )}
      </div>
      <button
        type="button"
        onClick={onSync}
        disabled={syncing}
        className="inline-flex cursor-pointer items-center gap-1.5 rounded-full border border-violet-400/16 bg-violet-400/8 px-3 py-1.5 text-[11px] font-semibold text-violet-200/80 transition-all hover:bg-violet-400/14 disabled:opacity-50"
      >
        <RefreshCw size={11} className={syncing ? 'animate-spin' : ''} />
        {t('offline.syncNow')}
      </button>
    </div>
  );
});

// ─── Track Row ─────────────────────────────────────────────

const OfflineTrackRow = React.memo(function OfflineTrackRow({
  track,
  queue,
  canPlay,
  showCachedBadge,
}: {
  track: Track;
  queue: Track[];
  canPlay: boolean;
  showCachedBadge: boolean;
}) {
  const { t } = useTranslation();
  const play = usePlayerStore((s) => s.play);
  const artwork = art(track.artwork_url, 't200x200');

  return (
    <div
      className={`group flex items-center gap-4 rounded-[26px] border px-4 py-3 transition-all duration-300 ease-[var(--ease-apple)] ${
        canPlay
          ? 'border-white/8 bg-white/[0.04] hover:bg-white/[0.07] hover:border-white/14 hover:shadow-[0_4px_24px_rgba(0,0,0,0.15)]'
          : 'border-white/6 bg-white/[0.025] opacity-60'
      }`}
      style={{ contentVisibility: 'auto', containIntrinsicSize: '82px' }}
    >
      <button
        type="button"
        onClick={() => canPlay && play(track, queue)}
        disabled={!canPlay}
        className={`relative flex size-11 shrink-0 items-center justify-center overflow-hidden rounded-[18px] border transition-all ${
          canPlay
            ? 'cursor-pointer border-white/12 bg-white/[0.08] text-white/90 hover:scale-[1.03] shadow-[inset_0_1px_0_rgba(255,255,255,0.06)]'
            : 'cursor-not-allowed border-white/8 bg-white/[0.04] text-white/25'
        }`}
      >
        {artwork ? (
          <>
            <img src={artwork} alt="" className="size-full object-cover" decoding="async" loading="lazy" />
            {canPlay && (
              <div className="absolute inset-0 flex items-center justify-center bg-black/0 opacity-0 transition-all group-hover:bg-black/40 group-hover:opacity-100">
                <Play size={16} fill="white" strokeWidth={0} />
              </div>
            )}
          </>
        ) : (
          <Music size={18} />
        )}
      </button>

      <div className="min-w-0 flex-1">
        <div className="truncate text-[14px] font-semibold text-white/92">{track.title}</div>
        <div className="mt-1 truncate text-[12px] text-white/42">{track.user.username}</div>
      </div>

      <div className="hidden items-center gap-2 shrink-0 sm:flex">
        {showCachedBadge ? (
          <span className="inline-flex items-center gap-1.5 rounded-full border border-emerald-400/16 bg-emerald-400/8 px-2.5 py-1 text-[11px] font-medium text-emerald-100/80">
            <Download size={12} />
            {t('offline.cached')}
          </span>
        ) : (
          <span className="inline-flex items-center rounded-full border border-white/8 bg-white/[0.04] px-2.5 py-1 text-[11px] font-medium text-white/30">
            {t('offline.notCached')}
          </span>
        )}
      </div>

      <div className="w-14 shrink-0 text-right text-[12px] font-medium tabular-nums text-white/30">
        {dur(track.duration)}
      </div>
    </div>
  );
});

// ─── Section Card ──────────────────────────────────────────

function OfflineSection({
  icon,
  title,
  subtitle,
  items,
  cachedUrns,
  emptyText,
  likesMode = false,
}: {
  icon: React.ReactNode;
  title: string;
  subtitle: string;
  items: Track[];
  cachedUrns: Set<string>;
  emptyText: string;
  likesMode?: boolean;
}) {
  const playableQueue = useMemo(() => buildPlayableQueue(items, cachedUrns), [items, cachedUrns]);

  return (
    <section className="relative overflow-hidden rounded-[34px] border border-white/10 bg-[linear-gradient(180deg,rgba(255,255,255,0.07),rgba(255,255,255,0.02))] p-[1px] shadow-[0_24px_80px_rgba(0,0,0,0.28),0_0_1px_rgba(255,255,255,0.1)] backdrop-blur-[40px]">
      {/* Inner glow highlight */}
      <div className="pointer-events-none absolute inset-0 rounded-[34px] bg-[radial-gradient(ellipse_at_top,rgba(255,255,255,0.06),transparent_60%)]" />

      <div className="relative rounded-[33px] bg-black/25 px-5 py-5">
        <div className="flex items-start justify-between gap-4">
          <div className="flex items-start gap-3">
            <div className="flex size-11 shrink-0 items-center justify-center rounded-[18px] border border-white/12 bg-white/[0.08] text-white/88 shadow-[inset_0_1px_0_rgba(255,255,255,0.1),0_2px_8px_rgba(0,0,0,0.12)]">
              {icon}
            </div>
            <div>
              <h2 className="text-[18px] font-semibold tracking-tight text-white/94">{title}</h2>
              <p className="mt-1 text-[12px] leading-5 text-white/40">{subtitle}</p>
            </div>
          </div>
          <div className="rounded-full border border-white/8 bg-white/[0.05] px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.16em] text-white/35">
            {items.length}
          </div>
        </div>

        {items.length > 0 ? (
          <div className="mt-5">
            <VirtualList
              items={items}
              rowHeight={82}
              overscan={8}
              getItemKey={(track) => track.urn}
              renderItem={(track) => {
                const isCached = cachedUrns.has(track.urn);
                return (
                  <OfflineTrackRow
                    track={track}
                    queue={likesMode ? playableQueue : items}
                    canPlay={likesMode ? isCached : true}
                    showCachedBadge={isCached}
                  />
                );
              }}
            />
          </div>
        ) : (
          <div className="mt-5 rounded-[24px] border border-dashed border-white/8 bg-white/[0.02] px-5 py-10 text-center text-[13px] text-white/30">
            {emptyText}
          </div>
        )}
      </div>
    </section>
  );
}

// ─── Description Card ──────────────────────────────────────

const DescriptionCard = React.memo(function DescriptionCard({
  mode,
}: {
  mode: 'online' | 'offline' | 'blocked';
}) {
  const { t } = useTranslation();

  const descriptions: Record<typeof mode, string> = {
    blocked: t('offline.blockedDescription'),
    offline: t('offline.offlineDescription'),
    online: t('offline.readyDescription'),
  };

  return (
    <div className="rounded-[24px] border border-white/8 bg-white/[0.03] px-5 py-4 text-[13px] leading-relaxed text-white/45 backdrop-blur-sm">
      {descriptions[mode]}
    </div>
  );
});

// ─── Main Page ─────────────────────────────────────────────

export const OfflinePage = React.memo(() => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const appMode = useAppStatusStore((s) =>
    s.soundcloudBlocked
      ? 'blocked'
      : !s.navigatorOnline || !s.backendReachable
        ? 'offline'
        : 'online',
  );
  const [state, setState] = useState<OfflineLibraryState>(EMPTY_STATE);
  const [loading, setLoading] = useState(true);
  const [pendingStats, setPendingStats] = useState<PendingStats>(EMPTY_STATS);
  const [syncing, setSyncing] = useState(false);
  const bgFetchDone = useRef(false);

  useEffect(() => {
    let cancelled = false;

    // 1) Instant load from offline-index (cached on disk)
    const loadOffline = async () => {
      const [likedTracks, cachedUrns] = await Promise.all([
        getOfflineLikedTracks(),
        listCachedUrns(),
      ]);
      const cachedSet = new Set(cachedUrns);
      const cachedTracks = await getOfflineTracksByUrns(cachedUrns);
      if (cancelled) return;

      setState({ likedTracks, cachedTracks, cachedUrns: cachedSet });
      setLoading(false);
    };

    // 2) Background: fetch ALL likes from API, save to offline-index, then update state once
    const syncAllLikes = async () => {
      if (bgFetchDone.current) return;
      try {
        const allLikes = await fetchAllLikedTracks();
        bgFetchDone.current = true;
        if (cancelled) return;

        // Re-read cached urns (cheap FS op) and rebuild state
        const cachedUrns = await listCachedUrns();
        const cachedSet = new Set(cachedUrns);
        const cachedTracks = await getOfflineTracksByUrns(cachedUrns);
        if (cancelled) return;

        setState({ likedTracks: allLikes, cachedTracks, cachedUrns: cachedSet });
      } catch {
        // Offline or banned — offline-index data is enough
      }
    };

    void loadOffline().then(() => {
      if (!cancelled) void syncAllLikes();
    });

    return () => { cancelled = true; };
  }, []);

  // Load pending actions stats
  useEffect(() => {
    const loadStats = () => {
      api<PendingStats>('/pending-actions/stats')
        .then(setPendingStats)
        .catch(() => {});
    };

    loadStats();
    const interval = setInterval(loadStats, 30000);
    return () => clearInterval(interval);
  }, []);

  const handleSync = useCallback(() => {
    setSyncing(true);
    api<{ synced: number; failed: number }>('/pending-actions/sync', { method: 'POST' })
      .then(() => {
        // Refresh stats
        api<PendingStats>('/pending-actions/stats')
          .then(setPendingStats)
          .catch(() => {});
      })
      .catch(() => {})
      .finally(() => setSyncing(false));
  }, []);

  const cachedLikesCount = useMemo(
    () => state.likedTracks.filter((track) => state.cachedUrns.has(track.urn)).length,
    [state.cachedUrns, state.likedTracks],
  );

  function getStatusTitle(mode: 'online' | 'offline' | 'blocked') {
    if (mode === 'blocked') return t('offline.blockedTitle');
    if (mode === 'offline') return t('offline.offlineTitle');
    return t('offline.readyTitle');
  }

  const statusTitle = getStatusTitle(appMode);

  return (
    <div className="relative min-h-full overflow-hidden px-6 py-6 md:px-8 md:py-8">
      {/* Ambient glow background */}
      <div className="pointer-events-none absolute inset-0" style={{ contain: 'strict', transform: 'translateZ(0)' }}>
        <div className="absolute left-[-10%] top-[-8%] h-[480px] w-[480px] rounded-full bg-accent/[0.07] blur-[140px]" />
        <div className="absolute bottom-[-14%] right-[-10%] h-[520px] w-[520px] rounded-full bg-sky-400/[0.05] blur-[160px]" />
        {appMode === 'blocked' && (
          <div className="absolute left-[40%] top-[20%] h-[300px] w-[300px] rounded-full bg-amber-500/[0.04] blur-[120px]" />
        )}
      </div>

      <div className="relative mx-auto flex w-full max-w-[1320px] flex-col gap-5" style={{ isolation: 'isolate' }}>
        {/* Header */}
        <div className="flex flex-wrap items-center gap-3">
          <StatusBadge />
          <div className="text-[24px] font-semibold tracking-[-0.03em] text-white/94">
            {statusTitle}
          </div>

          <div className="h-5 w-px bg-white/8" />

          <div className="rounded-full border border-white/8 bg-white/[0.04] px-3 py-1.5 text-[12px] font-medium text-white/50">
            {t('offline.statsLikes')}:{' '}
            <span className="text-white/85">{state.likedTracks.length}</span>
          </div>
          <div className="rounded-full border border-white/8 bg-white/[0.04] px-3 py-1.5 text-[12px] font-medium text-white/50">
            {t('offline.statsCached')}:{' '}
            <span className="text-white/85">{state.cachedTracks.length}</span>
          </div>

          <PendingBadge stats={pendingStats} syncing={syncing} onSync={handleSync} />

          <button
            type="button"
            onClick={() => {
              useAppStatusStore.getState().resetConnectivity();
              navigate('/home');
            }}
            className="ml-auto inline-flex cursor-pointer items-center gap-2 rounded-[16px] border border-white/10 bg-white/[0.06] px-4 py-2.5 text-[13px] font-semibold text-white/80 shadow-[0_2px_8px_rgba(0,0,0,0.1)] transition-all hover:bg-white/[0.10] hover:border-white/14"
          >
            <RotateCcw size={15} />
            {t('offline.tryOnline')}
          </button>
        </div>

        {/* Description */}
        <DescriptionCard mode={appMode} />

        {/* Content */}
        {loading ? (
          <div className="grid gap-6 xl:grid-cols-2">
            {Array.from({ length: 2 }).map((_, index) => (
              <div
                key={index}
                className="h-[420px] animate-pulse rounded-[34px] border border-white/6 bg-white/[0.02] backdrop-blur-[24px]"
              />
            ))}
          </div>
        ) : (
          <div className="grid gap-6 xl:grid-cols-2">
            <OfflineSection
              icon={<Heart size={18} />}
              title={t('offline.likesTitle')}
              subtitle={t('offline.likesSubtitle', {
                cached: cachedLikesCount,
                total: state.likedTracks.length,
              })}
              items={state.likedTracks}
              cachedUrns={state.cachedUrns}
              emptyText={t('offline.likesEmpty')}
              likesMode
            />
            <OfflineSection
              icon={<Download size={18} />}
              title={t('offline.cachedTitle')}
              subtitle={t('offline.cachedSubtitle')}
              items={state.cachedTracks}
              cachedUrns={state.cachedUrns}
              emptyText={t('offline.cachedEmpty')}
            />
          </div>
        )}
      </div>
    </div>
  );
});
