import { useSyncExternalStore } from 'react';
import { Animated } from 'react-native';

/**
 * Позиция воспроизведения — ВНЕ React-стейта, чтобы тики ядра (~10Гц) не
 * ре-рендерили дерево. Кросс-платформенно (Animated есть на всех ОС):
 *   • прогресс-бары/волна читают `positionValue` (Animated.Value сырых секунд)
 *     и обновляются императивно — 0 ре-рендеров;
 *   • таймкоды берут `useWholeSeconds()`/`useDurationMs()` (useSyncExternalStore
 *     с флором до секунды) — ре-рендерится только сам таймкод, 1Гц.
 */
export const positionValue = new Animated.Value(0);

let rawSecs = 0;
let wholeSecs = 0;
let durationMs = 0;
const listeners = new Set<() => void>();

const emit = () => {
  for (const l of listeners) l();
};

export function setPosition(secs: number): void {
  // Гейт на NaN/Inf: неудачный seek (durationSecs мгновенно NaN) иначе кидал бы
  // сюда NaN и таймкод мигал бы «NaN» кадр до следующего position-события.
  if (!Number.isFinite(secs)) return;
  const clamped = secs < 0 ? 0 : secs;
  rawSecs = clamped;
  positionValue.setValue(clamped);
  const w = Math.floor(clamped);
  if (w !== wholeSecs) {
    wholeSecs = w;
    emit();
  }
}

export function setDuration(ms: number): void {
  const next = Number.isFinite(ms) && ms > 0 ? ms : 0;
  if (next === durationMs) return;
  durationMs = next;
  emit();
}

export const resetPosition = (): void => setPosition(0);

/** Синхронное чтение (напр. для относительного seek с клавиатуры). */
export const getPositionSecs = (): number => rawSecs;

const subscribe = (l: () => void): (() => void) => {
  listeners.add(l);
  return () => {
    listeners.delete(l);
  };
};

export const useWholeSeconds = (): number =>
  useSyncExternalStore(subscribe, () => wholeSecs, () => wholeSecs);

export const useDurationMs = (): number =>
  useSyncExternalStore(subscribe, () => durationMs, () => durationMs);
