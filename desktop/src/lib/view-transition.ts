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
  // Document-scoped transitions capture the page into snapshots and lift the
  // named elements out of it. A dialog is portalled, fixed-position and its own
  // compositing layer, so that lift leaves the dialog's surface unpainted: the
  // equalizer panel lost its background every time a preset was picked. Dialogs
  // keep plain state updates; the pill still cross-fades via CSS.
  if (document.querySelector('[role="dialog"][data-state="open"]')) {
    update();
    return;
  }
  try {
    const transition = doc.startViewTransition(() => {
      flushSync(update);
    });
    // Starting a transition while another is running rejects the older one with
    // "Transition was skipped". That is a normal outcome (rapid tab clicks), not
    // an app error, so keep it out of the unhandled-rejection log.
    void Promise.resolve(transition).catch(() => {});
  } catch {
    // A rejected transition must never swallow the update itself.
    update();
  }
}
