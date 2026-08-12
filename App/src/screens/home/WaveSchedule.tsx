import { useState } from 'react';
import { Pressable, View } from 'react-native';
import type { Track } from '@sc/data';
import { Cover, EqBars, formatDuration, PauseIcon, PlayIcon, ScText, useScTheme } from '@sc/ui';
import type { PlayerState } from '../../player/PlayerContext';
import { LikeButton } from './controls';

const SCHEDULE_SIZE = 10;
const FRESH_WINDOW_MS = 14 * 24 * 3600 * 1000;

export function isFresh(track: Track): boolean {
  if (!track.created_at) return false;
  const ts = Date.parse(track.created_at);
  return Number.isFinite(ts) && Date.now() - ts < FRESH_WINDOW_MS;
}

export function ScheduleRow({
  track,
  index,
  queue,
  player,
  leading,
}: {
  track: Track;
  index: number;
  queue: Track[];
  player: PlayerState;
  /** текст вместо номера (напр. дата релиза в «верховьях») */
  leading?: string;
}) {
  const { accent } = useScTheme();
  const [hover, setHover] = useState(false);
  const playing = player.currentTrack?.id === track.id && player.playing;
  const isThis = player.currentTrack?.id === track.id;
  const fresh = !isThis && isFresh(track);

  return (
    <View
      onPointerEnter={() => setHover(true)}
      onPointerLeave={() => setHover(false)}
      style={{
        position: 'relative',
        flexDirection: 'row',
        alignItems: 'center',
        gap: 14,
        borderRadius: 12,
        borderWidth: 1,
        paddingVertical: 8,
        paddingLeft: 10,
        paddingRight: 12,
        borderColor: isThis ? accent.glow : hover ? 'rgba(255,255,255,0.06)' : 'transparent',
        backgroundColor: isThis ? accent.glow : hover ? 'rgba(255,255,255,0.035)' : 'transparent',
      }}
    >
      {isThis && (
        <View style={{ position: 'absolute', left: 0, top: 10, bottom: 10, width: 2.5, borderRadius: 2, backgroundColor: accent.base }} />
      )}
      <ScText
        token="counter"
        numberOfLines={1}
        style={{ width: leading ? 48 : 28, textAlign: 'right', fontSize: 11, color: isThis ? accent.hover : 'rgba(255,255,255,0.3)' }}
      >
        {leading ?? String(index + 1).padStart(2, '0')}
      </ScText>

      <Pressable
        onPress={() => player.toggle(track, queue)}
        style={({ pressed }) => ({ width: 44, height: 44, borderRadius: 8, overflow: 'hidden', borderWidth: 1, borderColor: 'rgba(255,255,255,0.08)', transform: [{ scale: pressed ? 0.94 : 1 }] })}
      >
        <Cover url={track.artwork_url} size={44} radius={0} artSize="t200x200" />
        {(hover || playing) && (
          <View style={{ position: 'absolute', top: 0, right: 0, bottom: 0, left: 0, alignItems: 'center', justifyContent: 'center', backgroundColor: 'rgba(0,0,0,0.45)' }}>
            {playing ? <PauseIcon size={12} color="#fff" /> : <PlayIcon size={12} color="#fff" />}
          </View>
        )}
      </Pressable>

      <View style={{ flex: 1, minWidth: 0 }}>
        <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8 }}>
          <ScText numberOfLines={1} style={{ flexShrink: 1, fontSize: 13.5, fontWeight: '500', lineHeight: 16, color: 'rgba(255,255,255,0.88)' }}>
            {track.title}
          </ScText>
          {playing && <EqBars active count={3} height={9} barWidth={2} />}
          {fresh && (
            <View style={{ borderRadius: 5, borderWidth: 1, borderColor: 'rgba(52,211,153,0.3)', backgroundColor: 'rgba(52,211,153,0.1)', paddingHorizontal: 6, paddingVertical: 1 }}>
              <ScText style={{ fontSize: 9, fontWeight: '700', letterSpacing: 0.9, color: 'rgba(167,243,208,0.9)' }}>NEW</ScText>
            </View>
          )}
        </View>
        <ScText numberOfLines={1} style={{ marginTop: 1, fontSize: 12, lineHeight: 14, color: 'rgba(255,255,255,0.4)' }}>
          {track.artist.name}
        </ScText>
      </View>

      {hover && <LikeButton track={track} />}

      <ScText token="counter" style={{ width: 36, textAlign: 'right', fontSize: 11.5, color: 'rgba(255,255,255,0.3)' }}>
        {formatDuration(track.duration_ms)}
      </ScText>
    </View>
  );
}

/** «Волна» — программа эфира: плотный нумерованный список в 2 колонки. */
export function WaveSchedule({ tracks, player }: { tracks: Track[]; player: PlayerState }) {
  const items = tracks.slice(0, SCHEDULE_SIZE);
  const left = items.filter((_, i) => i % 2 === 0);
  const right = items.filter((_, i) => i % 2 === 1);
  return (
    <View style={{ flexDirection: 'row', gap: 28 }}>
      <View style={{ flex: 1, minWidth: 0, gap: 6 }}>
        {left.map((t, i) => (
          <ScheduleRow key={t.id} track={t} index={i * 2} queue={tracks} player={player} />
        ))}
      </View>
      <View style={{ flex: 1, minWidth: 0, gap: 6 }}>
        {right.map((t, i) => (
          <ScheduleRow key={t.id} track={t} index={i * 2 + 1} queue={tracks} player={player} />
        ))}
      </View>
    </View>
  );
}
