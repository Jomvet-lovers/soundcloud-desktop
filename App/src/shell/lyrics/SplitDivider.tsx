import { useRef, useState } from 'react';
import { type GestureResponderEvent, PanResponder, View } from 'react-native';
import { clampSplit, LYRICS_SPLIT_DEFAULT } from '../../player/lyrics-ui';

const DOUBLE_TAP_MS = 320;

/** Перетаскиваемый разделитель колонок (донор `SplitDivider`). Тот же PanResponder +
 *  `measureInWindow` паттерн, что у `now-playing/ProgressLane.AbLoopOverlay` —
 *  абсолютный pageX относительно измеренного контейнера, без web-only pointer events,
 *  работает на всех RN-хостах. Двойной клик/тап — сброс к 50/50. */
export function SplitDivider({ splitRatio, onChange }: { splitRatio: number; onChange: (ratio: number) => void }) {
  const rootRef = useRef<View>(null);
  const pageXRef = useRef(0);
  const widthRef = useRef(0);
  const lastTapRef = useRef(0);
  const [active, setActive] = useState(false);

  const remeasure = () =>
    rootRef.current?.measureInWindow((x, _y, w) => {
      pageXRef.current = x;
      widthRef.current = w;
    });

  const ratioAt = (e: GestureResponderEvent) => {
    const w = widthRef.current || 1;
    return clampSplit((e.nativeEvent.pageX - pageXRef.current) / w);
  };

  const responder = useRef(
    PanResponder.create({
      onStartShouldSetPanResponder: () => true,
      onMoveShouldSetPanResponder: () => true,
      onPanResponderGrant: () => {
        remeasure();
        const now = Date.now();
        if (now - lastTapRef.current < DOUBLE_TAP_MS) {
          lastTapRef.current = 0;
          onChange(LYRICS_SPLIT_DEFAULT);
          return;
        }
        lastTapRef.current = now;
        setActive(true);
      },
      onPanResponderMove: (e) => onChange(ratioAt(e)),
      onPanResponderRelease: () => setActive(false),
      onPanResponderTerminate: () => setActive(false),
    }),
  ).current;

  const pct = splitRatio * 100;

  return (
    <View ref={rootRef} onLayout={remeasure} pointerEvents="box-none" style={{ position: 'absolute', left: 0, right: 0, top: 0, bottom: 0 }}>
      <View
        pointerEvents="none"
        style={{
          position: 'absolute',
          top: 0,
          bottom: 0,
          left: `${pct}%`,
          width: 1,
          backgroundColor: active ? 'rgba(255,255,255,0.2)' : 'rgba(255,255,255,0.05)',
        }}
      />
      <View
        {...responder.panHandlers}
        // @ts-expect-error cursor — веб-стиль RN-web, вне RN-типов
        style={{
          position: 'absolute',
          top: '50%',
          left: `${pct}%`,
          marginTop: -28,
          marginLeft: -6,
          width: 12,
          height: 56,
          borderRadius: 999,
          alignItems: 'center',
          justifyContent: 'center',
          gap: 3,
          borderWidth: 1,
          cursor: 'col-resize',
          backgroundColor: active ? 'rgba(255,255,255,0.12)' : 'rgba(255,255,255,0.04)',
          borderColor: active ? 'rgba(255,255,255,0.18)' : 'rgba(255,255,255,0.08)',
        }}
      >
        <View style={{ width: 2, height: 4, borderRadius: 1, backgroundColor: 'rgba(255,255,255,0.35)' }} />
        <View style={{ width: 2, height: 4, borderRadius: 1, backgroundColor: 'rgba(255,255,255,0.35)' }} />
        <View style={{ width: 2, height: 4, borderRadius: 1, backgroundColor: 'rgba(255,255,255,0.35)' }} />
      </View>
    </View>
  );
}
