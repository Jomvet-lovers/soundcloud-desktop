import { Image, Platform, StyleSheet, View } from 'react-native';
import Svg, { Defs, LinearGradient, RadialGradient, Rect, Stop } from 'react-native-svg';
import { artUrl, useScTheme } from '@sc/ui';

/** Иммерсивный фон (донор `LyricsBackdrop`): размытая обложка низкой прозрачности +
 *  цветной bloom из accent-темы + тёмный readability-veil. Без donor'ского canvas
 *  color-extraction (нет кросс-платформенного доступа к пикселям) — цвет bloom'а
 *  берём из темы. Блюр — только web (`filter` нативно недоступен без сторонних либ,
 *  на native ограничиваемся затемнением). */
export function LyricsBackdrop({ artworkUrl }: { artworkUrl: string | null | undefined }) {
  const { accent } = useScTheme();
  const uri = artUrl(artworkUrl, 't500x500');

  return (
    <View style={StyleSheet.absoluteFill} pointerEvents="none">
      {/* Blur-слой в своём GPU-layer, чтобы repaint контента поверх не пересчитывал blur. */}
      <View
        // @ts-expect-error contain/translateZ — веб-стиль RN-web, вне RN-типов
        style={[StyleSheet.absoluteFill, { contain: 'strict', transform: [{ translateZ: 0 }] }]}
      >
        {uri && (
          <Image
            source={{ uri }}
            style={[
              StyleSheet.absoluteFill,
              { opacity: 0.32, transform: [{ scale: 1.15 }] },
              // @ts-expect-error filter — веб-стиль RN-web, вне RN-типов
              Platform.OS === 'web' ? { filter: 'blur(60px) saturate(1.2)' } : null,
            ]}
          />
        )}
      </View>

      <View style={[StyleSheet.absoluteFill, { isolation: 'isolate' }]}>
        <Svg style={StyleSheet.absoluteFill} width="100%" height="100%" preserveAspectRatio="none">
          <Defs>
            <RadialGradient id="lyricsBloom" cx="26%" cy="28%" r="65%">
              <Stop offset="0" stopColor={accent.base} stopOpacity={0.28} />
              <Stop offset="1" stopColor={accent.base} stopOpacity={0} />
            </RadialGradient>
            <LinearGradient id="lyricsVeil" x1="0" y1="0" x2="0" y2="1">
              <Stop offset="0" stopColor="#08080a" stopOpacity={0.4} />
              <Stop offset="0.48" stopColor="#08080a" stopOpacity={0.58} />
              <Stop offset="1" stopColor="#08080a" stopOpacity={0.86} />
            </LinearGradient>
          </Defs>
          <Rect x={0} y={0} width="100%" height="100%" fill="url(#lyricsBloom)" />
          <Rect x={0} y={0} width="100%" height="100%" fill="url(#lyricsVeil)" />
        </Svg>
      </View>
    </View>
  );
}
