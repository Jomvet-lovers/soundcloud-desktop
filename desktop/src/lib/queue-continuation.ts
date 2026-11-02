/**
 * The top-up «layer» for the queue of one specific playback context
 * (likes, playlist, following feed…). While the active source is not exhausted,
 * its pages land in the queue before the wave kicks in: the context plays
 * TO THE END. The source is done — it hands over to the autopilot (the wave), see
 * lib/queue-autopilot.ts.
 *
 * One source per playback session: armed on play() from a context,
 * reset when a new play() starts (the player store calls the reset handler) and
 * on exhaustion.
 */

import { shuffleArray, type Track, usePlayerStore } from '../stores/player';
import { api } from './api';
import { fetchAllLikedTracks, fetchAllPlaylistTracks, fetchLikedTracksPage } from './hooks';

export interface QueueContinuationSource {
  /** Name for logs */
  readonly kind: string;

  /**
   * The next batch of tracks. An empty array = source exhausted (the wave comes next).
   * The caller filters duplicates against the live queue — this is pure pagination.
   */
  next(): Promise<Track[]>;
}

let active: QueueContinuationSource | null = null;

export function setQueueContinuationSource(src: QueueContinuationSource | null): void {
  active = src;
}

export function getQueueContinuationSource(): QueueContinuationSource | null {
  return active;
}

interface PagedTracks {
  collection: Track[];
  has_more: boolean;
}

/**
 * A source over any page-based endpoint `{ collection, has_more }`.
 * Pulls one page per call; `has_more=false` or an empty page = exhausted.
 */
export function createPagedContinuationSource(
  kind: string,
  base: string,
  opts?: { pageSize?: number; startPage?: number },
): QueueContinuationSource {
  const pageSize = opts?.pageSize ?? 50;
  let page = Math.max(0, opts?.startPage ?? 0);
  let done = false;
  return {
    kind,
    async next() {
      if (done) return [];
      const sep = base.includes('?') ? '&' : '?';
      const data = await api<PagedTracks>(`${base}${sep}limit=${pageSize}&page=${page}`);
      page += 1;
      const collection = data.collection ?? [];
      if (!data.has_more || collection.length === 0) done = true;
      return collection;
    },
  };
}

const LIKES_PAGE_SIZE = 50;
const PLAYLIST_PAGE_SIZE = 200;

/**
 * Likes, start to finish. `alreadyLoaded` is how many likes are already in the queue
 * at start (the first page of the list): those are skipped, and the seam is closed by
 * the dedupe on the autopilot side.
 *
 * The cursor is pulled from the start of the chain: track_likes does not accept a numeric
 * offset (`invalid cursor format`), and the Nth page is only reachable by walking the
 * previous ones — so pages go in sequence and the extra ones are cut by the counter.
 */
export function createLikesContinuationSource(alreadyLoaded = 0): QueueContinuationSource {
  let cursor: string | null = null;
  let skip = Math.max(0, alreadyLoaded);
  let done = false;
  return {
    kind: 'likes',
    async next() {
      while (!done) {
        const data = await fetchLikedTracksPage(cursor);
        cursor = data.next_cursor ?? null;
        const collection = data.collection ?? [];
        if (!cursor || collection.length === 0) done = true;
        const rest = skip > 0 ? collection.slice(Math.min(skip, collection.length)) : collection;
        skip = Math.max(0, skip - collection.length);
        if (rest.length > 0) return rest;
      }
      return [];
    },
  };
}

/**
 * Shuffled lazy source for shuffle mode. On the first `next()` it pulls the WHOLE set
 * (`fetchAll`), cuts what is already queued (the initial shuffled page), shuffles
 * the rest and hands it out in chunks — so under shuffle the whole set plays in
 * random order without the queue ballooning at once. Exhausted — the autopilot
 * falls through to the wave.
 */
function createShuffledContinuationSource(
  kind: string,
  fetchAll: () => Promise<Track[]>,
): QueueContinuationSource {
  let buffer: Track[] | null = null;
  let pos = 0;
  return {
    kind,
    async next() {
      if (buffer === null) {
        const all = await fetchAll();
        const queued = new Set(usePlayerStore.getState().queue.map((t) => t.urn));
        buffer = all.filter((t) => !queued.has(t.urn));
        shuffleArray(buffer);
      }
      const chunk = buffer.slice(pos, pos + LIKES_PAGE_SIZE);
      pos += chunk.length;
      return chunk;
    },
  };
}

export function createShuffledLikesContinuationSource(): QueueContinuationSource {
  return createShuffledContinuationSource('likes-shuffled', () => fetchAllLikedTracks());
}

/**
 * Arm «likes to the end» under the current queue (the one play() already set up) —
 * one call for every likes entry point (LikesTab, the shelf on home, the preview
 * in library, masthead-shuffle). Called in `onPlay` right after play(): that call
 * reset the previous source, and we install our own.
 *
 * Under shuffle the source shuffles the WHOLE likes list (not just the loaded
 * page). Without shuffle — sequential pagination starting from the live
 * queue length.
 */
export function armLikesContinuation(): void {
  if (usePlayerStore.getState().shuffle) {
    setQueueContinuationSource(createShuffledLikesContinuationSource());
    return;
  }
  const loaded = usePlayerStore.getState().queue.length;
  setQueueContinuationSource(createLikesContinuationSource(loaded));
}

/**
 * Arm «playlist to the end» under the current queue — called in `onPlay` right
 * after play() from a playlist. The playlist is paginated (`usePlaylistTracks`), and
 * play() only queues the loaded slice; without a source the queue ended
 * there and the wave kicked in mid-playlist. Same trick as for likes.
 */
export function armPlaylistContinuation(playlistUrn: string): void {
  if (usePlayerStore.getState().shuffle) {
    setQueueContinuationSource(
      createShuffledContinuationSource('playlist-shuffled', () =>
        fetchAllPlaylistTracks(playlistUrn),
      ),
    );
    return;
  }
  const loaded = usePlayerStore.getState().queue.length;
  setQueueContinuationSource(
    createPagedContinuationSource(
      'playlist',
      `/playlists/${encodeURIComponent(playlistUrn)}/tracks`,
      {
        pageSize: PLAYLIST_PAGE_SIZE,
        startPage: Math.floor(Math.max(0, loaded) / PLAYLIST_PAGE_SIZE),
      },
    ),
  );
}
