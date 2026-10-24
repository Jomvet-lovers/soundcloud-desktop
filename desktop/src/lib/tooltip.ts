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
 * Native `title` attributes never stay in the DOM: each one is moved to
 * `data-tooltip` (and to `aria-label` when the element carries no other name)
 * as soon as it appears, at startup and via a MutationObserver for later
 * renders, so the WebView cannot draw its own bubble at any point. The custom
 * bubble is driven by `data-tooltip`.
 *
 * React component props named `title` (Card/Row headings etc.) never reach the
 * DOM as attributes, so they are unaffected by the conversion.
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

/** Move a native `title` into the custom-tooltip channel and the accessible
 *  name, then drop the attribute so the WebView can never show its bubble. */
function adopt(el: Element): void {
  const title = el.getAttribute('title');
  if (title === null) return;
  if (!el.hasAttribute('data-tooltip') && title.trim()) {
    el.setAttribute('data-tooltip', title);
  }
  el.removeAttribute('title');
  if (
    title.trim() &&
    !el.hasAttribute('aria-label') &&
    !el.hasAttribute('aria-labelledby') &&
    !(el.textContent ?? '').trim()
  ) {
    el.setAttribute('aria-label', title);
  }
}

function adoptAll(root: ParentNode): void {
  if (root instanceof Element) adopt(root);
  for (const el of root.querySelectorAll('[title]')) adopt(el);
}

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

  // Only the primary button dismisses: a right-click leaves the pointer on
  // the control, and hiding here restores the native `title` under the cursor
  // — the WebView would then draw its own bubble instead of ours.
  document.addEventListener(
    'pointerdown',
    (e) => {
      if (e.button !== 0) return;
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

  // Native titles are converted as soon as they reach the DOM, at startup and
  // on every later render. Removing the attribute, instead of only intercepting
  // hover, is what makes the WebView's own bubble impossible rather than rare.
  adoptAll(document);
  const titleObserver = new MutationObserver((records) => {
    for (const record of records) {
      if (record.type === 'attributes') {
        if (record.target instanceof Element) adopt(record.target);
        continue;
      }
      for (const node of record.addedNodes) {
        if (node instanceof Element) adoptAll(node);
      }
    }
  });
  titleObserver.observe(document.documentElement, {
    subtree: true,
    childList: true,
    attributes: true,
    attributeFilter: ['title'],
  });
}
