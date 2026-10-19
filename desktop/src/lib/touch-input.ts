/**
 * Touch and pen input plumbing, installed once from main.tsx.
 *
 * Four delegated jobs on `document`/`window`, no per-element listeners:
 *
 * - `html[data-input-touch]`: set when a real touch pointer has been seen
 *   (seeded from `(any-pointer: coarse)` at load). index.css keys hover-reveal
 *   and hit-area refinements off it. Pen deliberately does NOT set it: a pen
 *   hovers like a mouse, so hover-reveal stays hover-dependent there.
 * - Input mode override: virtual displays that deliver touch as mouse input
 *   (SlothMon, Spacedesk, Moonlight, ...) never report `pointerType: touch`,
 *   so Settings > General > Touch can force the touch behaviors on.
 * - Titlebar drag: Tauri's `data-tauri-drag-region` only reacts to mousedown,
 *   and WebView2 does not synthesize a mouse-drag for touch/pen gestures. Here
 *   6px of pointer travel over a drag region starts the native window drag.
 * - Long-press menu: rows/cards open the app-wide menu from React
 *   `onContextMenu`. Touch long-press may or may not produce one (depends on
 *   WebView2's gesture and Windows Ink emulation), so a still 450ms press
 *   synthesizes a bubbling contextmenu at the press point; React-routing
 *   handlers (`trackMenuHandler` family) open the store menu unchanged. A
 *   native one (barrel button, OS press-and-hold) always wins: it cancels the
 *   timer before it can fire.
 * - Touch keyboard: on touch devices that tap a text field, optionally launch
 *   Windows TabTip (Settings > General > Touch, default off).
 */

import { getCurrentWindow } from '@tauri-apps/api/window';
import { useSettingsStore } from '../stores/settings';
import { trackedInvoke } from './diagnostics';

const DRAG_THRESHOLD_PX = 6;
const LONG_PRESS_MS = 450;
const LONG_PRESS_SLOP_PX = 8;
const OSK_RECENT_TOUCH_MS = 1200;
const OSK_COOLDOWN_MS = 4000;

const TEXT_ENTRY_TYPES = new Set(['text', 'search', 'url', 'tel', 'email', 'password', 'number']);

const isTextEdit = (el: EventTarget | null): boolean =>
  el instanceof HTMLElement &&
  (el.isContentEditable ||
    el.tagName === 'TEXTAREA' ||
    (el.tagName === 'INPUT' && TEXT_ENTRY_TYPES.has((el as HTMLInputElement).type as string)));

const isInteractive = (el: Element): boolean =>
  !!el.closest('button, input, a, select, textarea, [contenteditable="true"]');

let installed = false;

export function initTouchInput(): void {
  if (installed) return;
  installed = true;

  const html = document.documentElement;
  let touchSeen = false;
  let lastTouchDownAt = 0;
  let lastNativeMenuAt = 0;
  let lastKbLaunchAt = 0;

  const markTouch = () => {
    if (touchSeen) return;
    touchSeen = true;
    html.setAttribute('data-input-touch', '');
  };

  const applyMode = () => {
    if (useSettingsStore.getState().inputMode === 'touch') {
      html.setAttribute('data-input-touch', '');
      return;
    }
    // Auto: keep the flag for real touch, drop it if it was set only by force.
    if (!touchSeen) html.removeAttribute('data-input-touch');
  };

  // Seed before any gesture: a touchscreen present at all already makes
  // hover-reveal awkward, so the refinements apply from load, not first tap.
  if (window.matchMedia('(any-pointer: coarse)').matches) markTouch();
  applyMode();

  interface Press {
    pointerId: number;
    x: number;
    y: number;
    target: Element;
    timer: number;
  }
  let press: Press | null = null;
  let dragPending: { pointerId: number; x: number; y: number } | null = null;

  const cancelPress = () => {
    if (!press) return;
    window.clearTimeout(press.timer);
    press = null;
  };

  const startWindowDrag = () => {
    dragPending = null;
    cancelPress();
    void getCurrentWindow().startDragging();
  };

  window.addEventListener(
    'pointerdown',
    (e) => {
      const target = e.target;
      if (!(target instanceof Element)) return;

      if (e.pointerType === 'touch') {
        markTouch();
        lastTouchDownAt = performance.now();
      }

      cancelPress();
      dragPending = null;

      if (e.button !== 0) return;
      // Mouse keeps Tauri's native drag region; pen/touch get the fallback.
      // Forced touch mode treats mouse-injected presses (virtual displays
      // deliver touch as mouse) as touch, except titlebar drag where the
      // native region already handles mouse.
      const forced = useSettingsStore.getState().inputMode === 'touch';
      if (e.pointerType === 'mouse') {
        if (!forced) return;
      } else if (!isInteractive(target) && target.closest('[data-tauri-drag-region]')) {
        dragPending = { pointerId: e.pointerId, x: e.clientX, y: e.clientY };
      }

      // Text fields keep the native long-press selection flow instead.
      if (isInteractive(target)) return;

      press = {
        pointerId: e.pointerId,
        x: e.clientX,
        y: e.clientY,
        target,
        timer: window.setTimeout(() => {
          const held = press;
          press = null;
          if (!held) return;
          // Windows handled press-and-hold as right-click natively; don't double-open.
          if (performance.now() - lastNativeMenuAt < 400) return;
          // Bubbles to the React root, where the onContextMenu handlers live.
          const ev = new MouseEvent('contextmenu', {
            bubbles: true,
            cancelable: true,
            composed: true,
            clientX: held.x,
            clientY: held.y,
          });
          held.target.dispatchEvent(ev);
        }, LONG_PRESS_MS),
      };
    },
    { capture: true },
  );

  window.addEventListener(
    'pointermove',
    (e) => {
      if (dragPending && e.pointerId === dragPending.pointerId) {
        const dx = e.clientX - dragPending.x;
        const dy = e.clientY - dragPending.y;
        if (dx * dx + dy * dy >= DRAG_THRESHOLD_PX * DRAG_THRESHOLD_PX) startWindowDrag();
      }
      if (press && e.pointerId === press.pointerId) {
        const dx = e.clientX - press.x;
        const dy = e.clientY - press.y;
        // The press became a scroll/drag: it is not a click intent anymore.
        if (dx * dx + dy * dy > LONG_PRESS_SLOP_PX * LONG_PRESS_SLOP_PX) cancelPress();
      }
    },
    { capture: true },
  );

  const cancelAll = () => {
    cancelPress();
    dragPending = null;
  };
  window.addEventListener('pointerup', cancelAll, { capture: true });
  window.addEventListener('pointercancel', cancelAll, { capture: true });
  window.addEventListener(
    'scroll',
    () => {
      cancelPress();
      dragPending = null;
    },
    { capture: true, passive: true },
  );
  window.addEventListener(
    'contextmenu',
    (e) => {
      if (!e.isTrusted) return;
      // Pen press-and-hold or barrel button already produced the real menu.
      lastNativeMenuAt = performance.now();
      cancelPress();
    },
    { capture: true },
  );
  window.addEventListener('blur', cancelAll);

  // Optional OSK: only on touch (or forced touch mode), only right after a
  // touch tap in auto mode, only when the setting is on. Sits in General >
  // Touch because Windows never opens TabTip for desktop apps on its own.
  document.addEventListener('focusin', (e) => {
    // Forced mode injects input as mouse: there is no touch pointerdown to time.
    const forced = useSettingsStore.getState().inputMode === 'touch';
    if (!forced) {
      if (!touchSeen) return;
      if (performance.now() - lastTouchDownAt > OSK_RECENT_TOUCH_MS) return;
    }
    if (!useSettingsStore.getState().touchKeyboardAutoShow) return;
    if (!isTextEdit(e.target)) return;
    const now = performance.now();
    if (now - lastKbLaunchAt < OSK_COOLDOWN_MS) return;
    lastKbLaunchAt = now;
    void trackedInvoke('launch_touch_keyboard').catch(() => undefined);
  });

  // Mode changes apply live, without a restart or reload.
  useSettingsStore.subscribe((s, prev) => {
    if (s.inputMode !== prev.inputMode) {
      applyMode();
      cancelAll();
    }
  });
}
