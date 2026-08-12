import { useEffect, useRef } from 'react';
import { Animated, View } from 'react-native';
import { useSc } from '@sc/data';
import { useScTheme } from '@sc/ui';

/** FFT-визуализатор лирики для нативных хостов (ios/android/mac/win) — полосы вместо
 *  canvas-волны. Живёт от события ядра `spectrum`; высота баров едет императивно через
 *  `Animated.Value.setValue` (0 ре-рендеров), см. Core/CLAUDE.md п.10. */
const BINS = 48;

export function LyricsVisualizer() {
  const sc = useSc();
  const { accent } = useScTheme();
  const bars = useRef(Array.from({ length: BINS }, () => new Animated.Value(0))).current;

  useEffect(() => {
    const off = sc.on('spectrum', (spec) => {
      if (!spec || spec.length === 0) return;
      const step = spec.length / BINS;
      for (let i = 0; i < BINS; i++) {
        const v = spec[Math.min(spec.length - 1, Math.floor(i * step))] ?? 0;
        bars[i].setValue(Math.max(0, Math.min(1, v)));
      }
    });
    return off;
  }, [sc, bars]);

  return (
    <View pointerEvents="none" style={{ position: 'absolute', left: 0, right: 0, bottom: 0, height: '48%', flexDirection: 'row', alignItems: 'flex-end', gap: 2, paddingHorizontal: 6, zIndex: 0, opacity: 0.5 }}>
      {bars.map((v, i) => (
        <Animated.View
          key={i}
          style={{
            flex: 1,
            borderTopLeftRadius: 2,
            borderTopRightRadius: 2,
            backgroundColor: accent.base,
            transform: [{ scaleY: v }],
            height: '100%',
          }}
        />
      ))}
    </View>
  );
}
