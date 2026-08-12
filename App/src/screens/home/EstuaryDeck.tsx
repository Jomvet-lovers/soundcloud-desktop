import { useEffect, useMemo, useState } from 'react';
import { LayoutChangeEvent, Platform, Pressable, StyleSheet, View } from 'react-native';
import Svg, { Defs, LinearGradient, RadialGradient, Rect, Stop } from 'react-native-svg';
import { type Track, useSc } from '@sc/data';
import {
  Cover,
  EqBars,
  formatDuration,
  LiveWaveform,
  PlayIcon,
  RefreshIcon,
  ScText,
  TrackStatusBadges,
  useScTheme,
} from '@sc/ui';
import type { PlayerState } from '../../player/PlayerContext';
import { positionValue, useWholeSeconds } from '../../player/position';
import { HideLikedToggle, HideListenedToggle, LanguageFilter } from './controls';

interface EstuaryDeckProps {
  track: Track | null;
  queue: Track[];
  player: PlayerState;
  hideListened: boolean;
  onHideListened: (v: boolean) => void;
  hideLiked: boolean;
  onHideLiked: (v: boolean) => void;
  languages: string[];
  onLanguages: (langs: string[]) => void;
  spinning: boolean;
  onRefresh: () => void;
  onPlayWave: () => void;
  canPlay: boolean;
}

/** On-air дека «Течения»: LIVE-шапка, играющий трек, несущая частота (волна во
 *  всю ширину), пульт волны. Единственная тяжёлая blur-поверхность страницы. */
export function EstuaryDeck({
  track,
  queue,
  player,
  hideListened,
  onHideListened,
  hideLiked,
  onHideLiked,
  languages,
  onLanguages,
  spinning,
  onRefresh,
  onPlayWave,
  canPlay,
}: EstuaryDeckProps) {
  const sc = useSc();
  const { accent, perf } = useScTheme();
  const [peaks, setPeaks] = useState<number[]>([]);
  const [waveW, setWaveW] = useState(0);
  const [playHover, setPlayHover] = useState(false);

  const isCurrent = !!player.currentTrack && track?.id === player.currentTrack.id;

  useEffect(() => {
    setPeaks([]);
    if (track?.waveform_url) sc.tracks.waveform(track.waveform_url).then(setPeaks).catch(() => {});
  }, [sc, track?.waveform_url]);

  const onWaveLayout = (e: LayoutChangeEvent) => setWaveW(Math.round(e.nativeEvent.layout.width));
  const durSecs = track ? track.duration_ms / 1000 : 0;
  // Прогресс волны — Animated (0 ре-рендеров): клип едет за positionValue.
  const progressValue = useMemo(
    () =>
      isCurrent
        ? positionValue.interpolate({ inputRange: [0, Math.max(1, durSecs)], outputRange: [0, 1], extrapolate: 'clamp' })
        : undefined,
    [isCurrent, durSecs],
  );

  return (
    <View
      style={{
        position: 'relative',
        overflow: 'hidden',
        borderRadius: 20,
        borderWidth: 1,
        borderColor: 'rgba(255,255,255,0.1)',
        shadowColor: '#000',
        shadowOffset: { width: 0, height: 24 },
        shadowOpacity: 0.45,
        shadowRadius: 60,
        // @ts-expect-error web-only: живой blur фона (нативные бэкенды позже)
        backdropFilter: 'blur(24px) saturate(1.3)',
        WebkitBackdropFilter: 'blur(24px) saturate(1.3)',
      }}
    >
      <Svg style={StyleSheet.absoluteFill} width="100%" height="100%" preserveAspectRatio="none">
        <Defs>
          <LinearGradient id="deckBg" x1="0" y1="0" x2="1" y2="1">
            <Stop offset="0" stopColor="#ffffff" stopOpacity={0.07} />
            <Stop offset="0.55" stopColor="#ffffff" stopOpacity={0.025} />
            <Stop offset="1" stopColor="#ffffff" stopOpacity={0.025} />
          </LinearGradient>
          <RadialGradient id="deckWash" cx="12%" cy="0%" rx="60%" ry="90%">
            <Stop offset="0" stopColor={accent.base} stopOpacity={0.35} />
            <Stop offset="0.6" stopColor={accent.base} stopOpacity={0} />
          </RadialGradient>
        </Defs>
        <Rect x={0} y={0} width="100%" height="100%" fill="url(#deckBg)" />
        <Rect x={0} y={0} width="100%" height="100%" fill="url(#deckWash)" />
      </Svg>

      <View style={{ padding: 24, gap: 16 }}>
        <View style={{ flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', gap: 12 }}>
          <View style={{ flexDirection: 'row', alignItems: 'center', gap: 10 }}>
            <View style={{ width: 8, height: 8, borderRadius: 4, backgroundColor: accent.base }} />
            <ScText style={{ fontSize: 11, fontWeight: '700', letterSpacing: 11 * 0.22, textTransform: 'uppercase', color: 'rgba(255,255,255,0.9)' }}>
              ТЕЧЕНИЕ
            </ScText>
            {player.playing && <EqBars active count={3} height={10} barWidth={3} />}
            <ScText style={{ fontSize: 11.5, color: 'rgba(255,255,255,0.45)' }}>
              течение подстраивается под тебя
            </ScText>
          </View>
          <ScText token="counter" style={{ fontSize: 11, color: 'rgba(255,255,255,0.35)' }}>
            река доливает очередь — течение не останавливается
          </ScText>
        </View>

        {track ? (
          <TrackHeadRow track={track} queue={queue} player={player} isCurrent={isCurrent} />
        ) : (
          <View style={{ flexDirection: 'row', alignItems: 'center', gap: 12 }}>
            <View style={{ width: 56, height: 56, borderRadius: 12, backgroundColor: 'rgba(255,255,255,0.04)' }} />
            <View style={{ flex: 1 }}>
              <ScText style={{ fontSize: 15, fontWeight: '600', color: 'rgba(255,255,255,0.9)' }}>
                Жми play — поймай волну
              </ScText>
              <ScText style={{ marginTop: 2, fontSize: 12, color: 'rgba(255,255,255,0.45)' }}>
                Следующий трек — думаем, тебе зайдёт
              </ScText>
            </View>
          </View>
        )}

        <View
          onLayout={onWaveLayout}
          style={[
            { height: 56 },
            // Отражение волны как «вода» (донор WebkitBoxReflect) — только web+beauty.
            Platform.OS === 'web' && perf.mode === 'beauty'
              ? ({ WebkitBoxReflect: 'below 2px linear-gradient(transparent 62%, rgba(255,255,255,0.13))' } as object)
              : null,
          ]}
        >
          {peaks.length > 0 && waveW > 0 && (
            <LiveWaveform peaks={peaks} progressValue={progressValue} width={waveW} height={56}
              onSeek={(f) => track && player.seek(f * (track.duration_ms / 1000))} />
          )}
        </View>

        <View
          style={{
            flexDirection: 'row',
            alignItems: 'center',
            flexWrap: 'wrap',
            gap: 12,
            borderTopWidth: 1,
            borderTopColor: 'rgba(255,255,255,0.06)',
            paddingTop: 16,
          }}
        >
          <Pressable
            onPress={onPlayWave}
            disabled={!canPlay}
            onPointerEnter={() => setPlayHover(true)}
            onPointerLeave={() => setPlayHover(false)}
            style={({ pressed }) => ({
              flexDirection: 'row',
              alignItems: 'center',
              gap: 10,
              height: 40,
              paddingHorizontal: 20,
              borderRadius: 12,
              backgroundColor: accent.base,
              opacity: canPlay ? 1 : 0.4,
              shadowColor: accent.glow,
              shadowOffset: { width: 0, height: playHover ? 9 : 6 },
              shadowOpacity: 1,
              shadowRadius: playHover ? 30 : 24,
              transform: [{ translateY: playHover && !pressed ? -1.5 : 0 }, { scale: pressed ? 0.98 : 1 }],
            })}
          >
            <PlayIcon size={14} color={accent.contrast} />
            <ScText style={{ fontSize: 13.5, fontWeight: '600', color: accent.contrast }}>
              Плыть по течению
            </ScText>
          </Pressable>

          <View style={{ width: 1, height: 24, backgroundColor: 'rgba(255,255,255,0.07)' }} />

          <HideListenedToggle value={hideListened} onChange={onHideListened} />
          <HideLikedToggle value={hideLiked} onChange={onHideLiked} />
          <LanguageFilter selected={languages} onChange={onLanguages} />

          <Pressable
            onPress={onRefresh}
            disabled={spinning}
            style={{
              width: 32,
              height: 32,
              alignItems: 'center',
              justifyContent: 'center',
              borderRadius: 16,
              borderWidth: 1,
              borderColor: 'rgba(255,255,255,0.08)',
              backgroundColor: 'rgba(255,255,255,0.06)',
              opacity: spinning ? 0.4 : 1,
            }}
          >
            <RefreshIcon size={13} color="rgba(255,255,255,0.7)" />
          </Pressable>
        </View>
      </View>
    </View>
  );
}

/** Обложка + название/артист + таймкод над волной. */
function TrackHeadRow({ track, queue, player, isCurrent }: { track: Track; queue: Track[]; player: PlayerState; isCurrent: boolean }) {
  const { accent } = useScTheme();
  return (
    <View style={{ flexDirection: 'row', alignItems: 'center', gap: 12 }}>
      <Pressable onPress={() => player.toggle(track, queue)} style={{ width: 56, height: 56, borderRadius: 12, overflow: 'hidden' }}>
        <Cover url={track.artwork_url} size={56} radius={0} artSize="t200x200" />
      </Pressable>
      <View style={{ flex: 1, minWidth: 0 }}>
        <ScText numberOfLines={1} style={{ fontSize: 15, fontWeight: '600', color: isCurrent ? accent.base : 'rgba(255,255,255,0.92)' }}>
          {track.title}
        </ScText>
        <ScText numberOfLines={1} style={{ marginTop: 2, fontSize: 12, color: 'rgba(255,255,255,0.45)' }}>
          {track.artist.name}
        </ScText>
      </View>
      {track.badge && (
        <TrackStatusBadges storageState={track.badge.storage_state} indexState={track.badge.index_state} storageQuality={track.badge.storage_quality} />
      )}
      <DeckTime durationMs={track.duration_ms} isCurrent={isCurrent} accentColor={accent.base} />
    </View>
  );
}

/** Таймкод деки — изолирован: подписан на секунды (1Гц), не тянет ре-рендер деки. */
function DeckTime({ durationMs, isCurrent, accentColor }: { durationMs: number; isCurrent: boolean; accentColor: string }) {
  const secs = useWholeSeconds();
  return (
    <ScText token="counter" style={{ fontSize: 11, color: 'rgba(255,255,255,0.35)' }}>
      <ScText token="counter" style={{ fontSize: 11, color: accentColor }}>
        {isCurrent ? formatDuration(secs * 1000) : '0:00'}
      </ScText>
      {'  /  '}
      {formatDuration(durationMs)}
    </ScText>
  );
}
