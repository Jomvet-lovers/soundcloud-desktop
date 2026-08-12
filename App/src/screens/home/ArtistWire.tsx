import { useEffect, useMemo, useRef, useState } from 'react';
import { Animated, Image, Pressable, View } from 'react-native';
import Svg, { Defs, LinearGradient, Rect, Stop } from 'react-native-svg';
import { type ClusterNeighbor, type Track, useSc } from '@sc/data';
import { type AccentSet, artUrl, Cover, formatDuration, PlayIcon, ScText, Shelf, useScTheme } from '@sc/ui';
import type { PlayerState } from '../../player/PlayerContext';

const ARTIST_WAVE_LIMIT = 8;
const QUEUE_CAP = 24;
const HOVER_MS = 180;

/** «Твои артисты» — отмель: круги на линии воды, под каждым — трек-сид.
 *  Клик собирает полную волну артиста и играет её с сида. */
export function ArtistWire({
  neighbors,
  byId,
  player,
}: {
  neighbors: ClusterNeighbor[];
  byId: Map<string, Track>;
  player: PlayerState;
}) {
  const sc = useSc();
  const { accent } = useScTheme();

  const pairs = useMemo(() => {
    const out: Array<{ neighbor: ClusterNeighbor; track: Track }> = [];
    for (const n of neighbors) {
      const track = byId.get(n.track_id);
      if (track) out.push({ neighbor: n, track });
    }
    return out;
  }, [neighbors, byId]);

  if (pairs.length === 0) return null;

  const play = async (neighbor: ClusterNeighbor, seed: Track) => {
    if (player.currentTrack?.id === seed.id) {
      player.toggle(seed);
      return;
    }
    // Сид играем СРАЗУ (nowbar меняется мгновенно), волну артиста дозаливаем в хвост.
    player.toggle(seed, [seed]);
    try {
      const clusters = await sc.home.artistWave(neighbor.artist_id, ARTIST_WAVE_LIMIT);
      const ids = [...new Set(clusters.flatMap((c) => c.track_ids))].slice(0, QUEUE_CAP);
      const rest = ids.length ? (await sc.tracks.resolveMany(ids)).filter((t) => t.id !== seed.id) : [];
      if (rest.length) player.extendQueue(rest);
    } catch {
      // сеть недоступна — уже играет трек-сид
    }
  };

  return (
    <View style={{ position: 'relative', paddingTop: 8, paddingBottom: 4 }}>
      <Waterline />
      <Shelf gap={28}>
        {pairs.map(({ neighbor, track }) => (
          <ArtistBuoy
            key={neighbor.artist_id}
            neighbor={neighbor}
            track={track}
            accent={accent}
            onPress={() => void play(neighbor, track)}
          />
        ))}
      </Shelf>
    </View>
  );
}

function Waterline() {
  return (
    <View pointerEvents="none" style={{ position: 'absolute', left: 0, right: 0, top: 58, height: 1 }}>
      <Svg width="100%" height={1}>
        <Defs>
          <LinearGradient id="artist-wire-waterline" x1="0" y1="0" x2="1" y2="0">
            <Stop offset="0" stopColor="#ffffff" stopOpacity={0} />
            <Stop offset="0.08" stopColor="#ffffff" stopOpacity={0.2} />
            <Stop offset="0.92" stopColor="#ffffff" stopOpacity={0.2} />
            <Stop offset="1" stopColor="#ffffff" stopOpacity={0} />
          </LinearGradient>
        </Defs>
        <Rect x={0} y={0} width="100%" height={1} fill="url(#artist-wire-waterline)" />
      </Svg>
    </View>
  );
}

function ArtistBuoy({
  neighbor,
  track,
  accent,
  onPress,
}: {
  neighbor: ClusterNeighbor;
  track: Track;
  accent: AccentSet;
  onPress: () => void;
}) {
  const [hover, setHover] = useState(false);
  const anim = useRef(new Animated.Value(0)).current;

  useEffect(() => {
    Animated.timing(anim, { toValue: hover ? 1 : 0, duration: HOVER_MS, useNativeDriver: true }).start();
  }, [hover, anim]);

  const lift = anim.interpolate({ inputRange: [0, 1], outputRange: [0, -4] });
  const avatar = artUrl(neighbor.avatar_url, 't300x300');

  return (
    <Pressable
      onPress={onPress}
      onPointerEnter={() => setHover(true)}
      onPointerLeave={() => setHover(false)}
      style={{ width: 128, alignItems: 'center' }}
    >
      <Animated.View style={{ alignItems: 'center', gap: 10, width: '100%', transform: [{ translateY: lift }] }}>
        <View style={{ width: 92, height: 92 }}>
          <View
            style={{
              width: 92,
              height: 92,
              borderRadius: 46,
              overflow: 'hidden',
              borderWidth: 1,
              borderColor: 'rgba(255,255,255,0.14)',
              backgroundColor: 'rgba(255,255,255,0.06)',
            }}
          >
            {avatar && <Image source={{ uri: avatar }} style={{ width: '100%', height: '100%' }} />}
            <Animated.View
              pointerEvents="none"
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                right: 0,
                bottom: 0,
                alignItems: 'center',
                justifyContent: 'center',
                backgroundColor: 'rgba(0,0,0,0.4)',
                opacity: anim,
              }}
            >
              <PlayIcon size={14} color="#ffffff" />
            </Animated.View>
          </View>

          <Animated.View
            pointerEvents="none"
            style={{
              position: 'absolute',
              top: 0,
              left: 0,
              right: 0,
              bottom: 0,
              borderRadius: 46,
              borderWidth: 2,
              borderColor: accent.base,
              opacity: anim,
              shadowColor: accent.glow,
              shadowOffset: { width: 0, height: 0 },
              shadowOpacity: 1,
              shadowRadius: 24,
              elevation: 8,
            }}
          />
        </View>

        <ScText numberOfLines={1} style={{ maxWidth: 128, fontSize: 13, fontWeight: '600', color: 'rgba(255,255,255,0.85)' }}>
          {neighbor.artist_name}
        </ScText>

        <View
          style={{
            flexDirection: 'row',
            alignItems: 'center',
            gap: 8,
            width: '100%',
            borderRadius: 10,
            borderWidth: 1,
            padding: 6,
            borderColor: hover ? accent.glow : 'rgba(255,255,255,0.06)',
            backgroundColor: hover ? 'rgba(255,255,255,0.05)' : 'rgba(255,255,255,0.03)',
          }}
        >
          <Cover url={track.artwork_url} size={28} radius={6} artSize="t200x200" style={{ borderWidth: 1, borderColor: 'rgba(255,255,255,0.08)' }} />
          <View style={{ flex: 1, minWidth: 0 }}>
            <ScText
              numberOfLines={1}
              style={{ fontSize: 10.5, fontWeight: '500', lineHeight: 13, color: hover ? 'rgba(255,255,255,0.85)' : 'rgba(255,255,255,0.6)' }}
            >
              {track.title}
            </ScText>
            <ScText
              numberOfLines={1}
              style={{ fontSize: 9.5, lineHeight: 12, fontFamily: 'JetBrains Mono', color: 'rgba(255,255,255,0.3)' }}
            >
              {formatDuration(track.duration_ms)}
            </ScText>
          </View>
        </View>
      </Animated.View>
    </Pressable>
  );
}
