import { useEffect, useRef, useState } from 'react';
import { ActivityIndicator, Animated, Pressable, View } from 'react-native';
import type { Track, User } from '@sc/data';
import { Cover, ScText, TrackRow } from '@sc/ui';
import { badgeOf } from '../shared/cards';
import { formatCount } from './format';
import type { TrackAura } from './useTrackAura';

const PANEL = { backgroundColor: 'rgba(255,255,255,0.035)', borderWidth: 1, borderColor: 'rgba(255,255,255,0.06)' } as const;
const LABEL = { fontSize: 11, fontWeight: '700' as const, textTransform: 'uppercase' as const, letterSpacing: 1.8, color: 'rgba(255,255,255,0.4)' };

/** Панель, плавно приподнимающаяся на ховере (донор `hover:-translate-y-0.5`). */
function LiftCard({ onPress, children, style }: { onPress?: () => void; children: React.ReactNode; style?: object }) {
  const [hover, setHover] = useState(false);
  const lift = useRef(new Animated.Value(0)).current;
  useEffect(() => {
    Animated.timing(lift, { toValue: hover ? 1 : 0, duration: 220, useNativeDriver: true }).start();
  }, [hover, lift]);
  return (
    <Pressable onPress={onPress} onHoverIn={() => setHover(true)} onHoverOut={() => setHover(false)}>
      <Animated.View style={[{ transform: [{ translateY: lift.interpolate({ inputRange: [0, 1], outputRange: [0, -2] }) }] }, style]}>{children}</Animated.View>
    </Pressable>
  );
}

/** Аватар с зумом на ховере (донор `group-hover:scale-105/110`). */
function ZoomAvatar({ url, size }: { url: string | null | undefined; size: number }) {
  const [hover, setHover] = useState(false);
  const z = useRef(new Animated.Value(0)).current;
  useEffect(() => {
    Animated.spring(z, { toValue: hover ? 1 : 0, useNativeDriver: true, speed: 30, bounciness: 6 }).start();
  }, [hover, z]);
  return (
    <Pressable onHoverIn={() => setHover(true)} onHoverOut={() => setHover(false)}>
      <Animated.View style={{ transform: [{ scale: z.interpolate({ inputRange: [0, 1], outputRange: [1, 1.08] }) }], borderRadius: size / 2 }}>
        <Cover url={url} size={size} radius={size / 2} artSize="t200x200" />
      </Animated.View>
    </Pressable>
  );
}

// стопка «кому зашло»: на ховере аватар выходит вперёд (полностью видим), получает
// акцент-рамку и подсветку; клик — на страницу лайкнувшего
function FavoriterAvatar({ url, index, count, accent, accentGlow, onPress }: { url: string | null | undefined; index: number; count: number; accent: string; accentGlow: string; onPress: () => void }) {
  const [hover, setHover] = useState(false);
  // JS-драйвер: анимируем borderColor (нативный драйвер умеет только opacity/transform)
  const z = useRef(new Animated.Value(0)).current;
  useEffect(() => {
    Animated.spring(z, { toValue: hover ? 1 : 0, useNativeDriver: false, speed: 30, bounciness: 6 }).start();
  }, [hover, z]);
  return (
    <Pressable
      onHoverIn={() => setHover(true)}
      onHoverOut={() => setHover(false)}
      onPress={onPress}
      style={{ marginLeft: index === 0 ? 0 : -12, zIndex: hover ? 100 : count - index }}
    >
      <Animated.View
        style={{
          borderRadius: 20,
          borderWidth: 2,
          opacity: z.interpolate({ inputRange: [0, 1], outputRange: [0.85, 1] }),
          borderColor: z.interpolate({ inputRange: [0, 1], outputRange: ['rgba(8,8,10,0.7)', accent] }),
          transform: [
            { scale: z.interpolate({ inputRange: [0, 1], outputRange: [1, 1.12] }) },
            { translateY: z.interpolate({ inputRange: [0, 1], outputRange: [0, -4] }) },
          ],
          boxShadow: hover ? `0 6px 18px ${accentGlow}` : undefined,
        }}
      >
        <Cover url={url} size={36} radius={18} artSize="t200x200" />
      </Animated.View>
    </Pressable>
  );
}

/** «Конверт»: кто сделал + кто вайбит + похожее (донор `RoomSleeve`). */
export function TrackSleeve({ track, uploader, favoriters, related, relatedLoading, aura, currentId, playing, onOpenUser, onPlayRelated }: {
  track: Track;
  uploader: User | null;
  favoriters: User[];
  related: Track[];
  relatedLoading: boolean;
  aura: TrackAura;
  currentId: string | null;
  playing: boolean;
  onOpenUser: (id: string) => void;
  onPlayRelated: (track: Track, queue: Track[]) => void;
}) {
  const artistId = uploader?.id ?? track.uploader?.id ?? track.artist.id;
  const artistName = uploader?.username ?? track.uploader?.username ?? track.artist.name;
  const artistAvatar = uploader?.avatar_url ?? track.uploader?.avatar_url ?? track.artist.avatar_url;
  const shown = favoriters.slice(0, 8);
  const total = track.likes_count ?? favoriters.length;
  const extra = Math.max(0, total - shown.length);
  const followers = uploader?.followers_count;
  const trackCount = uploader?.track_count;

  return (
    <View style={{ gap: 20 }}>
      <LiftCard onPress={() => onOpenUser(artistId)} style={{ ...PANEL, borderRadius: 24, padding: 24, alignItems: 'center', gap: 12 }}>
        <View style={{ boxShadow: `0 14px 36px ${aura.accentGlow}`, borderRadius: 40 }}>
          <ZoomAvatar url={artistAvatar} size={80} />
        </View>
        <View style={{ alignItems: 'center', gap: 4 }}>
          <ScText numberOfLines={1} style={{ fontSize: 15, fontWeight: '700', color: 'rgba(255,255,255,0.9)' }}>{artistName}</ScText>
          {(followers != null || trackCount != null) && (
            <ScText style={{ fontSize: 11, color: 'rgba(255,255,255,0.35)', fontVariant: ['tabular-nums'] }}>
              {followers != null ? `${formatCount(followers)} подписчиков` : ''}
              {followers != null && trackCount != null ? ' · ' : ''}
              {trackCount != null ? `${formatCount(trackCount)} треков` : ''}
            </ScText>
          )}
        </View>
      </LiftCard>

      {shown.length > 0 && (
        <View style={{ ...PANEL, borderRadius: 24, padding: 20, gap: 14 }}>
          <ScText style={LABEL}>Кому зашло</ScText>
          <View style={{ flexDirection: 'row', alignItems: 'center' }}>
            {shown.map((fu, i) => (
              <FavoriterAvatar
                key={fu.id}
                url={fu.avatar_url}
                index={i}
                count={shown.length}
                accent={aura.accent}
                accentGlow={aura.accentGlow}
                onPress={() => onOpenUser(fu.id)}
              />
            ))}
            {extra > 0 && <ScText style={{ marginLeft: 12, fontSize: 12, fontWeight: '700', color: aura.accent, fontVariant: ['tabular-nums'] }}>+{formatCount(extra)}</ScText>}
          </View>
        </View>
      )}

      <View style={{ gap: 8 }}>
        <ScText style={{ ...LABEL, paddingHorizontal: 2 }}>Похожие треки</ScText>
        {relatedLoading ? (
          <View style={{ paddingVertical: 24, alignItems: 'center' }}>
            <ActivityIndicator color="rgba(255,255,255,0.15)" />
          </View>
        ) : related.length === 0 ? (
          <ScText style={{ fontSize: 12, color: 'rgba(255,255,255,0.25)', paddingHorizontal: 2 }}>Похожего пока нет</ScText>
        ) : (
          <View style={{ gap: 2 }}>
            {related.map((rt) => (
              <TrackRow
                key={rt.id}
                title={rt.title}
                artist={rt.artist.name}
                artworkUrl={rt.artwork_url}
                durationMs={rt.duration_ms}
                active={rt.id === currentId}
                playing={playing}
                accent={aura.accent}
                accentSoft={aura.accentSoft}
                badge={badgeOf(rt) ?? undefined}
                playCount={rt.play_count}
                onToggle={() => onPlayRelated(rt, related)}
              />
            ))}
          </View>
        )}
      </View>
    </View>
  );
}
