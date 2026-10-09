import { useSyncExternalStore } from 'react';
import type { HistoryEntry } from './hooks';

/**
 * Plays reported locally but not yet confirmed on SoundCloud.
 * The SC write path accepts the report (204) without recording it, so
 * entries stay provisional until they show up in the SC collection.
 */
type Listener = () => void;

let pending: HistoryEntry[] = [];
const listeners = new Set<Listener>();

const PENDING_TTL_MS = 30 * 60 * 1000;
const MAX_PENDING = 50;

/** SC entries carry a bare numeric id, local/pending entries a full URN.
 *  Normalize to the URN form for all comparisons. */
export function normHistoryKey(scTrackId: string): string {
  return scTrackId.startsWith('soundcloud:tracks:') ? scTrackId : `soundcloud:tracks:${scTrackId}`;
}

function notify() {
  for (const l of listeners) l();
}

export function addPendingHistory(entry: HistoryEntry) {
  const now = Date.now();
  const key = normHistoryKey(entry.scTrackId);
  pending = [
    entry,
    ...pending.filter(
      (e) => normHistoryKey(e.scTrackId) !== key && now - Date.parse(e.playedAt) < PENDING_TTL_MS,
    ),
  ].slice(0, MAX_PENDING);
  notify();
}

/** Drop entries the SC collection already contains. */
export function confirmHistory(scTrackIds: Set<string>) {
  if (scTrackIds.size === 0 || pending.length === 0) return;
  const next = pending.filter((e) => !scTrackIds.has(normHistoryKey(e.scTrackId)));
  if (next.length !== pending.length) {
    pending = next;
    notify();
  }
}

function subscribe(l: Listener) {
  listeners.add(l);
  return () => {
    listeners.delete(l);
  };
}

function getSnapshot() {
  return pending;
}

export function usePendingHistory(): HistoryEntry[] {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}

/** SC collection first, then unconfirmed local plays (deduped by track). */
export function mergeHistory(sc: HistoryEntry[], pend: HistoryEntry[]): HistoryEntry[] {
  if (pend.length === 0) return sc;
  const seen = new Set(sc.map((e) => normHistoryKey(e.scTrackId)));
  return [...pend.filter((e) => !seen.has(normHistoryKey(e.scTrackId))), ...sc];
}

export function scTrackIdSet(items: HistoryEntry[]): Set<string> {
  return new Set(items.map((e) => normHistoryKey(e.scTrackId)));
}
