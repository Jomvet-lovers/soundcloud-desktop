import {useSyncExternalStore} from 'react';

// Isolated module to avoid circular deps: api-client/host-status ↔ subscription
let cachedPremium = false;
const listeners = new Set<() => void>();

export function getIsPremium(): boolean {
  return cachedPremium;
}

export function setIsPremium(value: boolean): void {
  if (value === cachedPremium) return;
  cachedPremium = value;
  for (const l of listeners) l();
}

function subscribePremium(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function usePremium(): boolean {
  return useSyncExternalStore(subscribePremium, getIsPremium);
}

// Канал «premium-флаг под подозрением — свериться с /me/subscription».
// Дёргают api-client (star 403) и host-status (вход в star-reserve);
// обрабатывает subscription.ts (single-flight + cooldown там).
let recheckHandler: (() => void) | null = null;
let recheckPending = false;

export function setPremiumRecheckHandler(fn: () => void): void {
  recheckHandler = fn;
  if (recheckPending) {
    recheckPending = false;
    fn();
  }
}

export function requestPremiumRecheck(): void {
  if (recheckHandler) recheckHandler();
  else recheckPending = true;
}
