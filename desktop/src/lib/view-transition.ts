import { flushSync } from 'react-dom';

/**
 * Run a state update inside a View Transition so the browser morphs the named
 * elements (e.g. the active tab pill) with a natural easing. Falls back to a
 * plain update when the runtime lacks support or the user prefers reduced
 * motion. Only composited properties are animated — the browser handles it.
 */
export function withViewTransition(update: () => void): void {
  const doc = document as Document & {
    startViewTransition?: (callback: () => void) => unknown;
  };
  if (
    typeof doc.startViewTransition !== 'function' ||
    window.matchMedia('(prefers-reduced-motion: reduce)').matches
  ) {
    update();
    return;
  }
  doc.startViewTransition(() => {
    flushSync(update);
  });
}
