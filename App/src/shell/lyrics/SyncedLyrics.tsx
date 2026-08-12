import { useMemo } from 'react';
import type { LyricLine } from '@sc/data';
import { SyncedLyrics as SyncedLyricsBrick } from '@sc/ui';
import { getPositionSecs } from '../../player/position';
import { usePlayerState } from '../../player/PlayerContext';

/** Адаптер кирпича `@sc/ui` `SyncedLyrics`: маппит доменные строки (`at_ms`→сек),
 *  инъектит императивный доступ к позиции (без ре-рендеров) + seek/playing из
 *  плеера. Сам караоке-движок (по-символьная волна, ♪♪♪) — в Core/ui, кроссплатформ. */
export function SyncedLyrics({ lines }: { lines: LyricLine[] }) {
  const { seek, playing } = usePlayerState();
  const synced = useMemo(
    () =>
      lines
        .filter((l) => l.at_ms != null)
        .map((l) => ({ time: (l.at_ms as number) / 1000, text: l.text })),
    [lines],
  );
  return (
    <SyncedLyricsBrick
      lines={synced}
      getPositionSecs={getPositionSecs}
      isPlaying={playing}
      onSeek={seek}
    />
  );
}
