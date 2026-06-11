import React, {useMemo} from 'react';
import {useTranslation} from 'react-i18next';
import {ClusterFeedbackProvider} from '../../../lib/recsFeedback';
import type {Track} from '../../../stores/player';
import {TrackCard} from '../../music/TrackCard';
import {genreColor} from '../../search/utils';
import {HorizontalScroll} from '../../ui/HorizontalScroll';

const RELEASES_CAP = 10;
const SHELF_CAP = 18;

function relDate(track: Track, t: (k: string) => string): string | null {
  const stamp = track.release_date ?? track.created_at;
  if (!stamp) return null;
  const ts = Date.parse(stamp);
  if (!Number.isFinite(ts)) return null;
  const days = Math.floor((Date.now() - ts) / 86_400_000);
  if (days <= 0) return t('soundwave.ether.relToday');
  if (days === 1) return t('soundwave.ether.relYesterday');
  return new Date(ts).toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
}

/** «Свежие релизы» — датированная сетка крупных обложек (программа выхода). */
export const ReleasesGrid = React.memo(function ReleasesGrid({ tracks }: { tracks: Track[] }) {
  const { t } = useTranslation();
  const ctx = useMemo(() => ({ clusterId: 'fresh_drops' }), []);
  const items = tracks.slice(0, RELEASES_CAP);
  return (
    <ClusterFeedbackProvider value={ctx}>
      <div className="grid grid-cols-2 gap-x-4 gap-y-5 sm:grid-cols-3 lg:grid-cols-5">
        {items.map((track) => (
          <div key={track.urn}>
            <TrackCard track={track} queue={items} />
            <p className="mt-1.5 pl-0.5 font-mono text-[10.5px] text-white/30">
              {relDate(track, t) ?? '—'}
            </p>
          </div>
        ))}
      </div>
    </ClusterFeedbackProvider>
  );
});

/** «Тот же вайб» — полка с жанровой тонировкой каждой карточки. */
export const VibeShelf = React.memo(function VibeShelf({ tracks }: { tracks: Track[] }) {
  const ctx = useMemo(() => ({ clusterId: 'same_vibe' }), []);
  const items = tracks.slice(0, SHELF_CAP);
  return (
    <ClusterFeedbackProvider value={ctx}>
      <HorizontalScroll>
        {items.map((track) => {
          const tone = genreColor(track.genre ?? null);
          return (
            <div key={track.urn} className="w-[176px] shrink-0">
              <div className="relative rounded-2xl p-1.5">
                <span
                  className="pointer-events-none absolute inset-0 rounded-2xl"
                  style={{
                    background: `linear-gradient(180deg, ${tone}, transparent 72%)`,
                    opacity: 0.13,
                  }}
                />
                <div className="relative">
                  <TrackCard track={track} queue={items} />
                </div>
              </div>
              {track.genre && (
                <p className="mt-1 flex items-center gap-1.5 pl-2 text-[10.5px] text-white/35">
                  <span className="size-1.5 rounded-full" style={{ background: tone }} />
                  <span className="truncate">{track.genre}</span>
                </p>
              )}
            </div>
          );
        })}
      </HorizontalScroll>
    </ClusterFeedbackProvider>
  );
});

/** «Глубокие закопы» — слабый сигнал: полка приглушена, hover проявляет. */
export const DeepShelf = React.memo(function DeepShelf({ tracks }: { tracks: Track[] }) {
  const ctx = useMemo(() => ({ clusterId: 'deep_cuts' }), []);
  const items = tracks.slice(0, SHELF_CAP);
  return (
    <ClusterFeedbackProvider value={ctx}>
      <div className="opacity-75 saturate-[0.6] transition-[opacity,filter] duration-300 hover:opacity-100 hover:saturate-100">
        <HorizontalScroll>
          {items.map((track) => (
            <div key={track.urn} className="w-[168px] shrink-0">
              <TrackCard track={track} queue={items} />
            </div>
          ))}
        </HorizontalScroll>
      </div>
    </ClusterFeedbackProvider>
  );
});
