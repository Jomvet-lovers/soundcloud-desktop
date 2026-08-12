import { useEffect, useRef, useState } from 'react';
import type { LayoutChangeEvent, ViewStyle } from 'react-native';
import { Animated, View } from 'react-native';

/** Слайдер прогресса/громкости (донор `.npb-lane`/`.npb-vol-slider` = Radix): тап и
 *  drag. Позиция берётся из `clientX - измеренный left` через pointer-события — они на
 *  RN-web несут реальные координаты (в отличие от `Pressable.onPress.locationX`, откуда
 *  раньше приходил NaN и seek не срабатывал). */
export function Slider({
  value = 0,
  animatedPct,
  onSeek,
  color,
  glowColor,
  height = 3,
  hoverHeight = 5,
  thumbSize = 12,
  disabled = false,
  tickFrac,
  style,
}: {
  /** статичное заполнение 0..1 (громкость) */
  value?: number;
  /** Animated-заполнение как '%'-строка (прогресс — едет без ре-рендеров) */
  animatedPct?: Animated.AnimatedInterpolation<string>;
  onSeek: (fraction: number) => void;
  color: string;
  glowColor: string;
  height?: number;
  hoverHeight?: number;
  thumbSize?: number;
  disabled?: boolean;
  /** засечка-ориентир (напр. 100% громкости = 0.5 диапазона 0-200) */
  tickFrac?: number;
  style?: ViewStyle;
}) {
  const rootRef = useRef<View>(null);
  const geom = useRef({ x: 0, w: 0 });
  const dragging = useRef(false);
  const [hovered, setHovered] = useState(false);
  const [dragFrac, setDragFrac] = useState<number | null>(null);
  const active = hovered || dragFrac != null;
  const hover = useRef(new Animated.Value(0)).current;

  useEffect(() => {
    Animated.timing(hover, { toValue: active ? 1 : 0, duration: 150, useNativeDriver: false }).start();
  }, [active, hover]);

  // Геометрию берём СИНХРОННО из DOM-ноды события (`currentTarget.getBoundingClientRect`)
  // — `measureInWindow` асинхронный, к моменту клика geom.x ещё нулевой и фракция
  // клампилась в 1.0. Фолбэк на onLayout-ширину + measureInWindow для нативных хостов.
  type PtrEvt = { currentTarget?: { getBoundingClientRect?: () => { left: number; width: number } }; nativeEvent: { clientX?: number; pageX?: number } };
  const captureGeom = (e: PtrEvt) => {
    const el = e.currentTarget;
    if (el && typeof el.getBoundingClientRect === 'function') {
      const r = el.getBoundingClientRect();
      if (r.width) { geom.current = { x: r.left, w: r.width }; return; }
    }
    rootRef.current?.measureInWindow((x, _y, w) => { if (w) geom.current = { x, w }; });
  };
  const onLayout = (e: LayoutChangeEvent) => {
    geom.current.w = e.nativeEvent.layout.width;
  };

  const fracAt = (clientX: number): number | null => {
    const { x, w } = geom.current;
    if (!w || !Number.isFinite(clientX)) return null;
    return Math.max(0, Math.min(1, (clientX - x) / w));
  };

  const onDown = (e: PtrEvt) => {
    if (disabled) return;
    captureGeom(e);
    const cx = e.nativeEvent.clientX ?? e.nativeEvent.pageX;
    if (cx == null) return;
    dragging.current = true;
    const f = fracAt(cx);
    if (f != null) setDragFrac(f);
  };
  const onMove = (e: PtrEvt) => {
    if (!dragging.current) return;
    const cx = e.nativeEvent.clientX ?? e.nativeEvent.pageX;
    if (cx == null) return;
    const f = fracAt(cx);
    if (f != null) setDragFrac(f);
  };
  const onUp = () => {
    if (!dragging.current) return;
    dragging.current = false;
    setDragFrac((f) => {
      if (f != null) onSeek(f);
      return null;
    });
  };

  const pct = `${Math.max(0, Math.min(1, value)) * 100}%` as const;
  const fill = dragFrac != null ? (`${dragFrac * 100}%` as const) : (animatedPct ?? pct);

  return (
    <View
      ref={rootRef}
      onLayout={onLayout}
      onPointerDown={onDown}
      onPointerMove={onMove}
      onPointerUp={onUp}
      onPointerCancel={onUp}
      onPointerEnter={() => setHovered(true)}
      onPointerLeave={() => setHovered(false)}
      // @ts-expect-error cursor + touchAction — web-стили RN-web, вне RN-типов
      style={[{ height: 20, justifyContent: 'center', cursor: disabled ? 'default' : 'pointer', touchAction: 'none' }, style]}
    >
      <Animated.View
        pointerEvents="none"
        style={{
          height: hover.interpolate({ inputRange: [0, 1], outputRange: [height, hoverHeight] }),
          borderRadius: 999,
          backgroundColor: 'rgba(255,255,255,0.08)',
          justifyContent: 'center',
        }}
      >
        <Animated.View
          style={{ position: 'absolute', left: 0, top: 0, bottom: 0, width: fill, borderRadius: 999, backgroundColor: color }}
        />
        {tickFrac != null && (
          <View pointerEvents="none" style={{ position: 'absolute', top: '50%', marginTop: -1.5, width: 1, height: 3, backgroundColor: 'rgba(255,255,255,0.25)', left: `${tickFrac * 100}%` }} />
        )}
        <Animated.View
          pointerEvents="none"
          style={{
            position: 'absolute',
            left: fill,
            marginLeft: -thumbSize / 2,
            width: thumbSize,
            height: thumbSize,
            borderRadius: thumbSize / 2,
            backgroundColor: color,
            boxShadow: `0 0 10px ${glowColor}`,
            opacity: hover,
            transform: [{ scale: hover }],
          }}
        />
      </Animated.View>
    </View>
  );
}
