import type { HistoryEntry } from '../../lib/hooks';
import type { Track } from '../../stores/player';

/** History entries carry a bare numeric `scTrackId`; the player/stream/cache
 *  layer needs the full SC track URN (it derives the canonical
 *  `soundcloud_tracks_<id>.m4a` storage name from it). Passing the bare id
 *  produces a non-canonical `<id>.m4a` upload. */
export function historyTrackUrn(scTrackId: string): string {
  return scTrackId.startsWith('soundcloud:tracks:') ? scTrackId : `soundcloud:tracks:${scTrackId}`;
}

export function historyEntryToTrack(entry: HistoryEntry): Track {
  return {
    id: 0,
    urn: historyTrackUrn(entry.scTrackId),
    title: entry.title,
    duration: entry.duration,
    artwork_url: entry.artworkUrl,
    user: {
      id: 0,
      urn: entry.artistUrn || '',
      username: entry.artistName,
      avatar_url: '',
      permalink_url: '',
    },
  };
}
