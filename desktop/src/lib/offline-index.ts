import { BaseDirectory, exists, mkdir, readTextFile, writeTextFile } from '@tauri-apps/plugin-fs';
import type { Track } from '../stores/player';

const BASE_DIR = BaseDirectory.AppData;
const UNSCOPED_INDEX_PATH = 'offline-index.json';

/**
 * What the offline lists actually read. Raw api-v2 payloads carry far more
 * (`media`/transcodings alone were 6.5 MB of a 22 MB file for 5.5k tracks), and
 * the index is serialized on every change.
 */
const TRACK_KEYS = new Set([
  'id',
  'urn',
  'title',
  'duration',
  'full_duration',
  'artwork_url',
  'permalink_url',
  'genre',
  'tag_list',
  'language',
  'release_year',
  'release_date',
  'created_at',
  'last_modified',
  'sharing',
  'comment_count',
  'playback_count',
  'likes_count',
  'favoritings_count',
  'reposts_count',
  'user_favorite',
  'access',
  'enrichment',
  '_scd_meta',
]);
const USER_KEYS = new Set(['id', 'urn', 'username', 'avatar_url', 'permalink_url', 'verified']);

interface OfflineIndex {
  likedUrns: string[];
  tracksByUrn: Record<string, Track>;
  updatedAt: number | null;
  /** User-arranged order of the cached list ("Свой порядок" sort mode). */
  cacheOrder: string[];
}

function emptyIndex(): OfflineIndex {
  return {
    likedUrns: [],
    tracksByUrn: {},
    updatedAt: null,
    cacheOrder: [],
  };
}

let accountId: number | null = null;
let indexCache: OfflineIndex | null = null;
let loadPromise: Promise<OfflineIndex> | null = null;
let persistTimer: ReturnType<typeof setTimeout> | null = null;
let dirReady: Promise<void> | null = null;
/** Bumped on account switch so an in-flight load cannot fill the new cache. */
let generation = 0;

function indexPath(id: number | null): string {
  return id === null ? UNSCOPED_INDEX_PATH : `offline-index.${id}.json`;
}

function ensureDir() {
  if (!dirReady) {
    dirReady = mkdir('', { baseDir: BASE_DIR, recursive: true }).catch(() => {});
  }
  return dirReady;
}

function pick(source: object, keys: Set<string>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  const record = source as Record<string, unknown>;
  for (const key of keys) {
    if (record[key] !== undefined) out[key] = record[key];
  }
  return out;
}

/** Drop the payload the offline lists never render, before it hits the disk. */
function compactTrack(track: Track): Track {
  const out = pick(track, TRACK_KEYS) as unknown as Track;
  out.user = (track.user ? pick(track.user, USER_KEYS) : {}) as Track['user'];
  return out;
}

function hasForeignKeys(value: object, allowed: Set<string>): boolean {
  return Object.keys(value).some((key) => !allowed.has(key));
}

/** A row written before compaction existed: it still carries the raw payload. */
function isFatTrack(track: Track): boolean {
  return (
    hasForeignKeys(track, TRACK_KEYS) || (!!track.user && hasForeignKeys(track.user, USER_KEYS))
  );
}

/** A stored row is worth rewriting when it changed, or still has the old fat shape. */
function needsRewrite(prev: Track | undefined, next: Track): boolean {
  if (!prev || isFatTrack(prev)) return true;
  return (
    prev.title !== next.title ||
    prev.duration !== next.duration ||
    prev.full_duration !== next.full_duration ||
    prev.artwork_url !== next.artwork_url ||
    prev.user_favorite !== next.user_favorite ||
    prev.sharing !== next.sharing ||
    prev.permalink_url !== next.permalink_url ||
    prev.user?.username !== next.user?.username ||
    prev.user?.avatar_url !== next.user?.avatar_url
  );
}

/** One-time migration of an index written before compaction existed. */
function compactIndex(index: OfflineIndex): boolean {
  let changed = false;
  for (const [urn, track] of Object.entries(index.tracksByUrn)) {
    if (!track || !isFatTrack(track)) continue;
    index.tracksByUrn[urn] = compactTrack(track);
    changed = true;
  }
  return changed;
}

async function readIndexFile(path: string): Promise<OfflineIndex> {
  await ensureDir();

  try {
    if (!(await exists(path, { baseDir: BASE_DIR }))) {
      return emptyIndex();
    }

    const raw = await readTextFile(path, { baseDir: BASE_DIR });
    const parsed = JSON.parse(raw) as OfflineIndex;
    return {
      likedUrns: Array.isArray(parsed.likedUrns) ? parsed.likedUrns : [],
      tracksByUrn: parsed.tracksByUrn ?? {},
      updatedAt: parsed.updatedAt ?? null,
      cacheOrder: Array.isArray(parsed.cacheOrder) ? parsed.cacheOrder : [],
    };
  } catch {
    return emptyIndex();
  }
}

async function loadIndex(): Promise<OfflineIndex> {
  if (indexCache) {
    return indexCache;
  }

  if (!loadPromise) {
    const loadGeneration = generation;
    const path = indexPath(accountId);
    loadPromise = readIndexFile(path)
      .then((parsed) => {
        // Older builds stored the raw api-v2 payload; shrink it once, in place.
        const migrated = compactIndex(parsed);
        if (loadGeneration === generation) {
          indexCache = parsed;
          if (migrated) schedulePersist();
        }
        return parsed;
      })
      .finally(() => {
        if (loadGeneration === generation) {
          loadPromise = null;
        }
      });
  }

  return loadPromise;
}

function writeIndex(path: string, index: OfflineIndex) {
  void ensureDir().then(() =>
    writeTextFile(path, JSON.stringify(index), { baseDir: BASE_DIR }).catch(() => {}),
  );
}

function schedulePersist() {
  if (persistTimer) {
    clearTimeout(persistTimer);
  }

  const path = indexPath(accountId);
  const scheduledGeneration = generation;
  persistTimer = setTimeout(() => {
    persistTimer = null;
    if (!indexCache || scheduledGeneration !== generation) return;

    writeIndex(path, indexCache);
  }, 120);
}

/**
 * Point the offline index at an account-scoped file (`offline-index.<id>.json`;
 * signed-out use keeps the unscoped name). Pending writes for the previous
 * account are flushed; the cache is dropped so the next read loads the new file.
 */
export async function setOfflineAccount(id: number | null): Promise<void> {
  if (id === accountId) return;

  if (persistTimer) {
    clearTimeout(persistTimer);
    persistTimer = null;
    if (indexCache) writeIndex(indexPath(accountId), indexCache);
  }

  accountId = id;
  generation += 1;
  indexCache = null;
  loadPromise = null;
}

export async function rememberTracks(tracks: Track[]) {
  if (tracks.length === 0) return;

  const currentGeneration = generation;
  const index = await loadIndex();
  if (currentGeneration !== generation) return;
  let changed = false;

  for (const track of tracks) {
    if (!track?.urn) continue;
    // Every mounted row and every play calls this. Writing on a no-op kept
    // re-serializing the whole index (22 MB before compaction).
    if (!needsRewrite(index.tracksByUrn[track.urn], track)) continue;
    index.tracksByUrn[track.urn] = compactTrack(track);
    changed = true;
  }

  if (changed) {
    schedulePersist();
  }
}

/**
 * Remember liked tracks.
 *
 * `replace` is for a full snapshot (`fetchAllLikedTracks`): that list becomes
 * the authoritative `likedUrns` and stamps `updatedAt`. A partial page only
 * adds what it saw, so merely opening Library cannot shrink the offline mirror
 * to the first page or make it look freshly synced.
 */
export async function rememberLikedTracks(tracks: Track[], opts?: { replace?: boolean }) {
  const currentGeneration = generation;
  const index = await loadIndex();
  if (currentGeneration !== generation) return;
  let rowsChanged = false;
  for (const track of tracks) {
    if (!track?.urn) continue;
    if (!needsRewrite(index.tracksByUrn[track.urn], track)) continue;
    index.tracksByUrn[track.urn] = compactTrack(track);
    rowsChanged = true;
  }

  if (opts?.replace) {
    index.likedUrns = tracks.map((track) => track.urn);
    index.updatedAt = Date.now();
    schedulePersist();
    return;
  }

  const known = new Set(index.likedUrns);
  const fresh = tracks
    .map((track) => track.urn)
    .filter((urn): urn is string => Boolean(urn) && !known.has(urn));
  if (fresh.length > 0) {
    // Pages arrive newest first, so the unseen ones go in front.
    index.likedUrns = [...fresh, ...index.likedUrns];
    schedulePersist();
  } else if (rowsChanged) {
    schedulePersist();
  }
}

export async function getOfflineLikedTracks() {
  const index = await loadIndex();
  return index.likedUrns
    .map((urn) => index.tracksByUrn[urn])
    .filter((track): track is Track => Boolean(track));
}

export async function getOfflineTracksByUrns(urns: string[]) {
  const index = await loadIndex();
  return urns
    .map((urn) => index.tracksByUrn[urn])
    .filter((track): track is Track => Boolean(track));
}

export async function getOfflineIndexUpdatedAt() {
  const index = await loadIndex();
  return index.updatedAt;
}

export async function getCacheOrder(): Promise<string[]> {
  const index = await loadIndex();
  return index.cacheOrder;
}

export async function saveCacheOrder(urns: string[]) {
  const currentGeneration = generation;
  const index = await loadIndex();
  if (currentGeneration !== generation) return;
  index.cacheOrder = urns;
  schedulePersist();
}
