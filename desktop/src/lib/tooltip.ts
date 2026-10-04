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
 * Any element with a DOM `title` attribute is intercepted: the attribute is
 * temporarily removed while the pointer hovers so the WebView does not draw
 * its own bubble, and restored on leave (unless React replaced it meanwhile).
 * `data-tooltip` takes precedence when an element carries both.
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
let savedTitle: string | null = null;
let showTimer: number | undefined;
let hideTimer: number | undefined;
let frame = 0;
let shown = false;

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
  return title?.trim() ? title : null;
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

/** Put the native `title` back if we removed it and React hasn't rewritten it. */
function restoreTitle(): void {
  if (anchor && savedTitle !== null && anchor.isConnected && !anchor.hasAttribute('title')) {
    anchor.setAttribute('title', savedTitle);
  }
  savedTitle = null;
}

function hide(): void {
  clearTimers();
  restoreTitle();
  anchor = null;
  pending = null;
  shown = false;
  if (tip) tip.dataset.visible = '0';
}

/** Position above the anchor, flip below when clipped, clamp to viewport. */
function show(el: HTMLElement, suppressNative: boolean): void {
  if (!el.isConnected) return;
  const text = textOf(el);
  if (!text) return;

  anchor = el;
  if (suppressNative) {
    const title = el.getAttribute('title');
    savedTitle = title;
    if (title !== null) el.removeAttribute('title');
  }

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

function scheduleShow(el: HTMLElement, delay: number, suppressNative: boolean): void {
  if (anchor === el && shown) return;
  if (pending === el && showTimer !== undefined) return; // keep the running delay
  clearTimers();
  if (anchor && anchor !== el) hide();
  pending = el;
  showTimer = window.setTimeout(() => {
    showTimer = undefined;
    pending = null;
    show(el, suppressNative);
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
        if (anchor || pending) hide();
        return;
      }
      scheduleShow(el, SHOW_DELAY_MS, true);
    },
    true,
  );

  document.addEventListener(
    'pointerout',
    (e) => {
      const el = anchor ?? pending;
      if (!el) return;
      const rel = (e as PointerEvent).relatedTarget as Node | null;
      if (rel && el.contains(rel)) return;
      if (hideTimer !== undefined) return;
      hideTimer = window.setTimeout(() => {
        hideTimer = undefined;
        if (anchor === el || pending === el) hide();
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
      hide();
      show(el, false);
    },
    true,
  );

  document.addEventListener(
    'focusout',
    () => {
      if (anchor) hide();
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
        if (el.matches(':hover')) scheduleShow(el, 0, true);
        else if (anchor === el) show(el, false);
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
