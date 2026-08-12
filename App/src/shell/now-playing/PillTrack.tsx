import { useEffect, useRef } from 'react';
import { Animated, Easing, Pressable, View } from 'react-native';
import Svg, { Circle, Defs, LinearGradient, Rect, Stop } from 'react-native-svg';
import type { Track } from '@sc/data';
import { Cover, ScText, useScTheme } from '@sc/ui';
import { loadPercent, useDownloadProgress } from '../../player/download';
import { lyricsUi } from '../../player/lyrics-ui';

const EQ_DELAYS_MS = [-200, -560, -80, -380];
const EQ_CYCLE = 900;

/** Мини-эквалайзер в углу обложки (донор `.npb-eq`) — 4 бара, scaleY 0.32↔1. */
function EqBars({ playing, glow }: { playing: boolean; glow: string }) {
  const bars = useRef(EQ_DELAYS_MS.map(() => new Animated.Value(0.32))).current;
  const opacity = useRef(new Animated.Value(0)).current;

  useEffect(() => {
    Animated.timing(opacity, { toValue: playing ? 1 : 0, duration: 300, useNativeDriver: true }).start();
    if (!playing) return;
    const runners = bars.map((v, i) => {
      const phase = (EQ_CYCLE - (Math.abs(EQ_DELAYS_MS[i]) % EQ_CYCLE)) % EQ_CYCLE;
      const loop = Animated.loop(
        Animated.sequence([
          Animated.timing(v, { toValue: 1, duration: EQ_CYCLE / 2, easing: Easing.inOut(Easing.sin), useNativeDriver: true }),
          Animated.timing(v, { toValue: 0.32, duration: EQ_CYCLE / 2, easing: Easing.inOut(Easing.sin), useNativeDriver: true }),
        ]),
      );
      const runner = Animated.sequence([Animated.delay(phase), loop]);
      runner.start();
      return runner;
    });
    return () => runners.forEach((r) => r.stop());
  }, [playing, bars, opacity]);

  return (
    <Animated.View style={{ position: 'absolute', left: 6, bottom: 6, flexDirection: 'row', alignItems: 'flex-end', gap: 1.5, height: 11, opacity }}>
      {bars.map((v, i) => (
        <Animated.View
          key={i}
          style={{
            width: 2,
            height: 11,
            borderRadius: 1,
            backgroundColor: '#ffffff',
            boxShadow: `0 0 6px ${glow}`,
            transform: [{ scaleY: v }],
          }}
        />
      ))}
    </Animated.View>
  );
}

/** Вращающееся кольцо-«блик» по краю обложки (донор `.npb-ring`, conic-gradient). */
function ArtRing({ playing }: { playing: boolean }) {
  const opacity = useRef(new Animated.Value(0)).current;
  const spin = useRef(new Animated.Value(0)).current;

  useEffect(() => {
    Animated.timing(opacity, { toValue: playing ? 0.5 : 0, duration: 300, useNativeDriver: true }).start();
    if (!playing) return;
    spin.setValue(0);
    const loop = Animated.loop(
      Animated.timing(spin, { toValue: 1, duration: 7000, easing: Easing.linear, useNativeDriver: true }),
    );
    loop.start();
    return () => loop.stop();
  }, [playing, opacity, spin]);

  const rotate = spin.interpolate({ inputRange: [0, 1], outputRange: ['0deg', '360deg'] });

  return (
    <Animated.View pointerEvents="none" style={{ position: 'absolute', inset: 0, opacity, transform: [{ rotate }] }}>
      <Svg width={48} height={48} viewBox="0 0 48 48">
        <Circle cx={24} cy={24} r={19} fill="none" stroke="rgba(255,255,255,0.55)" strokeWidth={1.5} strokeDasharray="21.5 97.9" />
      </Svg>
    </Animated.View>
  );
}

/** Заглушка обложки, когда ничего не играет (донор `.npb-artfb`): accent→фиолет. */
function ArtFallback({ accent }: { accent: string }) {
  return (
    <Svg width={48} height={48} style={{ position: 'absolute', top: 0, left: 0, borderRadius: 13 }}>
      <Defs>
        <LinearGradient id="npb-artfb" x1="0" y1="0" x2="1" y2="1">
          <Stop offset="0" stopColor={accent} />
          <Stop offset="1" stopColor="#3a2bd0" />
        </LinearGradient>
      </Defs>
      <Rect x={0} y={0} width={48} height={48} rx={13} fill="url(#npb-artfb)" />
    </Svg>
  );
}

export function PillTrack({ track, playing }: { track: Track | null; playing: boolean }) {
  const { accent } = useScTheme();
  const progress = useDownloadProgress();

  return (
    <View style={{ flexDirection: 'row', alignItems: 'center', gap: 12, minWidth: 0 }}>
      <Pressable
        onPress={() => track && lyricsUi.open({ rightPanelOpen: false })}
        style={{
          width: 48,
          height: 48,
          borderRadius: 13,
          boxShadow: '0 8px 20px -8px rgba(0,0,0,0.75), inset 0 0 0 1px rgba(255,255,255,0.1)',
        }}
      >
        {track ? (
          <>
            <Cover url={track.artwork_url} size={48} radius={13} artSize="t200x200" />
            <ArtRing playing={playing} />
            <EqBars playing={playing} glow={accent.glow} />
          </>
        ) : (
          <ArtFallback accent={accent.base} />
        )}
        {progress != null && (
          <View style={{ position: 'absolute', top: 0, left: 0, right: 0, bottom: 0, borderRadius: 13, alignItems: 'center', justifyContent: 'center', backgroundColor: 'rgba(0,0,0,0.55)' }}>
            <ScText style={{ fontSize: 11, fontWeight: '700', color: '#fff' }}>{loadPercent(progress)}%</ScText>
          </View>
        )}
      </Pressable>

      <View style={{ minWidth: 0, maxWidth: 150, gap: 2 }}>
        {track ? (
          <>
            <ScText numberOfLines={1} style={{ fontSize: 13, fontWeight: '600', letterSpacing: -0.13, color: 'rgba(255,255,255,0.95)' }}>
              {track.title}
            </ScText>
            <ScText numberOfLines={1} style={{ fontSize: 11, fontWeight: '500', color: 'rgba(255,255,255,0.5)' }}>
              {track.artist.name}
            </ScText>
          </>
        ) : (
          <ScText style={{ fontSize: 11, fontWeight: '500', color: 'rgba(255,255,255,0.5)' }}>Ничего не играет</ScText>
        )}
      </View>
    </View>
  );
}
