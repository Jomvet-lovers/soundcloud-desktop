import { useEffect, useRef } from 'react';
import { Animated, Pressable, StyleSheet, View } from 'react-native';
import { AudioLinesIcon, CloseIcon, GlassSurface, modalGlass, PowerIcon, RotateCcwIcon, ScText } from '@sc/ui';
import { useEq } from '../player/EqContext';
import { EQ_BAND_COUNT, EQ_LABELS, EQ_PRESETS } from '../player/eq-presets';
import { BandSlider } from './equalizer/BandSlider';
import { PresetChip } from './equalizer/PresetChip';

const CARD_WIDTH = 520;
const SCALE_HEIGHT = 140;

function EqHeader({
  enabled,
  onToggle,
  onReset,
  onClose,
}: {
  enabled: boolean;
  onToggle: () => void;
  onReset: () => void;
  onClose: () => void;
}) {
  return (
    <View style={{ flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', paddingHorizontal: 24, paddingTop: 20, paddingBottom: 12 }}>
      <View style={{ flexDirection: 'row', alignItems: 'center', gap: 12 }}>
        <View style={{ width: 36, height: 36, borderRadius: 12, backgroundColor: 'rgba(255,255,255,0.06)', alignItems: 'center', justifyContent: 'center' }}>
          <AudioLinesIcon size={18} color="rgba(255,255,255,0.6)" />
        </View>
        <ScText style={{ fontSize: 17, fontWeight: '700', color: 'rgba(255,255,255,0.9)' }}>Эквалайзер</ScText>
      </View>

      <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8 }}>
        <Pressable
          onPress={onToggle}
          style={{
            width: 36,
            height: 36,
            borderRadius: 12,
            borderWidth: 1,
            alignItems: 'center',
            justifyContent: 'center',
            backgroundColor: enabled ? 'rgba(52,211,153,0.15)' : 'rgba(255,255,255,0.04)',
            borderColor: enabled ? 'rgba(52,211,153,0.2)' : 'rgba(255,255,255,0.06)',
            boxShadow: enabled ? '0 0 12px rgba(52,211,153,0.15)' : undefined,
          }}
        >
          <PowerIcon size={15} color={enabled ? 'rgb(52,211,153)' : 'rgba(255,255,255,0.25)'} />
        </Pressable>

        <Pressable
          onPress={onReset}
          style={{
            width: 36,
            height: 36,
            borderRadius: 12,
            borderWidth: 1,
            borderColor: 'rgba(255,255,255,0.06)',
            backgroundColor: 'rgba(255,255,255,0.04)',
            alignItems: 'center',
            justifyContent: 'center',
          }}
        >
          <RotateCcwIcon size={14} color="rgba(255,255,255,0.25)" />
        </Pressable>

        <Pressable
          onPress={onClose}
          style={{
            width: 36,
            height: 36,
            borderRadius: 12,
            borderWidth: 1,
            borderColor: 'rgba(255,255,255,0.06)',
            backgroundColor: 'rgba(255,255,255,0.04)',
            alignItems: 'center',
            justifyContent: 'center',
          }}
        >
          <CloseIcon size={15} color="rgba(255,255,255,0.25)" />
        </Pressable>
      </View>
    </View>
  );
}

function EqBandsRow({ enabled, gains, onDrag }: { enabled: boolean; gains: number[]; onDrag: (index: number, gain: number) => void }) {
  return (
    <View style={{ paddingHorizontal: 24, paddingBottom: 16, opacity: enabled ? 1 : 0.3 }} pointerEvents={enabled ? 'auto' : 'none'}>
      <View style={{ flexDirection: 'row', alignItems: 'flex-end' }}>
        <View style={{ height: SCALE_HEIGHT, justifyContent: 'space-between', marginRight: 10, marginTop: -22 }}>
          <ScText style={{ fontSize: 9, color: 'rgba(255,255,255,0.2)' }}>+12</ScText>
          <ScText style={{ fontSize: 9, color: 'rgba(255,255,255,0.2)' }}>0</ScText>
          <ScText style={{ fontSize: 9, color: 'rgba(255,255,255,0.2)' }}>-12</ScText>
        </View>

        <View style={{ flex: 1, flexDirection: 'row', justifyContent: 'space-between' }}>
          {Array.from({ length: EQ_BAND_COUNT }, (_, i) => (
            <BandSlider key={i} gain={gains[i] ?? 0} label={EQ_LABELS[i]} onDrag={(g) => onDrag(i, g)} />
          ))}
        </View>
      </View>
    </View>
  );
}

function EqPresets({ enabled, preset, onPick }: { enabled: boolean; preset: string; onPick: (id: string) => void }) {
  return (
    <View style={{ paddingHorizontal: 24, paddingBottom: 20, opacity: enabled ? 1 : 0.3 }} pointerEvents={enabled ? 'auto' : 'none'}>
      <ScText style={{ fontSize: 11, color: 'rgba(255,255,255,0.3)', fontWeight: '500', marginBottom: 10 }}>Пресеты</ScText>
      <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: 6 }}>
        {Object.entries(EQ_PRESETS).map(([id, p]) => (
          <PresetChip key={id} label={p.labelRu} active={preset === id} onPress={() => onPick(id)} />
        ))}
        {preset === 'custom' && <PresetChip label="Пользовательский" active onPress={() => {}} />}
      </View>
    </View>
  );
}

/** Центрированная модалка эквалайзера (донор `Desktop/desktop/.../music/EqualizerPanel.tsx`).
 *  Состояние живёт в общем `EqContext` — он сам пушит `set_eq` в ядро на изменение,
 *  здесь только рендер и драг полос (см. `equalizer/BandSlider.tsx`). */
export function EqualizerPanel({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { enabled, gains, preset, setEnabled, setBand, applyPreset, reset } = useEq();
  const anim = useRef(new Animated.Value(0)).current;

  useEffect(() => {
    if (!open) return;
    anim.setValue(0);
    Animated.timing(anim, { toValue: 1, duration: 200, useNativeDriver: true }).start();
  }, [open, anim]);

  if (!open) return null;

  const translateY = anim.interpolate({ inputRange: [0, 1], outputRange: [8, 0] });

  return (
    <View style={[StyleSheet.absoluteFill, { zIndex: 90 }]}>
      <Pressable onPress={onClose} style={[StyleSheet.absoluteFill, { backgroundColor: 'rgba(0,0,0,0.5)' }]} />

      <View pointerEvents="box-none" style={[StyleSheet.absoluteFill, { alignItems: 'center', justifyContent: 'center' }]}>
        <Animated.View style={{ width: CARD_WIDTH, maxWidth: '92%', opacity: anim, transform: [{ translateY }] }}>
          <GlassSurface recipe={modalGlass}>
            <EqHeader enabled={enabled} onToggle={() => setEnabled(!enabled)} onReset={reset} onClose={onClose} />
            <EqBandsRow enabled={enabled} gains={gains} onDrag={setBand} />
            <EqPresets enabled={enabled} preset={preset} onPick={applyPreset} />
          </GlassSurface>
        </Animated.View>
      </View>
    </View>
  );
}
