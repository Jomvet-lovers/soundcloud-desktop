import { View } from 'react-native';
import { type Track } from '@sc/data';
import { genreColor, ScText, Shelf, TrackCard } from '@sc/ui';
import { useCardProps } from '../shared/cards';
import type { PlayerState } from '../../player/PlayerContext';
import { ScheduleRow } from './WaveSchedule';

export { useCardProps };

const BROOK_CAP = 6;
const SHELF_CAP = 18;

function relDate(track: Track): string {
  if (!track.created_at) return '—';
  const ts = Date.parse(track.created_at);
  if (!Number.isFinite(ts)) return '—';
  const days = Math.floor((Date.now() - ts) / 86_400_000);
  if (days <= 0) return 'сегодня';
  if (days === 1) return 'вчера';
  return new Date(ts).toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
}

/** «Свежее» — верховья: узкий датированный ручей-список. */
export function ReleaseBrook({ tracks, player }: { tracks: Track[]; player: PlayerState }) {
  const items = tracks.slice(0, BROOK_CAP);
  return (
    <View style={{ gap: 4 }}>
      {items.map((t, i) => (
        <ScheduleRow key={t.id} track={t} index={i} queue={items} player={player} leading={relDate(t)} />
      ))}
    </View>
  );
}

/** «Тот же вайб» — полка с жанровой тонировкой каждой карточки. */
export function VibeShelf({ tracks, player }: { tracks: Track[]; player: PlayerState }) {
  const items = tracks.slice(0, SHELF_CAP);
  const card = useCardProps(player);
  return (
    <Shelf gap={12}>
      {items.map((t) => {
        const tone = genreColor(t.genre);
        return (
          <View key={t.id} style={{ width: 176 }}>
            <View style={{ borderRadius: 16, padding: 6, backgroundColor: hexTint(tone) }}>
              <TrackCard {...card(t, items)} />
            </View>
            {t.genre && (
              <View style={{ flexDirection: 'row', alignItems: 'center', gap: 6, marginTop: 4, paddingLeft: 8 }}>
                <View style={{ width: 6, height: 6, borderRadius: 3, backgroundColor: tone }} />
                <ScText numberOfLines={1} style={{ fontSize: 10.5, color: 'rgba(255,255,255,0.35)' }}>
                  {t.genre}
                </ScText>
              </View>
            )}
          </View>
        );
      })}
    </Shelf>
  );
}

/** «Глубже» — слабый сигнал: полка приглушена. */
export function DeepShelf({ tracks, player }: { tracks: Track[]; player: PlayerState }) {
  const items = tracks.slice(0, SHELF_CAP);
  const card = useCardProps(player);
  return (
    <View style={{ opacity: 0.78 }}>
      <Shelf gap={12}>
        {items.map((t) => (
          <View key={t.id} style={{ width: 168 }}>
            <TrackCard {...card(t, items)} />
          </View>
        ))}
      </Shelf>
    </View>
  );
}

/** Полупрозрачная жанровая подложка карточки (аналог linear-gradient 0.13). */
function hexTint(color: string): string {
  if (color.startsWith('#') && color.length === 7) {
    const r = parseInt(color.slice(1, 3), 16);
    const g = parseInt(color.slice(3, 5), 16);
    const b = parseInt(color.slice(5, 7), 16);
    return `rgba(${r}, ${g}, ${b}, 0.1)`;
  }
  return 'rgba(255,255,255,0.03)';
}
