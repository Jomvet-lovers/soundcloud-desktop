import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { StyleSheet, View } from 'react-native';
import { type Cluster, type ClusterNeighbor, type Me, type Track, useSc } from '@sc/data';

const EMPTY_BY_ID: Map<string, Track> = new Map();
import { RiverBraid, type RiverAnchor, ScText, SparklesIcon, useScTheme } from '@sc/ui';
import { seedLikes } from '../../player/likes';
import { usePlayerState } from '../../player/PlayerContext';
import { ArchiveStation } from './ArchiveStation';
import { ArtistWire } from './ArtistWire';
import { EstuaryDeck } from './EstuaryDeck';
import { RiverMasthead } from './RiverMasthead';
import { RiverSection } from './RiverSection';
import { DeepShelf, ReleaseBrook, VibeShelf } from './stations';
import { useSoundprint } from './useSoundprint';
import { WaveFrame } from './WaveFrame';
import { WaveSchedule } from './WaveSchedule';

const CLUSTER_CAP = 12;

interface HomeData {
  byId: Map<string, Track>;
  clusters: Cluster[];
  liked: Track[];
}

export function HomeScreen({ me, onOpenSearch }: { me: Me | null; onOpenSearch: () => void }) {
  const sc = useSc();
  const { accent } = useScTheme();
  const player = usePlayerState();

  const [selected, setSelected] = useState<string | null>(null);
  const [hideListened, setHideListened] = useState(false);
  const [hideLiked, setHideLiked] = useState(false);
  const [languages, setLanguages] = useState<string[]>([]);
  const [spinning, setSpinning] = useState(false);
  const [refreshKey, setRefreshKey] = useState(0);
  const [data, setData] = useState<HomeData | null>(null);

  useEffect(() => {
    let alive = true;
    setSpinning(true);
    (async () => {
      const filters: Record<string, unknown> = { hide_listened: hideListened };
      if (languages.length) filters.languages = languages.join(',');
      const [clusters, likedPage] = await Promise.all([
        sc.home.clusters(40, filters),
        sc.library.likedTracks(200, 0).catch(() => null),
      ]);
      const ids = [...new Set(clusters.flatMap((c) => c.track_ids.slice(0, CLUSTER_CAP)))];
      const tracks = ids.length ? await sc.tracks.resolveMany(ids) : [];
      if (!alive) return;
      const byId = new Map<string, Track>();
      for (const t of tracks) {
        byId.set(t.id, t);
        byId.set(t.id.split(':').pop() ?? t.id, t);
      }
      const liked = likedPage?.items ?? [];
      seedLikes(liked); // общий стор лайков — источник истины для всех полок/плеера
      setData({ byId, clusters, liked });
    })()
      .catch(() => alive && setData((d) => d))
      .finally(() => alive && setSpinning(false));
    return () => {
      alive = false;
    };
  }, [sc, hideListened, languages, refreshKey]);

  const resolved = useMemo(() => {
    const m = new Map<string, Track[]>();
    if (!data) return m;
    for (const c of data.clusters) {
      const seen = new Set<string>();
      const out: Track[] = [];
      for (const id of c.track_ids.slice(0, CLUSTER_CAP)) {
        const t = data.byId.get(id) ?? data.byId.get(id.split(':').pop() ?? id);
        if (t && !seen.has(t.id)) {
          seen.add(t.id);
          out.push(t);
        }
      }
      m.set(c.id, out);
    }
    return m;
  }, [data]);

  const liked = data?.liked ?? [];
  const sound = useSoundprint(liked, selected);

  const noLikes = (ts: Track[]) => (hideLiked ? ts.filter((t) => !t.user_favorite) : ts);
  const wave = noLikes(resolved.get('wave') ?? []);
  const topArtists = noLikes(resolved.get('top_artists') ?? []);
  const freshDrops = noLikes(resolved.get('fresh_drops') ?? []);
  const sameVibe = noLikes(resolved.get('same_vibe') ?? []);
  const adjacent = noLikes(resolved.get('adjacent') ?? []);
  const deepCuts = noLikes(resolved.get('deep_cuts') ?? []);

  const head = player.currentTrack ?? wave[0] ?? null;

  return (
    <WaveFrame sound={sound}>
      <RiverMasthead
        me={me}
        sound={sound}
        selected={selected}
        onSelect={setSelected}
        playing={player.playing}
        onOpenSearch={onOpenSearch}
      />

      <EstuaryDeck
        track={head}
        queue={wave.length ? wave : head ? [head] : []}
        player={player}
        hideListened={hideListened}
        onHideListened={setHideListened}
        hideLiked={hideLiked}
        onHideLiked={setHideLiked}
        languages={languages}
        onLanguages={setLanguages}
        spinning={spinning}
        onRefresh={() => setRefreshKey((k) => k + 1)}
        onPlayWave={() => wave.length && player.playQueue(wave)}
        canPlay={wave.length > 0}
      />

      {data && wave.length === 0 && topArtists.length === 0 ? (
        <ColdState accent={accent.base} />
      ) : (
        <>
          <RiverBody
            player={player}
            tint={sound.tint}
            byId={data?.byId ?? EMPTY_BY_ID}
            neighbors={data?.clusters.find((c) => c.id === 'top_artists')?.neighbors ?? []}
            wave={wave}
            topArtists={topArtists}
            freshDrops={freshDrops}
            sameVibe={sameVibe}
            adjacent={adjacent}
            deepCuts={deepCuts}
          />
          <ArchiveStation liked={liked} player={player} />
        </>
      )}
    </WaveFrame>
  );
}

interface BodyProps {
  player: ReturnType<typeof usePlayerState>;
  tint: string[];
  byId: Map<string, Track>;
  neighbors: ClusterNeighbor[];
  wave: Track[];
  topArtists: Track[];
  freshDrops: Track[];
  sameVibe: Track[];
  adjacent: Track[];
  deepCuts: Track[];
}

type AnchorMeta = { ref: View; kind: RiverAnchor['kind']; order: number };

/** Русло с притоками: секции + река-SVG. Якоря меряем ИМПЕРАТИВНО (`measure` →
 *  pageX/pageY минус корень) при каждой смене размера корня + добор после
 *  отрисовки — на web `onLayout`/ResizeObserver ловит смену РАЗМЕРА, но не сдвиг
 *  позиции секции, поэтому догрузка контента иначе оставляет старые y («2 реки»). */
function RiverBody({ player, tint, byId, neighbors, wave, topArtists, freshDrops, sameVibe, adjacent, deepCuts }: BodyProps) {
  const [size, setSize] = useState({ w: 0, h: 0 });
  const [anchors, setAnchors] = useState<RiverAnchor[]>([]);
  const rootRef = useRef<View>(null);
  const secs = useRef(new Map<string, AnchorMeta>());

  const bind = (key: string, kind: RiverAnchor['kind'], order: number) => (el: View | null) => {
    if (el) secs.current.set(key, { ref: el, kind, order });
    else secs.current.delete(key);
  };

  const remeasure = useCallback(() => {
    const node = rootRef.current;
    if (!node) return;
    node.measure((_x, _y, w, h, rpx, rpy) => {
      const metas = [...secs.current.values()];
      const out: RiverAnchor[] = [];
      let pending = metas.length;
      const commit = () => {
        setSize((s) => (s.w === Math.round(w) && s.h === Math.round(h) ? s : { w: Math.round(w), h: Math.round(h) }));
        setAnchors(out);
      };
      if (pending === 0) return commit();
      for (const m of metas) {
        m.ref.measure((_a, _b, cw, _ch, px, py) => {
          out.push({ x: px - rpx, y: py - rpy, width: cw, kind: m.kind, order: m.order });
          if (--pending === 0) commit();
        });
      }
    });
  }, []);

  const layoutKey = [wave, topArtists, freshDrops, sameVibe, adjacent, deepCuts]
    .map((c) => (c.length ? '1' : '0'))
    .join('');

  useEffect(() => {
    remeasure();
    const t = setTimeout(remeasure, 150);
    return () => clearTimeout(t);
  }, [layoutKey, remeasure]);

  return (
    <View ref={rootRef} onLayout={remeasure} style={{ position: 'relative' }}>
      <View style={StyleSheet.absoluteFill} pointerEvents="none">
        <RiverBraid width={size.w} height={size.h} anchors={anchors} tint={tint} />
      </View>

      <View style={{ gap: 48, zIndex: 1 }}>
        {wave.length > 0 && (
          <View ref={bind('wave', 'node', 1)}>
            <RiverSection title="Волна" why="Бесконечный микс — последние лайки, сетка артистов и ИИ-реки от треков. Чем больше слушаешь — тем точнее.">
              <WaveSchedule tracks={wave} player={player} />
            </RiverSection>
          </View>
        )}

        {(topArtists.length > 0 || freshDrops.length > 0) && (
          <View style={{ flexDirection: 'row', gap: 32 }}>
            {topArtists.length > 0 && (
              <View ref={bind('artists', 'node', 2)} style={{ flex: 7, minWidth: 0 }}>
                <RiverSection title="От любимых" why="Артисты, которых ставишь чаще всего — по одному свежему треку.">
                  {neighbors.length > 0 ? (
                    <ArtistWire neighbors={neighbors} byId={byId} player={player} />
                  ) : (
                    <WaveSchedule tracks={topArtists} player={player} />
                  )}
                </RiverSection>
              </View>
            )}
            {freshDrops.length > 0 && (
              <View ref={bind('fresh', 'branch', 3)} style={{ flex: 5, minWidth: 0 }}>
                <RiverSection title="Свежее" why="Недавно появилось у артистов, которых ты слушаешь." tone="panel">
                  <ReleaseBrook tracks={freshDrops} player={player} />
                </RiverSection>
              </View>
            )}
          </View>
        )}

        {(sameVibe.length > 0 || adjacent.length > 0) && (
          <View style={{ flexDirection: 'row', gap: 32 }}>
            {sameVibe.length > 0 && (
              <View ref={bind('vibe', 'node', 4)} style={{ flex: 7, minWidth: 0 }}>
                <RiverSection title="Тот же вайб" why="Звук в твоём эмоциональном спектре.">
                  <VibeShelf tracks={sameVibe} player={player} />
                </RiverSection>
              </View>
            )}
            {adjacent.length > 0 && (
              <View ref={bind('adjacent', 'branch', 5)} style={{ flex: 5, minWidth: 0 }}>
                <RiverSection title="Близкие миры" why="Кого ставят рядом с твоими любимыми." tone="panel">
                  <VibeShelf tracks={adjacent} player={player} />
                </RiverSection>
              </View>
            )}
          </View>
        )}

        {deepCuts.length > 0 && (
          <View ref={bind('deep', 'node', 6)}>
            <RiverSection title="Глубже" why="Не на слуху, но в твоём ключе — за горизонтом." tone="deep">
              <DeepShelf tracks={deepCuts} player={player} />
            </RiverSection>
          </View>
        )}

        <View ref={bind('delta', 'delta', 9)} style={{ borderRadius: 16, borderWidth: 1, borderStyle: 'dashed', borderColor: 'rgba(255,255,255,0.1)', paddingVertical: 24, paddingHorizontal: 20, alignItems: 'center' }}>
          <ScText token="counter" style={{ fontSize: 12, color: 'rgba(255,255,255,0.4)' }}>
            ∞ · течение продолжается — очередь доливается сама
          </ScText>
        </View>
      </View>
    </View>
  );
}

function ColdState({ accent }: { accent: string }) {
  return (
    <View style={{ alignItems: 'center', paddingVertical: 48, gap: 10 }}>
      <SparklesIcon size={20} color={accent} />
      <ScText style={{ fontSize: 16, fontWeight: '600', color: 'rgba(255,255,255,0.9)' }}>Волна ещё настраивается</ScText>
      <ScText style={{ fontSize: 13, color: 'rgba(255,255,255,0.5)' }}>Лайкни пару треков — настроим волну под твой вкус</ScText>
    </View>
  );
}
