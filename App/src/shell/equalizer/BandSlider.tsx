import { memo, useCallback, useRef, useState } from 'react';
import type { GestureResponderEvent } from 'react-native';
import { View } from 'react-native';
import { ScText } from '@sc/ui';
import { EQ_MAX_GAIN, EQ_MIN_GAIN } from '../../player/eq-presets';

const TRACK_HEIGHT = 140;
const TRACK_WIDTH = 28;
const POSITIVE_COLOR = 'rgb(52,211,153)';
const NEGATIVE_COLOR = 'rgb(96,165,250)';
const ZERO_COLOR = 'rgba(255,255,255,0.5)';

function colorFor(gain: number): string {
  if (gain > 0) return POSITIVE_COLOR;
  if (gain < 0) return NEGATIVE_COLOR;
  return ZERO_COLOR;
}

function gainFromOffset(locationY: number): number {
  const t = 1 - Math.max(0, Math.min(1, locationY / TRACK_HEIGHT));
  const raw = t * (EQ_MAX_GAIN - EQ_MIN_GAIN) + EQ_MIN_GAIN;
  const stepped = Math.round(raw * 2) / 2;
  return Math.min(EQ_MAX_GAIN, Math.max(EQ_MIN_GAIN, stepped));
}

interface BandSliderProps {
  gain: number;
  label: string;
  onDrag: (gain: number) => void;
}

/** Вертикальный band-слайдер эквалайзера (донор `music/EqualizerPanel.tsx` BandSlider).
 *  Во время драга гейн живёт локально (изолированный ре-рендер этого листа) и коммитится
 *  в `EqContext` троттлингом по rAF — там на каждое изменение `gains` летит `set_eq` в ядро,
 *  без троттлинга драг по пикселю дёргал бы весь стек полос+пресетов на каждый кадр. */
export const BandSlider = memo(function BandSlider({ gain, label, onDrag }: BandSliderProps) {
  const [dragGain, setDragGain] = useState<number | null>(null);
  const pendingRef = useRef<number | null>(null);
  const rafRef = useRef<number | null>(null);

  const flush = useCallback(() => {
    rafRef.current = null;
    const value = pendingRef.current;
    pendingRef.current = null;
    if (value !== null) onDrag(value);
  }, [onDrag]);

  const scheduleCommit = useCallback(
    (next: number) => {
      pendingRef.current = next;
      setDragGain(next);
      if (rafRef.current === null) rafRef.current = requestAnimationFrame(flush);
    },
    [flush],
  );

  const respond = useCallback(
    (e: GestureResponderEvent) => scheduleCommit(gainFromOffset(e.nativeEvent.locationY)),
    [scheduleCommit],
  );

  const release = useCallback(() => {
    if (rafRef.current !== null) {
      cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    }
    const value = pendingRef.current;
    pendingRef.current = null;
    if (value !== null) onDrag(value);
    setDragGain(null);
  }, [onDrag]);

  const shownGain = dragGain ?? gain;
  const pct = (shownGain - EQ_MIN_GAIN) / (EQ_MAX_GAIN - EQ_MIN_GAIN);
  const color = colorFor(shownGain);
  const thumbY = (1 - pct) * TRACK_HEIGHT;
  const centerY = TRACK_HEIGHT / 2;
  const fillTop = Math.min(centerY, thumbY);
  const fillHeight = Math.abs(centerY - thumbY);
  const gainText = shownGain === 0 ? '0' : `${shownGain > 0 ? '+' : ''}${shownGain.toFixed(1)}`;

  return (
    <View style={{ alignItems: 'center', gap: 8, width: TRACK_WIDTH }}>
      <ScText style={{ fontSize: 10, fontWeight: '700', height: 14, color }}>{gainText}</ScText>

      <View
        onStartShouldSetResponder={() => true}
        onResponderGrant={respond}
        onResponderMove={respond}
        onResponderRelease={release}
        onResponderTerminate={release}
        style={{ width: TRACK_WIDTH, height: TRACK_HEIGHT }}
      >
        <View
          style={{
            position: 'absolute',
            left: (TRACK_WIDTH - 3) / 2,
            width: 3,
            height: '100%',
            borderRadius: 2,
            backgroundColor: 'rgba(255,255,255,0.06)',
          }}
        />
        <View
          style={{
            position: 'absolute',
            left: (TRACK_WIDTH - 8) / 2,
            top: centerY,
            width: 8,
            height: 1,
            backgroundColor: 'rgba(255,255,255,0.1)',
          }}
        />
        <View
          style={{
            position: 'absolute',
            left: (TRACK_WIDTH - 3) / 2,
            width: 3,
            borderRadius: 2,
            top: fillTop,
            height: fillHeight,
            backgroundColor: color,
            opacity: shownGain === 0 ? 0 : 1,
          }}
        />
        <View
          style={{
            position: 'absolute',
            left: (TRACK_WIDTH - 16) / 2,
            top: thumbY - 8,
            width: 16,
            height: 16,
            borderRadius: 8,
            backgroundColor: color,
            boxShadow: shownGain !== 0 ? `0 0 12px ${color}` : undefined,
          }}
        />
      </View>

      <ScText style={{ fontSize: 9, color: 'rgba(255,255,255,0.3)', fontWeight: '500' }}>{label}</ScText>
    </View>
  );
});
