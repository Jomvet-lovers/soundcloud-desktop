import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import type { Track } from '../stores/player';
import { usePlayerStore } from '../stores/player';
import { useSettingsStore } from '../stores/settings';
import {
  api,
  buildStorageUrls,
  downloadFallbackUrls,
  getSessionId,
  resolveTrackFromStreaming,
  streamFallbackUrls,
} from './api';
import {
  enforceAudioCacheLimit,
  ensureTrackCached,
  getCacheInfo,
  removeCachedTrack,
  type StreamLoadResult,
  streamTrackCached,
} from './cache';
import { trackedInvoke as invoke, logInfo } from './diagnostics';
import { isUrnDisliked } from './dislikes';
import { recordEvent } from './events';
import { art } from './formatters';
import { addPendingHistory } from './history-pending';
import { rememberTracks } from './offline-index';
import { queryClient } from './query-client';
import { getArtistDisplay, getDisplayTitle } from './track-display';

const SKIP_THRESHOLD_SEC = 30;
/** Sustained-play threshold before reporting server-side history. */
const REPORT_THRESHOLD_MS = 15_000;
let reportTimer: number | undefined;
/** Минимум, чтобы засчитать «прослушано полностью» для коротких треков (50% длительности). */
const FULL_PLAY_RATIO = 0.5;
/** Битый кеш: сыграло меньше этого на треке от EARLY_END_MIN_EXPECTED_SEC — лечим перекачкой. */
const EARLY_END_PLAYED_SEC = 10;
const EARLY_END_MIN_EXPECTED_SEC = 30;
/** Один лечебный перекач на урн за сессию — защита от лупа на 30s-превью и мёртвых источниках. */
const healedUrns = new Set<string>();
/** Consecutive disliked-track auto-skips before the player gives up (see the subscriber). */
const AUTO_SKIP_LIMIT = 200;
let autoSkipDepth = 0;

/* ── Audio engine state ──────────────────────────────────────── */

let currentUrn: string | null = null;
let hasTrack = false;
let fallbackDuration = 0;
let cachedTime = 0;
let cachedDuration = 0;
let downloadProgress: number | null = null;
let loadGen = 0;
let lastEndedUrn: string | null = null;
// A seek requested before its track finished loading (jumping from a comment
// on a track that isn't playing yet) — applied once afterLoad lands.
let pendingSeek: { urn: string; seconds: number } | null = null;
const listeners = new Set<() => void>();
const API_PREVIEW_DURATION_MS = 30_000;

// The 10Hz tick fan-out drives every UI subscriber (progress, waveform clip-path,
// time readouts). When the window is hidden it's pure waste — the WebView doesn't
// throttle us, and MediaSession/Discord presence run off Rust events, not this.
// cachedTime/cachedDuration keep updating; we just skip the DOM-touching fan-out.
function notify() {
  if (typeof document !== 'undefined' && document.visibilityState === 'hidden') return;
  for (const l of listeners) l();
}

// Re-sync subscribers the moment the window comes back, so nothing shows a stale frame.
if (typeof document !== 'undefined') {
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible') {
      for (const l of listeners) l();
    }
  });
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

export function getDownloadProgress(): number | null {
  return downloadProgress;
}

function setDownloadProgress(value: number | null): void {
  if (downloadProgress === value) return;
  downloadProgress = value;
  notify();
}

// Drag scrubbing fires an input event per pointer move. Each one used to become
// its own IPC seek, and rodio's try_seek holds the player lock, so a long drag
// could stall playback. Leading + trailing throttle: the first move applies at
// once, then at most one seek per window, and the newest target always lands.
const SEEK_THROTTLE_MS = 120;
let seekTimer: ReturnType<typeof setTimeout> | null = null;
let queuedSeek: number | null = null;
let queuedSeekUrn: string | null = null;

function sendSeek(seconds: number, urn: string) {
  // The track may have changed between scheduling and sending.
  if (!hasTrack || currentUrn !== urn) return;
  invoke('audio_seek', { position: seconds }).catch(console.error);
  setTimeout(() => updateMediaPosition(), 150);
}

export function seek(seconds: number) {
  if (!hasTrack) return;
  // Optimistic time keeps the thumb on the pointer while the IPC is coalesced.
  cachedTime = seconds;
  notify();
  if (seekTimer !== null) {
    queuedSeek = seconds;
    queuedSeekUrn = currentUrn;
    return;
  }
  const urn = currentUrn;
  if (urn === null) return;
  sendSeek(seconds, urn);
  seekTimer = setTimeout(() => {
    seekTimer = null;
    const target = queuedSeek;
    const targetUrn = queuedSeekUrn;
    queuedSeek = null;
    queuedSeekUrn = null;
    if (target !== null && targetUrn !== null) sendSeek(target, targetUrn);
  }, SEEK_THROTTLE_MS);
}

/** Jump to `seconds` in `urn`, waiting for the load when the track isn't ready
 *  yet — a comment click on a track that isn't playing starts it there. */
export function seekInTrack(urn: string, seconds: number) {
  if (currentUrn === urn && hasTrack) {
    seek(seconds);
    return;
  }
  pendingSeek = { urn, seconds };
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
  // A seek queued for the previous track must not land on the next one.
  queuedSeek = null;
  queuedSeekUrn = null;
}

export async function switchAudioDevice(
  deviceName: string | null,
  manual = false,
  deviceLabel: string | null = null,
) {
  const previousFollow = useSettingsStore.getState().followDefaultOutput;
  if (manual) {
    // Follow-off before the switch keeps the default-device monitor from
    // stealing the sink while it is being opened.
    await invoke('audio_set_follow_default_output', { follow: deviceName == null });
  }

  try {
    await invoke('audio_switch_device', { deviceName, deviceLabel });
  } catch (error) {
    // The switch failed, so the sink stayed where it was: put the follow choice
    // back, otherwise Rust keeps following nothing while the UI still says it
    // follows the system default.
    if (manual) {
      await invoke('audio_set_follow_default_output', { follow: previousFollow }).catch(() => {});
    }
    throw error;
  }

  // Persist so the pick survives a restart. A failed switch throws above and
  // leaves the previous choice intact.
  useSettingsStore.getState().setAudioOutputDevice(deviceName);
  useSettingsStore.getState().setAudioOutputDeviceName(deviceName ? deviceLabel : null);
  useSettingsStore.getState().setFollowDefaultOutput(deviceName == null);
}

/** Resolves once zustand persist has loaded `sc-settings-v21` from disk. */
function whenSettingsHydrated(): Promise<void> {
  const persist = useSettingsStore.persist;
  if (persist.hasHydrated()) return Promise.resolve();
  return new Promise<void>((resolve) => {
    const unsubscribe = persist.onFinishHydration(() => {
      unsubscribe();
      resolve();
    });
    // Hydration can finish between the check above and the subscription.
    if (persist.hasHydrated()) {
      unsubscribe();
      resolve();
    }
  });
}

/** Set once per session: this is a boot step, not a retryable action. */
let restoreBootDone = false;

/** Apply the persisted output device at startup. A saved device that is gone
 *  (unplugged BT sink, hot-swapped USB dock) falls back to the OS default
 *  through the switch path, and the saved id stays for the next time.
 *
 *  Waits for persist hydration first: `lib/audio` is also reached through the
 *  static import graph (`AppShell` → `NowPlayingBar`), so this module evaluates
 *  while `main.tsx` is still importing `App` — before `bootstrap()` awaits
 *  `rehydrate()`. Reading the store at module-eval time only ever saw the
 *  defaults, which made the restore a silent no-op on every launch. */
export async function restoreAudioDevice() {
  if (restoreBootDone) return;
  restoreBootDone = true;

  await whenSettingsHydrated();
  const { audioOutputDevice, audioOutputDeviceName, followDefaultOutput } =
    useSettingsStore.getState();
  logInfo(
    `[Audio] restore: hydrated=${useSettingsStore.persist.hasHydrated()} ` +
      `saved=${audioOutputDevice ?? '(none)'} label=${audioOutputDeviceName ?? '(none)'} ` +
      `follow=${followDefaultOutput}`,
  );

  if (audioOutputDevice) {
    try {
      await switchAudioDevice(audioOutputDevice, true, audioOutputDeviceName);
      logInfo('[Audio] restore: switched to the saved output device');
    } catch (error) {
      // The switch itself already fell back to the system default; say so once
      // and keep the saved id so the device is picked up again when it returns.
      console.warn('[Audio] saved output device unavailable:', error);
      logInfo(`[Audio] restore FAILED: ${getLoadErrorText(error) ?? String(error)}`);
      toast.error('Saved output device not found', {
        id: 'audio-device-restore',
        description: 'Using the system default for now.',
      });
    }
    return;
  }

  // No manual pick: hand the persisted follow choice to Rust explicitly instead
  // of relying on the store default matching Rust's startup value.
  await invoke('audio_set_follow_default_output', { follow: followDefaultOutput }).catch((error) =>
    logInfo(`[Audio] restore: follow flag failed: ${String(error)}`),
  );
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

type TrackMetadataPatch = Partial<Track> & {
  full_duration?: number;
};

function getResolvedDurationMs(track: {
  duration?: number;
  full_duration?: number;
}): number | null {
  if (typeof track.full_duration === 'number' && track.full_duration > 0) {
    return track.full_duration;
  }
  if (typeof track.duration === 'number' && track.duration > 0) {
    return track.duration;
  }
  return null;
}

function getPreviewResolveUrl(track: Pick<Track, 'duration' | 'permalink_url'>): string | null {
  if (track.duration !== API_PREVIEW_DURATION_MS || !track.permalink_url) {
    return null;
  }

  try {
    const url = new URL(track.permalink_url);
    return url.hostname.endsWith('soundcloud.com') ? url.toString() : null;
  } catch {
    return null;
  }
}

function mergeTrackMetadata(base: Track, patch: TrackMetadataPatch): Track {
  const resolvedDuration = getResolvedDurationMs(patch);

  return {
    ...base,
    ...patch,
    duration:
      resolvedDuration == null ||
      (resolvedDuration === API_PREVIEW_DURATION_MS && base.duration > API_PREVIEW_DURATION_MS)
        ? base.duration
        : resolvedDuration,
    permalink_url: patch.permalink_url ?? base.permalink_url,
    user: patch.user ? { ...base.user, ...patch.user } : base.user,
  };
}

function commitTrackMetadata(track: Track) {
  usePlayerStore.getState().replaceTrackMetadata(track);
  void rememberTracks([track]);

  if (currentUrn !== track.urn) return;

  if (track.duration <= 0) {
    updateMetadata(track);
    return;
  }

  const durationSecs = track.duration / 1000;
  fallbackDuration = durationSecs;
  cachedDuration = durationSecs;
  updateMetadata(track, durationSecs);
  notify();
}

async function fetchFreshTrackMetadata(track: Track): Promise<Track> {
  try {
    const freshTrack = await api<Track>(`/tracks/${encodeURIComponent(track.urn)}`);
    return mergeTrackMetadata(track, freshTrack);
  } catch (error) {
    console.warn('[Audio] Failed to hydrate track metadata:', error);
    return track;
  }
}

async function resolveTrackMetadata(track: Track): Promise<Track> {
  const resolveUrl = getPreviewResolveUrl(track);
  if (!resolveUrl) return track;

  try {
    const resolvedTrack = await resolveTrackFromStreaming(resolveUrl);
    return mergeTrackMetadata(track, resolvedTrack);
  } catch (error) {
    console.warn('[Audio] Failed to resolve preview duration:', error);
    return track;
  }
}

/** True when a file path no longer exists on disk. */
function isFileMissing(e: unknown): boolean {
  const s = typeof e === 'string' ? e : e instanceof Error ? e.message : String(e);
  return /no such file|os error 2|cannot find the (file|path)|system cannot find/i.test(s);
}

/**
 * Load a cached file, surviving the raw-А → clean-Б transcode swap: if the path
 * was deleted between cache-resolve and read, re-resolve through the cache (the
 * clean file now, or a fresh download) and retry once.
 */
async function loadCachedFile(
  urn: string,
  path: string,
  startPaused: boolean,
  reResolve: () => Promise<string | null>,
): Promise<{ duration_secs: number | null }> {
  try {
    return await invoke<{ duration_secs: number | null }>('audio_load_file', {
      path,
      cacheKey: urn,
      startPaused,
    });
  } catch (e) {
    if (!isFileMissing(e)) throw e;
    console.warn('[Audio] cached file vanished, re-resolving:', urn);
    const fresh = await reResolve();
    if (!fresh) throw e;
    return await invoke<{ duration_secs: number | null }>('audio_load_file', {
      path: fresh,
      cacheKey: urn,
      startPaused,
    });
  }
}

async function loadTrack(track: Track) {
  const gen = ++loadGen;
  const isNewTrack = currentUrn !== track.urn;
  stopTrack();
  currentUrn = track.urn;
  // A pending seek aimed at another track must not fire when that track loads.
  if (pendingSeek && pendingSeek.urn !== track.urn) pendingSeek = null;
  const urn = track.urn;

  // A-B loop is per-track: drop it only when loading a genuinely different track —
  // NOT on same-track reloads (repeat-one, device/EQ reload, or the loop's own
  // restart). Done here, after currentUrn is advanced, so the resulting store
  // notification doesn't re-enter the track-changed branch of the subscriber.
  if (isNewTrack && usePlayerStore.getState().abLoop) {
    usePlayerStore.getState().clearAbLoop();
  }

  void hydrateTrackMetadata(track, gen);

  fallbackDuration = track.duration / 1000;
  cachedDuration = fallbackDuration;
  cachedTime = 0;
  setDownloadProgress(null);
  usePlayerStore.getState().setPlaybackTransport(null, null);
  notify();

  // Sync EQ state to Rust
  const { eqEnabled, eqGains, normalizeVolume } = useSettingsStore.getState();
  invoke('audio_set_eq', { enabled: eqEnabled, gains: eqGains }).catch(console.error);
  invoke('audio_set_normalization', { enabled: normalizeVolume }).catch(console.error);

  // Sync volume + playback rate (pitch is folded into the speed value sent to Rust)
  invoke('audio_set_volume', { volume: usePlayerStore.getState().volume }).catch(console.error);
  invoke('audio_set_playback_rate', { rate: getEffectivePlaybackRate() }).catch(console.error);

  try {
    const highQualityStreaming = useSettingsStore.getState().highQualityStreaming;

    // The cached file can be swapped (raw А → clean Б) or evicted between resolve
    // and read; re-resolve through the cache to recover the current path.
    const reResolve = async (): Promise<string | null> => {
      const info = await getCacheInfo(urn);
      if (info?.path) return info.path;
      try {
        return (await ensureTrackCached(urn, highQualityStreaming, track.duration)).path;
      } catch {
        return null;
      }
    };

    // Strategy 1: Cache hit — instant
    const cached = await getCacheInfo(urn);
    if (cached?.path) {
      if (gen !== loadGen) return;
      usePlayerStore.getState().setPlaybackTransport(cached.quality, cached.source);
      console.log('[Audio] Playing from cache:', urn);
      const loadResult = await loadCachedFile(
        urn,
        cached.path,
        !usePlayerStore.getState().isPlaying,
        reResolve,
      );
      if (gen !== loadGen) return;
      if (loadResult?.duration_secs) {
        fallbackDuration = loadResult.duration_secs;
        cachedDuration = loadResult.duration_secs;
        updateMetadata(track, loadResult.duration_secs);
        notify();
      }
      afterLoad(track, gen);
      return;
    }

    // Strategy 2: fetch to cache and play. Rust mirrors the winning response
    // into the decoder and starts as soon as a prebuffer has landed when the
    // container allows it (progressive MP3/ADTS, front-moov m4a); otherwise it
    // waits for the finished file exactly like before.
    setDownloadProgress(0);

    // This click owns the bandwidth until its first audio arrives (streaming
    // start) or the whole file is cached; viewport prefetch waits for that.
    setNetworkLoad(true);
    try {
      const startPaused = !usePlayerStore.getState().isPlaying;
      let loaded: StreamLoadResult;
      try {
        loaded = await streamTrackCached(urn, highQualityStreaming, track.duration, startPaused);
      } catch (error) {
        if (gen !== loadGen) {
          // Superseded by a newer load, which already stopped the engine at its
          // own start. A global `audio_stop` here would kill that newer track
          // (engine::stop has no owner check), so just drop this attempt.
          return;
        }
        if (!highQualityStreaming) throw error;
        console.warn('[Audio] HQ load failed, retrying without hq:', error);
        loaded = await streamTrackCached(urn, false, track.duration, startPaused);
      }

      if (gen !== loadGen) {
        // Same as above: the newer load owns the engine now.
        return;
      }
      // A streaming load keeps the progress readout until the download finishes.
      if (!loaded.streaming) setDownloadProgress(null);
      usePlayerStore.getState().setPlaybackTransport(loaded.quality, loaded.source);

      console.log(
        loaded.streaming
          ? '[Audio] Playing while downloading:'
          : '[Audio] Playing downloaded track:',
        urn,
      );
      if (loaded.durationSecs) {
        fallbackDuration = loaded.durationSecs;
        cachedDuration = loaded.durationSecs;
        updateMetadata(track, loaded.durationSecs);
        notify();
      }
      void enforceAudioCacheLimit().catch(console.error);

      afterLoad(track, gen);
    } finally {
      setNetworkLoad(false);
    }
  } catch (e) {
    console.error('[Audio] Load failed:', e);
    setDownloadProgress(null);
    usePlayerStore.getState().setPlaybackTransport(null, null);
    if (gen !== loadGen) return;
    const errorText = getLoadErrorText(e);
    toast.error('Failed to load track', {
      description: errorText ? `${track.title}: ${errorText}` : track.title,
    });
    usePlayerStore.getState().pause();
  }
}

function afterLoad(track: Track, gen: number) {
  if (gen !== loadGen) {
    // A newer load already reset the engine; stopping globally here would kill it.
    return;
  }
  hasTrack = true;

  const historyTrack =
    usePlayerStore.getState().currentTrack?.urn === track.urn
      ? usePlayerStore.getState().currentTrack
      : track;

  // Record to listening history (fire-and-forget), skip on repeat-one (same track looping)
  if (historyTrack?.urn && historyTrack.title && usePlayerStore.getState().repeat !== 'one') {
    const entry = {
      id: `pending:${historyTrack.urn}:${Date.now()}`,
      scTrackId: historyTrack.urn,
      title: getDisplayTitle(historyTrack),
      artistName: getArtistDisplay(historyTrack).primary || historyTrack.user?.username || '',
      artistUrn: historyTrack.user?.urn || null,
      artworkUrl: historyTrack.artwork_url || null,
      duration: historyTrack.duration || 0,
      playedAt: new Date().toISOString(),
    };
    api('/history', {
      method: 'POST',
      body: JSON.stringify({
        scTrackId: historyTrack.urn,
        title: entry.title,
        artistName: entry.artistName,
        artistUrn: entry.artistUrn,
        artworkUrl: entry.artworkUrl,
        duration: entry.duration,
      }),
    })
      .then(() => {
        addPendingHistory(entry);
        void queryClient.invalidateQueries({ queryKey: ['history'] });
      })
      .catch(() => {});
    // Server-side attribution via the official analytics audio flow (the
    // backend mirrors it from the same track still playing after 25s).
    if (historyTrack.permalink_url && historyTrack.user?.urn && historyTrack.duration > 0) {
      if (reportTimer !== undefined) clearTimeout(reportTimer);
      const urn = historyTrack.urn;
      const reportBody = JSON.stringify({
        permalinkUrl: historyTrack.permalink_url,
        trackUrn: historyTrack.urn,
        ownerUrn: historyTrack.user.urn,
        durationMs: historyTrack.duration,
      });
      reportTimer = window.setTimeout(() => {
        const s = usePlayerStore.getState();
        if (s.currentTrack?.urn === urn && s.isPlaying && s.repeat !== 'one') {
          api('/history/report', { method: 'POST', body: reportBody }).catch(() => {});
        }
      }, REPORT_THRESHOLD_MS);
    }
  }

  const isPlaying = usePlayerStore.getState().isPlaying;
  invoke(isPlaying ? 'audio_play' : 'audio_pause').catch(console.error);
  // A jump requested while the track was still loading lands now.
  if (pendingSeek && pendingSeek.urn === track.urn) {
    const { seconds } = pendingSeek;
    pendingSeek = null;
    seek(seconds);
  }
  updatePlaybackState(isPlaying);
  updateMediaPosition();
  preloadQueue();
}

async function hydrateTrackMetadata(track: Track, gen: number) {
  let nextTrack = await fetchFreshTrackMetadata(track);
  if (gen !== loadGen || currentUrn !== track.urn) return;

  nextTrack = await resolveTrackMetadata(nextTrack);
  if (gen !== loadGen || currentUrn !== track.urn) return;
  commitTrackMetadata(nextTrack);
}

/** Трек «закончился» через пару секунд при заявленных минутах — в кеше битый
 *  файл (заголовок целый, данные обрезаны: легаси без .meta.json или яд из
 *  storage до серверного duration-гейта). Сносим файл и перекачиваем вместо
 *  тихого скипа на следующий. */
function maybeHealEarlyEnd(): boolean {
  if (!currentUrn || navigator.onLine === false) return false;
  const state = usePlayerStore.getState();
  const track = state.currentTrack;
  if (!track || track.urn !== currentUrn || state.abLoop) return false;
  if (track.duration / 1000 < EARLY_END_MIN_EXPECTED_SEC) return false;
  if (cachedTime >= EARLY_END_PLAYED_SEC) return false;
  if (healedUrns.has(track.urn)) return false;
  healedUrns.add(track.urn);
  console.warn(
    `[Audio] ended after ${cachedTime.toFixed(1)}s of ${(track.duration / 1000).toFixed(0)}s — purging cache and refetching:`,
    track.urn,
  );
  void removeCachedTrack(track.urn)
    .catch(() => {})
    .then(() => {
      if (usePlayerStore.getState().currentTrack?.urn === track.urn) {
        return loadTrack(track);
      }
    });
  return true;
}

function handleTrackEnd() {
  const state = usePlayerStore.getState();
  // A-B loop whose end sits at (or within a tick of) the track end: the Rust-side
  // loop can't catch it before the sink drains, so restart the segment from A here.
  if (state.abLoop?.b != null && state.currentTrack) {
    const track = state.currentTrack;
    const a = state.abLoop.a;
    // loadTrack bumps loadGen synchronously; capture it so that if the user switches
    // tracks during the (async) reload, this stale restart-seek is dropped instead of
    // jumping the newly-loaded track to A.
    const loadPromise = loadTrack(track);
    const gen = loadGen;
    void loadPromise.then(() => {
      if (gen === loadGen && usePlayerStore.getState().currentTrack?.urn === track.urn) {
        seek(a);
      }
    });
    return;
  }
  if (state.repeat === 'one') {
    // rodio sink is empty after track ends — must reload
    if (state.currentTrack) void loadTrack(state.currentTrack);
    return;
  }
  // Всегда через next(). Если упёрся в конец очереди — store сам позовёт
  // autopilot (см. setEndOfQueueFallback в lib/queue-autopilot.ts).
  // Clear currentUrn so subscriber detects change even if next track has same URN.
  currentUrn = null;
  usePlayerStore.getState().next();
}

/* ── Tauri event listeners ───────────────────────────────────── */

listen<number>('audio:tick', (event) => {
  cachedTime = event.payload;
  if (cachedDuration <= 0) cachedDuration = fallbackDuration;
  notify();
});

listen<{ urn: string; progress: number }>('track:download-progress', (event) => {
  const { urn, progress } = event.payload;
  if (urn === currentUrn) {
    setDownloadProgress(progress);
  }
});

// The streamed track's download finished and Rust rebuilt the player from the
// complete bytes (exact duration, seek source, loudness gain).
listen<{ urn: string }>('audio:stream-complete', (event) => {
  if (event.payload.urn !== currentUrn) return;
  setDownloadProgress(null);
  void enforceAudioCacheLimit().catch(console.error);
});

listen<{ urn: string; error: string }>('audio:stream-failed', (event) => {
  if (event.payload.urn !== currentUrn) return;
  setDownloadProgress(null);
  const errorText = getLoadErrorText(event.payload.error);
  toast.error('Failed to load track', {
    description: errorText ?? undefined,
  });
});

listen('audio:ended', () => {
  if (maybeHealEarlyEnd()) return;
  if (currentUrn) {
    // Засчитываем full_play только если трек реально игрался: либо ≥30s,
    // либо проиграно ≥50% длительности (для коротких треков). Иначе это
    // зависшая загрузка / зеро-длительность баг — не отправляем.
    const playedEnough =
      cachedTime >= SKIP_THRESHOLD_SEC ||
      (cachedDuration > 0 && cachedTime >= cachedDuration * FULL_PLAY_RATIO);
    if (playedEnough) {
      const positionPct = cachedDuration > 0 ? Math.min(1, cachedTime / cachedDuration) : undefined;
      recordEvent('full_play', currentUrn, positionPct);
    }
    lastEndedUrn = currentUrn;
  }
  hasTrack = false;
  handleTrackEnd();
});

listen('audio:device-reconnected', () => {
  console.log('[Audio] Device reconnected');
});

listen<string>('audio:default-device-changed', (event) => {
  console.log(`[Audio] Default output changed to '${event.payload}'`);
});

/* ── Track comments (floating pills) ─────────────────────────── */

export interface TimelineComment {
  id: number;
  body: string;
  timestamp_ms: number;
  user_avatar_url: string | null;
}

const commentListeners = new Set<(comment: TimelineComment) => void>();

/** Feed the pending comments to the Rust playback timeline; it re-emits each
 *  one as `comments:show` when the playhead sweeps past its timestamp. */
export function setCommentsTimeline(comments: TimelineComment[]) {
  invoke('audio_set_comments_timeline', { comments }).catch(console.error);
}

export function clearCommentsTimeline() {
  invoke('audio_clear_comments_timeline').catch(console.error);
}

export function subscribeFloatingComments(
  listener: (comment: TimelineComment) => void,
): () => void {
  commentListeners.add(listener);
  return () => {
    commentListeners.delete(listener);
  };
}

listen<TimelineComment>('comments:show', (event) => {
  for (const l of commentListeners) l(event.payload);
});

/* ── Store subscriber ────────────────────────────────────────── */

usePlayerStore.subscribe((state, prev) => {
  const nextUrn = state.currentTrack?.urn ?? null;
  const trackChanged = nextUrn !== currentUrn;
  const playToggled = state.isPlaying !== prev.isPlaying;

  if (trackChanged) {
    const previousUrn = currentUrn;
    const previousTime = cachedTime;
    const previousHadTrack = hasTrack;

    if (
      previousUrn &&
      previousHadTrack &&
      previousTime < SKIP_THRESHOLD_SEC &&
      previousUrn !== lastEndedUrn
    ) {
      const previousDuration = cachedDuration > 0 ? cachedDuration : fallbackDuration;
      const positionPct = previousDuration > 0 ? previousTime / previousDuration : undefined;
      recordEvent('skip', previousUrn, positionPct);
    }
    lastEndedUrn = null;

    if (state.currentTrack) {
      // Автоскип дизлайкнутых треков: пропускаем без загрузки/плэя.
      if (isUrnDisliked(state.currentTrack.urn)) {
        currentUrn = null;
        fallbackDuration = 0;
        cachedDuration = 0;
        cachedTime = 0;
        hasTrack = false;
        usePlayerStore.getState().setPlaybackTransport(null, null);
        notify();
        // next() re-enters this subscriber synchronously, and with repeat='all'
        // a queue whose every track is disliked would recurse forever. Defer one
        // step and stop skipping once the budget is spent.
        if (++autoSkipDepth > AUTO_SKIP_LIMIT) {
          autoSkipDepth = 0;
          usePlayerStore.getState().pause();
          return;
        }
        queueMicrotask(() => {
          if (usePlayerStore.getState().isPlaying) usePlayerStore.getState().next();
        });
        return;
      }
      autoSkipDepth = 0;
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

  if (
    state.playbackRate !== prev.playbackRate ||
    state.pitchSemitones !== prev.pitchSemitones ||
    state.pitchControlMode !== prev.pitchControlMode
  ) {
    invoke('audio_set_playback_rate', { rate: getEffectivePlaybackRate() }).catch(console.error);
  }

  // A-B loop: only push an active region (both bounds set); otherwise clear it.
  if (state.abLoop !== prev.abLoop) {
    const ab = state.abLoop;
    const active = ab != null && ab.b != null;
    invoke('audio_set_ab_loop', {
      a: active ? ab.a : null,
      b: active ? ab.b : null,
    }).catch(console.error);
  }
});

/** Combine playback rate and (manual) pitch into a single Rust-side speed value.
 *  Rust uses rodio's `set_speed` which couples tempo+pitch — so manual pitch is
 *  applied as a multiplier on top of the user's rate.
 */
function getEffectivePlaybackRate(): number {
  const { playbackRate, pitchControlMode, pitchSemitones } = usePlayerStore.getState();
  if (pitchControlMode === 'manual' && Math.abs(pitchSemitones) > 0.001) {
    return playbackRate * 2 ** (pitchSemitones / 12);
  }
  return playbackRate;
}

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

/* ── Restore persisted output device ────────────────────────── */

void restoreAudioDevice().catch(console.error);

/* ── Native Media Controls (souvlaki: MPRIS/SMTC) ───────────── */
function updateMetadata(track: Track, durationSecs?: number) {
  const coverUrl = art(track.artwork_url, 't500x500') || undefined;
  const display = getArtistDisplay(track);
  const title = getDisplayTitle(track);
  invoke('audio_set_metadata', {
    title,
    artist: display.primary || track.user.username,
    coverUrl: coverUrl || null,
    durationSecs: durationSecs ?? track.duration / 1000,
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

/* ── Preloading ──────────────────────────────────────────────── */

let preloadTimer: ReturnType<typeof setTimeout> | null = null;

/** Hover targets requested directly; viewport prefetch leaves them alone. */
const hoverPrefetched = new Set<string>();
const viewportRequested = new Set<string>();
let viewportTimer: ReturnType<typeof setTimeout> | null = null;
/** Picked but not sent yet: waiting on the debounce or on a user load. */
let queuedViewport: string[] | null = null;
/** Rows prefetched per viewport, and the settle delay before asking. */
const VIEWPORT_PREFETCH_LIMIT = 3;
const VIEWPORT_PREFETCH_DELAY_MS = 1200;

/** A click that has to fetch audio owns the bandwidth from load start until its
 *  first audio arrives (streaming start) or the whole file is cached. Batches
 *  queued in that window wait; already-running prefetches are left alone. */
let networkLoads = 0;
let networkLoadActive = false;

function setNetworkLoad(active: boolean) {
  // Loads can overlap (a click while the previous track is still fetching), so
  // the gate closes only when the last one is done.
  networkLoads = Math.max(0, networkLoads + (active ? 1 : -1));
  networkLoadActive = networkLoads > 0;
  if (networkLoadActive) {
    // Hold a batch that is only waiting on its debounce instead of firing it
    // while the click is still fetching.
    if (viewportTimer) {
      clearTimeout(viewportTimer);
      viewportTimer = null;
    }
    return;
  }
  const pending = queuedViewport;
  queuedViewport = null;
  if (pending) prefetchVisibleTracks(pending);
}

export function preloadTrack(urn: string) {
  // A hover is a stronger signal than the viewport scan: drop the queued batch
  // so this request goes out first, and never duplicate it later.
  hoverPrefetched.add(urn);
  if (viewportTimer) {
    clearTimeout(viewportTimer);
    viewportTimer = null;
  }
  queuedViewport = null;
  if (preloadTimer) clearTimeout(preloadTimer);
  preloadTimer = setTimeout(() => {
    const sessionId = getSessionId();
    const hq = useSettingsStore.getState().highQualityStreaming;
    invoke('track_preload', {
      entries: [
        {
          urn,
          urls: streamFallbackUrls(urn, hq),
          downloadUrls: downloadFallbackUrls(urn, hq),
          storageUrls: buildStorageUrls(urn),
          sessionId,
          hq,
        },
      ],
    }).catch(console.error);
  }, 500);
}

/** Prefetch a few rows of the visible viewport, so a click that never hovered
 *  still lands on a warm cache. Debounced, deduped for the session, and skipped
 *  for the playing track; Rust skips anything already cached. */
export function prefetchVisibleTracks(urns: string[]) {
  const picked: string[] = [];
  for (const urn of urns) {
    if (picked.length >= VIEWPORT_PREFETCH_LIMIT) break;
    if (urn === currentUrn || hoverPrefetched.has(urn) || viewportRequested.has(urn)) continue;
    picked.push(urn);
  }
  if (picked.length === 0) return;

  // Same batch as the one already queued (e.g. a re-render on scroll): keep the
  // timer running instead of pushing the request back.
  if (queuedViewport && queuedViewport.join('|') === picked.join('|')) return;
  queuedViewport = picked;
  if (networkLoadActive) return; // resumed by setNetworkLoad(false)
  if (viewportTimer) clearTimeout(viewportTimer);
  viewportTimer = setTimeout(() => {
    viewportTimer = null;
    queuedViewport = null;
    const sessionId = getSessionId();
    const hq = useSettingsStore.getState().highQualityStreaming;
    invoke('track_preload', {
      entries: picked.map((urn) => ({
        urn,
        urls: streamFallbackUrls(urn, hq),
        downloadUrls: downloadFallbackUrls(urn, hq),
        storageUrls: buildStorageUrls(urn),
        sessionId,
        hq,
      })),
    }).catch(console.error);
    for (const urn of picked) viewportRequested.add(urn);
  }, VIEWPORT_PREFETCH_DELAY_MS);
}

export function preloadQueue() {
  const { queue, queueIndex } = usePlayerStore.getState();
  const entries: Array<{
    urn: string;
    urls: string[];
    downloadUrls: string[];
    storageUrls: string[];
    sessionId: string | null;
    hq: boolean;
    durationMs?: number;
  }> = [];
  const sessionId = getSessionId();
  const hq = useSettingsStore.getState().highQualityStreaming;

  for (let i = 1; i <= 3; i++) {
    const idx = queueIndex + i;
    if (idx < queue.length) {
      entries.push({
        urn: queue[idx].urn,
        urls: streamFallbackUrls(queue[idx].urn, hq),
        downloadUrls: downloadFallbackUrls(queue[idx].urn, hq),
        storageUrls: buildStorageUrls(queue[idx].urn),
        sessionId,
        hq,
        durationMs: queue[idx].duration,
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
