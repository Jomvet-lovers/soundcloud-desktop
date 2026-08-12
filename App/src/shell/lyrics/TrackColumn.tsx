import { useMemo, useState } from 'react';
import type { LayoutChangeEvent } from 'react-native';
import { Animated, Pressable, ScrollView, View } from 'react-native';
import type { Track } from '@sc/data';
import { Cover, formatDuration, ScText, useScTheme, VolumeIcon } from '@sc/ui';
import { positionValue, useWholeSeconds } from '../../player/position';
import { PLAYBACK_RATE_MAX, PLAYBACK_RATE_MIN, usePlayerState, VOLUME_MAX } from '../../player/PlayerContext';
import { Slider } from '../now-playing/Slider';
import { LyricsControls } from './LyricsControls';

const PAD_H = 48;
const CONTENT_MAX = 360;
const fmtRate = (r: number) => `${r.toFixed(2).replace(/\.00$/, '').replace(/(\.\d)0$/, '$1')}x`;

/** Тикающее «текущее / общее» под прогрессом (донор `ProgressTime`) — единственный
 *  1Гц-компонент колонки, тик не тянет ре-рендер остального. */
function ProgressTime({ durationSecs }: { durationSecs: number }) {
  const secs = useWholeSeconds();
  return (
    <ScText style={{ fontSize: 12, color: 'rgba(255,255,255,0.4)', fontVariant: ['tabular-nums'] }}>
      {formatDuration(secs * 1000)} / {formatDuration(durationSecs * 1000)}
    </ScText>
  );
}

/** Левая колонка оверлея лирики (донор `TrackColumn`, Image #4) — now-playing карточка:
 *  обложка, тайтл/артист, прогресс+seek с центр-таймкодом, ряд управления, и карточка
 *  громкость+скорость. Контент капнут по 360px и центрирован, как в доноре. */
export function TrackColumn({ track, maxWidth = CONTENT_MAX }: { track: Track; maxWidth?: number }) {
  const { accent } = useScTheme();
  const player = usePlayerState();
  const [colW, setColW] = useState(maxWidth + PAD_H * 2);
  const onLayout = (e: LayoutChangeEvent) => setColW(e.nativeEvent.layout.width);
  const width = Math.min(maxWidth, Math.max(180, colW - PAD_H * 2));

  const durationSecs = track.duration_ms / 1000;
  const idle = 'rgba(255,255,255,0.55)';
  const boosted = player.volume > 100;
  const rateActive = Math.abs(player.playbackRate - 1) >= 0.001;
  const rateFrac = (player.playbackRate - PLAYBACK_RATE_MIN) / (PLAYBACK_RATE_MAX - PLAYBACK_RATE_MIN);

  const animatedPct = useMemo(
    () => positionValue.interpolate({ inputRange: [0, Math.max(1, durationSecs)], outputRange: ['0%', '100%'], extrapolate: 'clamp' }) as unknown as Animated.AnimatedInterpolation<string>,
    [durationSecs],
  );

  return (
    <ScrollView
      showsVerticalScrollIndicator={false}
      onLayout={onLayout}
      contentContainerStyle={{ flexGrow: 1, alignItems: 'center', justifyContent: 'center', gap: 16, paddingHorizontal: PAD_H, paddingVertical: 24 }}
    >
      <Cover url={track.artwork_url} size={width} radius={16} artSize="t500x500" />

      <View style={{ width, alignItems: 'center', gap: 4 }}>
        <ScText numberOfLines={1} style={{ fontSize: 18, fontWeight: '700', color: 'rgba(255,255,255,0.95)', textAlign: 'center' }}>
          {track.title}
        </ScText>
        <ScText numberOfLines={1} style={{ fontSize: 14, color: 'rgba(255,255,255,0.4)', textAlign: 'center' }}>
          {track.artist.name}
        </ScText>
      </View>

      <View style={{ width }}>
        <Slider
          animatedPct={animatedPct}
          onSeek={(f) => player.seek(f * durationSecs)}
          color={accent.base}
          glowColor={accent.glow}
          height={4}
          hoverHeight={6}
          thumbSize={12}
        />
        <View style={{ alignItems: 'center', marginTop: 4 }}>
          <ProgressTime durationSecs={durationSecs} />
        </View>
      </View>

      <LyricsControls track={track} player={player} />

      <View style={{ width, gap: 8, borderRadius: 22, borderWidth: 1, borderColor: 'rgba(255,255,255,0.07)', backgroundColor: 'rgba(0,0,0,0.3)', padding: 12 }}>
        {/* Громкость */}
        <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8 }}>
          <Pressable onPress={player.toggleMute} style={{ width: 36, height: 36, borderRadius: 18, alignItems: 'center', justifyContent: 'center' }}>
            <VolumeIcon size={16} color={player.volume === 0 ? accent.base : idle} muted={player.volume === 0} low={player.volume < 50} />
          </Pressable>
          <View style={{ flex: 1 }}>
            <Slider
              value={player.volume / VOLUME_MAX}
              onSeek={(f) => player.setVolume(f * VOLUME_MAX)}
              color={player.volume === 0 ? idle : boosted ? '#fbbf24' : 'rgba(255,255,255,0.6)'}
              glowColor={accent.glow}
              height={3}
              hoverHeight={4}
              thumbSize={10}
              tickFrac={0.5}
            />
          </View>
          <ScText style={{ fontSize: 10, width: 34, textAlign: 'right', fontVariant: ['tabular-nums'], color: boosted ? 'rgba(251,191,36,0.7)' : 'rgba(255,255,255,0.3)' }}>
            {player.volume}%
          </ScText>
        </View>

        {/* Скорость — вложенный блок (донор `PlaybackRateSlider`) */}
        <View style={{ borderRadius: 14, borderWidth: 1, borderColor: 'rgba(255,255,255,0.06)', backgroundColor: 'rgba(255,255,255,0.02)', paddingHorizontal: 12, paddingVertical: 10 }}>
          <View style={{ flexDirection: 'row', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
            <ScText style={{ fontSize: 10, fontWeight: '700', letterSpacing: 0.8, textTransform: 'uppercase', color: 'rgba(255,255,255,0.45)' }}>
              Скорость
            </ScText>
            <Pressable onPress={rateActive ? player.resetPlaybackRate : undefined} disabled={!rateActive}>
              <ScText style={{ fontSize: 11, minWidth: 42, textAlign: 'right', fontWeight: '700', fontVariant: ['tabular-nums'], color: rateActive ? accent.base : 'rgba(255,255,255,0.45)' }}>
                {fmtRate(player.playbackRate)}
              </ScText>
            </Pressable>
          </View>
          <Slider
            value={rateFrac}
            onSeek={(f) => player.setPlaybackRate(PLAYBACK_RATE_MIN + f * (PLAYBACK_RATE_MAX - PLAYBACK_RATE_MIN))}
            color={accent.base}
            glowColor={accent.glow}
            height={3}
            hoverHeight={4}
            thumbSize={10}
          />
          <View style={{ height: 8, marginTop: 4 }}>
            <View style={{ position: 'absolute', top: 0, width: 1, height: 6, backgroundColor: 'rgba(255,255,255,0.15)', left: `${((1 - PLAYBACK_RATE_MIN) / (PLAYBACK_RATE_MAX - PLAYBACK_RATE_MIN)) * 100}%` }} />
          </View>
        </View>
      </View>
    </ScrollView>
  );
}
