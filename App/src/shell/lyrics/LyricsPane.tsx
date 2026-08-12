import { ActivityIndicator, View } from 'react-native';
import type { Lyrics } from '@sc/data';
import { MicVocalIcon, ScText } from '@sc/ui';
import type { LyricsTab } from '../../player/lyrics-ui';
import { CommentsPane } from './CommentsPane';
import { PlainLyrics } from './PlainLyrics';
import { SyncedLyrics } from './SyncedLyrics';
import type { CommentsLoad } from './useComments';

const SOURCE_LABELS: Record<string, string> = {
  lrclib: 'LRCLib',
  musixmatch: 'Musixmatch',
  genius: 'Genius',
  netease: 'NetEase',
  self_gen: 'AI',
};

function LyricsLoading() {
  return (
    <View style={{ flex: 1, alignItems: 'center', justifyContent: 'center', gap: 12 }}>
      <ActivityIndicator color="rgba(255,255,255,0.4)" />
      <ScText style={{ fontSize: 13, color: 'rgba(255,255,255,0.3)' }}>Загружаем лирику…</ScText>
    </View>
  );
}

function LyricsEmpty() {
  return (
    <View style={{ flex: 1, alignItems: 'center', justifyContent: 'center', gap: 14, paddingHorizontal: 48 }}>
      <MicVocalIcon size={40} color="rgba(255,255,255,0.08)" />
      <ScText style={{ fontSize: 15, fontWeight: '600', color: 'rgba(255,255,255,0.35)' }}>Лирика не найдена</ScText>
      <ScText style={{ fontSize: 12, color: 'rgba(255,255,255,0.2)', textAlign: 'center', lineHeight: 18 }}>
        Для этого трека нет ни синхронизированного, ни обычного текста
      </ScText>
    </View>
  );
}

function SourceBadge({ source }: { source: string | null }) {
  const label = source ? (SOURCE_LABELS[source] ?? source) : null;
  if (!label) return null;
  return (
    <View style={{ paddingHorizontal: 48, paddingTop: 14 }}>
      <ScText
        style={{
          alignSelf: 'flex-start',
          fontSize: 12,
          fontWeight: '700',
          letterSpacing: 0.3,
          color: 'rgba(255,255,255,0.32)',
          backgroundColor: 'rgba(255,255,255,0.05)',
          borderWidth: 1,
          borderColor: 'rgba(255,255,255,0.08)',
          paddingHorizontal: 11,
          paddingVertical: 4,
          borderRadius: 999,
          overflow: 'hidden',
        }}
      >
        {label}
      </ScText>
    </View>
  );
}

/** Правая колонка сплита (донор `LyricsPane`): состояния загрузки/пусто/синхро/plain,
 *  либо лента комментариев трека. Ручной поиск лирики (донор ManualSearchPanel) не
 *  портирован — в `@sc/data` есть только `tracks.lyrics(id)` по треку, ручного
 *  query-поиска в ядре нет. */
export function LyricsPane({
  tab,
  loading,
  data,
  comments,
}: {
  tab: LyricsTab;
  loading: boolean;
  data: Lyrics | null;
  comments: CommentsLoad;
}) {
  if (tab === 'comments') return <CommentsPane loading={comments.loading} items={comments.items} />;
  if (loading) return <LyricsLoading />;
  if (!data || data.lines.length === 0) return <LyricsEmpty />;

  return (
    <>
      <SourceBadge source={data.source} />
      {data.synced ? <SyncedLyrics lines={data.lines} /> : <PlainLyrics lines={data.lines} />}
    </>
  );
}
