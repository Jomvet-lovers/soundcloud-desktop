//! Шелл «Эфира»: данные волны (кластеры + бесконечный рефилл очереди) и
//! композиция станций на шкале-тюнере. Контракты не трогать: useInfiniteWave
//! маунтится здесь один раз, eager expandQueue не возвращать.

import React, {useCallback, useMemo, useRef, useState} from 'react';
import {useTranslation} from 'react-i18next';
import {api} from '../../../lib/api';
import {Sparkles} from '../../../lib/icons';
import {isUrnLiked, useLiked} from '../../../lib/likes';
import {useAuthStore} from '../../../stores/auth';
import type {Track} from '../../../stores/player';
import {usePlayerStore} from '../../../stores/player';
import {useSettingsStore} from '../../../stores/settings';
import {
  ClusterEmptyState,
  type ClusterHydrated,
  type ClusterId,
  ClusterRow,
  ClusterSkeletonState,
  NeighborsRow,
  useClusterWave,
} from '../../music/cluster';
import {useInfiniteWave} from '../../music/soundwave/use-infinite-wave';
import {ArtistWire} from './ArtistWire';
import {OnAirDeck} from './OnAirDeck';
import {StationSection} from './StationSection';
import {DeepShelf, ReleasesGrid, VibeShelf} from './stations';
import {type StationDef, TunerRail} from './TunerRail';
import {useActiveStation} from './useActiveStation';
import {WaveSchedule} from './WaveSchedule';

const STATION_ORDER: ClusterId[] = [
  'wave',
  'top_artists',
  'adjacent',
  'fresh_drops',
  'same_vibe',
  'deep_cuts',
];

/** Частоты диапазона: ядро (твоё) → фронтир (неизведанное). */
const STATION_FREQ: Record<string, number> = {
  wave: 89.1,
  top_artists: 92.6,
  adjacent: 96.3,
  fresh_drops: 100.1,
  same_vibe: 103.8,
  deep_cuts: 107.5,
};

export const EtherWave = React.memo(function EtherWave() {
  const { t } = useTranslation();
  const isAuthenticated = useAuthStore((s) => s.isAuthenticated);
  const selectedLanguages = useSettingsStore((s) => s.soundwaveLanguages);
  const setSelectedLanguages = useSettingsStore((s) => s.setSoundwaveLanguages);
  const hideLiked = useSettingsStore((s) => s.soundwaveHideLiked);
  const setHideLiked = useSettingsStore((s) => s.setSoundwaveHideLiked);
  const hideListened = useSettingsStore((s) => s.soundwaveHideListened);
  const setHideListened = useSettingsStore((s) => s.setSoundwaveHideListened);
  const currentTrack = usePlayerStore((s) => s.currentTrack);
  const [isRefreshing, setIsRefreshing] = useState(false);

  const stableLanguages = useMemo(() => [...selectedLanguages].sort(), [selectedLanguages]);
  const langKey = stableLanguages.join(',') || 'all';

  const url = useMemo(() => {
    if (!isAuthenticated) return null;
    const qs = new URLSearchParams();
    if (stableLanguages.length > 0) qs.set('languages', stableLanguages.join(','));
    qs.set('hide_listened', hideListened ? '1' : '0');
    const suffix = qs.toString() ? `?${qs}` : '';
    return `/recommendations${suffix}`;
  }, [isAuthenticated, stableLanguages, hideListened]);

  const { data, isLoading, isFetching, refetch } = useClusterWave({
    queryKey: ['cluster-wave', 'home', langKey, hideListened],
    url,
    enabled: isAuthenticated,
  });

  const rawClusters = useMemo(() => data?.clusters ?? [], [data]);
  const rawAllTracks = useMemo(() => data?.allTracks ?? [], [data]);

  // Live-тик лайков: hide-liked фильтры пересчитываются, когда лайк текущего
  // трека переключился (основная цель лайка с этой поверхности).
  const likesVersion = useLiked(currentTrack?.urn ?? '');
  const hideLikedFilter = useCallback((tr: Track) => !tr.user_favorite && !isUrnLiked(tr.urn), []);

  // biome-ignore lint/correctness/useExhaustiveDependencies: likesVersion тикает живой isUrnLiked.
  const filteredAllTracks = useMemo(() => {
    if (!hideLiked) return rawAllTracks;
    return rawAllTracks.filter((tr) => !tr.user_favorite && !isUrnLiked(tr.urn));
  }, [rawAllTracks, hideLiked, likesVersion]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: likesVersion тикает живой isUrnLiked.
  const filteredClusters = useMemo(() => {
    if (!hideLiked) return rawClusters;
    return rawClusters
      .map((c) => {
        const trackById = new Map<string, Track>();
        for (const tr of c.tracks) {
          const id = tr.urn.split(':').pop();
          if (id) trackById.set(id, tr);
        }
        return {
          ...c,
          tracks: c.tracks.filter((tr) => !tr.user_favorite && !isUrnLiked(tr.urn)),
          neighbors: c.neighbors?.filter((n) => {
            const matchTrack = trackById.get(String(n.track_id));
            if (!matchTrack) return true;
            return !matchTrack.user_favorite && !isUrnLiked(matchTrack.urn);
          }),
        };
      })
      .filter((c) => c.tracks.length > 0) as ClusterHydrated[];
  }, [rawClusters, hideLiked, likesVersion]);

  const orderedClusters = useMemo(() => {
    const byId = new Map(filteredClusters.map((c) => [c.id, c]));
    return STATION_ORDER.map((id) => byId.get(id)).filter((c): c is ClusterHydrated => !!c);
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
    hideListened,
  });

  // Клик по артисту → очередь из его лучших треков (sort=popular).
  const neighborArtistByTrack = useMemo(() => {
    const m = new Map<string, string>();
    for (const c of orderedClusters) {
      if (!c.neighbors) continue;
      for (const n of c.neighbors) m.set(String(n.track_id), n.artist_id);
    }
    return m;
  }, [orderedClusters]);
  const neighborMapRef = useRef(neighborArtistByTrack);
  neighborMapRef.current = neighborArtistByTrack;

  const resolveArtistQueue = useCallback(async (track: Track): Promise<Track[]> => {
    const trackId = track.urn.split(':').pop();
    const artistId = trackId ? neighborMapRef.current.get(trackId) : undefined;
    if (!artistId) return [track];
    try {
      const res = await api<{ collection: Track[] }>(
        `/artists/${encodeURIComponent(artistId)}/tracks?role=primary&sort=popular&limit=60`,
      );
      const seen = new Set<string>([track.urn]);
      const ordered: Track[] = [track];
      for (const tr of res.collection ?? []) {
        if (!seen.has(tr.urn)) {
          seen.add(tr.urn);
          ordered.push(tr);
        }
      }
      return ordered;
    } catch {
      return [track];
    }
  }, []);

  const stations: StationDef[] = useMemo(
    () =>
      orderedClusters.map((c) => ({
        id: c.id,
        freq: STATION_FREQ[c.id] ?? 99.9,
        title: t(`soundwave.home.cluster.${c.id}`),
      })),
    [orderedClusters, t],
  );
  const { active, register, jump } = useActiveStation(stations.map((s) => s.id));

  if (!isAuthenticated) return null;

  const handleRefresh = async () => {
    setIsRefreshing(true);
    try {
      await refetch();
    } finally {
      setTimeout(() => setIsRefreshing(false), 350);
    }
  };

  const handlePlayWave = () => {
    if (!filteredAllTracks.length) return;
    usePlayerStore.getState().play(filteredAllTracks[0], filteredAllTracks);
  };

  const showCold = !isLoading && orderedClusters.length === 0;

  const stationBody = (cluster: ClusterHydrated) => {
    switch (cluster.id) {
      case 'wave':
        return <WaveSchedule tracks={cluster.tracks} />;
      case 'top_artists':
        return cluster.neighbors?.length ? (
          <ArtistWire cluster={cluster} resolveQueue={resolveArtistQueue} />
        ) : (
          <ClusterRow
            hideHeader
            clusterId={cluster.id}
            title=""
            description=""
            icon={null}
            index={0}
            tracks={cluster.tracks}
            queue={cluster.tracks}
          />
        );
      case 'adjacent':
        return cluster.neighbors?.length ? (
          <NeighborsRow
            hideHeader
            title=""
            description=""
            icon={null}
            index={0}
            cluster={cluster}
            queue={cluster.tracks}
            resolveQueue={resolveArtistQueue}
          />
        ) : (
          <ClusterRow
            hideHeader
            clusterId={cluster.id}
            title=""
            description=""
            icon={null}
            index={0}
            tracks={cluster.tracks}
            queue={cluster.tracks}
          />
        );
      case 'fresh_drops':
        return <ReleasesGrid tracks={cluster.tracks} />;
      case 'same_vibe':
        return <VibeShelf tracks={cluster.tracks} />;
      default:
        return <DeepShelf tracks={cluster.tracks} />;
    }
  };

  return (
    <div>
      {stations.length > 1 && <TunerRail stations={stations} activeId={active} onSelect={jump} />}

      <div className="mt-6">
        <OnAirDeck
          track={waveTrack}
          queue={
            waveCluster?.tracks?.length
              ? waveCluster.tracks
              : filteredAllTracks.length
                ? filteredAllTracks
                : waveTrack
                  ? [waveTrack]
                  : []
          }
          isCurrent={isCurrent}
          hideListened={hideListened}
          onHideListened={setHideListened}
          hideLiked={hideLiked}
          onHideLiked={setHideLiked}
          languages={selectedLanguages}
          onLanguages={setSelectedLanguages}
          spinning={isRefreshing || isFetching}
          onRefresh={() => void handleRefresh()}
          onPlayWave={handlePlayWave}
          canPlay={filteredAllTracks.length > 0}
        />
      </div>

      {isLoading ? (
        <div className="pt-10">
          <ClusterSkeletonState rows={3} itemsPerRow={6} />
        </div>
      ) : showCold ? (
        <div className="pt-10">
          <ClusterEmptyState
            icon={<Sparkles size={20} style={{ color: 'var(--color-accent)' }} />}
            title={t('soundwave.coldTitle')}
            description={t('soundwave.coldDesc')}
          />
        </div>
      ) : (
        orderedClusters.map((cluster) => (
          <StationSection
            key={cluster.id}
            freq={STATION_FREQ[cluster.id] ?? 99.9}
            title={t(`soundwave.home.cluster.${cluster.id}`)}
            why={t(`soundwave.home.cluster.${cluster.id}Desc`)}
            refCb={register(cluster.id)}
          >
            {stationBody(cluster)}
          </StationSection>
        ))
      )}
    </div>
  );
});
