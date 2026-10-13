/**
 * Дозагрузка очереди, когда она кончилась.
 *
 * Триггерится из player store: как только `next()` упирается в конец очереди
 * с repeat='off', она зовёт зарегистрированный здесь fallback вместо паузы.
 * Покрывает оба сценария:
 *   1) трек докрутился до конца естественно → audio:ended → handleTrackEnd → next()
 *   2) юзер вручную клацнул «Next» на последнем треке → next()
 *
 * Стратегия (по приоритету):
 *   0) контекстный источник (лайки/плейлист/…), если активен, — доигрываем его
 *      ДО КОНЦА, подкачивая страницы (см. lib/queue-continuation.ts);
 *   1) «волна» рекомендаций от последнего трека — SC related, затем треки его
 *      артиста (см. lib/queue-wave.ts). Это автоплей уровня официального
 *      клиента: плейлист/лайки кончились — играем похожее.
 * Волну выключает настройка autoplayStation («Autoplay related tracks»);
 * с ней выключенной поведение прежнее: контекст доигран — пауза.
 *
 * Не вызывать параллельно: повторный вызов пока летит первый — игнор.
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
 * Дотягивает очередь из активного контекстного источника.
 * @returns true — добавили порцию свежих треков и поехали дальше; false —
 *   источника нет / он исчерпан / упал (тогда вызывающий уходит в волну).
 */
async function continueFromContextSource(): Promise<boolean> {
  const source = getQueueContinuationSource();
  if (!source) return false;

  const existing = new Set(usePlayerStore.getState().queue.map((t) => t.urn));
  // Тянем страницы, пока не наберём свежие треки либо источник не кончится
  // (страница может оказаться целиком из дублей, уже доскролленных в очередь).
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

/** Волна: related последнего трека → треки его артиста. Пусто — пауза. */
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
