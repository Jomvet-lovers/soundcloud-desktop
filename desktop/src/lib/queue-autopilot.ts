/**
 * Top up the queue once it runs out.
 *
 * Triggered from the player store: as soon as `next()` hits the end of the queue
 * with repeat='off', it calls the fallback registered here instead of pausing.
 * Covers both scenarios:
 *   1) the track ran to its natural end → audio:ended → handleTrackEnd → next()
 *   2) the user pressed «Next» on the last track by hand → next()
 *
 * Strategy (by priority):
 *   0) the context source (likes/playlist/…), when active — play it
 *      TO THE END, pulling pages as we go (see lib/queue-continuation.ts);
 *   1) the «wave» of recommendations from the last track — SC related, then its
 *      artist's tracks (see lib/queue-wave.ts). This is official-client
 *      level autoplay: playlist/likes ran out — play something similar.
 * The wave is switched off by the autoplayStation setting («Autoplay related tracks»);
 * with it off the behaviour is as before: context played out — pause.
 *
 * Do not call in parallel: a second call while the first is in flight is ignored.
 */

import {
  setEndOfQueueFallback,
  setPlaybackContextResetHandler,
  type Track,
  usePlayerStore,
} from '../stores/player';
import { useSettingsStore } from '../stores/settings';
import { getQueueContinuationSource, setQueueContinuationSource } from './queue-continuation';
import { fetchWaveTracks, resetWaveState } from './queue-wave';

let inFlight = false;

/**
 * Tops the queue up from the active context source.
 * @returns true — a batch of fresh tracks was added and playback moves on; false —
 *   no source / it is exhausted / it failed (the caller then falls through to the wave).
 */
async function continueFromContextSource(): Promise<boolean> {
  const source = getQueueContinuationSource();
  if (!source) return false;

  const existing = new Set(usePlayerStore.getState().queue.map((t) => t.urn));
  // Pull pages until we collect fresh tracks or the source runs out
  // (a page may turn out to be all duplicates already scrolled into the queue).
  for (;;) {
    let batch: Track[];
    try {
      batch = await source.next();
    } catch (e) {
      console.debug(`[autopilot] source "${source.kind}" failed → wave:`, e);
      setQueueContinuationSource(null);
      return false;
    }
    if (batch.length === 0) {
      console.debug(`[autopilot] source "${source.kind}" exhausted → wave`);
      setQueueContinuationSource(null);
      return false;
    }
    const fresh = batch.filter((t) => !existing.has(t.urn));
    if (fresh.length > 0) {
      usePlayerStore.getState().addToQueue(fresh);
      usePlayerStore.getState().next();
      return true;
    }
    for (const t of batch) existing.add(t.urn);
  }
}

/** The wave: last track's related → its artist's tracks. Empty — pause */
async function continueWithWave(lastTrack: Track): Promise<boolean> {
  let fresh: Track[];
  try {
    fresh = await fetchWaveTracks(lastTrack);
  } catch (e) {
    console.debug('[autopilot] wave failed:', e);
    return false;
  }
  if (fresh.length === 0) {
    console.debug('[autopilot] no fresh recommendations');
    return false;
  }
  console.debug('[autopilot] wave:', fresh.length, 'fresh tracks from', lastTrack.urn);
  usePlayerStore.getState().addToQueue(fresh);
  usePlayerStore.getState().next();
  return true;
}

export function setupQueueAutopilot(): void {
  // A new play() from the UI invalidates the previous context's continuation
  // and the wave's seed chain.
  setPlaybackContextResetHandler(() => {
    setQueueContinuationSource(null);
    resetWaveState();
  });
  setEndOfQueueFallback((lastTrack) => {
    if (inFlight) return;
    inFlight = true;
    void (async () => {
      try {
        if (await continueFromContextSource()) return;
        if (!useSettingsStore.getState().autoplayStation) {
          usePlayerStore.getState().pause();
          return;
        }
        if (await continueWithWave(lastTrack)) return;
        usePlayerStore.getState().pause();
      } finally {
        inFlight = false;
      }
    })();
  });
}
