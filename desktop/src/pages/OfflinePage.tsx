import { useCallback, useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { toast } from 'sonner';
import type { OfflineEntry } from '../components/offline/types';
import { useOfflineLibrary } from '../components/offline/useOfflineLibrary';
import { clearCache, clearLikedCache, ensureTrackCached } from '../lib/cache';
import { art, dur } from '../lib/formatters';
import { Download, Loader2, Pause, Play, Shuffle, Trash2 } from '../lib/icons';
import { useCacheLikes } from '../lib/likes-cache';
import { useIsPlayingFrom } from '../lib/useTrackPlay';
import { withViewTransition } from '../lib/view-transition';
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
  const navigate = useNavigate();
  const lib = useOfflineLibrary();
  const cacheLikes = useCacheLikes(() => void lib.refreshInventory());
  const hasSession = useAuthStore((s) => s.hasSession);
  const online = lib.appMode === 'online';
  const [section, setSection] = useState<'likes' | 'cached'>('likes');
  const [query, setQuery] = useState('');
  const [clearingCache, setClearingCache] = useState(false);

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
  const playableUrns = useMemo(() => new Set(playable.map((t) => t.urn)), [playable]);
  // Same toggle semantics as the track/playlist hero control and the profile
  // likes button: play from the top, pause when this list plays, resume when
  // the current track belongs to it.
  const isPlayingThis = useIsPlayingFrom(playableUrns);
  const playAll = useCallback(() => {
    if (!playable.length) return;
    const { play, pause, resume, currentTrack } = usePlayerStore.getState();
    if (isPlayingThis) {
      pause();
      return;
    }
    if (currentTrack && playableUrns.has(currentTrack.urn)) {
      resume();
      return;
    }
    void play(playable[0], playable);
  }, [playable, playableUrns, isPlayingThis]);

  const handleSignIn = useCallback(() => {
    // Dropping the offline bypass re-renders App into <Login/>; there is no
    // /login route, so just reset the location to a real one.
    useAppStatusStore.getState().setOfflineBypass(false);
    navigate('/');
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

  const clearAllCache = useCallback(async () => {
    if (clearingCache) return;
    setClearingCache(true);
    try {
      await clearCache();
      await clearLikedCache();
      await lib.refreshInventory();
      toast.success('Cache cleared');
    } catch {
      toast.error('Something went wrong');
    } finally {
      setClearingCache(false);
    }
  }, [clearingCache, lib.refreshInventory]);

  const currentUrn = usePlayerStore((s) => s.currentTrack?.urn);
  const isPlaying = usePlayerStore((s) => s.isPlaying);

  return (
    <div className="px-5 py-6 md:px-8">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-[24px] font-semibold tracking-tight text-white/92">
          {'Local library'}
        </h1>
        <div className="flex items-center gap-2">
          <span className="text-[11px] text-white/40">{online ? 'online' : 'offline'}</span>
          {!online && (
            <button
              type="button"
              onClick={handleTryOnline}
              className="rounded-lg border border-white/[0.1] px-3 py-1.5 text-[12px] text-white/70 hover:bg-white/[0.06]"
            >
              {'Try online again'}
            </button>
          )}
          {!hasSession && (
            <button
              type="button"
              onClick={handleSignIn}
              className="rounded-lg px-3 py-1.5 text-[12px] font-semibold text-white"
              style={{ background: 'var(--color-accent)' }}
            >
              {'Sign in'}
            </button>
          )}
        </div>
      </div>

      <p className="mt-1 text-[12px] text-white/40">
        {lib.stats.cachedCount} files · {formatBytes(lib.stats.totalBytes)} · {'Likes coverage'}{' '}
        {lib.stats.likedCachedCount}/{lib.stats.likedCount}
      </p>

      <div className="mt-4 flex flex-wrap items-center gap-2">
        <button
          type="button"
          onClick={() => withViewTransition(() => setSection('likes'))}
          className={`relative rounded-lg px-3 py-1.5 text-[12px] font-medium transition-colors ${
            section === 'likes' ? 'text-white/90' : 'text-white/45 hover:text-white/70'
          }`}
        >
          <span
            aria-hidden
            className={`pointer-events-none absolute inset-0 rounded-lg ${
              section === 'likes' ? 'vt-offline-pill bg-white/[0.1]' : ''
            }`}
          />
          <span className="relative">
            {'Likes'} {lib.likesEntries.length}
          </span>
        </button>
        <button
          type="button"
          onClick={() => withViewTransition(() => setSection('cached'))}
          className={`relative rounded-lg px-3 py-1.5 text-[12px] font-medium transition-colors ${
            section === 'cached' ? 'text-white/90' : 'text-white/45 hover:text-white/70'
          }`}
        >
          <span
            aria-hidden
            className={`pointer-events-none absolute inset-0 rounded-lg ${
              section === 'cached' ? 'vt-offline-pill bg-white/[0.1]' : ''
            }`}
          />
          <span className="relative">
            {'Cache'} {lib.cachedEntries.length}
          </span>
        </button>

        <button
          type="button"
          disabled={!playable.length}
          onClick={playAll}
          aria-label={isPlayingThis ? 'Pause' : 'Play'}
          className="ml-2 w-[48px] h-[48px] shrink-0 rounded-full border border-white/[0.18] hover:border-white/[0.4] flex items-center justify-center text-white/90 hover:text-white transition-colors cursor-pointer disabled:opacity-40 disabled:cursor-default"
        >
          {isPlayingThis ? (
            <span key="pause" className="animate-icon-pop flex items-center justify-center">
              <Pause size={18} fill="currentColor" strokeWidth={0} />
            </span>
          ) : (
            <span key="play" className="animate-icon-pop flex items-center justify-center">
              <Play size={18} fill="currentColor" strokeWidth={0} className="ml-0.5" />
            </span>
          )}
        </button>
        <button
          type="button"
          disabled={!playable.length}
          onClick={() => {
            const shuffled = [...playable].sort(() => Math.random() - 0.5);
            void usePlayerStore.getState().play(shuffled[0], shuffled);
          }}
          className="flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-[12px] text-white/60 hover:text-white/90 hover:bg-white/[0.06] transition-colors disabled:opacity-40"
        >
          <Shuffle size={13} /> {'Shuffle'}
        </button>

        {section === 'likes' && (
          <button
            type="button"
            disabled={cacheLikes.caching}
            onClick={() => void cacheLikes.start().catch(() => {})}
            className="flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-[12px] text-white/60 hover:text-white/90 hover:bg-white/[0.06] transition-colors disabled:opacity-40"
          >
            {cacheLikes.caching ? (
              <>
                <Loader2 size={13} className="animate-spin" />
                {cacheLikes.progress
                  ? `${cacheLikes.progress.done}/${cacheLikes.progress.total}`
                  : 'Collecting the list…'}
              </>
            ) : (
              <>
                <Download size={13} /> {'Download all likes'}
              </>
            )}
          </button>
        )}

        {section === 'cached' && (
          <button
            type="button"
            disabled={clearingCache || lib.cachedEntries.length === 0}
            onClick={() => void clearAllCache()}
            className="flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-[12px] text-white/60 hover:text-white/90 hover:bg-white/[0.06] transition-colors disabled:opacity-40"
          >
            {clearingCache ? <Loader2 size={13} className="animate-spin" /> : <Trash2 size={13} />}
            {'Clear cache'}
          </button>
        )}

        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          autoComplete="off"
          placeholder={'Search by title or artist'}
          className="ml-auto w-56 rounded-lg border border-white/[0.08] bg-white/[0.04] px-3 py-1.5 text-[12px] text-white/80 outline-none placeholder:text-white/25 focus:border-white/20"
        />
      </div>

      <div className="mt-4 flex flex-col">
        {lib.loading ? (
          <p className="py-6 text-[13px] text-white/35">{'Loading...'}</p>
        ) : entries.length === 0 ? (
          <p className="py-6 text-[13px] text-white/35">
            {query.trim()
              ? 'Nothing matches your search'
              : section === 'likes'
                ? 'No liked tracks have been cached on this device yet.'
                : 'The cache is empty.'}
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
                  title={'Play'}
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
                    title={'Download to device'}
                  >
                    <Download size={14} />
                  </button>
                ) : (
                  <button
                    type="button"
                    onClick={() => void lib.removeCached(entry.urn)}
                    className="rounded p-1.5 text-white/40 hover:bg-white/[0.06] hover:text-white/80"
                    title={'Remove from cache'}
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
