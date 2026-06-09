import type {CacheInventoryEntry} from '../../lib/cache';
import type {Track} from '../../stores/player';

export type OfflineSection = 'likes' | 'cached';

export type SortMode = 'custom' | 'recent' | 'title' | 'artist' | 'duration' | 'size';

/** Строка офлайн-библиотеки: метаданные трека + факты о файле на диске.
 *  `inv === null` — лайк без файла; `stub` — файл без записи в офлайн-индексе. */
export interface OfflineEntry {
  urn: string;
  track: Track;
  inv: CacheInventoryEntry | null;
  stub?: boolean;
}
