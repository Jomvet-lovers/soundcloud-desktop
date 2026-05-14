import { useEffect, useState } from 'react';

/**
 * Returns `true` while `active` is on, then auto-flips to `false` after `delayMs`.
 * Used by playing-state overlays on cards: show the pause indicator briefly,
 * then fade so it doesn't sit on top of the artwork (hover still reveals it via CSS).
 */
export function useAutoHide(active: boolean, delayMs = 1500): boolean {
  const [visible, setVisible] = useState(active);

  useEffect(() => {
    if (!active) {
      setVisible(false);
      return;
    }
    setVisible(true);
    const id = window.setTimeout(() => setVisible(false), delayMs);
    return () => window.clearTimeout(id);
  }, [active, delayMs]);

  return visible;
}
