import { useEffect, useMemo, useRef, useState } from 'react';
import { Animated, type LayoutChangeEvent, Pressable, type TextStyle, View } from 'react-native';
import { type Comment, type Track, useSc } from '@sc/data';
import { Cover, formatDuration, LiveWaveform, ScText } from '@sc/ui';
import { positionValue, useWholeSeconds } from '../../player/position';
import type { TrackAura } from './useTrackAura';

const HEIGHT = 96;
// голоса группируем в слоты — иначе сотня комментов сливается в сплошную полосу
const SLOTS = 46;

function VoiceDot({ x, containerWidth, body, avatarUrl, count, aura, onPress }: { x: number; containerWidth: number; body: string; avatarUrl: string | null | undefined; count: number; aura: TrackAura; onPress: () => void }) {
  const [hover, setHover] = useState(false);
  const [bw, setBw] = useState(0);
  const a = useRef(new Animated.Value(0)).current;
  useEffect(() => {
    Animated.timing(a, { toValue: hover ? 1 : 0, duration: 200, useNativeDriver: true }).start();
  }, [hover, a]);
  // бабл центрируем на пипке (-bw/2); у краёв дорожки прижимаем внутрь по фактической ширине
  const lo = 8 - x;
  const hi = containerWidth - 8 - bw - x;
  const tx = bw === 0 ? -3.5 : hi < lo ? lo : Math.min(Math.max(-bw / 2, lo), hi);
  return (
    <Pressable
      onPress={onPress}
      onHoverIn={() => setHover(true)}
      onHoverOut={() => setHover(false)}
      style={{ position: 'absolute', bottom: 0, left: x, alignItems: 'center', zIndex: hover ? 50 : 1 }}
    >
      <Animated.View
        pointerEvents="none"
        onLayout={(e) => setBw(e.nativeEvent.layout.width)}
        style={{ position: 'absolute', left: 0, bottom: 18, opacity: a, transform: [{ translateX: tx }, { translateY: a.interpolate({ inputRange: [0, 1], outputRange: [4, 0] }) }] }}
      >
        <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8, maxWidth: 260, paddingHorizontal: 12, paddingVertical: 8, borderRadius: 16, backgroundColor: 'rgba(20,20,26,0.96)', boxShadow: '0 16px 40px rgba(0,0,0,0.55)' }}>
          <Cover url={avatarUrl} size={24} radius={12} artSize="t200x200" />
          <View style={{ flexShrink: 1 }}>
            <ScText numberOfLines={2} style={{ fontSize: 12, color: 'rgba(255,255,255,0.88)' }}>{body}</ScText>
            {count > 1 && (
              <ScText style={{ fontSize: 10, color: aura.accent, fontVariant: ['tabular-nums'] }}>ещё {count - 1} здесь</ScText>
            )}
          </View>
        </View>
      </Animated.View>
      <Animated.View style={{ transform: [{ translateX: -3.5 }, { scale: a.interpolate({ inputRange: [0, 1], outputRange: [1, 1.5] }) }] }}>
        <View style={{ width: 7, height: 7, borderRadius: 4, backgroundColor: aura.accent, boxShadow: `0 0 8px ${aura.accentGlow}` }} />
      </Animated.View>
    </Pressable>
  );
}

export function TrackWaveFloor({ track, isCurrent, comments, aura, onSeek }: {
  track: Track;
  isCurrent: boolean;
  comments: Comment[];
  aura: TrackAura;
  onSeek: (seconds: number) => void;
}) {
  const sc = useSc();
  const [peaks, setPeaks] = useState<number[]>([]);
  const [width, setWidth] = useState(0);
  const durSecs = track.duration_ms / 1000;

  useEffect(() => {
    if (!track.waveform_url) { setPeaks([]); return; }
    let alive = true;
    void sc.tracks.waveform(track.waveform_url).then((p) => { if (alive) setPeaks(p); }).catch(() => {});
    return () => { alive = false; };
  }, [sc, track.waveform_url]);

  const progressValue = useMemo(
    () => positionValue.interpolate({ inputRange: [0, Math.max(1, durSecs)], outputRange: [0, 1], extrapolate: 'clamp' }),
    [durSecs],
  );

  const dots = useMemo(() => {
    if (track.duration_ms <= 0) return [];
    const slots = new Map<number, { id: number; pct: number; body: string; ms: number; avatar: string | null; count: number }>();
    for (const c of comments) {
      if (c.timestamp_ms == null || !c.body.trim()) continue;
      const pct = Math.min(1, Math.max(0, c.timestamp_ms / track.duration_ms));
      const slot = Math.min(SLOTS - 1, Math.round(pct * SLOTS));
      const hit = slots.get(slot);
      if (hit) hit.count++;
      else slots.set(slot, { id: c.id, pct, body: c.body, ms: c.timestamp_ms, avatar: c.user.avatar_url, count: 1 });
    }
    return [...slots.values()].sort((a, b) => a.pct - b.pct);
  }, [comments, track.duration_ms]);

  const onLayout = (e: LayoutChangeEvent) => setWidth(e.nativeEvent.layout.width);

  return (
    <View>
      <View style={{ position: 'relative', width: '100%' }} onLayout={onLayout}>
        {width > 0 && (
          <LiveWaveform
            peaks={peaks}
            width={width}
            height={HEIGHT}
            progressValue={isCurrent ? progressValue : undefined}
            progress={isCurrent ? undefined : 0}
            accentColor={aura.accent}
            glowColor={aura.accentGlow}
            onSeek={(f) => onSeek(f * durSecs)}
          />
        )}
        {width > 0 && (
          <View pointerEvents="box-none" style={{ position: 'absolute', left: 0, right: 0, top: 0, bottom: 0 }}>
            {dots.map((d) => (
              <VoiceDot key={d.id} x={d.pct * width} containerWidth={width} body={d.body} avatarUrl={d.avatar} count={d.count} aura={aura} onPress={() => onSeek(d.ms / 1000)} />
            ))}
          </View>
        )}
      </View>
      <Ruler durSecs={durSecs} isCurrent={isCurrent} />
    </View>
  );
}

function Ruler({ durSecs, isCurrent }: { durSecs: number; isCurrent: boolean }) {
  const secs = useWholeSeconds();
  const cur = isCurrent ? secs : 0;
  const time: TextStyle = { fontSize: 11, fontVariant: ['tabular-nums'], color: 'rgba(255,255,255,0.35)' };
  return (
    <View style={{ flexDirection: 'row', justifyContent: 'space-between', marginTop: 10, paddingHorizontal: 2 }}>
      <ScText style={time}>{formatDuration(cur * 1000)}</ScText>
      <ScText style={time}>{formatDuration(durSecs * 1000)}</ScText>
    </View>
  );
}
