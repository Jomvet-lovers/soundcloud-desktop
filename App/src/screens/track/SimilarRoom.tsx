import { type ComponentType, useEffect, useMemo, useRef, useState } from 'react';
import { Animated, Pressable, View } from 'react-native';
import { type Cluster, type ClusterNeighbor, type Track, useSc } from '@sc/data';
import {
  AudioLinesIcon,
  ClusterHeading,
  CollabCard,
  CompassIcon,
  Disc3Icon,
  featuredGlass,
  GlassSurface,
  HeadphonesIcon,
  type IconProps,
  PlayIcon,
  ScText,
  Shelf,
  SparklesIcon,
  TrackCard,
  useScTheme,
} from '@sc/ui';
import { usePlayerState, type PlayerState } from '../../player/PlayerContext';
import { badgeOf, useCardProps } from '../shared/cards';

/** Станции «Похожего» в порядке показа (донор `CLUSTER_ORDER` + i18n). */
const STATIONS: { id: string; title: string; desc: string; Icon: ComponentType<IconProps> }[] = [
  { id: 'wave', title: 'Волна от трека', desc: 'Бесконечная дорожка вокруг этого трека — близкое звучание, тематика, сетка артистов.', Icon: AudioLinesIcon },
  { id: 'same_artist', title: 'Этого артиста', desc: 'Другие треки в том же ключе.', Icon: Disc3Icon },
  { id: 'same_vibe', title: 'Тот же вайб', desc: 'Близкое звучание у других артистов.', Icon: AudioLinesIcon },
  { id: 'featured_with', title: 'Совместные миры', desc: 'Артисты, с которыми этот пересекался в фитах.', Icon: CompassIcon },
  { id: 'fans_also', title: 'Те же слушатели', desc: 'Что ещё слушают фанаты этого трека.', Icon: HeadphonesIcon },
];

const bareId = (urn: string) => urn.split(':').pop() ?? urn;

// цвет — тема юзера, не жанр (блок вне жанрового скоупа)
export function SimilarRoom({ trackUrn }: { trackUrn: string }) {
  const sc = useSc();
  const player = usePlayerState();
  const { accent } = useScTheme();
  const card = useCardProps(player);
  const [clusters, setClusters] = useState<Cluster[]>([]);
  const [byId, setById] = useState<Map<string, Track>>(new Map());
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let alive = true;
    setLoading(true);
    setClusters([]);
    void sc.home
      .similar(bareId(trackUrn), 10)
      .then(async (cl) => {
        if (!alive) return;
        setClusters(cl);
        const ids = [...new Set(cl.flatMap((c) => c.track_ids))];
        if (ids.length === 0) { setLoading(false); return; }
        const tracks = await sc.tracks.resolveMany(ids);
        if (!alive) return;
        setById(new Map(tracks.map((t) => [bareId(t.id), t])));
        setLoading(false);
      })
      .catch(() => alive && setLoading(false));
    return () => { alive = false; };
  }, [sc, trackUrn]);

  const stations = useMemo(() => {
    const present = new Map(clusters.map((c) => [c.id, c]));
    return STATIONS.map((s) => ({ ...s, cluster: present.get(s.id) }))
      .filter((s): s is typeof s & { cluster: Cluster } => !!s.cluster)
      .map((s) => ({ ...s, tracks: s.cluster.track_ids.map((id) => byId.get(id)).filter((t): t is Track => !!t) }))
      .filter((s) => s.tracks.length > 0);
  }, [clusters, byId]);

  const allTracks = useMemo(() => stations.flatMap((s) => s.tracks), [stations]);
  const soft = `${accent.base}26`;

  if (!loading && stations.length === 0) return null;

  return (
    <GlassSurface
      recipe={featuredGlass}
      style={{ borderRadius: 26, borderColor: 'rgba(255,255,255,0.08)', boxShadow: `0 10px 40px rgba(0,0,0,0.35), 0 0 40px ${accent.glow}` }}
    >
      <View style={{ padding: 24, gap: 24 }}>
        <View style={{ flexDirection: 'row', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
          <View
            style={{
              width: 40,
              height: 40,
              borderRadius: 16,
              alignItems: 'center',
              justifyContent: 'center',
              boxShadow: `0 0 24px ${accent.glow}, inset 0 1px 0 rgba(255,255,255,0.2)`,
              ...({ backgroundImage: `linear-gradient(135deg, ${accent.base}, rgba(255,255,255,0.12))` } as object),
            }}
          >
            <AudioLinesIcon size={17} color={accent.contrast} />
          </View>
          <View style={{ flex: 1, minWidth: 160 }}>
            <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8 }}>
              <ScText style={{ fontSize: 18, fontWeight: '900', letterSpacing: -0.4, color: '#fff' }}>Похожее</ScText>
              <View style={{ flexDirection: 'row', alignItems: 'center', gap: 4, paddingHorizontal: 8, height: 18, borderRadius: 999, backgroundColor: soft, borderWidth: 1, borderColor: `${accent.base}40` }}>
                <SparklesIcon size={9} color={accent.base} />
                <ScText style={{ fontSize: 9, fontWeight: '900', letterSpacing: 1.4, color: 'rgba(255,255,255,0.9)' }}>AI</ScText>
              </View>
            </View>
            <ScText numberOfLines={1} style={{ fontSize: 11.5, color: 'rgba(255,255,255,0.45)', marginTop: 3 }}>Что близко по звуку и слушает та же аудитория.</ScText>
          </View>
          {allTracks.length > 0 && (
            <PlayAllButton accent={accent.base} glow={accent.glow} contrast={accent.contrast} onPress={() => player.toggle(allTracks[0], allTracks)} />
          )}
        </View>

        <View style={{ gap: 26 }}>
          {stations.map((s, idx) => (
            <View key={s.id} style={{ gap: 14 }}>
              <ClusterHeading icon={<s.Icon size={15} color={accent.base} />} index={idx} title={s.title} desc={s.desc} accent={accent.base} accentGlow={accent.glow} accentSoft={soft} />
              {s.id === 'featured_with' && s.cluster.neighbors.length > 0 ? (
                <CollabShelf neighbors={s.cluster.neighbors} byId={byId} queue={s.tracks} accent={accent.base} accentGlow={accent.glow} player={player} />
              ) : (
                <TileShelf tracks={s.tracks} card={card} />
              )}
            </View>
          ))}
        </View>
      </View>
    </GlassSurface>
  );
}

/** «Слушать всё» в блоке Похожего — живая: зум на ховере, поджатие на нажатии. */
function PlayAllButton({ accent, glow, contrast, onPress }: { accent: string; glow: string; contrast: string; onPress: () => void }) {
  const s = useRef(new Animated.Value(0)).current;
  const to = (v: number) => Animated.spring(s, { toValue: v, useNativeDriver: true, speed: 40, bounciness: 8 }).start();
  return (
    <Animated.View style={{ transform: [{ scale: s.interpolate({ inputRange: [0, 1], outputRange: [1, 1.05] }) }] }}>
      <Pressable
        onPress={onPress}
        onHoverIn={() => to(1)}
        onHoverOut={() => to(0)}
        onPressIn={() => to(0.5)}
        onPressOut={() => to(1)}
        style={{ flexDirection: 'row', alignItems: 'center', gap: 8, paddingLeft: 10, paddingRight: 16, height: 36, borderRadius: 999, backgroundColor: accent, boxShadow: `0 5px 18px ${glow}, inset 0 1px 0 rgba(255,255,255,0.25)` }}
      >
        <View style={{ width: 20, height: 20, borderRadius: 10, backgroundColor: 'rgba(255,255,255,0.92)', alignItems: 'center', justifyContent: 'center' }}>
          <PlayIcon size={11} color="#0a0a0c" />
        </View>
        <ScText style={{ fontSize: 12.5, fontWeight: '700', color: contrast }}>Слушать</ScText>
      </Pressable>
    </Animated.View>
  );
}

const CARD_W = 168;

function TileShelf({ tracks, card }: { tracks: Track[]; card: (t: Track, q: Track[]) => import('@sc/ui').TrackCardProps }) {
  return (
    <Shelf>
      {tracks.map((t) => (
        <View key={t.id} style={{ width: CARD_W }}>
          <TrackCard {...card(t, tracks)} />
        </View>
      ))}
    </Shelf>
  );
}

function CollabShelf({ neighbors, byId, queue, accent, accentGlow, player }: {
  neighbors: ClusterNeighbor[];
  byId: Map<string, Track>;
  queue: Track[];
  accent: string;
  accentGlow: string;
  player: PlayerState;
}) {
  const pairs = neighbors
    .map((n) => ({ n, t: byId.get(String(n.track_id)) }))
    .filter((p): p is { n: ClusterNeighbor; t: Track } => !!p.t);
  const currentId = player.currentTrack?.id ?? null;
  return (
    <Shelf>
      {pairs.map(({ n, t }) => (
        <CollabCard
          key={n.artist_id}
          title={t.title}
          artist={t.artist.name}
          artworkUrl={t.artwork_url}
          artistName={n.artist_name}
          artistAvatarUrl={n.avatar_url}
          badge={badgeOf(t) ?? undefined}
          active={t.id === currentId}
          playing={player.playing}
          accent={accent}
          accentGlow={accentGlow}
          onToggle={() => player.toggle(t, queue)}
        />
      ))}
    </Shelf>
  );
}
