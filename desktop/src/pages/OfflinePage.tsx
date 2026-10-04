import { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import type { OfflineEntry } from '../components/offline/types';
import { useOfflineLibrary } from '../components/offline/useOfflineLibrary';
import { ensureTrackCached } from '../lib/cache';
import { art, dur } from '../lib/formatters';
import { Download, Loader2, Pause, Play, Shuffle, Trash2 } from '../lib/icons';
import { useCacheLikes } from '../lib/likes-cache';
import { useAppStatusStore } from '../stores/app-status';
import { useAuthStore } from '../stores/auth';
import { usePlayerStore } from '../stores/player';

function formatBytes(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${Math.round(bytes / 1024)} KB`;
}

/** Offline — downloaded library, plain layout. */
export function OfflinePage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const lib = useOfflineLibrary();
  const cacheLikes = useCacheLikes(() => void lib.refreshInventory());
  const hasSession = useAuthStore((s) => s.hasSession);
  const online = lib.appMode === 'online';
  const [section, setSection] = useState<'likes' | 'cached'>('likes');
  const [query, setQuery] = useState('');

  const entries = useMemo(() => {
    const base = section === 'likes' ? lib.likesEntries : lib.cachedEntries;
    const q = query.trim().toLowerCase();
    if (!q) return base;
    return base.filter(
      (e) =>
        e.track.title?.toLowerCase().includes(q) ||
        e.track.user?.username?.toLowerCase().includes(q),
    );
  }, [section, query, lib.likesEntries, lib.cachedEntries]);

  const playable = useMemo(
    () => entries.filter((e) => e.inv !== null).map((e) => e.track),
    [entries],
  );

  const handleSignIn = useCallback(() => {
    useAppStatusStore.getState().setOfflineBypass(false);
    navigate('/login');
  }, [navigate]);

  const handleTryOnline = useCallback(() => {
    useAppStatusStore.getState().resetConnectivity();
    navigate('/home');
  }, [navigate]);

  const playEntry = useCallback(
    (entry: OfflineEntry) => {
      void usePlayerStore.getState().play(entry.track, playable.length ? playable : [entry.track]);
    },
    [playable],
  );

  const downloadEntry = useCallback(
    (entry: OfflineEntry) => {
      void ensureTrackCached(entry.urn, undefined, entry.track.duration)
        .then(() => lib.refreshInventory())
        .catch(() => {});
    },
    [lib],
  );

  const currentUrn = usePlayerStore((s) => s.currentTrack?.urn);
  const isPlaying = usePlayerStore((s) => s.isPlaying);

  return (
    <div className="px-5 py-6 md:px-8">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-[24px] font-semibold tracking-tight text-white/92">
          {t('offline.title')}
        </h1>
        <div className="flex items-center gap-2">
          <span className="text-[11px] text-white/40">
            {online ? t('offline.netOnline') : t('offline.netOffline')}
          </span>
          {!online && (
            <button
              type="button"
              onClick={handleTryOnline}
              className="rounded-lg border border-white/[0.1] px-3 py-1.5 text-[12px] text-white/70 hover:bg-white/[0.06]"
            >
              {t('offline.tryOnline')}
            </button>
          )}
          {!hasSession && (
            <button
              type="button"
              onClick={handleSignIn}
              className="rounded-lg px-3 py-1.5 text-[12px] font-semibold text-white"
              style={{ background: 'var(--color-accent)' }}
            >
              {t('offline.signIn')}
            </button>
          )}
        </div>
      </div>

      <p className="mt-1 text-[12px] text-white/40">
        {lib.stats.cachedCount} files · {formatBytes(lib.stats.totalBytes)} ·{' '}
        {t('offline.likesCoverage')} {lib.stats.likedCachedCount}/{lib.stats.likedCount}
      </p>

      <div className="mt-4 flex flex-wrap items-center gap-2">
        <button
          type="button"
          onClick={() => setSection('likes')}
          className={`rounded-lg px-3 py-1.5 text-[12px] font-medium ${
            section === 'likes'
              ? 'bg-white/[0.1] text-white/90'
              : 'text-white/45 hover:bg-white/[0.05]'
          }`}
        >
          {t('offline.likesTitle')} {lib.likesEntries.length}
        </button>
        <button
          type="button"
          onClick={() => setSection('cached')}
          className={`rounded-lg px-3 py-1.5 text-[12px] font-medium ${
            section === 'cached'
              ? 'bg-white/[0.1] text-white/90'
              : 'text-white/45 hover:bg-white/[0.05]'
          }`}
        >
          {t('offline.cachedTitle')} {lib.cachedEntries.length}
        </button>

        <button
          type="button"
          disabled={!playable.length}
          onClick={() => void usePlayerStore.getState().play(playable[0], playable)}
          className="ml-2 flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-[12px] font-semibold text-white disabled:opacity-40"
          style={{ background: 'var(--color-accent)' }}
        >
          <Play size={13} /> {t('offline.playAll')}
        </button>
        <button
          type="button"
          disabled={!playable.length}
          onClick={() => {
            const shuffled = [...playable].sort(() => Math.random() - 0.5);
            void usePlayerStore.getState().play(shuffled[0], shuffled);
          }}
          className="flex items-center gap-1.5 rounded-lg border border-white/[0.1] px-3 py-1.5 text-[12px] text-white/70 hover:bg-white/[0.06] disabled:opacity-40"
        >
          <Shuffle size={13} /> {t('offline.shuffle')}
        </button>

        {section === 'likes' && (
          <button
            type="button"
            disabled={cacheLikes.caching}
            onClick={() => void cacheLikes.start().catch(() => {})}
            className="flex items-center gap-1.5 rounded-lg border border-white/[0.1] px-3 py-1.5 text-[12px] text-white/70 hover:bg-white/[0.06] disabled:opacity-40"
          >
            {cacheLikes.caching ? (
              <>
                <Loader2 size={13} className="animate-spin" />
                {cacheLikes.progress
                  ? `${cacheLikes.progress.done}/${cacheLikes.progress.total}`
                  : t('offline.ctaStarting')}
              </>
            ) : (
              <>
                <Download size={13} /> {t('offline.ctaCacheLikes')}
              </>
            )}
          </button>
        )}

        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          autoComplete="off"
          placeholder={t('offline.searchPlaceholder')}
          className="ml-auto w-56 rounded-lg border border-white/[0.08] bg-white/[0.04] px-3 py-1.5 text-[12px] text-white/80 outline-none placeholder:text-white/25 focus:border-white/20"
        />
      </div>

      <div className="mt-4 flex flex-col">
        {lib.loading ? (
          <p className="py-6 text-[13px] text-white/35">{t('common.loading')}</p>
        ) : entries.length === 0 ? (
          <p className="py-6 text-[13px] text-white/35">
            {query.trim()
              ? t('offline.searchEmpty')
              : section === 'likes'
                ? t('offline.likesEmpty')
                : t('offline.cachedEmpty')}
          </p>
        ) : (
          entries.map((entry) => {
            const isCurrent = currentUrn === entry.urn;
            const artwork = art(entry.track.artwork_url, 't120x120');
            return (
              <div
                key={entry.urn}
                className="group flex items-center gap-3 border-b border-white/[0.05] px-1 py-2"
              >
                <button
                  type="button"
                  onClick={() => playEntry(entry)}
                  disabled={entry.inv === null}
                  className="relative size-10 flex-none overflow-hidden rounded bg-white/[0.06] disabled:opacity-50"
                  title={t('offline.actPlay')}
                >
                  {artwork ? <img src={artwork} alt="" className="size-full object-cover" /> : null}
                  {isCurrent && isPlaying ? (
                    <span className="absolute inset-0 flex items-center justify-center bg-black/50">
                      <Pause size={14} />
                    </span>
                  ) : null}
                </button>

                <div className="min-w-0 flex-1">
                  <p className="truncate text-[13px] text-white/88">{entry.track.title}</p>
                  <p className="truncate text-[11px] text-white/40">{entry.track.user?.username}</p>
                </div>

                <span className="font-mono text-[11px] tabular-nums text-white/35">
                  {dur(entry.track.duration)}
                </span>

                {entry.inv === null ? (
                  <button
                    type="button"
                    onClick={() => downloadEntry(entry)}
                    className="rounded p-1.5 text-white/40 hover:bg-white/[0.06] hover:text-white/80"
                    title={t('offline.actDownload')}
                  >
                    <Download size={14} />
                  </button>
                ) : (
                  <button
                    type="button"
                    onClick={() => void lib.removeCached(entry.urn)}
                    className="rounded p-1.5 text-white/40 hover:bg-white/[0.06] hover:text-white/80"
                    title={t('offline.removeCached')}
                  >
                    <Trash2 size={14} />
                  </button>
                )}
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
