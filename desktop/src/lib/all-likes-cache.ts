import type { Track } from '../stores/player';

/**
 * The in-flight "every liked track" fetch, shared across callers.
 *
 * Lives in its own module so `lib/auth-session` can drop the snapshot on an
 * account switch without importing the hooks barrel (which reaches the auth
 * store and would close a dependency cycle).
 */
let allLikesPromise: Promise<Track[]> | null = null;

export function getAllLikesPromise(): Promise<Track[]> | null {
  return allLikesPromise;
}

export function setAllLikesPromise(promise: Promise<Track[]> | null): void {
  allLikesPromise = promise;
}

/** Drop the shared snapshot: it belongs to whichever account fetched it. */
export function invalidateAllLikesCache(): void {
  allLikesPromise = null;
}
