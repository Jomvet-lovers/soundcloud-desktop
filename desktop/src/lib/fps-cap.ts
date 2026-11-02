//! Frame-rate cap.
//!
//! In a windowed webview (both wry/webkitgtk and CEF/chromium) animations run at the
//! monitor's refresh rate — on 120/144 Hz that is wasted CPU/GPU for the same picture.
//! Chromium has no «cap to N fps» flag for windowed (`windowless_frame_rate` is
//! off-screen only). So we throttle `requestAnimationFrame`: skipped frames are
//! re-scheduled (so rAF loops do not break), and `cancelAnimationFrame`
//! keeps working. On displays at or below the target FPS it is effectively a no-op.
//!
//! The skip decision is ONE per frame, and an allowed frame takes ALL waiting
//! callbacks at once. Otherwise it starves: each callback used to have its own
//! check against the shared `last`, the first one to run moved `last` to
//! `now`, and everyone else in the same frame saw a delta of 0 and was deferred. While
//! the app spins even one constant rAF loop, it moves `last` every
//! frame — and the deferred callbacks NEVER run. That is how the modal's
//! `data-state` transition died silently: it mounted and stayed at `opacity: 0`,
//! i.e. «no modal» with a live DOM.

const DEFAULT_FPS = 60;

let installed = false;

export function installFpsCap(targetFps: number = DEFAULT_FPS): void {
  if (installed || targetFps <= 0 || typeof window === 'undefined') return;
  installed = true;

  const minDelta = 1000 / targetFps;
  const rafNative = window.requestAnimationFrame.bind(window);
  const cafNative = window.cancelAnimationFrame.bind(window);

  /** Our handle → the callback waiting for the nearest allowed frame */
  const pending = new Map<number, FrameRequestCallback>();
  let nextHandle = 1;
  let nativeId: number | null = null;
  let last = 0;

  function schedule(): void {
    if (nativeId === null) nativeId = rafNative(pump);
  }

  function pump(now: number): void {
    nativeId = null;
    if (now - last < minDelta - 1) {
      schedule(); // skip the whole frame — everyone included, no exceptions
      return;
    }
    last = now;
    // Snapshot: a callback may request the next frame right away, and it must land
    // in the NEXT batch instead of being eaten by the current iteration.
    const batch = Array.from(pending.values());
    pending.clear();
    for (const cb of batch) cb(now);
  }

  window.requestAnimationFrame = (cb: FrameRequestCallback): number => {
    const handle = nextHandle++;
    pending.set(handle, cb);
    schedule();
    return handle;
  };

  window.cancelAnimationFrame = (handle: number): void => {
    // id not from our pool (e.g. handed out before the cap was installed) — go native.
    if (!pending.delete(handle)) cafNative(handle);
  };
}
