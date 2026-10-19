import { BaseDirectory, exists, mkdir, readTextFile, writeTextFile } from '@tauri-apps/plugin-fs';
import type { Track } from '../stores/player';

const BASE_DIR = BaseDirectory.AppData;
const UNSCOPED_INDEX_PATH = 'offline-index.json';

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

function cloneTrack(track: Track): Track {
  return {
    ...track,
    user: { ...track.user },
  };
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
        if (loadGeneration === generation) indexCache = parsed;
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
    index.tracksByUrn[track.urn] = cloneTrack(track);
    changed = true;
  }

  if (changed) {
    schedulePersist();
  }
}

export async function rememberLikedTracks(tracks: Track[]) {
  const currentGeneration = generation;
  const index = await loadIndex();
  if (currentGeneration !== generation) return;
  for (const track of tracks) {
    if (!track?.urn) continue;
    index.tracksByUrn[track.urn] = cloneTrack(track);
  }

  index.likedUrns = tracks.map((track) => track.urn);
  index.updatedAt = Date.now();
  schedulePersist();
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
