import { Howl } from 'howler';
import type { Track } from '../stores/player';
import { usePlayerStore } from '../stores/player';
import { api, streamUrl } from './api';
import { fetchAndCacheTrack, getCacheFilePath, getCacheUrl, isCached } from './cache';
import { art } from './formatters';

/* ── Audio engine state ──────────────────────────────────────── */

let currentHowl: Howl | null = null;
let currentUrn: string | null = null;
let fallbackDuration = 0;
let progressTimerId: ReturnType<typeof setInterval> | null = null;
let backgroundTimerId: ReturnType<typeof setInterval> | null = null;
let isWindowVisible = true;
let cachedTime = 0;
let cachedDuration = 0;
const listeners = new Set<() => void>();

function notify() {
  for (const l of listeners) l();
}

/** Subscribe to audio time changes (for useSyncExternalStore) */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Read current playback position (cached, updated in progress loop) */
export function getCurrentTime(): number {
  return cachedTime;
}

/** Read duration (cached, updated in progress loop) */
export function getDuration(): number {
  return cachedDuration;
}

function syncFromHowl() {
  if (!currentHowl) {
    cachedTime = 0;
    cachedDuration = fallbackDuration;
    return;
  }
  const t = currentHowl.seek();
  cachedTime = typeof t === 'number' ? t : 0;
  const d = currentHowl.duration();
  cachedDuration = d > 0 ? d : fallbackDuration;
}

/** Seek audio to position in seconds */
export function seek(seconds: number) {
  if (currentHowl) {
    currentHowl.seek(seconds);
    syncFromHowl();
    notify();
    // Delay SMTC update so Howler settles to actual position
    setTimeout(() => {
      updateMediaSessionPosition();
      lastMediaSessionSync = performance.now();
    }, 150);
  }
}

/** Smart prev: restart if >3s in, otherwise previous track */
export function handlePrev() {
  if (getCurrentTime() > 3) {
    seek(0);
  } else {
    usePlayerStore.getState().prev();
  }
}

/* ── Howl management ─────────────────────────────────────────── */

function toHowlerVolume(v: number) {
  return Math.min(1, Math.max(0, v / 200));
}

function destroyHowl() {
  stopProgressLoop();
  stopBackgroundTimer();
  if (currentHowl) {
    currentHowl.off();
    currentHowl.stop();
    currentHowl.unload();
    currentHowl = null;
  }
  cachedTime = 0;
}

let lastMediaSessionSync = 0;

function startProgressLoop() {
  stopProgressLoop();
  if (!isWindowVisible) {
    startBackgroundTimer();
    return;
  }
  progressTimerId = setInterval(() => {
    if (!currentHowl || (!currentHowl.playing() && !usePlayerStore.getState().isPlaying)) {
      stopProgressLoop();
      return;
    }
    syncFromHowl();
    notify();
    const now = performance.now();
    if (now - lastMediaSessionSync > 5000) {
      lastMediaSessionSync = now;
      updateMediaSessionPosition();
    }
}, 30); // ~33 fps
}

function stopProgressLoop() {
  if (progressTimerId !== null) {
    clearInterval(progressTimerId);
    progressTimerId = null;
  }
}

function startBackgroundTimer() {
  stopBackgroundTimer();
  backgroundTimerId = setInterval(() => {
    syncFromHowl();
    updateMediaSessionPosition();
  }, 5000);
}

function stopBackgroundTimer() {
  if (backgroundTimerId !== null) {
    clearInterval(backgroundTimerId);
    backgroundTimerId = null;
  }
}

/* ── Visibility change: stop UI updates when window hidden ── */

document.addEventListener('visibilitychange', () => {
  isWindowVisible = document.visibilityState === 'visible';
  if (!currentHowl?.playing()) return;
  if (isWindowVisible) {
    stopBackgroundTimer();
    syncFromHowl();
    notify(); // sync UI immediately on restore
    startProgressLoop();
  } else {
    stopProgressLoop();
    startBackgroundTimer();
  }
});

function createHowl(src: string, urn: string, onFail?: () => void): Howl {
  return new Howl({
    src: [src],
    html5: true,
    format: ['mp3'],
    volume: toHowlerVolume(usePlayerStore.getState().volume),
    onplay: () => {
      if (currentUrn === urn && !usePlayerStore.getState().isPlaying) {
        usePlayerStore.getState().resume();
      }
      startProgressLoop();
      updateMediaSessionState(true);
      updateMediaSessionPosition();
    },
    onpause: () => {
      if (currentUrn === urn && usePlayerStore.getState().isPlaying) {
        usePlayerStore.getState().pause();
      }
      updateMediaSessionState(false);
      updateMediaSessionPosition();
    },
    onload: () => {
      if (currentUrn !== urn) return;
      syncFromHowl(); // duration is now available from howl
      notify();
    },
    onend: () => {
      if (currentUrn !== urn) return;
      handleTrackEnd();
    },
    onloaderror: (_id, error) => {
      console.error(`[Audio] Load error (${src.slice(0, 60)}):`, error);
      if (currentUrn !== urn) return;
      onFail ? onFail() : usePlayerStore.getState().pause();
    },
    onplayerror: (_id, error) => {
      console.error(`[Audio] Play error (${src.slice(0, 60)}):`, error);
      if (currentUrn !== urn) return;
      onFail ? onFail() : usePlayerStore.getState().pause();
    },
  });
}

async function loadTrack(track: Track) {
  destroyHowl();
  currentUrn = track.urn;
  const urn = track.urn;

  fallbackDuration = track.duration / 1000;
  syncFromHowl();
  notify();

  const cachedPath = await getCacheFilePath(urn);
  if (currentUrn !== urn) return;

  const httpUrl = streamUrl(urn);

  const playHowl = (howl: Howl) => {
    currentHowl = howl;
    if (usePlayerStore.getState().isPlaying) howl.play();
  };

  const fallbackToStream = () => {
    if (currentUrn !== urn) return;
    destroyHowl();
    playHowl(createHowl(httpUrl, urn));
  };

  if (cachedPath) {
    const cacheUrl = getCacheUrl(urn);
    if (cacheUrl) {
      playHowl(createHowl(cacheUrl, urn, fallbackToStream));
    } else {
      playHowl(createHowl(httpUrl, urn));
    }
  } else {
    playHowl(createHowl(httpUrl, urn));
    fetchAndCacheTrack(urn).catch(() => {});
  }
}

function handleTrackEnd() {
  const state = usePlayerStore.getState();
  if (state.repeat === 'one') {
    currentHowl?.seek(0);
    currentHowl?.play();
  } else {
    const { queue, queueIndex, shuffle } = state;
    const isLast = !shuffle && queueIndex >= queue.length - 1;
    if (isLast && state.repeat === 'off' && queue.length > 0) {
      void autoplayRelated(queue[queueIndex]);
    } else {
      usePlayerStore.getState().next();
    }
  }
}

/* ── Store subscriber ────────────────────────────────────────── */

usePlayerStore.subscribe((state, prev) => {
  const trackChanged = state.currentTrack?.urn !== currentUrn;
  const playToggled = state.isPlaying !== prev.isPlaying;

  if (trackChanged) {
    if (state.currentTrack) {
      updateMediaSession(state.currentTrack);
      void loadTrack(state.currentTrack);
    } else {
      destroyHowl();
      currentUrn = null;
      fallbackDuration = 0;
      cachedDuration = 0;
      notify();
    }
    return;
  }

  if (playToggled && !trackChanged) {
    if (state.isPlaying) {
      if (!currentHowl && state.currentTrack) {
        void loadTrack(state.currentTrack);
      } else if (currentHowl && !currentHowl.playing()) {
        currentHowl.play();
      }
    } else {
      currentHowl?.pause();
    }
  }

  if (state.volume !== prev.volume && currentHowl) {
    currentHowl.volume(toHowlerVolume(state.volume));
  }
});

/* ── Media Session ───────────────────────────────────────────── */

function updateMediaSession(track: Track) {
  if (!('mediaSession' in navigator)) return;

  const artwork500 = art(track.artwork_url, 't500x500');
  const artwork128 = art(track.artwork_url, 't120x120');

  navigator.mediaSession.metadata = new MediaMetadata({
    title: track.title,
    artist: track.user.username,
    artwork: [
      ...(artwork128 ? [{ src: artwork128, sizes: '120x120', type: 'image/jpeg' }] : []),
      ...(artwork500 ? [{ src: artwork500, sizes: '500x500', type: 'image/jpeg' }] : []),
    ],
  });
}

function updateMediaSessionState(playing: boolean) {
  if (!('mediaSession' in navigator)) return;
  navigator.mediaSession.playbackState = playing ? 'playing' : 'paused';
}

function updateMediaSessionPosition() {
  if (!('mediaSession' in navigator) || !navigator.mediaSession.setPositionState) return;
  const duration = getDuration();
  if (duration > 0) {
    navigator.mediaSession.setPositionState({
      duration,
      position: Math.min(getCurrentTime(), duration),
      playbackRate: 1,
    });
  }
}

if ('mediaSession' in navigator) {
  const ms = navigator.mediaSession;
  ms.setActionHandler('play', () => usePlayerStore.getState().resume());
  ms.setActionHandler('pause', () => usePlayerStore.getState().pause());
  ms.setActionHandler('nexttrack', () => usePlayerStore.getState().next());
  ms.setActionHandler('previoustrack', () => handlePrev());
  ms.setActionHandler('seekto', (d) => {
    if (d.seekTime != null) seek(d.seekTime);
  });
  ms.setActionHandler('seekforward', (d) => {
    seek(Math.min(getCurrentTime() + (d.seekOffset ?? 10), getDuration()));
  });
  ms.setActionHandler('seekbackward', (d) => {
    seek(Math.max(getCurrentTime() - (d.seekOffset ?? 10), 0));
  });
}

/* ── Autoplay ────────────────────────────────────────────────── */

let autoplayLoading = false;

async function autoplayRelated(lastTrack: Track) {
  if (autoplayLoading) return;
  autoplayLoading = true;

  try {
    const { queue } = usePlayerStore.getState();
    const existingUrns = new Set(queue.map((t) => t.urn));
    const res = await api<{ collection: Track[] }>(
      `/tracks/${encodeURIComponent(lastTrack.urn)}/related?limit=20`,
    );
    const fresh = res.collection.filter((t) => !existingUrns.has(t.urn));
    if (fresh.length === 0) return;

    usePlayerStore.getState().addToQueue(fresh);
    usePlayerStore.getState().next();
  } catch (e) {
    console.error('Autoplay related failed:', e);
    usePlayerStore.getState().pause();
  } finally {
    autoplayLoading = false;
  }
}

/* ── Preloading ──────────────────────────────────────────────── */

let preloadTimer: ReturnType<typeof setTimeout> | null = null;
const MAX_CONCURRENT_PRELOADS = 2;
let activePreloads = 0;

export function preloadTrack(urn: string) {
  if (preloadTimer) clearTimeout(preloadTimer);
  preloadTimer = setTimeout(() => {
    if (activePreloads >= MAX_CONCURRENT_PRELOADS) return;
    isCached(urn).then((hit) => {
      if (!hit && activePreloads < MAX_CONCURRENT_PRELOADS) {
        activePreloads++;
        fetchAndCacheTrack(urn)
          .catch(() => {})
          .finally(() => {
            activePreloads--;
          });
      }
    });
  }, 500);
}

export function preloadQueue() {
  const { queue, queueIndex } = usePlayerStore.getState();
  for (let i = 1; i <= 2; i++) {
    const idx = queueIndex + i;
    if (idx < queue.length) {
      const urn = queue[idx].urn;
      isCached(urn).then((hit) => {
        if (!hit) fetchAndCacheTrack(urn).catch(() => {});
      });
    }
  }
}
