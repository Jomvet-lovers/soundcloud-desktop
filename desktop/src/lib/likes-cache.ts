//! Bulk like caching: shared launch of the Rust `track_cache_likes` loop and
//! subscription to its progress. Used from Settings (CacheCard) and OfflinePage.

import { listen } from '@tauri-apps/api/event';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useSettingsStore } from '../stores/settings';
import {
  cacheLikedTracks,
  cancelCacheLikes,
  isCacheLikesRunning,
  type LikeCacheEntry,
} from './cache';

export interface CacheLikesProgress {
  phase: 'start' | 'progress' | 'done' | 'cancelled';
  total: number;
  done: number;
  failed: number;
  skipped: number;
  urn?: string | null;
}

/** Collects entries for all likes and starts the background loop in Rust.
 *  Returns the number of queued tracks (0 = no likes). */
export async function startCacheLikes(): Promise<number> {
  const [
    { fetchAllLikedTracks },
    { buildStorageUrls, downloadFallbackUrls, streamFallbackUrls, getSessionId },
  ] = await Promise.all([import('./hooks'), import('./api')]);
  const hq = useSettingsStore.getState().highQualityStreaming;
  const sessionId = getSessionId();
  const tracks = await fetchAllLikedTracks(200);
  const entries: LikeCacheEntry[] = tracks.map((track) => ({
    urn: track.urn,
    urls: streamFallbackUrls(track.urn, hq),
    downloadUrls: downloadFallbackUrls(track.urn, hq),
    storageUrls: buildStorageUrls(track.urn),
    sessionId,
    hq,
    durationMs: track.duration,
  }));
  if (entries.length > 0) {
    await cacheLikedTracks(entries);
  }
  return entries.length;
}

/** Bulk-caching state: picks up an already running loop and listens for progress.
 *  `onFinish` is called on done/cancelled (e.g. re-read the inventory). */
export function useCacheLikes(onFinish?: (progress: CacheLikesProgress) => void) {
  const [caching, setCaching] = useState(false);
  const [progress, setProgress] = useState<CacheLikesProgress | null>(null);
  const finishRef = useRef(onFinish);
  finishRef.current = onFinish;

  useEffect(() => {
    void isCacheLikesRunning().then((running) => {
      if (running) setCaching(true);
    });
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listen<CacheLikesProgress>('track:cache-likes-progress', (event) => {
      const p = event.payload;
      if (p.phase === 'start') {
        setCaching(true);
        setProgress({ ...p, done: 0, failed: 0, skipped: 0 });
      } else if (p.phase === 'progress') {
        setProgress(p);
      } else {
        setCaching(false);
        setProgress(null);
        finishRef.current?.(p);
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const start = useCallback(async (): Promise<number> => {
    setCaching(true);
    try {
      const queued = await startCacheLikes();
      if (queued === 0) setCaching(false);
      return queued;
    } catch (err) {
      setCaching(false);
      setProgress(null);
      throw err;
    }
  }, []);

  const cancel = useCallback(() => {
    void cancelCacheLikes();
  }, []);

  return { caching, progress, start, cancel };
}
