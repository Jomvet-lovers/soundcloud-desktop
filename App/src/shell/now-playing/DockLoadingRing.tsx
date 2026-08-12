import { useState } from 'react';
import { type LayoutChangeEvent, View } from 'react-native';
import Svg, { Path } from 'react-native-svg';
import { useScTheme } from '@sc/ui';
import { useDownloadProgress } from '../../player/download';

const R = 28;

/** Акцентный контур, обегающий периметр пилюли по мере скачки трека (донор
 *  `DockLoadingRing`). Периметр меряем из onLayout, чертим скруглённый прямоугольник
 *  и открываем его штрих через strokeDashoffset. Активен только пока идёт загрузка. */
export function DockLoadingRing() {
  const { accent } = useScTheme();
  const progress = useDownloadProgress();
  const [size, setSize] = useState<{ w: number; h: number } | null>(null);

  const onLayout = (e: LayoutChangeEvent) => {
    const { width, height } = e.nativeEvent.layout;
    setSize((s) => (s && s.w === width && s.h === height ? s : { w: width, h: height }));
  };

  return (
    <View pointerEvents="none" onLayout={onLayout} style={{ position: 'absolute', left: 0, right: 0, top: 0, bottom: 0 }}>
      {progress != null && size && size.w > 0 && (
        <Ring w={size.w} h={size.h} pct={progress} color={accent.base} />
      )}
    </View>
  );
}

function Ring({ w, h, pct, color }: { w: number; h: number; pct: number; color: string }) {
  const r = Math.min(R, w / 2, h / 2);
  const inset = 1;
  const x = inset;
  const y = inset;
  const rw = w - inset * 2;
  const rh = h - inset * 2;
  // Скруглённый прямоугольник, старт сверху по центру — чтобы контур «раскрывался» симметрично.
  const d = `M ${x + rw / 2} ${y} H ${x + rw - r} A ${r} ${r} 0 0 1 ${x + rw} ${y + r} V ${y + rh - r} A ${r} ${r} 0 0 1 ${x + rw - r} ${y + rh} H ${x + r} A ${r} ${r} 0 0 1 ${x} ${y + rh - r} V ${y + r} A ${r} ${r} 0 0 1 ${x + r} ${y} Z`;
  const len = 2 * (rw + rh) - 8 * r + 2 * Math.PI * r;
  const shown = Math.max(0, Math.min(1, pct)) * len;

  return (
    <Svg width={w} height={h} style={{ position: 'absolute', top: 0, left: 0 }}>
      <Path d={d} fill="none" stroke="rgba(255,255,255,0.06)" strokeWidth={1.5} />
      <Path d={d} fill="none" stroke={color} strokeWidth={1.5} strokeLinecap="round" strokeDasharray={`${shown} ${len}`} />
    </Svg>
  );
}
