import {listen} from '@tauri-apps/api/event';
import {useCallback, useEffect, useState} from 'react';
import {useTranslation} from 'react-i18next';
import {toast} from 'sonner';
import {
  cacheLikedTracks,
  cancelCacheLikes,
  clearCache,
  clearImageCache,
  clearLikedCache,
  getCacheSize,
  getImageCacheSize,
  getLikedCacheSize,
  isCacheLikesRunning,
  type LikeCacheEntry,
} from '../../../lib/cache';
import {formatBytes} from '../../../lib/formatters';
import {Database, Download, Loader2, Trash2, X} from '../../../lib/icons';
import {useSettingsStore} from '../../../stores/settings';
import {Skeleton} from '../../ui/Skeleton';
import {Card, Divider, RangeSlider} from '../primitives';

function CacheRow({
                    label,
                    size,
                    clearing,
                    onClear,
                    clearLabel,
                  }: {
  label: string;
  size: number | null;
  clearing: boolean;
  onClear: () => void;
  clearLabel: string;
}) {
  return (
      <div className="flex items-center justify-between py-3">
        <div>
          <p className="text-[13px] text-white/60 font-medium">{label}</p>
          <div className="h-[25px] flex items-center">
            {size === null ? (
                <Skeleton className="w-25 h-[20px]"/>
            ) : (
                <p className="text-[17px] font-bold text-white/90 tabular-nums">{formatBytes(size)}</p>
            )}
          </div>
        </div>
        <button
            type="button"
            onClick={onClear}
            disabled={clearing || size === 0}
            className="flex items-center gap-2 px-4 py-2 rounded-xl text-[12px] font-semibold bg-red-500/10 text-red-400 hover:bg-red-500/20 border border-red-500/10 hover:border-red-500/20 transition-all duration-300 disabled:opacity-30 cursor-pointer disabled:cursor-not-allowed"
        >
          {clearing ? <Loader2 size={12} className="animate-spin"/> : <Trash2 size={12}/>}
          {clearLabel}
        </button>
      </div>
  );
}

export function CacheCard() {
  const {t} = useTranslation();
  const audioCacheLimitMB = useSettingsStore((s) => s.audioCacheLimitMB);
  const setAudioCacheLimitMB = useSettingsStore((s) => s.setAudioCacheLimitMB);
  const [audioSize, setAudioSize] = useState<number | null>(null);
  const [imagesSize, setImagesSize] = useState<number | null>(null);
  const [likedSize, setLikedSize] = useState<number | null>(null);
  const [clearingAudio, setClearingAudio] = useState(false);
  const [clearingImages, setClearingImages] = useState(false);
  const [clearingLiked, setClearingLiked] = useState(false);
  const [cachingLikes, setCachingLikes] = useState(false);
  const [progress, setProgress] = useState<{
    done: number;
    total: number;
    failed: number;
    skipped: number;
  } | null>(null);

  const refreshLikedSize = useCallback(() => {
    void getLikedCacheSize().then(setLikedSize);
  }, []);

  useEffect(() => {
    void getCacheSize().then(setAudioSize);
    void getImageCacheSize().then(setImagesSize);
    refreshLikedSize();
    void isCacheLikesRunning().then((running) => {
      if (running) setCachingLikes(true);
    });
  }, [refreshLikedSize]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listen<{
      phase: 'start' | 'progress' | 'done' | 'cancelled';
      total: number;
      done: number;
      failed: number;
      skipped: number;
    }>('track:cache-likes-progress', (event) => {
      const p = event.payload;
      if (p.phase === 'start') {
        setCachingLikes(true);
        setProgress({done: 0, total: p.total, failed: 0, skipped: 0});
      } else if (p.phase === 'progress') {
        setProgress({done: p.done, total: p.total, failed: p.failed, skipped: p.skipped});
      } else {
        setCachingLikes(false);
        setProgress(null);
        refreshLikedSize();
        if (p.phase === 'done') {
          toast.success(t('settings.cacheLikesDone', {done: p.done - p.failed, total: p.total}));
        }
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [refreshLikedSize, t]);

  const clearWith = useCallback(
      async (
          fn: () => Promise<unknown>,
          setBusy: (b: boolean) => void,
          setSize: (n: number) => void,
      ) => {
        setBusy(true);
        try {
          await fn();
          setSize(0);
          toast.success(t('settings.cacheCleared'));
        } catch {
          toast.error(t('common.error'));
        } finally {
          setBusy(false);
        }
      },
      [t],
  );

  const handleCacheLikes = useCallback(async () => {
    setCachingLikes(true);
    try {
      const [
        {fetchAllLikedTracks},
        {buildStorageUrls, downloadFallbackUrls, streamFallbackUrls, getSessionId},
      ] = await Promise.all([import('../../../lib/hooks'), import('../../../lib/api')]);
      const hq = useSettingsStore.getState().highQualityStreaming;
      const sessionId = getSessionId();
      const tracks = await fetchAllLikedTracks(200);
      const entries: LikeCacheEntry[] = tracks.map((track) => ({
        urn: track.urn,
        urls: streamFallbackUrls(track.urn, hq),
        downloadUrls: downloadFallbackUrls(track.urn, hq),
        storageUrls: buildStorageUrls(track.urn),
        sessionId,
        hq,
      }));
      if (entries.length === 0) {
        setCachingLikes(false);
        toast(t('settings.cacheLikesEmpty'));
        return;
      }
      await cacheLikedTracks(entries);
    } catch (err) {
      setCachingLikes(false);
      setProgress(null);
      toast.error(String(err));
    }
  }, [t]);

  const totalSize = (audioSize ?? 0) + (imagesSize ?? 0) + (likedSize ?? 0);
  const allLoaded = audioSize !== null && imagesSize !== null && likedSize !== null;
  const limitLabel =
      audioCacheLimitMB <= 0
          ? t('settings.unlimited')
          : audioCacheLimitMB >= 1024
              ? `${(audioCacheLimitMB / 1024).toFixed(audioCacheLimitMB % 1024 === 0 ? 0 : 1)} GB`
              : `${audioCacheLimitMB} MB`;
  const progressPct =
      progress && progress.total > 0
          ? Math.min(100, Math.round((progress.done / progress.total) * 100))
          : 0;

  return (
      <Card
          title={t('settings.cache')}
          icon={<Database size={17}/>}
          action={
            allLoaded ? (
                <span className="text-[12px] text-white/30 tabular-nums">
            {t('settings.total')}: {formatBytes(totalSize)}
          </span>
            ) : (
                <Skeleton className="h-[12px] w-[80px]"/>
            )
          }
      >
        <div className="divide-y divide-white/[0.04]">
          <CacheRow
              label={t('settings.audioCacheSize')}
              size={audioSize}
              clearing={clearingAudio}
              clearLabel={t('settings.clearCache')}
              onClear={() => clearWith(clearCache, setClearingAudio, setAudioSize)}
          />
          <CacheRow
              label={t('settings.assetsCacheSize')}
              size={imagesSize}
              clearing={clearingImages}
              clearLabel={t('settings.clearCache')}
              onClear={() => clearWith(clearImageCache, setClearingImages, setImagesSize)}
          />
          <CacheRow
              label={t('settings.likedCacheSize')}
              size={likedSize}
              clearing={clearingLiked}
              clearLabel={t('settings.clearCache')}
              onClear={() => clearWith(clearLikedCache, setClearingLiked, setLikedSize)}
          />
        </div>

        <div className="pt-3 space-y-2">
          <p className="text-[11px] text-white/30">{t('settings.cacheLikesDesc')}</p>
          {cachingLikes ? (
              <div className="space-y-2">
                <div className="flex items-center justify-between text-[12px] text-white/60">
              <span className="flex items-center gap-2">
                <Loader2 size={12} className="animate-spin"/>
                {progress
                    ? t('settings.cacheLikesProgress', {done: progress.done, total: progress.total})
                    : t('settings.cacheLikesStarting')}
              </span>
                  {progress && progress.failed > 0 && (
                      <span className="text-red-400/80 tabular-nums">
                  {t('settings.cacheLikesFailed', {count: progress.failed})}
                </span>
                  )}
                </div>
                <div className="h-1 bg-white/[0.06] rounded-full overflow-hidden">
                  <div
                      className="h-full bg-[var(--color-accent)] transition-[width] duration-300"
                      style={{width: `${progressPct}%`}}
                  />
                </div>
                <button
                    type="button"
                    onClick={() => void cancelCacheLikes()}
                    className="flex items-center gap-2 px-4 py-2 rounded-xl text-[12px] font-semibold bg-white/[0.04] text-white/60 hover:bg-white/[0.08] border border-white/[0.06] hover:border-white/[0.12] transition-all duration-200 cursor-pointer"
                >
                  <X size={12}/>
                  {t('common.cancel')}
                </button>
              </div>
          ) : (
              <button
                  type="button"
                  onClick={handleCacheLikes}
                  className="flex items-center gap-2 px-4 py-2 rounded-xl text-[12px] font-semibold bg-white/[0.06] text-white/75 hover:bg-white/[0.1] border border-white/[0.06] hover:border-white/[0.12] transition-all duration-200 cursor-pointer"
              >
                <Download size={12}/>
                {t('settings.cacheLikes')}
              </button>
          )}
        </div>

        <Divider/>
        <div className="pt-3 space-y-3">
          <div className="flex items-center justify-between">
            <div>
              <p className="text-[13px] text-white/60 font-medium">{t('settings.audioCacheLimit')}</p>
              <p className="text-[11px] text-white/30 mt-0.5">{t('settings.audioCacheLimitDesc')}</p>
            </div>
            <span className="text-[12px] text-white/30 tabular-nums">{limitLabel}</span>
          </div>
          <RangeSlider
              value={audioCacheLimitMB}
              min={0}
              max={8192}
              step={256}
              onChange={setAudioCacheLimitMB}
          />
        </div>
      </Card>
  );
}
