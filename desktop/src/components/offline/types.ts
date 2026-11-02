import type { CacheInventoryEntry } from '../../lib/cache';
import type { Track } from '../../stores/player';

export type OfflineSection = 'likes' | 'cached';

export type SortMode = 'custom' | 'recent' | 'title' | 'artist' | 'duration' | 'size';

/** Offline-library row: track metadata + facts about the file on disk.
 *  `inv === null` — a like without a file; `stub` — a file with no offline-index entry. */
export interface OfflineEntry {
  urn: string;
  track: Track;
  inv: CacheInventoryEntry | null;
  stub?: boolean;
}
