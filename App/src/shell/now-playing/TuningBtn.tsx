import { createElement, useState } from 'react';
import { Platform, Pressable, View } from 'react-native';
import { ScText, TuneIcon, useScTheme } from '@sc/ui';
import {
  effectivePitch,
  PITCH_SEMITONES_MAX,
  PITCH_SEMITONES_MIN,
  PLAYBACK_RATE_MAX,
  PLAYBACK_RATE_MIN,
  usePlayerState,
} from '../../player/PlayerContext';
import { IconButton } from './IconButton';
import { Slider } from './Slider';

const fmtRate = (r: number) => `${r.toFixed(2).replace(/\.00$/, '').replace(/(\.\d)0$/, '$1')}x`;
const fmtPitch = (s: number) => (Math.abs(s) < 0.001 ? '0' : `${s > 0 ? '+' : ''}${s.toFixed(1).replace(/\.0$/, '')}`);

/** Настройка звука (донор TuningBtn-поповер): скорость + тональность.
 *  Скорость → ядро (set_speed). Pitch: auto = производная от скорости (натуральный
 *  rodio-питч), manual = независимый сдвиг (ждёт DSP в ядре — пока UI-стейт). */
export function TuningBtn() {
  const { accent } = useScTheme();
  const player = usePlayerState();
  const { playbackRate, pitchSemitones, pitchMode } = player;
  const [open, setOpen] = useState(false);
  const isManual = pitchMode === 'manual';
  const effective = effectivePitch(playbackRate, pitchMode, pitchSemitones);
  const rateActive = Math.abs(playbackRate - 1) >= 0.001;
  const active = rateActive || (isManual && Math.abs(pitchSemitones) >= 0.001);

  return (
    <View style={{ position: 'relative' }}>
      <IconButton size={30} onPress={() => setOpen((o) => !o)}>
        <TuneIcon size={16} color={active ? accent.base : 'rgba(255,255,255,0.55)'} />
      </IconButton>

      {open && Platform.OS === 'web' &&
        createElement('div', { onClick: () => setOpen(false), style: { position: 'fixed', inset: 0, zIndex: 55 } })}

      {open && (
        <View
          style={{
            position: 'absolute', bottom: 40, right: 0, width: 300, padding: 12, borderRadius: 18,
            backgroundColor: 'rgba(16,16,18,0.97)', borderWidth: 1, borderColor: 'rgba(255,255,255,0.1)',
            zIndex: 60, gap: 10,
          }}
        >
          {/* режим тональности */}
          <View style={{ flexDirection: 'row', gap: 4, padding: 3, borderRadius: 14, borderWidth: 1, borderColor: 'rgba(255,255,255,0.07)', backgroundColor: 'rgba(255,255,255,0.03)' }}>
            <ModeTab label="АВТО" on={!isManual} onPress={() => player.setPitchMode('auto')} />
            <ModeTab label="РУЧНОЙ" on={isManual} onPress={() => player.setPitchMode('manual')} />
          </View>

          <TuneRow
            title="Скорость"
            value={fmtRate(playbackRate)}
            active={rateActive}
            accent={accent.base}
            glow={accent.glow}
            frac={(playbackRate - PLAYBACK_RATE_MIN) / (PLAYBACK_RATE_MAX - PLAYBACK_RATE_MIN)}
            tickFrac={(1 - PLAYBACK_RATE_MIN) / (PLAYBACK_RATE_MAX - PLAYBACK_RATE_MIN)}
            onSeek={(f) => player.setPlaybackRate(PLAYBACK_RATE_MIN + f * (PLAYBACK_RATE_MAX - PLAYBACK_RATE_MIN))}
            onReset={rateActive ? player.resetPlaybackRate : undefined}
          />

          <TuneRow
            title="Тональность"
            value={fmtPitch(effective)}
            active={isManual && Math.abs(pitchSemitones) >= 0.001}
            accent={accent.base}
            glow={accent.glow}
            frac={(effective - PITCH_SEMITONES_MIN) / (PITCH_SEMITONES_MAX - PITCH_SEMITONES_MIN)}
            tickFrac={0.5}
            dim={!isManual}
            // Крутить питч можно всегда — в auto тап сам переводит в manual (иначе «не тыкается»).
            onSeek={(f) => {
              if (!isManual) player.setPitchMode('manual');
              player.setPitchSemitones(PITCH_SEMITONES_MIN + f * (PITCH_SEMITONES_MAX - PITCH_SEMITONES_MIN));
            }}
            onReset={isManual && Math.abs(pitchSemitones) >= 0.001 ? player.resetPitchSemitones : undefined}
          />
        </View>
      )}
    </View>
  );
}

function ModeTab({ label, on, onPress }: { label: string; on: boolean; onPress: () => void }) {
  return (
    <Pressable onPress={onPress} style={{ flex: 1, height: 28, borderRadius: 10, alignItems: 'center', justifyContent: 'center', backgroundColor: on ? '#ffffff' : 'transparent' }}>
      <ScText style={{ fontSize: 10, fontWeight: '700', letterSpacing: 0.8, color: on ? '#000' : 'rgba(255,255,255,0.45)' }}>{label}</ScText>
    </Pressable>
  );
}

function TuneRow({
  title, value, active, accent, glow, frac, tickFrac, dim, onSeek, onReset,
}: {
  title: string; value: string; active: boolean; accent: string; glow: string;
  frac: number; tickFrac: number; dim?: boolean;
  onSeek?: (f: number) => void; onReset?: () => void;
}) {
  return (
    <View style={{ borderRadius: 14, borderWidth: 1, borderColor: 'rgba(255,255,255,0.06)', backgroundColor: 'rgba(255,255,255,0.02)', paddingHorizontal: 12, paddingVertical: 10, opacity: dim ? 0.65 : 1 }}>
      <View style={{ flexDirection: 'row', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6 }}>
        <ScText style={{ fontSize: 10, fontWeight: '700', letterSpacing: 0.8, textTransform: 'uppercase', color: 'rgba(255,255,255,0.45)' }}>{title}</ScText>
        <Pressable onPress={onReset} disabled={!onReset}>
          <ScText style={{ fontSize: 11, fontWeight: '700', color: active ? accent : 'rgba(255,255,255,0.45)' }}>{value}</ScText>
        </Pressable>
      </View>
      <Slider value={frac} onSeek={onSeek ?? (() => {})} color={accent} glowColor={glow} height={3} hoverHeight={4} thumbSize={11} disabled={!onSeek} />
      <View style={{ height: 8, marginTop: 3 }}>
        <View style={{ position: 'absolute', top: 0, width: 1, height: 6, backgroundColor: 'rgba(255,255,255,0.15)', left: `${tickFrac * 100}%` }} />
      </View>
    </View>
  );
}
