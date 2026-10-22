import { useCallback, useEffect, useState } from 'react';
import { toast } from 'sonner';
import {
  clearCache,
  clearImageCache,
  clearLikedCache,
  getCacheSize,
  getImageCacheSize,
  getLikedCacheSize,
  getTranscodeStatus,
  retryTranscode,
  type TranscodeStatus,
} from '../../../lib/cache';
import { formatBytes } from '../../../lib/formatters';
import { Database, Download, Loader2, RefreshCw, Trash2, X } from '../../../lib/icons';
import { useCacheLikes } from '../../../lib/likes-cache';
import { useSettingsStore } from '../../../stores/settings';
import { Skeleton } from '../../ui/Skeleton';
import { Card, Divider, RangeSlider } from '../primitives';

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
            <Skeleton className="w-25 h-[20px]" />
          ) : (
            <p className="text-[17px] font-bold text-white/90 tabular-nums">{formatBytes(size)}</p>
          )}
        </div>
      </div>
      <button
        type="button"
        onClick={onClear}
        disabled={clearing || size === 0}
        className="flex items-center gap-2 px-4 py-2 rounded-md text-[12px] font-semibold bg-red-500/10 text-red-400 hover:bg-red-500/20 border border-red-500/10 hover:border-red-500/20 transition-all duration-300 disabled:opacity-30 cursor-pointer disabled:cursor-not-allowed"
      >
        {clearing ? <Loader2 size={12} className="animate-spin" /> : <Trash2 size={12} />}
        {clearLabel}
      </button>
    </div>
  );
}

export function CacheCard() {
  const audioCacheLimitMB = useSettingsStore((s) => s.audioCacheLimitMB);
  const setAudioCacheLimitMB = useSettingsStore((s) => s.setAudioCacheLimitMB);
  const [audioSize, setAudioSize] = useState<number | null>(null);
  const [imagesSize, setImagesSize] = useState<number | null>(null);
  const [likedSize, setLikedSize] = useState<number | null>(null);
  const [clearingAudio, setClearingAudio] = useState(false);
  const [clearingImages, setClearingImages] = useState(false);
  const [clearingLiked, setClearingLiked] = useState(false);
  const [transcoder, setTranscoder] = useState<TranscodeStatus | null>(null);
  const [retryingEncoder, setRetryingEncoder] = useState(false);

  const refreshTranscoder = useCallback(() => {
    void getTranscodeStatus().then(setTranscoder).catch(console.error);
  }, []);

  const refreshLikedSize = useCallback(() => {
    void getLikedCacheSize().then(setLikedSize);
  }, []);

  const {
    caching: cachingLikes,
    progress,
    start: startLikes,
    cancel: cancelLikes,
  } = useCacheLikes((p) => {
    refreshLikedSize();
    if (p.phase === 'done') {
      toast.success(`Done: ${p.done - p.failed} of ${p.total}`);
    }
  });

  useEffect(() => {
    void getCacheSize().then(setAudioSize);
    void getImageCacheSize().then(setImagesSize);
    refreshLikedSize();
  }, [refreshLikedSize]);

  useEffect(() => {
    refreshTranscoder();
  }, [refreshTranscoder]);

  // The first-run ffmpeg download runs in the background; poll while it is
  // still preparing so the card flips to Ready without reopening Settings.
  useEffect(() => {
    if (transcoder?.ffmpeg !== 'preparing') return;
    const id = setInterval(refreshTranscoder, 2500);
    return () => clearInterval(id);
  }, [transcoder?.ffmpeg, refreshTranscoder]);

  const handleRetryEncoder = useCallback(async () => {
    setRetryingEncoder(true);
    try {
      const next = await retryTranscode();
      setTranscoder(next);
      if (next.ffmpeg === 'ready') toast.success('Audio encoder ready');
      else toast.error('The audio encoder could not be downloaded');
    } catch (err) {
      toast.error(String(err));
    } finally {
      setRetryingEncoder(false);
    }
  }, []);

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
        toast.success('Cache cleared');
      } catch {
        toast.error('Something went wrong');
      } finally {
        setBusy(false);
      }
    },
    [],
  );

  const handleCacheLikes = useCallback(async () => {
    try {
      const queued = await startLikes();
      if (queued === 0) toast('No liked tracks');
    } catch (err) {
      toast.error(String(err));
    }
  }, [startLikes]);

  const totalSize = (audioSize ?? 0) + (imagesSize ?? 0) + (likedSize ?? 0);
  const allLoaded = audioSize !== null && imagesSize !== null && likedSize !== null;
  const encoderState = transcoder?.ffmpeg ?? null;
  const encoderLabel =
    encoderState === 'ready'
      ? 'Ready'
      : encoderState === 'preparing'
        ? 'Preparing'
        : encoderState === 'unavailable'
          ? 'Unavailable'
          : '';
  const encoderDetail =
    encoderState === 'ready'
      ? 'Used for MP3, FLAC, WAV and cover art embedding'
      : encoderState === 'preparing'
        ? 'Downloading once in the background. MP3, FLAC and WAV need it'
        : encoderState === 'unavailable'
          ? 'Download failed. Check the connection, or export as M4A'
          : 'MP3, FLAC and WAV need the encoder';
  const limitLabel =
    audioCacheLimitMB <= 0
      ? 'Unlimited'
      : audioCacheLimitMB >= 1024
        ? `${(audioCacheLimitMB / 1024).toFixed(audioCacheLimitMB % 1024 === 0 ? 0 : 1)} GB`
        : `${audioCacheLimitMB} MB`;
  const progressPct =
    progress && progress.total > 0
      ? Math.min(100, Math.round((progress.done / progress.total) * 100))
      : 0;

  return (
    <Card
      title={'Cache'}
      icon={<Database size={17} />}
      action={
        allLoaded ? (
          <span className="text-[12px] text-white/30 tabular-nums">
            {'Total'}: {formatBytes(totalSize)}
          </span>
        ) : (
          <Skeleton className="h-[12px] w-[80px]" />
        )
      }
    >
      <div className="divide-y divide-white/[0.04]">
        <CacheRow
          label={'Audio'}
          size={audioSize}
          clearing={clearingAudio}
          clearLabel={'Clear'}
          onClear={() => clearWith(clearCache, setClearingAudio, setAudioSize)}
        />
        <CacheRow
          label={'Images & assets'}
          size={imagesSize}
          clearing={clearingImages}
          clearLabel={'Clear'}
          onClear={() => clearWith(clearImageCache, setClearingImages, setImagesSize)}
        />
        <CacheRow
          label={'Likes (protected cache)'}
          size={likedSize}
          clearing={clearingLiked}
          clearLabel={'Clear'}
          onClear={() => clearWith(clearLikedCache, setClearingLiked, setLikedSize)}
        />
      </div>

      <div className="pt-3 space-y-2">
        <p className="text-[11px] text-white/30">
          {'Downloads all likes into a separate folder that bypasses the audio cache limit.'}
        </p>
        {cachingLikes ? (
          <div className="space-y-2">
            <div className="flex items-center justify-between text-[12px] text-white/60">
              <span className="flex items-center gap-2">
                <Loader2 size={12} className="animate-spin" />
                {progress ? `${progress.done} of ${progress.total}` : 'Preparing...'}
              </span>
              {progress && progress.failed > 0 && (
                <span className="text-red-400/80 tabular-nums">{`failed: ${progress.failed}`}</span>
              )}
            </div>
            <div className="h-1 bg-white/[0.06] rounded-full overflow-hidden">
              <div
                className="h-full bg-[var(--color-accent)] transition-[width] duration-300"
                style={{ width: `${progressPct}%` }}
              />
            </div>
            <button
              type="button"
              onClick={cancelLikes}
              className="flex items-center gap-2 px-4 py-2 rounded-md text-[12px] font-semibold bg-white/[0.04] text-white/60 hover:bg-white/[0.08] border border-white/[0.06] hover:border-white/[0.12] transition-all duration-200 cursor-pointer"
            >
              <X size={12} />
              {'Cancel'}
            </button>
          </div>
        ) : (
          <button
            type="button"
            onClick={handleCacheLikes}
            className="flex items-center gap-2 px-4 py-2 rounded-md text-[12px] font-semibold bg-white/[0.06] text-white/75 hover:bg-white/[0.1] border border-white/[0.06] hover:border-white/[0.12] transition-all duration-200 cursor-pointer"
          >
            <Download size={12} />
            {'Cache liked tracks'}
          </button>
        )}
      </div>

      <Divider />
      <div className="pt-3 space-y-3">
        <div className="flex items-center justify-between">
          <div>
            <p className="text-[13px] text-white/60 font-medium">{'Audio cache limit'}</p>
            <p className="text-[11px] text-white/30 mt-0.5">
              {'Older cached tracks are deleted automatically when the limit is exceeded'}
            </p>
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

      <Divider />
      <div className="pt-3 flex items-center justify-between gap-4">
        <div className="min-w-0">
          <p className="text-[13px] text-white/60 font-medium">{'Audio encoder'}</p>
          <p className="text-[11px] text-white/30 mt-0.5">{encoderDetail}</p>
        </div>
        {encoderState === 'unavailable' ? (
          <div className="flex items-center gap-2 shrink-0">
            <span className="text-[12px] text-red-400/80">{'Unavailable'}</span>
            <button
              type="button"
              onClick={() => void handleRetryEncoder()}
              disabled={retryingEncoder}
              className="flex items-center gap-2 px-3 py-1.5 rounded-md text-[12px] font-semibold bg-white/[0.06] text-white/75 hover:bg-white/[0.1] border border-white/[0.06] hover:border-white/[0.12] transition-all duration-200 cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed"
            >
              {retryingEncoder ? (
                <Loader2 size={12} className="animate-spin" />
              ) : (
                <RefreshCw size={12} />
              )}
              {retryingEncoder ? 'Downloading' : 'Retry'}
            </button>
          </div>
        ) : (
          <span className="flex items-center gap-2 shrink-0 text-[12px] text-white/40">
            {encoderState === 'preparing' && <Loader2 size={12} className="animate-spin" />}
            {encoderLabel || '…'}
          </span>
        )}
      </div>
    </Card>
  );
}
