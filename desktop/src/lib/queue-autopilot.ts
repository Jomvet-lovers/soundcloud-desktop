/**
 * Дозагрузка очереди, когда она кончилась (в этом форке — без «волны»:
 * backend-рекомендаций нет, поэтому после исчерпания контекста просто пауза).
 *
 * Триггерится из player store: как только `next()` упирается в конец очереди
 * с repeat='off', она зовёт зарегистрированный здесь fallback вместо паузы.
 * Покрывает оба сценария:
 *   1) трек докрутился до конца естественно → audio:ended → handleTrackEnd → next()
 *   2) юзер вручную клацнул «Next» на последнем треке → next()
 *
 * Стратегия: контекстный источник (лайки/плейлист/…), если активен, —
 * доигрываем его ДО КОНЦА, подкачивая страницы (см. lib/queue-continuation.ts).
 * Источник исчерпан/его нет — пауза.
 *
 * Не вызывать параллельно: повторный вызов пока летит первый — игнор.
 */

import {
  setEndOfQueueFallback,
  setPlaybackContextResetHandler,
  usePlayerStore,
} from '../stores/player';
import { getQueueContinuationSource, setQueueContinuationSource } from './queue-continuation';

let inFlight = false;

async function continueFromContextSource(): Promise<void> {
  const source = getQueueContinuationSource();
  if (!source) {
    usePlayerStore.getState().pause();
    return;
  }

  const existing = new Set(usePlayerStore.getState().queue.map((t) => t.urn));
  // Тянем страницы, пока не наберём свежие треки либо источник не кончится
  // (страница может оказаться целиком из дублей, уже доскролленных в очередь).
  for (;;) {
    let batch: Awaited<ReturnType<typeof source.next>>;
    try {
      batch = await source.next();
    } catch {
      setQueueContinuationSource(null);
      usePlayerStore.getState().pause();
      return;
    }
    if (batch.length === 0) {
      setQueueContinuationSource(null);
      usePlayerStore.getState().pause();
      return;
    }
    const fresh = batch.filter((t) => !existing.has(t.urn));
    if (fresh.length > 0) {
      usePlayerStore.getState().addToQueue(fresh);
      usePlayerStore.getState().next();
      return;
    }
    for (const t of batch) existing.add(t.urn);
  }
}

export function setupQueueAutopilot(): void {
  // A new play() from the UI invalidates the previous context's continuation.
  setPlaybackContextResetHandler(() => setQueueContinuationSource(null));
  setEndOfQueueFallback(() => {
    if (inFlight) return;
    inFlight = true;
    void continueFromContextSource().finally(() => {
      inFlight = false;
    });
  });
}
