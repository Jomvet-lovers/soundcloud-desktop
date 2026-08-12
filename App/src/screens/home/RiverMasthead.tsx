import { useEffect, useRef } from 'react';
import { Animated, View } from 'react-native';
import type { Me } from '@sc/data';
import {
  AudioLinesIcon,
  auraRgba,
  Cover,
  GradientText,
  ScText,
  SoundprintBars,
  useScTheme,
  VibePortal,
} from '@sc/ui';
import type { Soundprint } from './useSoundprint';

/** Позывной-точка эфира: пульсирует, только пока играет (донор riv-pulse). */
function PulseDot({ color, playing }: { color: string; playing: boolean }) {
  const v = useRef(new Animated.Value(0)).current;
  useEffect(() => {
    if (!playing) {
      v.setValue(0);
      return;
    }
    const loop = Animated.loop(
      Animated.sequence([
        Animated.timing(v, { toValue: 1, duration: 800, useNativeDriver: true }),
        Animated.timing(v, { toValue: 0, duration: 800, useNativeDriver: true }),
      ]),
    );
    loop.start();
    return () => loop.stop();
  }, [playing, v]);
  return (
    <Animated.View
      style={{
        width: 6,
        height: 6,
        borderRadius: 3,
        backgroundColor: color,
        opacity: v.interpolate({ inputRange: [0, 1], outputRange: [1, 0.4] }),
        transform: [{ scale: v.interpolate({ inputRange: [0, 1], outputRange: [1, 1.35] }) }],
      }}
    />
  );
}

function greeting(name: string): string {
  const h = new Date().getHours();
  const word = h < 5 ? 'Доброй ночи' : h < 12 ? 'Доброе утро' : h < 18 ? 'Добрый день' : 'Добрый вечер';
  return `${word}, ${name}`;
}

/** Шапка «Течения»: позывные + приветствие-градиент; ниже — спектр вкуса
 *  (клик по жанру ретинтит страницу) и vibe-портал. */
export function RiverMasthead({
  me,
  sound,
  selected,
  onSelect,
  playing,
  onOpenSearch,
}: {
  me: Me | null;
  sound: Soundprint;
  selected: string | null;
  onSelect: (genre: string | null) => void;
  playing: boolean;
  onOpenSearch: () => void;
}) {
  const { accent } = useScTheme();
  const name = me?.username ?? '';

  return (
    <View style={{ paddingTop: 8 }}>
      <View style={{ flexDirection: 'row', alignItems: 'flex-start', justifyContent: 'space-between', gap: 24 }}>
        <View style={{ flex: 1, minWidth: 0 }}>
          <View style={{ flexDirection: 'row', alignItems: 'center', gap: 10, marginBottom: 10 }}>
            <View
              style={{
                flexDirection: 'row',
                alignItems: 'center',
                gap: 6,
                borderRadius: 9999,
                borderWidth: 1,
                borderColor: 'rgba(255,255,255,0.12)',
                backgroundColor: 'rgba(255,255,255,0.03)',
                paddingHorizontal: 10,
                paddingVertical: 4,
              }}
            >
              <PulseDot color={accent.base} playing={playing} />
              <ScText style={{ fontSize: 9.5, fontWeight: '700', letterSpacing: 9.5 * 0.16, textTransform: 'uppercase', color: 'rgba(255,255,255,0.7)' }}>
                ТЕЧЕНИЕ
              </ScText>
            </View>
            <ScText style={{ fontSize: 10.5, fontWeight: '600', letterSpacing: 10.5 * 0.22, textTransform: 'uppercase', color: 'rgba(255,255,255,0.3)' }}>
              ЛИЧНАЯ РЕКА · ТЕЧЁТ 24/7
            </ScText>
          </View>

          <GradientText
            text={greeting(name)}
            gradient={sound.aura.nameGradient}
            style={{ fontSize: 32, fontWeight: '900', lineHeight: 34, letterSpacing: -0.5 }}
          />
          <ScText style={{ marginTop: 8, fontSize: 13.5, color: 'rgba(255,255,255,0.5)' }}>
            Твоя личная волна — подобрали под тебя
          </ScText>
        </View>

        <View
          style={{
            width: 64,
            height: 64,
            borderRadius: 32,
            overflow: 'hidden',
            borderWidth: 0.5,
            borderColor: auraRgba(sound.aura, 0.4),
            shadowColor: sound.accentGlow,
            shadowOffset: { width: 0, height: 10 },
            shadowOpacity: 1,
            shadowRadius: 30,
          }}
        >
          <Cover url={me?.avatar_url ?? null} size={64} radius={0} artSize="t300x300" />
        </View>
      </View>

      <View style={{ flexDirection: 'row', alignItems: 'stretch', gap: 16, marginTop: 24 }}>
        {sound.hasData && (
          <View
            style={{
              flex: 1,
              minWidth: 0,
              borderRadius: 16,
              borderWidth: 1,
              borderColor: 'rgba(255,255,255,0.07)',
              backgroundColor: 'rgba(255,255,255,0.025)',
              paddingHorizontal: 20,
              paddingVertical: 16,
            }}
          >
            <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8, marginBottom: 12 }}>
              <AudioLinesIcon size={13} color={sound.spectrum[0]?.color ?? accent.base} />
              <ScText style={{ fontSize: 10, fontWeight: '700', letterSpacing: 10 * 0.24, textTransform: 'uppercase', color: 'rgba(255,255,255,0.45)' }}>
                ТВОЙ ЗВУКОВОЙ ОТПЕЧАТОК
              </ScText>
            </View>
            <SoundprintBars spectrum={sound.spectrum} selected={selected} onSelect={onSelect} />
          </View>
        )}
        <View style={{ width: 360, justifyContent: 'flex-end' }}>
          <VibePortal onPress={onOpenSearch} />
        </View>
      </View>
    </View>
  );
}
