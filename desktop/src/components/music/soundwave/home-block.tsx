import React, {useCallback, useMemo, useState} from 'react';
import { useTranslation } from 'react-i18next';
import {
  AudioLines,
  Compass,
  Disc3,
  Headphones,
  playBlack14,
  RefreshCw,
  Sparkles,
  Star,
} from '../../../lib/icons';
import { isUrnLiked } from '../../../lib/likes';
import { useAuthStore } from '../../../stores/auth';
import type { Track } from '../../../stores/player';
import { usePlayerStore } from '../../../stores/player';
import { useSettingsStore } from '../../../stores/settings';
import {
  ClusterEmptyState,
  type ClusterHydrated,
  type ClusterId,
  ClusterRow,
  ClusterSkeletonState,
  fetchAndHydrateClusters,
  NeighborsRow,
  useClusterWave,
} from '../cluster';
import { AmbientLayer } from './ambient';
import { HideLikedToggle } from './hide-liked-toggle';
import { LanguageFilter } from './language-filter';
import { WaveTrackHeader } from './track-header';
import { useInfiniteWave } from './use-infinite-wave';
import {VibePortal} from './vibe-portal';
import { LiveWaveform } from './waveform';

// `wave` всегда первый. Остальные — стандартный набор для home-страницы.
const CLUSTER_ORDER: ClusterId[] = [
  'wave',
  'for_you',
  'top_artists',
  'adjacent',
  'fresh_drops',
  'same_vibe',
  'deep_cuts',
];

const CLUSTER_ICON: Partial<Record<ClusterId, React.ReactNode>> = {
  wave: <AudioLines size={14} />,
  for_you: <Sparkles size={14} />,
  top_artists: <Headphones size={14} />,
  adjacent: <Compass size={14} />,
  fresh_drops: <Disc3 size={14} />,
  same_vibe: <AudioLines size={14} />,
  deep_cuts: <Star size={14} />,
};

export const SoundWaveBlock = React.memo(function SoundWaveBlock() {
  const { t } = useTranslation();
  const isAuthenticated = useAuthStore((s) => s.isAuthenticated);
  const selectedLanguages = useSettingsStore((s) => s.soundwaveLanguages);
  const setSelectedLanguages = useSettingsStore((s) => s.setSoundwaveLanguages);
  const hideLiked = useSettingsStore((s) => s.soundwaveHideLiked);
  const setHideLiked = useSettingsStore((s) => s.setSoundwaveHideLiked);

  const currentTrack = usePlayerStore((s) => s.currentTrack);

  const [isRefreshing, setIsRefreshing] = useState(false);

  const stableLanguages = useMemo(() => [...selectedLanguages].sort(), [selectedLanguages]);
  const langKey = stableLanguages.join(',') || 'all';

  const url = useMemo(() => {
    if (!isAuthenticated) return null;
    const qs = new URLSearchParams();
    if (stableLanguages.length > 0) qs.set('languages', stableLanguages.join(','));
    const suffix = qs.toString() ? `?${qs}` : '';
    return `/recommendations${suffix}`;
  }, [isAuthenticated, stableLanguages]);

  const { data, isLoading, isFetching, refetch } = useClusterWave({
    queryKey: ['cluster-wave', 'home', langKey],
    url,
    enabled: isAuthenticated,
  });

  const rawClusters = useMemo(() => data?.clusters ?? [], [data]);
  const rawAllTracks = useMemo(() => data?.allTracks ?? [], [data]);

  const hideLikedFilter = useCallback((tr: Track) => !tr.user_favorite && !isUrnLiked(tr.urn), []);

  const filteredAllTracks = useMemo(() => {
    if (!hideLiked) return rawAllTracks;
    return rawAllTracks.filter(hideLikedFilter);
  }, [rawAllTracks, hideLiked, hideLikedFilter]);

  const filteredClusters = useMemo(() => {
    if (!hideLiked) return rawClusters;
    return rawClusters
      .map((c) => ({
        ...c,
        tracks: c.tracks.filter(hideLikedFilter),
        neighbors: c.neighbors?.filter((n) => {
          const matchTrack = c.tracks.find((tr) => tr.urn.endsWith(`:${n.track_id}`));
          if (!matchTrack) return true;
          return !matchTrack.user_favorite && !isUrnLiked(matchTrack.urn);
        }),
      }))
      .filter((c) => c.tracks.length > 0) as ClusterHydrated[];
  }, [rawClusters, hideLiked, hideLikedFilter]);

  const orderedClusters = useMemo(() => {
    const byId = new Map(filteredClusters.map((c) => [c.id, c]));
    return CLUSTER_ORDER.map((id) => byId.get(id)).filter((c): c is NonNullable<typeof c> => !!c);
  }, [filteredClusters]);

  const waveCluster = useMemo(
    () => orderedClusters.find((c) => c.id === 'wave') ?? null,
    [orderedClusters],
  );

  const waveTrack = currentTrack ?? filteredAllTracks[0] ?? null;
  const isCurrent = !!currentTrack && waveTrack?.urn === currentTrack.urn;

  useInfiniteWave({
      enabled: isAuthenticated,
    seedKind: 'user',
    initialTracks: waveCluster?.tracks ?? [],
    initialCursor: null,
    languages: stableLanguages,
    filterTrack: hideLiked ? hideLikedFilter : undefined,
  });

  // "От любимых": click on a top-artist card → fetch same_artist similar cluster
  // and play that as the queue, so next/prev stays within the artist's adjacent tracks.
  const resolveSameArtistQueue = useCallback(async (track: Track) => {
    const trackId = track.urn.split(':').pop();
    if (!trackId) return [track];
    try {
      const data = await fetchAndHydrateClusters(
        `/recommendations/similar/${encodeURIComponent(trackId)}`,
      );
      const sameArtist = data.clusters.find((c) => c.id === 'same_artist');
      const tracks = sameArtist?.tracks ?? [];
      const seen = new Set<string>();
      const ordered: Track[] = [];
      if (!seen.has(track.urn)) {
        seen.add(track.urn);
        ordered.push(track);
      }
      for (const t of tracks) {
        if (!seen.has(t.urn)) {
          seen.add(t.urn);
          ordered.push(t);
        }
      }
      return ordered;
    } catch {
      return [track];
    }
  }, []);

  if (!isAuthenticated) return null;

  const handleRefresh = async () => {
    setIsRefreshing(true);
    try {
      await refetch();
    } finally {
      setTimeout(() => setIsRefreshing(false), 350);
    }
  };

  const handlePlayAll = () => {
    if (!filteredAllTracks.length) return;
    usePlayerStore.getState().play(filteredAllTracks[0], filteredAllTracks);
  };

  const spinning = isRefreshing || isFetching;
    const showCold = !isLoading && orderedClusters.length === 0;

  return (
    <section
      className="relative rounded-3xl overflow-hidden glass-featured select-none"
      style={{
        boxShadow:
          '0 0 0 1px rgba(255,255,255,0.04) inset, 0 10px 60px rgba(0,0,0,0.45), 0 0 60px var(--color-accent-glow)',
        borderColor: 'rgba(255,255,255,0.08)',
      }}
    >
      <AmbientLayer particleCount={10} blur={35} intensity={0.5} />

      <div
        className="absolute inset-0 pointer-events-none"
        aria-hidden
        style={{
          background:
            'linear-gradient(180deg, rgba(8,8,10,0.45) 0%, rgba(8,8,10,0.35) 45%, rgba(8,8,10,0.85) 100%)',
          contain: 'strict',
        }}
      />

      <div className="relative p-6 flex flex-col gap-5" style={{ isolation: 'isolate' }}>
        <div className="flex items-center justify-between gap-4 flex-wrap">
          <div className="flex items-center gap-3 min-w-0">
            <div
              className="relative w-10 h-10 rounded-2xl flex items-center justify-center shrink-0"
              style={{
                background: 'linear-gradient(135deg, var(--color-accent), rgba(255,255,255,0.12))',
                boxShadow: '0 0 24px var(--color-accent-glow), inset 0 1px 0 rgba(255,255,255,0.2)',
              }}
            >
              <AudioLines size={18} style={{ color: 'var(--color-accent-contrast)' }} />
            </div>
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                <h2 className="soundwave-title text-[20px] font-bold tracking-tight leading-none">
                  SoundWave
                </h2>
                <span
                  className="relative overflow-hidden inline-flex items-center gap-1 text-[9px] font-bold uppercase tracking-[0.12em] px-2 py-[3px] rounded-full text-white/90"
                  style={{
                    background:
                      'linear-gradient(135deg, var(--color-accent-glow), rgba(255,255,255,0.06))',
                    border: '1px solid var(--color-accent-glow)',
                  }}
                >
                  <Sparkles size={9} style={{ color: 'var(--color-accent)' }} />
                  AI
                </span>
              </div>
              <p className="text-[11.5px] text-white/50 mt-1 truncate">{t('soundwave.tagline')}</p>
            </div>
          </div>

          <div className="flex items-center gap-2 shrink-0 flex-wrap justify-end">
            <HideLikedToggle value={hideLiked} onChange={setHideLiked} />
            <LanguageFilter selected={selectedLanguages} onChange={setSelectedLanguages} />
            <button
              type="button"
              onClick={handleRefresh}
              disabled={spinning}
              className="flex items-center justify-center w-8 h-8 rounded-full bg-white/[0.06] border border-white/[0.08] hover:bg-white/[0.1] hover:border-white/[0.14] transition-colors duration-200 text-white/70 hover:text-white/95 cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed"
              title={t('soundwave.refresh')}
            >
              <RefreshCw size={13} className={spinning ? 'animate-spin' : ''} />
            </button>
            <button
              type="button"
              onClick={handlePlayAll}
              disabled={filteredAllTracks.length === 0}
              className="flex items-center gap-2 pl-2.5 pr-4 h-10 rounded-full font-semibold text-[13px] transition-all duration-200 ease-[var(--ease-apple)] cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed active:scale-[0.97] hover:scale-[1.03]"
              style={{
                background: 'var(--color-accent)',
                color: 'var(--color-accent-contrast)',
                boxShadow:
                  '0 6px 22px var(--color-accent-glow), inset 0 1px 0 rgba(255,255,255,0.25)',
              }}
              title={t('soundwave.playAll')}
            >
              <span
                className="w-6 h-6 rounded-full flex items-center justify-center"
                style={{ background: 'rgba(255,255,255,0.9)' }}
              >
                {playBlack14}
              </span>
              {t('soundwave.playAll')}
            </button>
          </div>
        </div>

        <div className="flex flex-col gap-3">
          {waveTrack ? (
            <WaveTrackHeader
              track={waveTrack}
              queue={
                waveCluster?.tracks?.length
                  ? waveCluster.tracks
                    : filteredAllTracks.length
                        ? filteredAllTracks
                    : [waveTrack]
              }
              isCurrent={isCurrent}
            />
          ) : (
            <div className="flex items-center gap-3">
              <div className="w-14 h-14 rounded-xl bg-white/[0.04] ring-1 ring-white/[0.06] shrink-0" />
              <div className="flex-1 min-w-0">
                <p className="text-[15px] font-semibold text-white/90 leading-tight">
                  {t('soundwave.idleTitle')}
                </p>
                <p className="text-[12px] text-white/45 mt-0.5 truncate">
                  {t('soundwave.idleSub')}
                </p>
              </div>
            </div>
          )}

          <LiveWaveform track={waveTrack} isCurrent={isCurrent} />
        </div>

          <VibePortal/>

        <div className="min-h-[280px]">
            {isLoading ? (
            <ClusterSkeletonState rows={3} itemsPerRow={6} />
          ) : showCold ? (
            <ClusterEmptyState
              icon={<Sparkles size={20} style={{ color: 'var(--color-accent)' }} />}
              title={t('soundwave.coldTitle')}
              description={t('soundwave.coldDesc')}
            />
          ) : (
            <div className="flex flex-col gap-6">
              {orderedClusters.map((c, idx) =>
                (c.id === 'top_artists' || c.id === 'adjacent') && c.neighbors ? (
                  <NeighborsRow
                    key={c.id}
                    title={t(`soundwave.home.cluster.${c.id}`)}
                    description={t(`soundwave.home.cluster.${c.id}Desc`)}
                    icon={CLUSTER_ICON[c.id]}
                    index={idx}
                    cluster={c}
                    queue={c.tracks}
                    resolveQueue={c.id === 'top_artists' ? resolveSameArtistQueue : undefined}
                  />
                ) : (
                  <ClusterRow
                    key={c.id}
                    clusterId={c.id}
                    title={t(`soundwave.home.cluster.${c.id}`)}
                    description={t(`soundwave.home.cluster.${c.id}Desc`)}
                    icon={CLUSTER_ICON[c.id]}
                    index={idx}
                    tracks={c.tracks}
                    queue={c.tracks}
                  />
                ),
              )}
            </div>
          )}
        </div>
      </div>
    </section>
  );
});
