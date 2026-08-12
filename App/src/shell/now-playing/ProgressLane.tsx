import { useMemo, useRef, useState } from 'react';
import { Text, View } from 'react-native';
import { formatDuration, ScText } from '@sc/ui';
import type { AbLoop } from '../../player/PlayerContext';
import { AB_MIN_GAP } from '../../player/PlayerContext';
import { positionValue, useWholeSeconds } from '../../player/position';
import { Slider } from './Slider';

const timeStyle = { fontSize: 9.5, fontWeight: '600' as const, letterSpacing: 9.5 * 0.02, fontVariant: ['tabular-nums'] as ['tabular-nums'] };

/** Времена + прогресс (донор `.npb-lane`/`.npb-times`). Заполнение едет за
 *  positionValue (Animated, 0 ре-рендеров), тикающее время — изолированно 1Гц.
 *  Поверх трека — перетаскиваемые A-B-маркеры петли. */
export function ProgressLane({
  durationSecs,
  abLoop,
  onSeek,
  onNudgeAb,
  accentColor,
  glowColor,
  disabled,
}: {
  durationSecs: number;
  abLoop: AbLoop | null;
  onSeek: (secs: number) => void;
  onNudgeAb: (which: 'a' | 'b', value: number) => void;
  accentColor: string;
  glowColor: string;
  disabled: boolean;
}) {
  const animatedPct = useMemo(
    () => positionValue.interpolate({ inputRange: [0, Math.max(1, durationSecs)], outputRange: ['0%', '100%'], extrapolate: 'clamp' }),
    [durationSecs],
  );

  return (
    <View style={{ gap: 0 }}>
      <View style={{ flexDirection: 'row', justifyContent: 'space-between', paddingHorizontal: 1, paddingBottom: 1 }}>
        <CurrentTime />
        <ScText style={[timeStyle, { color: 'rgba(255,255,255,0.42)' }]}>{formatDuration(durationSecs * 1000)}</ScText>
      </View>
      <View style={{ position: 'relative', justifyContent: 'center' }}>
        <Slider
          animatedPct={animatedPct}
          onSeek={(f) => onSeek(f * durationSecs)}
          color={accentColor}
          glowColor={glowColor}
          height={3}
          hoverHeight={5}
          thumbSize={12}
          disabled={disabled}
        />
        <AbLoopOverlay abLoop={abLoop} durationSecs={durationSecs} onNudge={onNudgeAb} accentColor={accentColor} glowColor={glowColor} />
      </View>
    </View>
  );
}

/** Тикающее «текущее время» — единственный компонент, ре-рендерящийся 1Гц. */
function CurrentTime() {
  const secs = useWholeSeconds();
  return <ScText style={[timeStyle, { color: 'rgba(255,255,255,0.9)', fontWeight: '700' }]}>{formatDuration(secs * 1000)}</ScText>;
}

/** Маркеры A-B (донор `AbLoopOverlay`): полоса от A до B + перетаскиваемые ручки.
 *  Во время drag двигаем локально (без RPC), коммитим `onNudge` на отпускании. */
function AbLoopOverlay({
  abLoop,
  durationSecs,
  onNudge,
  accentColor,
  glowColor,
}: {
  abLoop: AbLoop | null;
  durationSecs: number;
  onNudge: (which: 'a' | 'b', value: number) => void;
  accentColor: string;
  glowColor: string;
}) {
  const geom = useRef({ x: 0, w: 0 });
  const rootRef = useRef<View>(null);
  const [drag, setDrag] = useState<{ which: 'a' | 'b'; value: number } | null>(null);

  const measure = () => rootRef.current?.measureInWindow((x, _y, w) => { geom.current = { x, w }; });

  if (!abLoop || durationSecs <= 0) {
    return <View ref={rootRef} pointerEvents="none" style={{ position: 'absolute', left: 0, right: 0, top: 0, bottom: 0 }} onLayout={measure} />;
  }

  const a = drag?.which === 'a' ? drag.value : abLoop.a;
  const b = drag?.which === 'b' ? drag.value : abLoop.b;
  const aPct = clamp01(a / durationSecs) * 100;
  const bPct = b != null ? clamp01(b / durationSecs) * 100 : null;

  // Drag через window-pointer-события (RN-web responder мышь не ловит): маркер
  // может уехать за пределы ручки — слушаем на window до pointerup, коммит `onNudge`.
  const startDrag = (which: 'a' | 'b') => (e: { nativeEvent: { clientX?: number } }) => {
    measure();
    const lo = which === 'a' ? 0 : abLoop.a + AB_MIN_GAP;
    const hi = which === 'a' ? (abLoop.b ?? durationSecs) - AB_MIN_GAP : durationSecs;
    const valAt = (clientX: number) => {
      const { x, w } = geom.current;
      return Math.max(lo, Math.min(hi, ((clientX - x) / (w || 1)) * durationSecs));
    };
    let latest = which === 'a' ? abLoop.a : (abLoop.b ?? abLoop.a);
    if (e.nativeEvent.clientX != null) latest = valAt(e.nativeEvent.clientX);
    setDrag({ which, value: latest });
    const move = (ev: PointerEvent) => {
      latest = valAt(ev.clientX);
      setDrag({ which, value: latest });
    };
    const up = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      onNudge(which, latest);
      setDrag(null);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  };

  return (
    <View ref={rootRef} pointerEvents="box-none" style={{ position: 'absolute', left: 0, right: 0, top: 0, bottom: 0 }} onLayout={measure}>
      {bPct != null && (
        <View pointerEvents="none" style={{ position: 'absolute', top: '50%', height: 5, marginTop: -2.5, left: `${aPct}%`, width: `${Math.max(0, bPct - aPct)}%`, borderRadius: 999, backgroundColor: `${accentColor}40` }} />
      )}
      <Handle left={aPct} accentColor={accentColor} glowColor={glowColor} onDown={startDrag('a')} />
      {bPct != null && <Handle left={bPct} accentColor={accentColor} glowColor={glowColor} onDown={startDrag('b')} />}
      {drag && (
        <View
          pointerEvents="none"
          style={{ position: 'absolute', bottom: '100%', marginBottom: 7, left: `${(drag.which === 'a' ? aPct : bPct ?? aPct)}%`, transform: [{ translateX: -18 }], width: 36, alignItems: 'center' }}
        >
          <Text style={{ fontSize: 10, fontWeight: '600', color: '#fff', backgroundColor: 'rgba(0,0,0,0.8)', paddingHorizontal: 5, paddingVertical: 1.5, borderRadius: 5, overflow: 'hidden', fontVariant: ['tabular-nums'] }}>
            {formatDuration(drag.value * 1000)}
          </Text>
        </View>
      )}
    </View>
  );
}

function Handle({ left, accentColor, glowColor, onDown }: { left: number; accentColor: string; glowColor: string; onDown: (e: { nativeEvent: { clientX?: number } }) => void }) {
  return (
    <View
      onPointerDown={onDown}
      // @ts-expect-error cursor + touchAction — веб-стили RN-web, вне RN-типов
      style={{ position: 'absolute', top: '50%', marginTop: -10, marginLeft: -7, left: `${left}%`, width: 14, height: 20, alignItems: 'center', justifyContent: 'center', cursor: 'ew-resize', touchAction: 'none' }}
    >
      <View pointerEvents="none" style={{ width: 3, height: 14, borderRadius: 2, backgroundColor: accentColor, boxShadow: `0 0 8px ${glowColor}` }} />
    </View>
  );
}

const clamp01 = (v: number) => Math.max(0, Math.min(1, v));
