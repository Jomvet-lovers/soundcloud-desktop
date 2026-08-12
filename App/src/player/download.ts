import { useSyncExternalStore } from 'react';

/**
 * Прогресс скачки текущего трека (донор `useLoadProgress`) — держим ВНЕ React-стейта
 * плеера: событие `download_progress` идёт пачками (ядро шлёт до ~100/с), поэтому
 * коалесцируем — снапшот `progress` меняется максимум раз в кадр и только на смену
 * целого процента. Иначе `useSyncExternalStore` не получает стабильный снапшот и
 * React уходит в «Maximum update depth».
 */
let currentUrn: string | null = null;
let progress: number | null = null; // то, что читает getSnapshot (меняется только в flush)
let pending: number | null = null; // последняя входящая доля, ждёт flush
let flushScheduled = false;
let holdTimer: ReturnType<typeof setTimeout> | null = null;
const listeners = new Set<() => void>();

function commit(next: number | null): void {
  if (next === progress) return;
  progress = next;
  for (const l of listeners) l();
}

function flush(): void {
  flushScheduled = false;
  if (pending === null) return;
  const frac = pending;
  pending = null;
  if (holdTimer) {
    clearTimeout(holdTimer);
    holdTimer = null;
  }
  if (frac >= 0.999) {
    // Завершение: показываем 100% и гасим коротким кадром — так дожатая
    // загрузка не мигает; повторные «100%» лишь продлевают удержание.
    commit(1);
    holdTimer = setTimeout(() => {
      holdTimer = null;
      commit(null);
    }, 320);
    return;
  }
  // Дедуп до целого процента — снимок стабилен между кадрами.
  if (progress == null || Math.round(progress * 100) !== Math.round(frac * 100)) commit(frac);
}

function schedule(): void {
  if (flushScheduled) return;
  flushScheduled = true;
  requestAnimationFrame(flush);
}

/** Новый трек начал грузиться — сбрасываем контур. */
export function beginLoad(urn: string | null): void {
  if (holdTimer) {
    clearTimeout(holdTimer);
    holdTimer = null;
  }
  currentUrn = urn;
  pending = null;
  commit(null);
}

/** Событие ядра `download_progress`: доля 0..1, только для текущего трека. */
export function reportDownload(urn: string, fraction: number): void {
  if (urn !== currentUrn) return;
  pending = Math.max(0, Math.min(1, fraction));
  schedule();
}

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => {
    listeners.delete(l);
  };
};

export const useDownloadProgress = (): number | null =>
  useSyncExternalStore(subscribe, () => progress, () => progress);

/** Целое 1..100 для отображения. */
export const loadPercent = (fraction: number): number =>
  Math.max(1, Math.min(100, Math.round(Math.max(0, Math.min(1, fraction)) * 100)));
