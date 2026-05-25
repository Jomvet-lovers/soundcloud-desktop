import { memo } from 'react';
import type { TrackScdMeta } from '../../stores/player';

type Variant = 'inline' | 'overlay';

interface Props {
  meta?: TrackScdMeta;
  variant?: Variant;
  showIndex?: boolean;
}

const baseStyle =
  'inline-flex items-center gap-1 px-1.5 py-0.5 rounded-md text-[9.5px] font-semibold uppercase tracking-wide';

function StorageBadge({ meta, variant }: { meta: TrackScdMeta; variant: Variant }) {
  const inOverlay = variant === 'overlay';
  if (meta.storage_state === 'pending') return null;
  if (meta.storage_state === 'failed' || meta.storage_state === 'missing') {
    const tone = inOverlay
      ? 'bg-rose-400/85 text-black'
      : 'bg-rose-500/15 text-rose-300';
    return (
      <span className={`${baseStyle} ${tone}`}>
        <span className="w-1.5 h-1.5 rounded-full bg-current" />
        issue
      </span>
    );
  }
  const isHq = meta.storage_quality === 'hq';
  const tone = isHq
    ? inOverlay
      ? 'bg-emerald-400/90 text-black'
      : 'bg-emerald-500/15 text-emerald-300'
    : inOverlay
      ? 'bg-white/85 text-black'
      : 'bg-white/[0.08] text-white/70';
  return <span className={`${baseStyle} ${tone}`}>{isHq ? 'HQ' : 'cached'}</span>;
}

function IndexBadge({ meta, variant }: { meta: TrackScdMeta; variant: Variant }) {
  const inOverlay = variant === 'overlay';
  if (meta.index_state === 'indexed') {
    const tone = inOverlay ? 'bg-sky-400/90 text-black' : 'bg-sky-500/15 text-sky-300';
    return <span className={`${baseStyle} ${tone}`}>analyzed</span>;
  }
  if (meta.index_state === 'pending') {
    const tone = inOverlay
      ? 'bg-amber-400/85 text-black'
      : 'bg-amber-500/15 text-amber-300';
    return (
      <span className={`${baseStyle} ${tone}`}>
        <span className="w-1.5 h-1.5 rounded-full bg-current animate-pulse" />
        analyzing
      </span>
    );
  }
  if (meta.index_state === 'failed') {
    const tone = inOverlay
      ? 'bg-rose-400/85 text-black'
      : 'bg-rose-500/15 text-rose-300';
    return (
      <span className={`${baseStyle} ${tone}`}>
        <span className="w-1.5 h-1.5 rounded-full bg-current" />
        analysis failed
      </span>
    );
  }
  return null;
}

function TrackStatusBadgesInner({ meta, variant = 'inline', showIndex = true }: Props) {
  if (!meta) return null;
  return (
    <span className="inline-flex items-center gap-1">
      <StorageBadge meta={meta} variant={variant} />
      {showIndex ? <IndexBadge meta={meta} variant={variant} /> : null}
    </span>
  );
}

export const TrackStatusBadges = memo(TrackStatusBadgesInner);
