import { useCallback, useRef, useState } from 'react';

/* ── Per-context tab memory ───────────────────────────────────── */

/**
 * In-page tab selections survive a leave-and-return (Search → user → back)
 * for the same context key — a search word, an entity urn — while a *new*
 * context (a different word, another profile) always opens on its default
 * tab. Memory is session-only; a FIFO cap keeps it bounded.
 */
const TAB_MEMORY_CAP = 48;
const tabMemory = new Map<string, string>();

function rememberTab(key: string, value: string) {
  tabMemory.delete(key);
  tabMemory.set(key, value);
  if (tabMemory.size > TAB_MEMORY_CAP) {
    const oldest = tabMemory.keys().next().value;
    if (oldest != null) tabMemory.delete(oldest);
  }
}

/** `useState` backed by per-key memory. Restores a tab for a re-entered
 *  context and resets on a key change while mounted: the key (search word,
 *  :urn param) changing is the same signal the old reset effects used. */
export function useRestoredTab<T extends string>(
  key: string | undefined,
  initial: T,
): [T, (next: T) => void] {
  const [value, setValue] = useState<T>(() =>
    key != null ? ((tabMemory.get(key) as T | undefined) ?? initial) : initial,
  );
  const lastKey = useRef(key);

  // Re-derive once per key change: an unseen key falls back to the default,
  // which is what keeps every fresh search on Tracks.
  if (key !== lastKey.current) {
    lastKey.current = key;
    const next = key != null ? ((tabMemory.get(key) as T | undefined) ?? initial) : initial;
    if (next !== value) setValue(next);
  }

  const set = useCallback(
    (next: T) => {
      if (key != null) rememberTab(key, next);
      setValue(next);
    },
    [key],
  );
  return [value, set];
}
