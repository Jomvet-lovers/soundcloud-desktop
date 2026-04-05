import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import i18n from '../i18n';
import type { Track } from '../stores/player';
import { usePlayerStore } from '../stores/player';
import { useSettingsStore } from '../stores/settings';
import { api, getSessionId, streamUrl } from './api';
import {
  enforceAudioCacheLimit,
  ensureTrackCached,
  getCacheInfo,
  type TrackCacheInfo,
} from './cache';
import { trackedInvoke as invoke } from './diagnostics';
import { art } from './formatters';

/* ── Audio engine state ──────────────────────────────────────── */

let currentUrn: string | null = null;
let hasTrack = false;
let fallbackDuration = 0;
let cachedTime = 0;
let cachedDuration = 0;
let loadGen = 0;
const listeners = new Set<() => void>();

function notify() {
  for (const l of listeners) l();
}

export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getCurrentTime(): number {
  return cachedTime;
}

export function getDuration(): number {
  return cachedDuration;
}

export function seek(seconds: number) {
  if (!hasTrack) return;
  invoke('audio_seek', { position: seconds }).catch(console.error);
  cachedTime = seconds;
  notify();
  setTimeout(() => updateMediaPosition(), 150);
}

export function handlePrev() {
  if (getCurrentTime() > 3) {
    seek(0);
  } else {
    usePlayerStore.getState().prev();
  }
}

/* ── Native audio control ────────────────────────────────────── */

function stopTrack() {
  invoke('audio_stop').catch(console.error);
  hasTrack = false;
  cachedTime = 0;
}

export async function switchAudioDevice(deviceName: string | null, manual = false) {
  if (manual) {
    await invoke('audio_set_follow_default_output', { follow: deviceName == null });
  }

  await invoke('audio_switch_device', { deviceName });
}

/** Reload the current track on new audio device, preserving position */
export async function reloadCurrentTrack() {
  const track = usePlayerStore.getState().currentTrack;
  if (!track) return;
  const wasPlaying = usePlayerStore.getState().isPlaying;
  const pos = cachedTime;
  await loadTrack(track);
  if (pos > 0) seek(pos);
  if (!wasPlaying) invoke('audio_pause').catch(console.error);
}

function getLoadErrorText(error: unknown): string | null {
  let message: string | null = null;

  if (typeof error === 'string') {
    message = error;
  } else if (error instanceof Error) {
    message = error.message;
  } else if (typeof error === 'object' && error) {
    if ('message' in error && typeof error.message === 'string') {
      message = error.message;
    } else if ('error' in error && typeof error.error === 'string') {
      message = error.error;
    }
  }

  if (!message) {
    const fallback = String(error).trim();
    if (fallback && fallback !== '[object Object]') {
      message = fallback;
    }
  }

  if (!message) return null;

  const normalized = message
    .trim()
    .replace(/^Error invoking remote method '[^']+':\s*/i, '')
    .replace(/^Command [^:]+ failed:\s*/i, '');

  const unquoted =
    normalized.startsWith('"') && normalized.endsWith('"')
      ? normalized.slice(1, -1).trim()
      : normalized;

  const sanitized = unquoted
    .replace(/\bhttps?:\/\/[^\s"')\]]+/gi, '')
    .replace(/\bscproxy:\/\/[^\s"')\]]+/gi, '')
    .replace(/\b(Bearer)\s+[A-Za-z0-9._~-]+/gi, '$1 [redacted]')
    .replace(
      /\b(oauth_token|token|sig|signature|client_id|x-session-id)=([^&\s]+)/gi,
      '$1=[redacted]',
    )
    .replace(/\s+\bfrom\b\s*(?=$|[):;,.])/gi, ' ')
    .replace(/\s{2,}/g, ' ')
    .replace(/\s+([):;,.])/g, '$1')
    .trim();

  return sanitized || null;
}

async function loadTrack(track: Track) {
  const gen = ++loadGen;
  stopTrack();
  currentUrn = track.urn;
  const urn = track.urn;

  void hydrateTrackMetadata(urn, gen);

  fallbackDuration = track.duration / 1000;
  cachedDuration = fallbackDuration;
  cachedTime = 0;
  usePlayerStore.setState({ downloadProgress: null, downloadSource: null });
  usePlayerStore.getState().setPlaybackTransport(null, null);
  notify();

  // Sync EQ state to Rust
  const { eqEnabled, eqGains, normalizeVolume } = useSettingsStore.getState();
  invoke('audio_set_eq', { enabled: eqEnabled, gains: eqGains }).catch(console.error);
  invoke('audio_set_normalization', { enabled: normalizeVolume }).catch(console.error);

  // Sync volume
  invoke('audio_set_volume', { volume: usePlayerStore.getState().volume }).catch(console.error);

  try {
    const highQualityStreaming = useSettingsStore.getState().highQualityStreaming;

    // Strategy 1: Cache hit — instant
    const cached = await getCacheInfo(urn);
    if (cached?.path) {
      if (gen !== loadGen) return;
      usePlayerStore.getState().setPlaybackTransport(cached.quality, cached.source);
      console.log('[Audio] Playing from cache:', urn);
      await invoke('audio_load_file', { path: cached.path, cacheKey: urn });
      if (gen !== loadGen) return;
      afterLoad(track, gen);
      return;
    }

    // Strategy 2: Download full track to cache — Rust picks storage/API internally
    usePlayerStore.setState({ downloadProgress: 0, downloadSource: 'api' });

    let cachedInfo: TrackCacheInfo;
    try {
      cachedInfo = await ensureTrackCached(urn, highQualityStreaming);
    } catch (error) {
      if (!highQualityStreaming) throw error;
      console.warn('[Audio] HQ load failed, retrying without hq:', error);
      cachedInfo = await ensureTrackCached(urn, false);
    }

    if (gen !== loadGen) return;
    usePlayerStore.setState({ downloadProgress: null, downloadSource: null });
    usePlayerStore.getState().setPlaybackTransport(cachedInfo.quality, cachedInfo.source);

    console.log('[Audio] Playing downloaded track:', urn);
    await invoke('audio_load_file', { path: cachedInfo.path, cacheKey: urn });
    void enforceAudioCacheLimit().catch(console.error);

    if (gen !== loadGen) return;
    afterLoad(track, gen);
  } catch (e) {
    console.error('[Audio] Load failed:', e);
    usePlayerStore.setState({ downloadProgress: null, downloadSource: null });
    usePlayerStore.getState().setPlaybackTransport(null, null);
    if (gen !== loadGen) return;
    const errorText = getLoadErrorText(e);
    toast.error(i18n.t('track.loadError'), {
      description: errorText ? `${track.title}: ${errorText}` : track.title,
    });
    usePlayerStore.getState().pause();
  }
}

function afterLoad(track: Track, gen: number) {
  if (gen !== loadGen) {
    invoke('audio_stop').catch(console.error);
    return;
  }
  hasTrack = true;

  // Record to listening history (fire-and-forget), skip on repeat-one (same track looping)
  if (track.urn && track.title && usePlayerStore.getState().repeat !== 'one') {
    api('/history', {
      method: 'POST',
      body: JSON.stringify({
        scTrackId: track.urn,
        title: track.title,
        artistName: track.user?.username || '',
        artistUrn: track.user?.urn || null,
        artworkUrl: track.artwork_url || null,
        duration: track.duration || 0,
      }),
    }).catch(() => {});
  }

  if (!usePlayerStore.getState().isPlaying) {
    invoke('audio_pause').catch(console.error);
  }

  updatePlaybackState(usePlayerStore.getState().isPlaying);
  updateMediaPosition();
  preloadQueue();
}

async function hydrateTrackMetadata(urn: string, gen: number) {
  try {
    const freshTrack = await api<Track>(`/tracks/${encodeURIComponent(urn)}`);
    if (gen !== loadGen || currentUrn !== urn) return;

    usePlayerStore.getState().replaceTrackMetadata(freshTrack);

    if (typeof freshTrack.duration === 'number' && freshTrack.duration > 0) {
      fallbackDuration = freshTrack.duration / 1000;
      cachedDuration = fallbackDuration;
      notify();
    }
  } catch (error) {
    console.warn('[Audio] Failed to hydrate track metadata:', error);
  }
}

function handleTrackEnd() {
  const state = usePlayerStore.getState();
  if (state.repeat === 'one') {
    // rodio sink is empty after track ends — must reload
    if (state.currentTrack) void loadTrack(state.currentTrack);
  } else {
    const { queue, queueIndex } = state;
    const isLast = queueIndex >= queue.length - 1;
    if (isLast && state.repeat === 'off' && queue.length > 0) {
      void autoplayRelated(queue[queueIndex]);
    } else {
      // Clear currentUrn so subscriber detects change even if next track has same URN
      currentUrn = null;
      usePlayerStore.getState().next();
    }
  }
}

/* ── Tauri event listeners ───────────────────────────────────── */

listen<number>('audio:tick', (event) => {
  cachedTime = event.payload;
  if (cachedDuration <= 0) cachedDuration = fallbackDuration;
  notify();
});

listen<{ urn: string; progress: number; source: string }>('track:download-progress', (event) => {
  const { urn, progress, source } = event.payload;
  if (urn === currentUrn) {
    usePlayerStore.setState({ downloadProgress: progress, downloadSource: source });
  }
});

listen('audio:ended', () => {
  hasTrack = false;
  handleTrackEnd();
});

listen('audio:device-reconnected', () => {
  console.log('[Audio] Device reconnected');
});

listen<string>('audio:default-device-changed', (event) => {
  console.log(`[Audio] Default output changed to '${event.payload}'`);
});

/* ── Store subscriber ────────────────────────────────────────── */

usePlayerStore.subscribe((state, prev) => {
  const nextUrn = state.currentTrack?.urn ?? null;
  const trackChanged = nextUrn !== currentUrn;
  const playToggled = state.isPlaying !== prev.isPlaying;

  if (trackChanged) {
    if (state.currentTrack) {
      updateMetadata(state.currentTrack);
      void loadTrack(state.currentTrack);
    } else {
      stopTrack();
      currentUrn = null;
      fallbackDuration = 0;
      cachedDuration = 0;
      usePlayerStore.getState().setPlaybackTransport(null, null);
      notify();
    }
    return;
  }

  if (playToggled && !trackChanged) {
    if (state.isPlaying) {
      if (!hasTrack && state.currentTrack) {
        void loadTrack(state.currentTrack);
      } else {
        invoke('audio_play').catch(console.error);
      }
    } else {
      invoke('audio_pause').catch(console.error);
    }
    updatePlaybackState(state.isPlaying);
  }

  if (state.volume !== prev.volume) {
    invoke('audio_set_volume', { volume: state.volume }).catch(console.error);
  }
});

/* ── EQ settings subscriber ──────────────────────────────────── */

useSettingsStore.subscribe((state, prev) => {
  if (state.eqEnabled !== prev.eqEnabled || state.eqGains !== prev.eqGains) {
    invoke('audio_set_eq', { enabled: state.eqEnabled, gains: state.eqGains }).catch(console.error);
  }

  if (state.normalizeVolume !== prev.normalizeVolume) {
    invoke('audio_set_normalization', { enabled: state.normalizeVolume }).catch(console.error);
    if (usePlayerStore.getState().currentTrack) {
      void reloadCurrentTrack();
    }
  }
});

/* ── Native Media Controls (souvlaki: MPRIS/SMTC) ───────────── */

function updateMetadata(track: Track) {
  const coverUrl = art(track.artwork_url, 't500x500') || undefined;
  invoke('audio_set_metadata', {
    title: track.title,
    artist: track.user.username,
    coverUrl: coverUrl || null,
    durationSecs: track.duration / 1000,
  }).catch(console.error);
}

function updatePlaybackState(playing: boolean) {
  invoke('audio_set_playback_state', { playing }).catch(console.error);
}

function updateMediaPosition() {
  const pos = getCurrentTime();
  if (pos > 0) {
    invoke('audio_set_media_position', { position: pos }).catch(console.error);
  }
}

// Listen for media control events from souvlaki (MPRIS/SMTC)
listen('media:play', () => usePlayerStore.getState().resume());
listen('media:pause', () => usePlayerStore.getState().pause());
listen('media:toggle', () => usePlayerStore.getState().togglePlay());
listen('media:next', () => usePlayerStore.getState().next());
listen('media:prev', () => handlePrev());
listen<number>('media:seek', (e) => seek(e.payload));
listen<number>('media:seek-relative', (e) => {
  const offset = e.payload;
  if (offset > 0) {
    seek(Math.min(getCurrentTime() + offset, getDuration()));
  } else {
    seek(Math.max(getCurrentTime() + offset, 0));
  }
});

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
    if (fresh.length === 0) {
      usePlayerStore.getState().pause();
      return;
    }

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

export function preloadTrack(urn: string) {
  if (preloadTimer) clearTimeout(preloadTimer);
  preloadTimer = setTimeout(() => {
    const sessionId = getSessionId();
    invoke('track_preload', {
      entries: [{ urn, url: streamUrl(urn), sessionId }],
    }).catch(console.error);
  }, 500);
}

export function preloadQueue() {
  const { queue, queueIndex } = usePlayerStore.getState();
  const entries: Array<{ urn: string; url: string; sessionId: string | null }> = [];
  const sessionId = getSessionId();

  for (let i = 1; i <= 3; i++) {
    const idx = queueIndex + i;
    if (idx < queue.length) {
      entries.push({
        urn: queue[idx].urn,
        url: streamUrl(queue[idx].urn),
        sessionId,
      });
    }
  }

  if (entries.length > 0) {
    invoke('track_preload', { entries }).catch(console.error);
  }
}

usePlayerStore.subscribe((state, prev) => {
  if (state.queueIndex !== prev.queueIndex || state.queue !== prev.queue) {
    preloadQueue();
  }
});
