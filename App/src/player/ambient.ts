import { useSyncExternalStore } from 'react';

/** Аура текущего экрана для глобального фона-атмосферы (донор per-page `Atmosphere`).
 *  Экран (страница трека и т.п.) выставляет цвета на маунте, снимает на анмаунте;
 *  слой в шелле читает и рисует орбы за сайдбаром/навбаром — свечение не рвётся. */
export interface Ambient {
  colors: string[];
  /** 0 (спокойно) .. 1 (энергично) — скорость дрейфа орбов */
  energy: number;
}

let current: Ambient | null = null;
const listeners = new Set<() => void>();

export function setAmbient(next: Ambient | null): void {
  current = next;
  for (const l of listeners) l();
}

export function useAmbient(): Ambient | null {
  return useSyncExternalStore(
    (cb) => { listeners.add(cb); return () => { listeners.delete(cb); }; },
    () => current,
    () => current,
  );
}
