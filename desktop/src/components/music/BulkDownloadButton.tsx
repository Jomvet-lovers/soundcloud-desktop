import React, { useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { useBulkCacheStore } from '../../lib/bulk-cache';
import { Download, Loader2, X } from '../../lib/icons';
import type { Track } from '../../stores/player';

interface Props {
  cacheKey: string;
  getTracks: () => Promise<Track[]> | Track[];
  className?: string;
  variant?: 'pill' | 'compact';
}

export const BulkDownloadButton = React.memo(function BulkDownloadButton({
  cacheKey,
  getTracks,
  className,
  variant = 'pill',
}: Props) {
  const { t } = useTranslation();
  const progress = useBulkCacheStore((s) => s.entries[cacheKey]);

  const handleClick = useCallback(() => {
    const { entries, start, cancel } = useBulkCacheStore.getState();
    if (entries[cacheKey]) {
      cancel(cacheKey);
      return;
    }
    start(cacheKey, getTracks, {
      success: (n) => t('cache.bulkSuccess', { count: n }),
      failed: (n) => t('cache.bulkFailed', { count: n }),
      allCached: () => t('cache.allCached'),
    });
  }, [cacheKey, getTracks, t]);

  const active = !!progress;
  const preparing = !!progress?.preparing;

  const icon = preparing ? (
    <Loader2 size={variant === 'compact' ? 14 : 16} className="animate-spin" />
  ) : active ? (
    <X size={variant === 'compact' ? 14 : 16} />
  ) : (
    <Download size={variant === 'compact' ? 14 : 16} />
  );

  const label =
    active && !preparing ? (
      <span className="tabular-nums">
        {progress.done}/{progress.total}
      </span>
    ) : (
      <span>{t('cache.bulkCache')}</span>
    );

  if (variant === 'compact') {
    return (
      <button
        type="button"
        onClick={handleClick}
        title={active ? t('cache.cancel') : t('cache.bulkCache')}
        className={`inline-flex items-center gap-2 px-3 py-2 rounded-xl text-[12px] font-medium transition-all duration-200 cursor-pointer ${
          active
            ? 'bg-accent/15 text-accent border border-accent/20'
            : 'glass hover:bg-white/[0.05] text-white/60 hover:text-white/80'
        } ${className ?? ''}`}
      >
        {icon}
        {label}
      </button>
    );
  }

  return (
    <button
      type="button"
      onClick={handleClick}
      title={active ? t('cache.cancel') : t('cache.bulkCache')}
      className={`inline-flex items-center gap-2 px-4 py-2.5 rounded-xl text-sm font-medium transition-all duration-200 ease-[var(--ease-apple)] cursor-pointer ${
        active
          ? 'bg-accent/15 text-accent border border-accent/20 shadow-[0_0_20px_rgba(255,85,0,0.1)]'
          : 'glass hover:bg-white/[0.05] text-white/60 hover:text-white/80'
      } ${className ?? ''}`}
    >
      {icon}
      {label}
      {active && !preparing && progress.total > 0 && (
        <span className="relative w-16 h-1 rounded-full bg-white/10 overflow-hidden" aria-hidden>
          <span
            className="absolute inset-y-0 left-0 bg-accent transition-[width] duration-300"
            style={{ width: `${(progress.done / progress.total) * 100}%` }}
          />
        </span>
      )}
    </button>
  );
});
