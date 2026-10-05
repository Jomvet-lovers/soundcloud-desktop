/**
 * Custom tooltips replacing the browser-native `title` bubbles.
 *
 * Performance contract:
 * - one fixed-position node for the whole app, created lazily on first show;
 * - one delegated listener set on `document` (no per-element handlers);
 * - no React state, no re-renders;
 * - only `transform` (positioning) and `opacity` (fade) are animated;
 * - layout is read once per show (`getBoundingClientRect`), never on move.
 *
 * Any element with a DOM `title` attribute is intercepted. The attribute is
 * removed as soon as the pointer enters the element — not when the custom
 * bubble appears — so the WebView can never race us with its own bubble while
 * the show delay runs (or restarts). The value is restored on leave, unless
 * React replaced it meanwhile. `data-tooltip` takes precedence when an element
 * carries both.
 *
 * React component props named `title` (Card/Row headings etc.) never reach the
 * DOM as attributes, so they are unaffected by the interception.
 */

const SHOW_DELAY_MS = 350;
const HIDE_DELAY_MS = 60;
const GAP = 8;
const EDGE = 6;

let installed = false;
let tip: HTMLDivElement | null = null;
let anchor: HTMLElement | null = null;
let pending: HTMLElement | null = null;
let pendingText = '';
let showTimer: number | undefined;
let hideTimer: number | undefined;
let frame = 0;
let shown = false;

/** Elements whose native `title` we removed, with the value to put back. */
const suppressed = new Map<HTMLElement, string>();

function node(): HTMLDivElement {
  if (tip) return tip;
  const el = document.createElement('div');
  el.className = 'scd-tooltip';
  el.setAttribute('role', 'tooltip');
  el.setAttribute('aria-hidden', 'true');
  document.body.appendChild(el);
  tip = el;
  return el;
}

function triggerOf(target: EventTarget | null): HTMLElement | null {
  if (!(target instanceof Element)) return null;
  return target.closest<HTMLElement>('[title], [data-tooltip]');
}

function textOf(el: HTMLElement): string | null {
  const explicit = el.getAttribute('data-tooltip');
  if (explicit?.trim()) return explicit;
  const title = el.getAttribute('title');
  if (title?.trim()) return title;
  // The native attribute may be temporarily removed by suppressNative().
  const saved = suppressed.get(el);
  return saved?.trim() ? saved : null;
}

/**
 * Remove the native `title` right away so the WebView does not draw its own
 * bubble while the custom show is delayed, re-scheduled or cancelled.
 */
function suppressNative(el: HTMLElement): void {
  if (suppressed.has(el)) return;
  const title = el.getAttribute('title');
  if (title !== null) {
    suppressed.set(el, title);
    el.removeAttribute('title');
  }
}

/** Put the native `title` back if we removed it and React hasn't rewritten it. */
function restoreNative(el: HTMLElement): void {
  const saved = suppressed.get(el);
  if (saved === undefined) return;
  suppressed.delete(el);
  if (el.isConnected && !el.hasAttribute('title')) el.setAttribute('title', saved);
}

function clearTimers(): void {
  if (showTimer !== undefined) {
    window.clearTimeout(showTimer);
    showTimer = undefined;
  }
  if (hideTimer !== undefined) {
    window.clearTimeout(hideTimer);
    hideTimer = undefined;
  }
}

/** Drop a scheduled (not yet visible) tooltip and restore its native title. */
function cancelPending(): void {
  if (showTimer !== undefined) {
    window.clearTimeout(showTimer);
    showTimer = undefined;
  }
  if (pending) restoreNative(pending);
  pending = null;
  pendingText = '';
}

/** Hide the visible tooltip and restore the anchor's native title. */
function hideAnchor(): void {
  if (anchor) restoreNative(anchor);
  anchor = null;
  shown = false;
  if (tip) tip.dataset.visible = '0';
}

function hide(): void {
  clearTimers();
  cancelPending();
  hideAnchor();
}

/** Position above the anchor, flip below when clipped, clamp to viewport. */
function show(el: HTMLElement, text: string): void {
  if (!el.isConnected || !text) return;

  anchor = el;

  const t = node();
  t.textContent = text;
  t.dataset.visible = '0';

  const r = el.getBoundingClientRect();
  const tr = t.getBoundingClientRect();
  let x = r.left + r.width / 2 - tr.width / 2;
  let y = r.top - tr.height - GAP;
  if (y < EDGE) y = r.bottom + GAP;
  const maxX = window.innerWidth - tr.width - EDGE;
  x = x < EDGE ? EDGE : x > maxX ? Math.max(EDGE, maxX) : x;
  t.style.transform = `translate3d(${Math.round(x)}px, ${Math.round(y)}px, 0)`;

  shown = true;
  cancelAnimationFrame(frame);
  frame = window.requestAnimationFrame(() => {
    frame = 0;
    if (anchor === el) t.dataset.visible = '1';
  });
}

function scheduleShow(el: HTMLElement, delay: number): void {
  if (anchor === el && shown) return;
  const text = textOf(el);
  if (!text) return;
  if (pending === el && showTimer !== undefined) return; // keep the running delay
  clearTimers();
  cancelPending();
  hideAnchor();
  pending = el;
  pendingText = text;
  suppressNative(el);
  showTimer = window.setTimeout(() => {
    showTimer = undefined;
    const target = pending;
    const label = pendingText;
    pending = null;
    pendingText = '';
    if (target) show(target, label);
  }, delay);
}

/** Install the delegated listeners once. Safe to call from module init/HMR. */
export function initTooltips(): void {
  if (installed) return;
  installed = true;

  document.addEventListener(
    'pointerover',
    (e) => {
      if ((e as PointerEvent).pointerType === 'touch') return;
      const el = triggerOf(e.target);
      if (!el) {
        // Leaving the tracked trigger for untitled chrome: drop whatever is
        // pending/visible for it, but never touch a different element's state.
        if (pending && e.target instanceof Node && pending.contains(e.target)) return;
        if (anchor && e.target instanceof Node && anchor.contains(e.target)) return;
        if (anchor) hideAnchor();
        if (pending) cancelPending();
        return;
      }
      scheduleShow(el, SHOW_DELAY_MS);
    },
    true,
  );

  document.addEventListener(
    'pointerout',
    (e) => {
      const el = triggerOf(e.target);
      if (!el || (el !== anchor && el !== pending)) return;
      const rel = (e as PointerEvent).relatedTarget as Node | null;
      if (rel && el.contains(rel)) return;
      if (hideTimer !== undefined) return;
      hideTimer = window.setTimeout(() => {
        hideTimer = undefined;
        // The WebView can emit a stray pointerout when the cursor jumps onto
        // an element (re-render or synthetic move). If the pointer is still
        // over it, ignore the event instead of dropping the tooltip.
        if (el.matches(':hover')) return;
        if (anchor === el) hideAnchor();
        else if (pending === el) cancelPending();
      }, HIDE_DELAY_MS);
    },
    true,
  );

  // Keyboard focus shows immediately and keeps the native title (Chromium
  // never draws its bubble for focus), so AT still gets the description.
  document.addEventListener(
    'focusin',
    (e) => {
      const el = triggerOf(e.target);
      if (!el || el === anchor) return;
      const text = textOf(el);
      if (!text) return;
      hide();
      show(el, text);
    },
    true,
  );

  document.addEventListener(
    'focusout',
    () => {
      if (anchor) hideAnchor();
    },
    true,
  );

  document.addEventListener(
    'pointerdown',
    () => {
      if (anchor || pending) hide();
    },
    true,
  );

  // After a click the control state (and its title) changes. Refresh the
  // tooltip once React has committed so it reports the new state instead of
  // staying hidden until the pointer leaves the element.
  document.addEventListener(
    'click',
    (e) => {
      const el = triggerOf(e.target);
      if (!el) return;
      window.setTimeout(() => {
        if (!el.isConnected) return;
        if (el.matches(':hover')) scheduleShow(el, 0);
        else if (anchor === el) {
          const text = textOf(el);
          if (text) show(el, text);
        }
      }, 0);
    },
    true,
  );

  document.addEventListener(
    'keydown',
    (e) => {
      if (e.key === 'Escape' && (anchor || pending)) hide();
    },
    true,
  );

  // Any scroll invalidates the anchor rect; hiding is cheaper than tracking.
  window.addEventListener(
    'scroll',
    () => {
      if (anchor || pending) hide();
    },
    true,
  );
  window.addEventListener(
    'resize',
    () => {
      if (anchor || pending) hide();
    },
    { passive: true },
  );
  window.addEventListener('blur', () => {
    if (anchor || pending) hide();
  });
}
