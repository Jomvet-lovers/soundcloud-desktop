/**
 * Единая точка дозагрузки очереди когда она кончилась.
 *
 * Триггерится из player store: как только `next()` упирается в конец очереди
 * с repeat='off', она зовёт зарегистрированный здесь fallback вместо паузы.
 * Это покрывает оба сценария:
 *   1) трек докрутился до конца естественно → audio:ended → handleTrackEnd → next()
 *   2) юзер вручную клацнул "Next" на последнем треке → next()
 *
 * Стратегия: сначала наша "волна от трека" (Qdrant + bandit). Если ничего не
 * отдала — фоллбек на SC `/tracks/{urn}/related`. Одинаково для всех источников
 * (home, артист, лайки, поиск, плейлист).
 *
 * Не вызывать параллельно: повторный вызов пока летит первый — игнор.
 */

import { setEndOfQueueFallback, type Track, usePlayerStore } from '../stores/player';
import {fetchRelatedTracks} from './related';
import { fetchSmartWave } from './soundwave';

let inFlight = false;

export async function autopilotContinueFromTrack(lastTrack: Track): Promise<void> {
  if (inFlight) {
    console.debug('[autopilot] skipping — already in flight');
    return;
  }
  inFlight = true;
  console.debug('[autopilot] continuation from', lastTrack.urn, lastTrack.title);

  try {
    const fresh = await fetchContinuation(lastTrack);
    if (fresh.length === 0) {
      console.warn('[autopilot] no continuation tracks (wave + SC related both empty)');
      usePlayerStore.getState().pause();
      return;
    }
    console.debug('[autopilot] adding', fresh.length, 'tracks to queue');
    usePlayerStore.getState().addToQueue(fresh);
    usePlayerStore.getState().next();
  } catch (e) {
    console.error('[autopilot] continuation failed:', e);
    usePlayerStore.getState().pause();
  } finally {
    inFlight = false;
  }
}

async function fetchContinuation(seed: Track): Promise<Track[]> {
  const existing = new Set(usePlayerStore.getState().queue.map((t) => t.urn));

  const fromWave = await fetchWaveContinuation(seed);
  const waveFresh = fromWave.filter((t) => !existing.has(t.urn));
  if (waveFresh.length > 0) {
    console.debug('[autopilot] using wave-from-track,', waveFresh.length, 'fresh tracks');
    return waveFresh;
  }

  console.debug('[autopilot] wave empty → falling back to SC related');
  const fromSc = await fetchScRelated(seed);
  const scFresh = fromSc.filter((t) => !existing.has(t.urn));
  console.debug('[autopilot] SC related returned', scFresh.length, 'fresh tracks');
  return scFresh;
}

async function fetchWaveContinuation(seed: Track): Promise<Track[]> {
  const trackId = seed.urn.split(':').pop();
  if (!trackId) return [];
  try {
    const batch = await fetchSmartWave({
      seedKind: 'track',
      seedId: trackId,
      limit: 20,
    });
    return batch.tracks;
  } catch (e) {
    console.debug('[autopilot] wave fetch failed:', e);
    return [];
  }
}

async function fetchScRelated(seed: Track): Promise<Track[]> {
  try {
      const res = await fetchRelatedTracks(seed.urn, 20);
      return res.collection;
  } catch (e) {
    console.debug('[autopilot] SC related fetch failed:', e);
    return [];
  }
}

// Регистрируем при загрузке модуля. Сторе пнёт сюда при end-of-queue.
setEndOfQueueFallback(autopilotContinueFromTrack);
